use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum FontError {
    #[error("read font {0}: {1}")]
    Io(String, std::io::Error),
    #[error("invalid font {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    SansMedium,
    SansBold,
    SansBlack,
    SerifBold,
    SerifBlack,
}

/// The five weights the design uses, loaded once at startup.
pub struct Fonts {
    faces: Vec<(Face, FontVec)>,
}

const FILES: [(Face, &str); 5] = [
    (Face::SansMedium, "NotoSansSC-Medium.otf"),
    (Face::SansBold, "NotoSansSC-Bold.otf"),
    (Face::SansBlack, "NotoSansSC-Black.otf"),
    (Face::SerifBold, "NotoSerifSC-Bold.otf"),
    (Face::SerifBlack, "NotoSerifSC-Black.otf"),
];

impl Fonts {
    pub fn load(dir: &Path) -> Result<Self, FontError> {
        let mut faces = Vec::new();
        for (face, file) in FILES {
            let path = dir.join(file);
            let bytes =
                std::fs::read(&path).map_err(|e| FontError::Io(path.display().to_string(), e))?;
            let font = FontVec::try_from_vec(bytes)
                .map_err(|_| FontError::Invalid(path.display().to_string()))?;
            faces.push((face, font));
        }
        Ok(Self { faces })
    }

    pub(crate) fn get(&self, face: Face) -> &FontVec {
        // `load` always fills every face, so the fallback is never taken.
        self.faces
            .iter()
            .find(|f| f.0 == face)
            .map(|f| &f.1)
            .unwrap_or(&self.faces[0].1)
    }
}

/// CSS-like text style: `size` is the em size in px, `spacing` is letter-spacing.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub face: Face,
    pub size: f32,
    pub spacing: f32,
    /// Line height as a multiple of `size`; `None` is the font's normal height.
    pub line_height: Option<f32>,
}

impl Style {
    pub fn new(face: Face, size: f32) -> Self {
        Self {
            face,
            size,
            spacing: 0.0,
            line_height: None,
        }
    }
    pub fn spacing(mut self, s: f32) -> Self {
        self.spacing = s;
        self
    }
    pub fn line_height(mut self, lh: f32) -> Self {
        self.line_height = Some(lh);
        self
    }
}

pub(crate) struct Metrics {
    pub scale: PxScale,
    pub ascent: f32,
    pub content: f32,
    pub line: f32,
}

pub(crate) fn metrics(font: &FontVec, style: &Style) -> Metrics {
    let upem = font.units_per_em().unwrap_or(1000.0);
    let k = style.size / upem;
    let ascent = font.ascent_unscaled() * k;
    let content = font.height_unscaled() * k;
    let line = style.line_height.map(|l| l * style.size).unwrap_or(content);
    Metrics {
        scale: PxScale::from(font.height_unscaled() * k),
        ascent,
        content,
        line,
    }
}

pub(crate) fn advance(font: &FontVec, scale: PxScale, c: char) -> f32 {
    font.as_scaled(scale).h_advance(font.glyph_id(c))
}
