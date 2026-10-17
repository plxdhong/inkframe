pub mod canvas;
pub mod dashboard;
mod icons;
pub mod palette;
pub mod patterns;
pub mod photo;
pub mod setup;
pub mod text;

pub use palette::Palette;
pub use text::Fonts;

use common::{Frame, Ink, HEIGHT, WIDTH};

/// Renders a frame as it should look on the panel, using the measured palette.
///
/// Written as a 4-bit indexed PNG: dithered photos barely compress as RGB
/// (~1 MB), and the Pi's Wi-Fi is slow enough that the size dominates how
/// long the phone waits for a preview.
pub fn preview_png(frame: &Frame, palette: &Palette) -> Result<Vec<u8>, png::EncodingError> {
    let mut plte = Vec::with_capacity(Ink::ALL.len() * 3);
    for ink in Ink::ALL {
        plte.extend_from_slice(&palette.rgb(ink));
    }
    let index = |ink: Ink| Ink::ALL.iter().position(|&i| i == ink).unwrap_or(0) as u8;
    let mut data = Vec::with_capacity(WIDTH.div_ceil(2) * HEIGHT);
    for row in frame.pixels.chunks(WIDTH) {
        for pair in row.chunks(2) {
            let hi = pair.first().map_or(0, |&i| index(i));
            let lo = pair.get(1).map_or(0, |&i| index(i));
            data.push(hi << 4 | lo);
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, WIDTH as u32, HEIGHT as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Four);
    enc.set_palette(plte);
    enc.set_compression(png::Compression::Balanced);
    let mut w = enc.write_header()?;
    w.write_image_data(&data)?;
    w.finish()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_png_round_trips_every_ink() -> Result<(), Box<dyn std::error::Error>> {
        let palette = Palette::default();
        let mut frame = Frame::filled(Ink::White);
        for (i, px) in frame.pixels.iter_mut().enumerate() {
            *px = Ink::ALL[(i * 7 + i / WIDTH) % Ink::ALL.len()];
        }
        let png = preview_png(&frame, &palette)?;
        let img = image::load_from_memory(&png)?.to_rgb8();
        assert_eq!((img.width(), img.height()), (WIDTH as u32, HEIGHT as u32));
        for (i, ink) in frame.pixels.iter().enumerate() {
            let (x, y) = ((i % WIDTH) as u32, (i / WIDTH) as u32);
            assert_eq!(img.get_pixel(x, y).0, palette.rgb(*ink), "pixel {x},{y}");
        }
        Ok(())
    }
}
