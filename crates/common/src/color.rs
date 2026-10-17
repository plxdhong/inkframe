use serde::{Deserialize, Serialize};

pub const WIDTH: usize = 800;
pub const HEIGHT: usize = 480;

/// The six inks of the Spectra 6 panel. Discriminants are the controller's 4-bit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum Ink {
    Black = 0x0,
    White = 0x1,
    Yellow = 0x2,
    Red = 0x3,
    Blue = 0x5,
    Green = 0x6,
}

impl Ink {
    pub const ALL: [Ink; 6] = [
        Ink::Black,
        Ink::White,
        Ink::Yellow,
        Ink::Red,
        Ink::Blue,
        Ink::Green,
    ];

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(code: u8) -> Option<Ink> {
        Self::ALL.into_iter().find(|i| i.code() == code)
    }
}

/// A full-screen image in panel inks, one byte per pixel, row-major.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub pixels: Vec<Ink>,
}

impl Frame {
    pub fn filled(ink: Ink) -> Self {
        Self {
            pixels: vec![ink; WIDTH * HEIGHT],
        }
    }

    /// Controller format: two pixels per byte, left pixel in the high nibble (192,000 bytes).
    pub fn pack(&self) -> Vec<u8> {
        self.pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[a, b]| (a.code() << 4) | b.code())
            .collect()
    }

    pub fn unpack(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != WIDTH * HEIGHT / 2 {
            return None;
        }
        let mut pixels = Vec::with_capacity(WIDTH * HEIGHT);
        for b in bytes {
            pixels.push(Ink::from_code(b >> 4)?);
            pixels.push(Ink::from_code(b & 0x0f)?);
        }
        Some(Self { pixels })
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{})", WIDTH, HEIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_roundtrip() {
        let mut f = Frame::filled(Ink::White);
        f.pixels[0] = Ink::Red;
        f.pixels[1] = Ink::Blue;
        let packed = f.pack();
        assert_eq!(packed.len(), 192_000);
        assert_eq!(packed[0], 0x35);
        assert_eq!(Frame::unpack(&packed), Some(f));
    }
}
