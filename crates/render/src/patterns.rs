//! Bring-up test images.

use crate::canvas::Canvas;
use crate::palette::Palette;
use crate::photo;
use crate::text::{Face, Fonts, Style};
use common::config::PhotoConfig;
use common::{Frame, Ink, HEIGHT, WIDTH};
use image::RgbImage;

/// Six vertical bars, each labelled with its ink name.
pub fn color_bars(fonts: &Fonts) -> Frame {
    let mut c = Canvas::new(fonts, Ink::White);
    let names = ["黑", "白", "黄", "红", "蓝", "绿"];
    let bw = (WIDTH / 6) as i32;
    for (i, ink) in Ink::ALL.into_iter().enumerate() {
        let x = i as i32 * bw;
        c.fill(x, 0, bw, HEIGHT as i32, ink);
        let label = if ink == Ink::Black || ink == Ink::Blue || ink == Ink::Red {
            Ink::White
        } else {
            Ink::Black
        };
        c.text(
            x as f32 + 36.0,
            380.0,
            &Style::new(Face::SansBlack, 48.0),
            label,
            names[i],
        );
    }
    c.border(0, 0, WIDTH as i32, HEIGHT as i32, 2, Ink::Black);
    c.into_frame()
}

/// A hue sweep over a lightness ramp, dithered, to judge palette calibration.
pub fn dither_sample(palette: &Palette) -> Frame {
    let mut img = RgbImage::new(WIDTH as u32, HEIGHT as u32);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let hue = x as f32 / WIDTH as f32 * 6.0;
        let light = 1.0 - y as f32 / HEIGHT as f32;
        let f = |n: f32| {
            let k = (n + hue) % 6.0;
            1.0 - (k.min(4.0 - k).clamp(0.0, 1.0))
        };
        let rgb = [f(5.0), f(3.0), f(1.0)].map(|c| {
            let c = if light > 0.5 {
                c + (1.0 - c) * (light - 0.5) * 2.0
            } else {
                c * light * 2.0
            };
            (c * 255.0) as u8
        });
        p.0 = rgb;
    }
    photo::dither(&img, palette)
}

/// Dithered sample run through the same enhancement as uploaded photos.
pub fn dither_sample_enhanced(palette: &Palette, cfg: &PhotoConfig) -> Frame {
    let mut img = RgbImage::from_fn(WIDTH as u32, HEIGHT as u32, |x, y| {
        let v = ((x + y) as f32 / (WIDTH + HEIGHT) as f32 * 255.0) as u8;
        image::Rgb([v, 255 - v, v / 2])
    });
    photo::enhance(&mut img, cfg);
    photo::dither(&img, palette)
}
