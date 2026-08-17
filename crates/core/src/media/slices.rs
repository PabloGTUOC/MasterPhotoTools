//! Slice-based pixel primitives (specification §9.1 rule 3).
//!
//! Edge scanning, dark-band detection and column profiling operate on row and
//! column slices of a luma buffer rather than per-pixel through an abstraction,
//! so the compiler can vectorise them. Shared by F4 (half-frame split) and F7
//! (print border) — written once here rather than twice in the tools.
//!
//! Every function takes an 8-bit luma buffer laid out row-major, `width` samples
//! per row.

/// A rectangular region, as a crop box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Bounds {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        }
    }

    pub fn width(&self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(&self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }
}

/// Mean brightness of every column: `out[x]` is the mean of column `x`.
///
/// This is F4's divider-finding profile.
pub fn column_mean_profile(luma: &[u8], width: usize, height: usize) -> Vec<f32> {
    let mut sums = vec![0u64; width];
    for y in 0..height {
        let row = &luma[y * width..(y + 1) * width];
        for (acc, &px) in sums.iter_mut().zip(row.iter()) {
            *acc += px as u64;
        }
    }
    let denom = height.max(1) as f32;
    sums.into_iter().map(|s| s as f32 / denom).collect()
}

/// Mean brightness of every row.
pub fn row_mean_profile(luma: &[u8], width: usize, height: usize) -> Vec<f32> {
    (0..height)
        .map(|y| {
            let row = &luma[y * width..(y + 1) * width];
            row.iter().map(|&p| p as u64).sum::<u64>() as f32 / width.max(1) as f32
        })
        .collect()
}

/// Fraction of a slice at or above `threshold`.
pub fn fraction_at_or_above(slice: &[u8], threshold: u8) -> f32 {
    if slice.is_empty() {
        return 0.0;
    }
    let n = slice.iter().filter(|&&p| p >= threshold).count();
    n as f32 / slice.len() as f32
}

/// Fraction of a slice at or below `threshold`.
pub fn fraction_at_or_below(slice: &[u8], threshold: u8) -> f32 {
    if slice.is_empty() {
        return 0.0;
    }
    let n = slice.iter().filter(|&&p| p <= threshold).count();
    n as f32 / slice.len() as f32
}

/// True if a line is "border": more than `tolerance` of it is at or above
/// `white`, or at or below `dark` (F4 step 1).
pub fn is_border_line(line: &[u8], dark: u8, white: u8, tolerance: f32) -> bool {
    fraction_at_or_above(line, white) > tolerance || fraction_at_or_below(line, dark) > tolerance
}

fn column_samples(luma: &[u8], width: usize, height: usize, x: usize) -> Vec<u8> {
    (0..height).map(|y| luma[y * width + x]).collect()
}

/// Scan inward from each edge while the line qualifies as border, removing at
/// most `max_crop_fraction` of each dimension from any one side (F4 step 1).
pub fn scan_border_inward(
    luma: &[u8],
    width: usize,
    height: usize,
    dark: u8,
    white: u8,
    tolerance: f32,
    max_crop_fraction: f32,
) -> Bounds {
    let max_x = ((width as f32) * max_crop_fraction) as usize;
    let max_y = ((height as f32) * max_crop_fraction) as usize;

    let mut left = 0usize;
    while left < max_x {
        let col = column_samples(luma, width, height, left);
        if !is_border_line(&col, dark, white, tolerance) {
            break;
        }
        left += 1;
    }

    let mut right = width;
    while right > width.saturating_sub(max_x) && right > left + 1 {
        let col = column_samples(luma, width, height, right - 1);
        if !is_border_line(&col, dark, white, tolerance) {
            break;
        }
        right -= 1;
    }

    let mut top = 0usize;
    while top < max_y {
        let row = &luma[top * width..(top + 1) * width];
        if !is_border_line(row, dark, white, tolerance) {
            break;
        }
        top += 1;
    }

    let mut bottom = height;
    while bottom > height.saturating_sub(max_y) && bottom > top + 1 {
        let row = &luma[(bottom - 1) * width..bottom * width];
        if !is_border_line(row, dark, white, tolerance) {
            break;
        }
        bottom -= 1;
    }

    Bounds {
        left: left as u32,
        top: top as u32,
        right: right as u32,
        bottom: bottom as u32,
    }
}

