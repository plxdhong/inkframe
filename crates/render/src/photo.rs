//! Photo pipeline: cover-crop to 800x480, enhance, then error-diffuse in Lab
//! against the measured palette.

use crate::palette::{rgb_to_lab, Palette};
use common::config::PhotoConfig;
use common::{Frame, Ink, HEIGHT, WIDTH};
use image::{imageops, DynamicImage, RgbImage};

#[derive(Debug, thiserror::Error)]
pub enum PhotoError {
    #[error("read: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(#[from] image::ImageError),
    #[error("image {0}x{1} exceeds the {2}px limit; scale it before uploading")]
    TooLarge(u32, u32, u32),
}

/// Decodes an upload, rejecting oversized images before allocating full buffers.
pub fn decode_upload(bytes: &[u8], max_dimension: u32) -> Result<DynamicImage, PhotoError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let (w, h) = reader.into_dimensions()?;
    if w > max_dimension || h > max_dimension {
        return Err(PhotoError::TooLarge(w, h, max_dimension));
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_dimension);
    limits.max_image_height = Some(max_dimension);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}

/// Scales to cover the panel and centre-crops to exactly 800x480.
pub fn fit_cover(img: &DynamicImage) -> RgbImage {
    let (w, h) = (img.width().max(1) as f32, img.height().max(1) as f32);
    let scale = (WIDTH as f32 / w).max(HEIGHT as f32 / h);
    let (nw, nh) = (
        ((w * scale).round() as u32).max(WIDTH as u32),
        ((h * scale).round() as u32).max(HEIGHT as u32),
    );
    let resized = imageops::resize(&img.to_rgb8(), nw, nh, imageops::FilterType::CatmullRom);
    let (x, y) = ((nw - WIDTH as u32) / 2, (nh - HEIGHT as u32) / 2);
    imageops::crop_imm(&resized, x, y, WIDTH as u32, HEIGHT as u32).to_image()
}

/// Brightness and contrast in linear-ish sRGB, saturation by mixing away from luma.
pub fn enhance(img: &mut RgbImage, cfg: &PhotoConfig) {
    for p in img.pixels_mut() {
        let [r, g, b] = p.0.map(|c| c as f32 / 255.0);
        let luma = 0.299 * r + 0.587 * g + 0.114 * b;
        let out = [r, g, b].map(|c| {
            let c = luma + (c - luma) * cfg.saturation;
            let c = (c - 0.5) * cfg.contrast + 0.5;
            (c * cfg.brightness).clamp(0.0, 1.0)
        });
        p.0 = out.map(|c| (c * 255.0).round() as u8);
    }
}

/// Serpentine Floyd–Steinberg in CIE Lab.
pub fn dither(img: &RgbImage, palette: &Palette) -> Frame {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut lab: Vec<[f32; 3]> = img
        .pixels()
        .map(|p| rgb_to_lab(p.0.map(f32::from)))
        .collect();
    let mut frame = Frame::filled(Ink::White);
    // Clamp to the palette's Lab bounding box so out-of-gamut colours cannot
    // accumulate unbounded error (which shows up as smeared bands).
    let (lo, hi) = palette.lab_bounds();
    for y in 0..h.min(HEIGHT) {
        let ltr = y % 2 == 0;
        for i in 0..w.min(WIDTH) {
            let x = if ltr { i } else { w.min(WIDTH) - 1 - i };
            let idx = y * w + x;
            let px = [0, 1, 2].map(|k| lab[idx][k].clamp(lo[k], hi[k]));
            let (ink, chosen) = palette.nearest_lab(px);
            frame.pixels[y * WIDTH + x] = ink;
            let err = [0, 1, 2].map(|k| px[k] - chosen[k]);
            let dir: isize = if ltr { 1 } else { -1 };
            let mut push = |dx: isize, dy: usize, f: f32| {
                let nx = x as isize + dx;
                let ny = y + dy;
                if nx >= 0 && (nx as usize) < w && ny < h {
                    let j = ny * w + nx as usize;
                    for k in 0..3 {
                        lab[j][k] += err[k] * f;
                    }
                }
            };
            push(dir, 0, 7.0 / 16.0);
            push(-dir, 1, 3.0 / 16.0);
            push(0, 1, 5.0 / 16.0);
            push(dir, 1, 1.0 / 16.0);
        }
    }
    frame
}

/// The full pipeline for an already-decoded image. Returns the 800x480 source
/// (kept so palette changes can reprocess it) and the panel frame.
pub fn process(img: &DynamicImage, cfg: &PhotoConfig, palette: &Palette) -> (RgbImage, Frame) {
    let fitted = fit_cover(img);
    let mut enhanced = fitted.clone();
    enhance(&mut enhanced, cfg);
    let frame = dither(&enhanced, palette);
    (fitted, frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_palette_colours_survive_dithering() {
        let pal = Palette::default();
        let mut img = RgbImage::new(WIDTH as u32, HEIGHT as u32);
        for (x, _, p) in img.enumerate_pixels_mut() {
            p.0 = if x < 400 {
                pal.rgb(Ink::Red)
            } else {
                pal.rgb(Ink::Blue)
            };
        }
        let f = dither(&img, &pal);
        assert!(f.pixels[..400].iter().all(|&i| i == Ink::Red));
        assert!(f.pixels[400..800].iter().all(|&i| i == Ink::Blue));
    }

    #[test]
    fn mid_grey_mixes_black_and_white() {
        let pal = Palette::default();
        let img = RgbImage::from_pixel(WIDTH as u32, HEIGHT as u32, image::Rgb([128, 128, 128]));
        let f = dither(&img, &pal);
        let blacks = f.pixels.iter().filter(|&&i| i == Ink::Black).count();
        let whites = f.pixels.iter().filter(|&&i| i == Ink::White).count();
        assert!(
            blacks > 50_000 && whites > 50_000,
            "blacks {blacks} whites {whites}"
        );
    }

    #[test]
    fn rejects_oversized_upload() {
        let img = DynamicImage::new_rgb8(3000, 10);
        let mut buf = Vec::new();
        let ok = img
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .is_ok();
        assert!(ok);
        assert!(matches!(
            decode_upload(&buf, 2400),
            Err(PhotoError::TooLarge(3000, 10, 2400))
        ));
    }

    #[test]
    fn cover_crop_is_exact_size() {
        let out = fit_cover(&DynamicImage::new_rgb8(1000, 1000));
        assert_eq!((out.width(), out.height()), (800, 480));
    }
}
