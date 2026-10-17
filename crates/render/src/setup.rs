//! Provisioning screen: the setup code the phone app asks for, in the same
//! block language as the dashboard (blue panel, yellow card, outlined tile).

use crate::canvas::Canvas;
use crate::text::{Face, Fonts, Style};
use common::{Frame, Ink};

const PAPER: Ink = Ink::White;

/// `name` is the BLE name (`inkframe-3fa2`), `code` the grouped setup code.
pub fn render(fonts: &Fonts, name: &str, code: &str) -> Frame {
    let mut c = Canvas::new(fonts, PAPER);

    // Left: blue panel, x 8..288.
    c.fill(8, 8, 280, 464, Ink::Blue);
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
    c.text(left, 96.0, &big, PAPER, "连接");
    c.text(left, 96.0 + lh, &big, PAPER, "Wi-Fi");
    let cap = Style::new(Face::SansBold, 17.0);
    let name_style = c.fit(Style::new(Face::SansBold, 20.0), name, right - left, 12.0);
    let name_top = 450.0 - c.line_height(&name_style);
    c.text(left, name_top, &name_style, PAPER, name);
    c.text(
        left,
        name_top - c.line_height(&cap) - 4.0,
        &cap,
        PAPER,
        "蓝牙名称",
    );

    // Right top: yellow card with the code, x 296..792, y 8..276.
    c.fill(296, 8, 496, 268, Ink::Yellow);
    let (cl, cr) = (318.0, 770.0);
    c.text(
        cl,
        28.0,
        &Style::new(Face::SansBold, 20.0),
        Ink::Black,
        "配网码",
    );
    let code_style = c.fit(
        Style::new(Face::SansBlack, 64.0)
            .spacing(2.0)
            .line_height(1.0),
        code,
        cr - cl,
        28.0,
    );
    let code_h = c.line_height(&code_style);
    c.text(cl, 142.0 - code_h / 2.0, &code_style, Ink::Black, code);
    let hint = Style::new(Face::SansBold, 17.0);
    c.text(
        cl,
        256.0 - c.line_height(&hint),
        &hint,
        Ink::Black,
        "在 App 中输入，不区分大小写",
    );

    // Right bottom: steps, outlined, x 296..792, y 284..472.
    c.border(296, 284, 496, 188, 4, Ink::Black);
    let step = Style::new(Face::SansBold, 22.0);
    let num = Style::new(Face::SerifBlack, 30.0).line_height(1.0);
    let steps = [
        "手机打开 InkFrame App",
        "点「添加设备」，靠近相框",
        "输入上方配网码，选择 Wi-Fi",
    ];
    let row_h = 52.0;
    let top = 284.0 + (188.0 - row_h * steps.len() as f32) / 2.0;
    for (i, s) in steps.iter().enumerate() {
        let y = top + i as f32 * row_h;
        let n = (i + 1).to_string();
        c.fill(318, (y + 8.0) as i32, 36, 36, Ink::Black);
        let nw = c.measure(&num, &n);
        c.text(
            336.0 - nw / 2.0,
            y + 26.0 - c.line_height(&num) / 2.0,
            &num,
            PAPER,
            &n,
        );
        let st = c.fit(step, s, 770.0 - 372.0, 14.0);
        c.text(
            372.0,
            y + 26.0 - c.line_height(&st) / 2.0,
            &st,
            Ink::Black,
            s,
        );
    }
    c.into_frame()
}
