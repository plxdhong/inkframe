use crate::text::{advance, metrics, Fonts, Style};
use ab_glyph::{point, Font};
use common::{Frame, Ink, HEIGHT, WIDTH};
use resvg::{tiny_skia, usvg};

/// An 800x480 drawing surface that only ever holds panel inks. Text and icons
/// are rasterised without anti-aliasing (coverage threshold 50%).
pub struct Canvas<'f> {
    frame: Frame,
    fonts: &'f Fonts,
}

impl<'f> Canvas<'f> {
    pub fn new(fonts: &'f Fonts, bg: Ink) -> Self {
        Self {
            frame: Frame::filled(bg),
            fonts,
        }
    }

    pub fn into_frame(self) -> Frame {
        self.frame
    }

    pub fn set(&mut self, x: i32, y: i32, ink: Ink) {
        if x >= 0 && y >= 0 && (x as usize) < WIDTH && (y as usize) < HEIGHT {
            self.frame.pixels[y as usize * WIDTH + x as usize] = ink;
        }
    }

    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, ink: Ink) {
        for yy in y.max(0)..(y + h).min(HEIGHT as i32) {
            for xx in x.max(0)..(x + w).min(WIDTH as i32) {
                self.frame.pixels[yy as usize * WIDTH + xx as usize] = ink;
            }
        }
    }

    /// A border drawn inside the box, like CSS `box-sizing: border-box`.
    pub fn border(&mut self, x: i32, y: i32, w: i32, h: i32, t: i32, ink: Ink) {
        self.fill(x, y, w, t, ink);
        self.fill(x, y + h - t, w, t, ink);
        self.fill(x, y, t, h, ink);
        self.fill(x + w - t, y, t, h, ink);
    }

    pub fn measure(&self, style: &Style, text: &str) -> f32 {
        let font = self.fonts.get(style.face);
        let m = metrics(font, style);
        let n = text.chars().count() as f32;
        text.chars().map(|c| advance(font, m.scale, c)).sum::<f32>() + style.spacing * n
    }

    pub fn line_height(&self, style: &Style) -> f32 {
        metrics(self.fonts.get(style.face), style).line
    }

    /// Baseline of a line box that starts at `top`.
    pub fn baseline(&self, top: f32, style: &Style) -> f32 {
        let m = metrics(self.fonts.get(style.face), style);
        top + (m.line - m.content) / 2.0 + m.ascent
    }

    /// Draws one line whose line box starts at `top`; returns the advance width.
    pub fn text(&mut self, x: f32, top: f32, style: &Style, ink: Ink, text: &str) -> f32 {
        let baseline = self.baseline(top, style);
        self.text_on_baseline(x, baseline, style, ink, text)
    }

    pub fn text_on_baseline(
        &mut self,
        x: f32,
        baseline: f32,
        style: &Style,
        ink: Ink,
        text: &str,
    ) -> f32 {
        let font = self.fonts.get(style.face);
        let m = metrics(font, style);
        let mut pen = x;
        let mut hits = Vec::new();
        for c in text.chars() {
            let id = font.glyph_id(c);
            let glyph = id.with_scale_and_position(m.scale, point(pen, baseline));
            if let Some(outline) = font.outline_glyph(glyph) {
                let b = outline.px_bounds();
                let (ox, oy) = (b.min.x as i32, b.min.y as i32);
                outline.draw(|gx, gy, cov| {
                    if cov >= 0.5 {
                        hits.push((ox + gx as i32, oy + gy as i32));
                    }
                });
            }
            pen += advance(font, m.scale, c) + style.spacing;
        }
        for (px, py) in hits {
            self.set(px, py, ink);
        }
        pen - x
    }

    pub fn text_right(&mut self, right: f32, top: f32, style: &Style, ink: Ink, text: &str) {
        let w = self.measure(style, text);
        self.text(right - w, top, style, ink, text);
    }

    /// Largest size not above `style.size` (and not below `min`) that fits `max_w`.
    pub fn fit(&self, style: Style, text: &str, max_w: f32, min: f32) -> Style {
        let mut s = style;
        while s.size > min && self.measure(&s, text) > max_w {
            s.size -= 1.0;
        }
        s
    }

    /// Upright CJK column (`writing-mode: vertical-rl`), centred on `cx`.
    pub fn text_vertical(&mut self, cx: f32, top: f32, style: &Style, ink: Ink, text: &str) -> f32 {
        let mut y = top;
        for c in text.chars() {
            let s = c.to_string();
            let w = self.measure(
                &Style {
                    spacing: 0.0,
                    ..*style
                },
                &s,
            );
            // Ideographic em box: the line box of a 1em line-height.
            let em_style = Style {
                line_height: Some(1.0),
                spacing: 0.0,
                ..*style
            };
            self.text(cx - w / 2.0, y, &em_style, ink, &s);
            y += style.size + style.spacing;
        }
        y - top
    }

    /// Rasterises an SVG (viewBox 0 0 48 48) at `size` px; every opaque pixel becomes `ink`.
    pub fn icon(&mut self, svg_body: &str, x: i32, y: i32, size: u32, ink: Ink) {
        let svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 48 48' width='{size}' height='{size}' shape-rendering='crispEdges'>{svg_body}</svg>"
        );
        let Ok(tree) = usvg::Tree::from_str(&svg, &usvg::Options::default()) else {
            return;
        };
        let Some(mut pix) = tiny_skia::Pixmap::new(size, size) else {
            return;
        };
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pix.as_mut());
        for (i, p) in pix.pixels().iter().enumerate() {
            if p.alpha() >= 128 {
                self.set(
                    x + (i as u32 % size) as i32,
                    y + (i as u32 / size) as i32,
                    ink,
                );
            }
        }
    }
}
