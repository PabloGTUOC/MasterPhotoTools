//! A minimal bitmap font, for F5's contact-sheet captions.
//!
//! Specification §2.6 lists no font library and permits exactly one external
//! binary, which is `exiftool`. Rather than add a font crate and ship a typeface
//! (G8), captions are drawn from a built-in 5×7 cell font scaled to the
//! requested size. It is legible at caption sizes, which is all F5 asks of it.

/// Glyph cell size, before scaling.
pub const GLYPH_WIDTH: u32 = 5;
pub const GLYPH_HEIGHT: u32 = 7;
/// Blank columns between glyphs, before scaling.
pub const GLYPH_SPACING: u32 = 1;

/// Rows of a glyph, one byte per row, low five bits used, MSB-of-five leftmost.
type Glyph = [u8; 7];

const UNKNOWN: Glyph = [
    0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
];

/// Look up a glyph for an ASCII character.
fn glyph(c: char) -> Glyph {
    match c.to_ascii_uppercase() {
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        ',' => [0, 0, 0, 0, 0b01100, 0b01100, 0b01000],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0b11111],
        '+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        '=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        '#' => [
            0b01010, 0b11111, 0b01010, 0b01010, 0b01010, 0b11111, 0b01010,
        ],
        '@' => [
            0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
        ],
        '&' => [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
        '\'' => [0b00100, 0b00100, 0, 0, 0, 0, 0],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '\\' => [
            0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
        ],
        ':' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0],
        '%' => [
            0b11000, 0b11001, 0b00010, 0b00100, 0b01000, 0b10011, 0b00011,
        ],
        '~' => [0, 0, 0b01001, 0b10110, 0, 0, 0],
        '*' => [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],
        _ => UNKNOWN,
    }
}

/// Width in pixels of `text` rendered at `scale`.
pub fn measure(text: &str, scale: u32) -> u32 {
    let scale = scale.max(1);
    let n = text.chars().count() as u32;
    if n == 0 {
        return 0;
    }
    n * GLYPH_WIDTH * scale + (n - 1) * GLYPH_SPACING * scale
}

/// The integer scale that renders text closest to, but not above, `target_height`.
pub fn scale_for_height(target_height: u32) -> u32 {
    (target_height / GLYPH_HEIGHT).max(1)
}

/// Draw `text` into `plot(x, y)` for each set pixel, with the top-left of the
/// first glyph at `(origin_x, origin_y)`.
pub fn draw<F: FnMut(i64, i64)>(text: &str, origin_x: i64, origin_y: i64, scale: u32, mut plot: F) {
    let scale = scale.max(1) as i64;
    let mut pen = origin_x;

    for c in text.chars() {
        let g = glyph(c);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..GLYPH_WIDTH {
                // Bit 4 is the leftmost of the five.
                let lit = bits & (1 << (GLYPH_WIDTH - 1 - col)) != 0;
                if !lit {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        plot(
                            pen + col as i64 * scale + dx,
                            origin_y + row as i64 * scale + dy,
                        );
                    }
                }
            }
        }
        pen += (GLYPH_WIDTH + GLYPH_SPACING) as i64 * scale;
    }
}

/// Shorten a caption per F5: names longer than 28 characters become
/// `name[:18] + "..." + extension`.
pub fn shorten_caption(name: &str) -> String {
    const LIMIT: usize = 28;
    const KEEP: usize = 18;

    if name.chars().count() <= LIMIT {
        return name.to_string();
    }

    let (stem, extension) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };

    let head: String = stem.chars().take(KEEP).collect();
    format!("{head}...{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_name_is_left_alone() {
        assert_eq!(shorten_caption("IMG_1234.jpg"), "IMG_1234.jpg");
        // Exactly at the limit is still left alone.
        let at_limit = "a".repeat(24) + ".jpg";
        assert_eq!(at_limit.len(), 28);
        assert_eq!(shorten_caption(&at_limit), at_limit);
    }

    #[test]
    fn a_long_name_keeps_eighteen_characters_and_its_extension() {
        let name = format!("{}.jpg", "b".repeat(40));
        let short = shorten_caption(&name);
        assert_eq!(short, format!("{}...{}", "b".repeat(18), ".jpg"));
        assert!(short.len() < name.len());
    }

    #[test]
    fn a_long_name_without_an_extension_still_shortens() {
        let name = "c".repeat(40);
        assert_eq!(shorten_caption(&name), format!("{}...", "c".repeat(18)));
    }

    #[test]
    fn measuring_accounts_for_inter_glyph_spacing() {
        // One glyph: just its width.
        assert_eq!(measure("A", 1), GLYPH_WIDTH);
        // Two glyphs: two widths plus one gap.
        assert_eq!(measure("AB", 1), GLYPH_WIDTH * 2 + GLYPH_SPACING);
        assert_eq!(measure("", 1), 0);
        // Scale multiplies everything.
        assert_eq!(measure("AB", 2), (GLYPH_WIDTH * 2 + GLYPH_SPACING) * 2);
    }

    #[test]
    fn a_scale_is_chosen_to_fit_the_requested_height() {
        assert_eq!(scale_for_height(7), 1);
        assert_eq!(scale_for_height(14), 2);
        assert_eq!(scale_for_height(12), 1);
        // Never zero, however small the strip.
        assert_eq!(scale_for_height(1), 1);
        assert_eq!(scale_for_height(0), 1);
    }

    #[test]
    fn drawing_marks_pixels_inside_the_expected_box() {
        let mut marked = Vec::new();
        draw("A", 0, 0, 1, |x, y| marked.push((x, y)));

        assert!(!marked.is_empty());
        for (x, y) in &marked {
            assert!((0..GLYPH_WIDTH as i64).contains(x), "x {x} out of cell");
            assert!((0..GLYPH_HEIGHT as i64).contains(y), "y {y} out of cell");
        }
    }

    #[test]
    fn a_space_draws_nothing() {
        let mut marked = 0;
        draw(" ", 0, 0, 3, |_, _| marked += 1);
        assert_eq!(marked, 0);
    }

    #[test]
    fn an_unmapped_character_still_draws_something_visible() {
        let mut marked = 0;
        draw("\u{263A}", 0, 0, 1, |_, _| marked += 1);
        assert!(marked > 0, "an unknown glyph should not silently vanish");
    }
}
