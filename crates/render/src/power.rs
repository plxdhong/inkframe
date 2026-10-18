//! Power-off screen. The panel keeps its last image without power, so a frame
//! that is shut down would otherwise keep showing a dashboard that looks live.

use crate::canvas::Canvas;
use crate::text::{Face, Fonts, Style};
use common::{Frame, Ink};

const PAPER: Ink = Ink::White;

/// `name` is the device name (`inkframe-3fa2`), `at` when it was shut down.
pub fn off(fonts: &Fonts, name: &str, at: &str) -> Frame {
    let mut c = Canvas::new(fonts, PAPER);

    // Left: black panel, x 8..288.
    c.fill(8, 8, 280, 464, Ink::Black);
    let (left, right) = (30.0, 266.0);
    c.text(
        left,
        30.0,
        &Style::new(Face::SansBold, 17.0).spacing(2.0),
        PAPER,
        "INKFRAME",
    );
    let big = Style::new(Face::SerifBlack, 64.0).line_height(1.1);
    let lh = c.line_height(&big);
    c.text(left, 96.0, &big, PAPER, "已");
    c.text(left, 96.0 + lh, &big, PAPER, "关机");
    let name_style = c.fit(Style::new(Face::SansBold, 20.0), name, right - left, 12.0);
    c.text(
        left,
        450.0 - c.line_height(&name_style),
        &name_style,
        PAPER,
        name,
    );

    // Right top: yellow card, x 296..792, y 8..276.
    c.fill(296, 8, 496, 268, Ink::Yellow);
    let (cl, cr) = (318.0, 770.0);
    c.text(
        cl,
        28.0,
        &Style::new(Face::SansBold, 20.0),
        Ink::Black,
        "可以安全断电",
    );
    let msg = c.fit(
        Style::new(Face::SansBlack, 48.0).line_height(1.2),
        "重新接通电源",
        cr - cl,
        24.0,
    );
    let mlh = c.line_height(&msg);
    c.text(cl, 142.0 - mlh, &msg, Ink::Black, "重新接通电源");
    c.text(cl, 142.0, &msg, Ink::Black, "即可开机");

    // Right bottom: outlined, x 296..792, y 284..472.
    c.border(296, 284, 496, 188, 4, Ink::Black);
    let cap = Style::new(Face::SansBold, 20.0);
    c.text(cl, 312.0, &cap, Ink::Black, "关机时间");
    let time = c.fit(Style::new(Face::SerifBlack, 44.0), at, cr - cl, 20.0);
    c.text(cl, 350.0, &time, Ink::Black, at);
    c.into_frame()
}
