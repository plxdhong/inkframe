//! Photo library on the data partition. Each photo keeps its 800x480 source
//! (so palette changes can reprocess it) and the ready-to-send packed frame.

use anyhow::{bail, Context};
use chrono::{DateTime, Utc};
use common::config::PhotoConfig;
use common::fsutil::write_atomic;
use common::Frame;
use rand::seq::SliceRandom;
use render::photo;
use render::Palette;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoMeta {
    pub id: String,
    pub added: DateTime<Utc>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub paused: bool,
}

pub struct PhotoStore {
    dir: PathBuf,
}

impl PhotoStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("photos"),
        }
    }

    fn photo_dir(&self, id: &str) -> anyhow::Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            bail!("invalid photo id");
        }
        Ok(self.dir.join(id))
    }

    pub fn list(&self) -> Vec<PhotoMeta> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<PhotoMeta> = rd
            .flatten()
            .filter_map(|e| std::fs::read(e.path().join("meta.json")).ok())
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect();
        out.sort_by_key(|m| m.added);
        out
    }

    pub fn get(&self, id: &str) -> Option<PhotoMeta> {
        let b = std::fs::read(self.photo_dir(id).ok()?.join("meta.json")).ok()?;
        serde_json::from_slice(&b).ok()
    }

    /// Decodes, size-checks, processes and stores an upload. CPU-heavy: call from a blocking thread.
    pub fn add(
        &self,
        bytes: &[u8],
        cfg: &PhotoConfig,
        palette: &Palette,
    ) -> anyhow::Result<PhotoMeta> {
        let img = photo::decode_upload(bytes, cfg.max_dimension)?;
        let (source, frame) = photo::process(&img, cfg, palette);
        let id = format!(
            "{}-{:04x}",
            Utc::now().format("%Y%m%d%H%M%S"),
            rand::random::<u16>()
        );
        let dir = self.photo_dir(&id)?;
        std::fs::create_dir_all(&dir)?;
        source.save(dir.join("source.png")).context("save source")?;
        write_atomic(&dir.join("frame.bin"), &frame.pack())?;
        let meta = PhotoMeta {
            id,
            added: Utc::now(),
            favorite: false,
            paused: false,
        };
        write_atomic(&dir.join("meta.json"), &serde_json::to_vec_pretty(&meta)?)?;
        Ok(meta)
    }

    pub fn update(
        &self,
        id: &str,
        favorite: Option<bool>,
        paused: Option<bool>,
    ) -> anyhow::Result<PhotoMeta> {
        let mut meta = self.get(id).context("no such photo")?;
        if let Some(f) = favorite {
            meta.favorite = f;
        }
        if let Some(p) = paused {
            meta.paused = p;
        }
        write_atomic(
            &self.photo_dir(id)?.join("meta.json"),
            &serde_json::to_vec_pretty(&meta)?,
        )?;
        Ok(meta)
    }

    pub fn delete(&self, id: &str) -> anyhow::Result<()> {
        let dir = self.photo_dir(id)?;
        if !dir.exists() {
            bail!("no such photo");
        }
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    pub fn frame(&self, id: &str) -> anyhow::Result<Frame> {
        let bytes = std::fs::read(self.photo_dir(id)?.join("frame.bin"))?;
        Frame::unpack(&bytes).context("corrupt frame file")
    }

    /// Re-dithers every stored source, e.g. after palette or enhancement changes.
    pub fn reprocess_all(&self, cfg: &PhotoConfig, palette: &Palette) -> anyhow::Result<usize> {
        let mut n = 0;
        for m in self.list() {
            let dir = self.photo_dir(&m.id)?;
            let img =
                image::open(dir.join("source.png")).with_context(|| format!("open {}", m.id))?;
            let (_, frame) = photo::process(&img, cfg, palette);
            write_atomic(&dir.join("frame.bin"), &frame.pack())?;
            n += 1;
        }
        Ok(n)
    }

    /// Pops the next photo from the round, dealing a new shuffled round when the
    /// deck is empty. Favourites appear `favorite_weight` times per round; the
    /// same photo never appears twice in a row.
    pub fn next_from_deck(
        &self,
        deck: &mut Vec<String>,
        favorite_weight: u32,
        last: Option<&str>,
    ) -> Option<String> {
        let active: Vec<PhotoMeta> = self.list().into_iter().filter(|m| !m.paused).collect();
        deck.retain(|id| active.iter().any(|m| &m.id == id));
        if deck.is_empty() {
            *deck = deal(&active, favorite_weight, last);
        }
        if deck.is_empty() {
            return None;
        }
        Some(deck.remove(0))
    }

    pub fn source_png(&self, id: &str) -> anyhow::Result<Vec<u8>> {
        Ok(std::fs::read(self.photo_dir(id)?.join("source.png"))?)
    }
}

