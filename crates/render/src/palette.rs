use common::config::PaletteConfig;
use common::Ink;

/// Measured panel colours, in sRGB and CIE Lab.
#[derive(Debug, Clone)]
pub struct Palette {
    entries: Vec<(Ink, [u8; 3], [f32; 3])>,
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn rgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|c| srgb_to_linear((c / 255.0).clamp(0.0, 1.0)));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

impl Palette {
    pub fn from_config(cfg: &PaletteConfig) -> Self {
        let defaults = PaletteConfig::default();
        let pick = |v: &str, d: &str| parse_hex(v).or_else(|| parse_hex(d)).unwrap_or([0, 0, 0]);
        let entries = [
            (Ink::Black, pick(&cfg.black, &defaults.black)),
            (Ink::White, pick(&cfg.white, &defaults.white)),
            (Ink::Yellow, pick(&cfg.yellow, &defaults.yellow)),
            (Ink::Red, pick(&cfg.red, &defaults.red)),
            (Ink::Blue, pick(&cfg.blue, &defaults.blue)),
            (Ink::Green, pick(&cfg.green, &defaults.green)),
        ]
        .into_iter()
        .map(|(ink, rgb)| (ink, rgb, rgb_to_lab(rgb.map(f32::from))))
        .collect();
        Self { entries }
    }

    pub fn rgb(&self, ink: Ink) -> [u8; 3] {
        self.entries
            .iter()
            .find(|e| e.0 == ink)
            .map(|e| e.1)
            .unwrap_or([0, 0, 0])
    }

    pub fn lab(&self, ink: Ink) -> [f32; 3] {
        self.entries
            .iter()
            .find(|e| e.0 == ink)
            .map(|e| e.2)
            .unwrap_or([0.0; 3])
    }

    pub fn nearest_rgb(&self, rgb: [u8; 3]) -> Ink {
        let d = |a: [u8; 3]| -> i32 { (0..3).map(|i| (a[i] as i32 - rgb[i] as i32).pow(2)).sum() };
        self.entries
            .iter()
            .min_by_key(|e| d(e.1))
            .map(|e| e.0)
            .unwrap_or(Ink::White)
    }

    pub fn lab_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for e in &self.entries {
            for k in 0..3 {
                lo[k] = lo[k].min(e.2[k]);
                hi[k] = hi[k].max(e.2[k]);
            }
        }
        (lo, hi)
    }

    pub fn nearest_lab(&self, lab: [f32; 3]) -> (Ink, [f32; 3]) {
        let d = |a: [f32; 3]| (0..3).map(|i| (a[i] - lab[i]).powi(2)).sum::<f32>();
        self.entries
            .iter()
            .min_by(|a, b| d(a.2).total_cmp(&d(b.2)))
            .map(|e| (e.0, e.2))
            .unwrap_or((Ink::White, [100.0, 0.0, 0.0]))
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::from_config(&PaletteConfig::default())
    }
}
