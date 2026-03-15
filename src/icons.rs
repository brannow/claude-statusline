// Standard Unicode only — renders in any modern terminal font (Menlo, SF Mono, etc.)
// No Nerd Font / Powerline glyphs required.

// Half-block edges — creates colored band start/end effect
pub const EDGE_LEFT: &str = "▐";  // U+2590 right half block (fg = band color)
pub const EDGE_RIGHT: &str = "▌"; // U+258C left half block (fg = band color)

// Progress bar
pub const BAR_FILLED: &str = "▰"; // U+25B0
pub const BAR_EMPTY: &str = "▱";  // U+25B1
