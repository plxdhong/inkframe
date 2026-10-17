//! Stroke icons from the visual design, as SVG bodies on a 48x48 grid.
//! Colour comes from the canvas, so every stroke is drawn in black here.

macro_rules! stroke {
    ($w:literal, $body:literal) => {
        concat!(
            "<g fill='none' stroke='#000' stroke-width='",
            $w,
            "' stroke-linecap='round' stroke-linejoin='round'>",
            $body,
            "</g>"
        )
    };
}

pub const RAIN: &str = stroke!("3", "<path d='M14 30h22a8 8 0 0 0 0-16 11 11 0 0 0-21-2A8 8 0 0 0 14 30z'/><path d='M17 36l-2 6M25 36l-2 6M33 36l-2 6'/>");
pub const THUNDER: &str = stroke!("3", "<path d='M14 30h22a8 8 0 0 0 0-16 11 11 0 0 0-21-2A8 8 0 0 0 14 30z'/><path d='M25 32l-4 7h6l-4 7'/>");
pub const SNOW: &str = stroke!("3", "<path d='M14 30h22a8 8 0 0 0 0-16 11 11 0 0 0-21-2A8 8 0 0 0 14 30z'/><path d='M17 38v4M15 40h4M25 38v4M23 40h4M33 38v4M31 40h4'/>");
pub const SUN: &str = stroke!("3", "<circle cx='24' cy='24' r='9'/><path d='M24 4v6M24 38v6M4 24h6M38 24h6M10 10l4 4M34 34l4 4M10 38l4-4M34 14l4-4'/>");
pub const PARTLY: &str = stroke!("3", "<path d='M18 8v3M8 18h3M10.9 10.9l2.1 2.1M25.1 10.9l-2.1 2.1'/><path d='M12 21a7 7 0 0 1 12-6'/><path d='M16 38h20a7 7 0 0 0 0-14 10 10 0 0 0-19-2A8 8 0 0 0 16 38z'/>");
pub const CLOUDY: &str = stroke!(
    "3",
    "<path d='M14 34h22a8 8 0 0 0 0-16 11 11 0 0 0-21-2A8 8 0 0 0 14 34z'/>"
);
pub const FOG: &str = stroke!("3", "<path d='M14 26h22a8 8 0 0 0 0-16 11 11 0 0 0-21-2A8 8 0 0 0 14 26z'/><path d='M8 34h32M12 41h24'/>");
pub const UMBRELLA: &str = stroke!(
    "3.5",
    "<path d='M6 24a18 18 0 0 1 36 0z'/><path d='M24 24v13a4 4 0 0 1-8 0'/>"
);
pub const SHIRT: &str = stroke!(
    "3.5",
    "<path d='M17 6L6 12l4 8 6-3v25h16V17l6 3 4-8-11-6a7 7 0 0 1-14 0z'/>"
);
pub const ALERT: &str = stroke!(
    "3.5",
    "<path d='M24 5L3 42h42z'/><path d='M24 18v12M24 36v1'/>"
);
