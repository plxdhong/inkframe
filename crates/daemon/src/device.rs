//! Stable device identity: a short id shown in BLE/mDNS names so several
//! frames on one network can be told apart.

use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Identity {
    /// 4 lowercase hex chars, e.g. `3fa2`.
    pub id: String,
    /// `inkframe-3fa2`: BLE local name, mDNS instance and host name.
    pub name: String,
}

impl Identity {
    /// Derived from /etc/machine-id; falls back to a random id persisted in
    /// the data dir (development machines, or a missing machine-id).
    pub fn load(data_dir: &Path) -> Self {
        let seed = std::fs::read_to_string("/etc/machine-id")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| persisted(data_dir));
        let id = match seed {
            Some(s) => hex4(&Sha256::digest(s.as_bytes())),
            None => format!("{:04x}", rand::random::<u16>()),
        };
        Self {
            name: format!("inkframe-{id}"),
            id,
        }
    }
}

fn hex4(digest: &[u8]) -> String {
    digest.iter().take(2).map(|b| format!("{b:02x}")).collect()
}

fn persisted(data_dir: &Path) -> Option<String> {
    let path = data_dir.join("device_id");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    let fresh = format!("{:016x}", rand::random::<u64>());
    common::fsutil::write_atomic(&path, fresh.as_bytes()).ok()?;
    Some(fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_id_is_stable() -> std::io::Result<()> {
        let tmp = tempfile::tempdir()?;
        let a = persisted(tmp.path());
        assert!(a.is_some());
        assert_eq!(persisted(tmp.path()), a);
        assert_eq!(hex4(&[0x3f, 0xa2, 0xff]), "3fa2");
        Ok(())
    }
}
