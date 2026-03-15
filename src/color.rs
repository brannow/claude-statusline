use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn fg(&self) -> String {
        format!("\x1b[38;2;{};{};{}m", self.0, self.1, self.2)
    }

    pub fn bg(&self) -> String {
        format!("\x1b[48;2;{};{};{}m", self.0, self.1, self.2)
    }

    pub fn fg_bold(&self) -> String {
        format!("\x1b[38;2;{};{};{};1m", self.0, self.1, self.2)
    }

    pub fn scale(&self, pct: u8) -> Rgb {
        Rgb(
            (self.0 as u16 * pct as u16 / 100) as u8,
            (self.1 as u16 * pct as u16 / 100) as u8,
            (self.2 as u16 * pct as u16 / 100) as u8,
        )
    }
}

pub const RST: &str = "\x1b[0m";

// Original bright palette — looks good on dark terminals as colored bands
pub const PALETTES: [Rgb; 12] = [
    Rgb(105, 145, 225), // blue
    Rgb(130, 190, 130), // green
    Rgb(190, 130, 175), // pink
    Rgb(200, 170, 100), // amber
    Rgb(100, 185, 185), // teal
    Rgb(175, 130, 190), // purple
    Rgb(110, 170, 210), // sky
    Rgb(180, 190, 110), // olive
    Rgb(200, 140, 130), // coral
    Rgb(130, 170, 180), // steel
    Rgb(190, 175, 120), // khaki
    Rgb(160, 130, 190), // violet
];

// Line 2 colors
pub const L2_BG: Rgb = Rgb(0, 0, 0);
pub const L2_TXT: Rgb = Rgb(170, 170, 170);
pub const L2_DIM: Rgb = Rgb(80, 80, 80);

// Threshold colors for percentages and durations
pub const SAGE: Rgb = Rgb(150, 210, 150);
pub const GOLD: Rgb = Rgb(215, 195, 125);
pub const CORAL: Rgb = Rgb(225, 150, 150);

// Lines changed colors
pub const ADDED_CLR: Rgb = Rgb(150, 210, 150);
pub const REMOVED_CLR: Rgb = Rgb(225, 150, 150);

pub struct Palette {
    pub base: Rgb,
    pub sep: Rgb,
    pub txt: Rgb,
}

pub fn palette_for(key: &str) -> Palette {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let idx = (hasher.finish() % 12) as usize;
    let base = PALETTES[idx];
    Palette {
        base,
        sep: base.scale(40), // dark separator on bright bg
        txt: base.scale(15), // very dark text on bright bg
    }
}

pub fn pct_color(pct: u64) -> Rgb {
    if pct > 80 {
        CORAL
    } else if pct > 50 {
        GOLD
    } else {
        SAGE
    }
}

pub fn duration_color(hours: u64) -> Rgb {
    if hours > 2 {
        CORAL
    } else if hours > 0 {
        GOLD
    } else {
        SAGE
    }
}