fn deal(active: &[PhotoMeta], favorite_weight: u32, last: Option<&str>) -> Vec<String> {
    let mut rng = rand::thread_rng();
    let mut counts: Vec<(String, usize)> = active
        .iter()
        .map(|m| {
            (
                m.id.clone(),
                if m.favorite {
                    favorite_weight.max(1) as usize
                } else {
                    1
                },
            )
        })
        .collect();
    counts.shuffle(&mut rng);
    let mut total: usize = counts.iter().map(|c| c.1).sum();
    let mut deck = Vec::with_capacity(total);
    let mut prev: Option<String> = last.map(str::to_string);
    while total > 0 {
        let not_prev = |c: &&(String, usize)| c.1 > 0 && prev.as_deref() != Some(c.0.as_str());
        // If one photo needs more than half the remaining slots it must go now,
        // otherwise it would end up back to back.
        let forced = counts
            .iter()
            .filter(not_prev)
            .find(|c| c.1 * 2 > total)
            .map(|c| c.0.clone());
        let pick = forced.or_else(|| {
            let cands: Vec<&(String, usize)> = counts.iter().filter(not_prev).collect();
            cands
                .choose_weighted(&mut rng, |c| c.1)
                .ok()
                .map(|c| c.0.clone())
        });
        // Only one photo left with copies: a repeat is unavoidable.
        let pick = pick.or_else(|| counts.iter().find(|c| c.1 > 0).map(|c| c.0.clone()));
        let Some(id) = pick else { break };
        if let Some(c) = counts.iter_mut().find(|c| c.0 == id) {
            c.1 -= 1;
        }
        total -= 1;
        deck.push(id.clone());
        prev = Some(id);
    }
    deck
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn meta(id: &str, favorite: bool) -> PhotoMeta {
        PhotoMeta {
            id: id.into(),
            added: Utc::now(),
            favorite,
            paused: false,
        }
    }

    #[test]
    fn round_contains_each_photo_once() {
        let photos: Vec<PhotoMeta> = (0..50).map(|i| meta(&format!("p{i}"), false)).collect();
        let deck = deal(&photos, 3, None);
        assert_eq!(deck.len(), 50);
        assert_eq!(deck.iter().collect::<HashSet<_>>().len(), 50);
    }

    #[test]
    fn favourites_weighted_without_back_to_back() {
        let photos = vec![
            meta("a", true),
            meta("b", false),
            meta("c", false),
            meta("d", false),
        ];
        for _ in 0..200 {
            let deck = deal(&photos, 3, Some("a"));
            assert_eq!(deck.iter().filter(|d| *d == "a").count(), 3);
            assert_ne!(deck[0], "a");
            assert!(deck.windows(2).all(|w| w[0] != w[1]), "{deck:?}");
        }
    }

    #[test]
    fn store_roundtrip_and_rotation() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let store = PhotoStore::new(tmp.path());
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(1200, 900)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        let pal = Palette::default();
        let cfg = PhotoConfig::default();
        let a = store.add(&png, &cfg, &pal)?;
        let b = store.add(&png, &cfg, &pal)?;
        assert_eq!(store.list().len(), 2);
        store.update(&b.id, None, Some(true))?;
        let mut deck = Vec::new();
        for _ in 0..3 {
            assert_eq!(
                store.next_from_deck(&mut deck, 3, None).as_deref(),
                Some(a.id.as_str())
            );
        }
        assert!(store.frame(&a.id).is_ok());
        store.delete(&a.id)?;
        assert_eq!(store.next_from_deck(&mut deck, 3, None), None);
        assert!(store.photo_dir("../etc").is_err());
        Ok(())
    }
}