/// Index of the darkest column in `profile`, ignoring `margin_fraction` of the
/// width at each end, then refined to the darkest column within `window` of that
/// (F4 step 2).
pub fn darkest_column(profile: &[f32], margin_fraction: f32, window: usize) -> Option<usize> {
    if profile.is_empty() {
        return None;
    }
    let margin = ((profile.len() as f32) * margin_fraction) as usize;
    let start = margin;
    let end = profile.len().saturating_sub(margin);
    if start >= end {
        return None;
    }

    let mut best = start;
    for (offset, value) in profile[start..end].iter().enumerate() {
        if *value < profile[best] {
            best = start + offset;
        }
    }

    // Refine within ±window, staying inside the profile.
    let lo = best.saturating_sub(window);
    let hi = (best + window + 1).min(profile.len());
    let mut refined = best;
    for (offset, value) in profile[lo..hi].iter().enumerate() {
        if *value < profile[refined] {
            refined = lo + offset;
        }
    }
    Some(refined)
}

/// Trim dark bands from all four sides: advance each edge while more than
/// `tolerance` of the line is at or below `dark`, up to `max_px` per side, then
/// inset by `inset` (F7 step 1, F4 step 4).
pub fn trim_dark_edges(
    luma: &[u8],
    width: usize,
    height: usize,
    dark: u8,
    tolerance: f32,
    max_px: usize,
    inset: u32,
) -> Bounds {
    let mut left = 0usize;
    while left < max_px && left + 1 < width {
        let col = column_samples(luma, width, height, left);
        if fraction_at_or_below(&col, dark) <= tolerance {
            break;
        }
        left += 1;
    }

    let mut right = width;
    while width - right < max_px && right > left + 1 {
        let col = column_samples(luma, width, height, right - 1);
        if fraction_at_or_below(&col, dark) <= tolerance {
            break;
        }
        right -= 1;
    }

    let mut top = 0usize;
    while top < max_px && top + 1 < height {
        let row = &luma[top * width..(top + 1) * width];
        if fraction_at_or_below(row, dark) <= tolerance {
            break;
        }
        top += 1;
    }

    let mut bottom = height;
    while height - bottom < max_px && bottom > top + 1 {
        let row = &luma[(bottom - 1) * width..bottom * width];
        if fraction_at_or_below(row, dark) <= tolerance {
            break;
        }
        bottom -= 1;
    }

    Bounds {
        left: (left as u32 + inset).min(right as u32),
        top: (top as u32 + inset).min(bottom as u32),
        right: (right as u32).saturating_sub(inset).max(left as u32),
        bottom: (bottom as u32).saturating_sub(inset).max(top as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `width × height` buffer: white surround, a dark vertical bar at
    /// `bar_x` of `bar_w` columns, mid-grey elsewhere inside the surround.
    fn synthetic(
        width: usize,
        height: usize,
        border: usize,
        bar_x: usize,
        bar_w: usize,
    ) -> Vec<u8> {
        let mut buf = vec![255u8; width * height];
        for y in border..height - border {
            for x in border..width - border {
                buf[y * width + x] = 128;
            }
            for x in bar_x..bar_x + bar_w {
                buf[y * width + x] = 2;
            }
        }
        buf
    }

    #[test]
    fn a_column_profile_dips_exactly_at_the_dark_bar() {
        let (w, h) = (100, 40);
        let buf = synthetic(w, h, 5, 48, 4);
        let profile = column_mean_profile(&buf, w, h);

        assert_eq!(profile.len(), w);
        let darkest = darkest_column(&profile, 0.20, 20).unwrap();
        assert!(
            (48..52).contains(&darkest),
            "expected the bar at 48..52, found {darkest}"
        );
    }

    #[test]
    fn the_margin_keeps_the_search_away_from_the_edges() {
        let (w, h) = (100, 20);
        // A dark bar inside the ignored margin must not be selected.
        let buf = synthetic(w, h, 0, 3, 3);
        let profile = column_mean_profile(&buf, w, h);
        let darkest = darkest_column(&profile, 0.20, 5).unwrap();
        assert!(darkest >= 20, "margin should exclude x=3, got {darkest}");
    }

    #[test]
    fn row_and_column_profiles_agree_on_a_uniform_field() {
        let buf = vec![100u8; 30 * 20];
        for v in column_mean_profile(&buf, 30, 20) {
            assert!((v - 100.0).abs() < f32::EPSILON);
        }
        for v in row_mean_profile(&buf, 30, 20) {
            assert!((v - 100.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn border_scanning_finds_the_white_surround() {
        // The border must be inside the 12% crop cap on *both* axes, or the cap
        // stops the scan first and this measures the cap rather than the border.
        let (w, h) = (100, 100);
        let buf = synthetic(w, h, 8, 48, 4);
        let bounds = scan_border_inward(&buf, w, h, 25, 235, 0.92, 0.12);

        assert_eq!(bounds.left, 8);
        assert_eq!(bounds.top, 8);
        assert_eq!(bounds.right, 92);
        assert_eq!(bounds.bottom, 92);
        assert_eq!(bounds.width(), 84);
        assert_eq!(bounds.height(), 84);
    }

    /// The cap, not the content, is what stops the scan when the surround is
    /// wider than `max_crop_fraction` allows.
    #[test]
    fn a_surround_wider_than_the_cap_is_only_partly_removed() {
        let (w, h) = (100, 60);
        let buf = synthetic(w, h, 8, 48, 4);
        let bounds = scan_border_inward(&buf, w, h, 25, 235, 0.92, 0.12);

        // 12% of 60 is 7, so one row of the 8-row surround survives on each side.
        assert_eq!(bounds.top, 7);
        assert_eq!(bounds.bottom, 53);
        // The horizontal cap is 12, which the 8-column surround fits inside.
        assert_eq!(bounds.left, 8);
        assert_eq!(bounds.right, 92);
    }

    #[test]
    fn border_scanning_never_removes_more_than_the_cap() {
        // Entirely white: every line qualifies, so only the cap stops it.
        let (w, h) = (100, 100);
        let buf = vec![255u8; w * h];
        let bounds = scan_border_inward(&buf, w, h, 25, 235, 0.92, 0.12);

        assert_eq!(bounds.left, 12);
        assert_eq!(bounds.right, 88);
        assert_eq!(bounds.top, 12);
        assert_eq!(bounds.bottom, 88);
    }

    #[test]
    fn fractions_count_the_right_pixels() {
        let line = [0u8, 10, 250, 255];
        assert_eq!(fraction_at_or_below(&line, 25), 0.5);
        assert_eq!(fraction_at_or_above(&line, 235), 0.5);
        assert_eq!(fraction_at_or_below(&[], 25), 0.0);
    }

    #[test]
    fn dark_edges_are_trimmed_with_an_inset() {
        let (w, h) = (60, 40);
        let mut buf = vec![200u8; w * h];
        // A 5px dark band on the left.
        for y in 0..h {
            for x in 0..5 {
                buf[y * w + x] = 3;
            }
        }
        let bounds = trim_dark_edges(&buf, w, h, 28, 0.70, 40, 1);
        assert_eq!(bounds.left, 6, "5 dark columns plus a 1px safety inset");
        assert_eq!(bounds.right, 59);
    }

    #[test]
    fn an_empty_profile_has_no_darkest_column() {
        assert_eq!(darkest_column(&[], 0.2, 5), None);
        // Margin swallowing the whole profile is not a panic.
        assert_eq!(darkest_column(&[1.0, 2.0], 0.5, 5), None);
    }
}
