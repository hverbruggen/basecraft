//! A frame: instanced quads for the GPU draw item.
//!
//! Each instance is 16 `f32`s: four corners (top-left, top-right,
//! bottom-left, bottom-right), an RGBA colour with straight alpha, and a
//! texture rectangle `(u, v, du, dv)` into the glyph atlas. `du == 0` means
//! a solid quad; `du < 0` a gradient, its opacity running from `u` at the
//! left edge to `v` at the right. The C++ item uploads the buffer as-is.
//!
//! The atlas holds printable ASCII (32..=126) in a 16 × 6 grid of square
//! cells, so glyph texture coordinates are computed here. The last cell
//! (127) holds a filled circle, for [`Frame::disc`].

pub const FLOATS_PER_INSTANCE: usize = 16;
pub const ATLAS_COLS: u32 = 16;
pub const ATLAS_ROWS: u32 = 6;
/// Advance of a monospace glyph as a fraction of its cell height. The atlas
/// draws glyphs at 0.8 of the cell, and monospace advances are about 0.6 of
/// the font size.
pub const GLYPH_ADVANCE: f32 = 0.5;

pub type Rgba = [f32; 4];

/// The atlas cell holding a filled circle.
const DISC: u8 = 127;

#[derive(Clone, Debug, Default)]
pub struct Frame {
    data: Vec<f32>,
}

impl Frame {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }

    pub fn len(&self) -> usize {
        self.data.len() / FLOATS_PER_INSTANCE
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn floats(&self) -> &[f32] {
        &self.data
    }

    /// The instance data as bytes in native endianness, for upload.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.data.iter().flat_map(|f| f.to_ne_bytes()).collect()
    }

    /// A general quad: corners top-left, top-right, bottom-left, bottom-right.
    pub fn quad(&mut self, corners: [[f32; 2]; 4], color: Rgba, uv: [f32; 4]) {
        for c in corners {
            self.data.extend_from_slice(&c);
        }
        self.data.extend_from_slice(&color);
        self.data.extend_from_slice(&uv);
    }

    /// An axis-aligned solid rectangle.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
        if w <= 0.0 || h <= 0.0 || color[3] <= 0.0 {
            return;
        }
        self.quad(
            [[x, y], [x + w, y], [x, y + h], [x + w, y + h]],
            color,
            [0.0; 4],
        );
    }

    /// A rectangle whose opacity runs smoothly from `from` (times the
    /// colour's alpha) at its left edge to `to` at its right.
    pub fn gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Rgba,
        (from, to): (f32, f32),
    ) {
        if w <= 0.0 || h <= 0.0 || color[3] <= 0.0 {
            return;
        }
        self.quad(
            [[x, y], [x + w, y], [x, y + h], [x + w, y + h]],
            color,
            [from, to, -1.0, 0.0],
        );
    }

    /// One glyph whose cell's top-left corner is at `(x, y)`, `size` high.
    pub fn glyph(&mut self, x: f32, y: f32, size: f32, ch: u8, color: Rgba) {
        let Some(uv) = glyph_uv(ch) else { return };
        // Cells are square; the glyph is centred horizontally in its cell.
        let x0 = x - size * (1.0 - GLYPH_ADVANCE) / 2.0;
        self.quad(
            [
                [x0, y],
                [x0 + size, y],
                [x0, y + size],
                [x0 + size, y + size],
            ],
            color,
            uv,
        );
    }

    /// A filled circle.
    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, color: Rgba) {
        let Some(uv) = glyph_uv(DISC) else { return };
        if r <= 0.0 {
            return;
        }
        // The atlas circle fills its cell less a small margin.
        let r = r * 1.06;
        self.quad(
            [
                [cx - r, cy - r],
                [cx + r, cy - r],
                [cx - r, cy + r],
                [cx + r, cy + r],
            ],
            color,
            uv,
        );
    }

    /// Text running top to bottom (turned a quarter clockwise), its line
    /// starting at `(x, y)` with `x` the left edge; returns its length.
    pub fn text_down(&mut self, x: f32, y: f32, size: f32, text: &str, color: Rgba) -> f32 {
        let advance = size * GLYPH_ADVANCE;
        let mut cy = y;
        for b in text.bytes() {
            if b != b' '
                && let Some(uv) = glyph_uv(b)
            {
                let y0 = cy - size * (1.0 - GLYPH_ADVANCE) / 2.0;
                // The glyph's top edge faces right.
                self.quad(
                    [
                        [x + size, y0],
                        [x + size, y0 + size],
                        [x, y0],
                        [x, y0 + size],
                    ],
                    color,
                    uv,
                );
            }
            cy += advance;
        }
        cy - y
    }

    /// A line of text starting at `(x, y)`; returns its width.
    pub fn text(&mut self, x: f32, y: f32, size: f32, text: &str, color: Rgba) -> f32 {
        let advance = size * GLYPH_ADVANCE;
        let mut cx = x;
        for b in text.bytes() {
            if b != b' ' {
                self.glyph(cx, y, size, b, color);
            }
            cx += advance;
        }
        cx - x
    }
}

pub fn text_width(text: &str, size: f32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let n = text.len() as f32;
    n * size * GLYPH_ADVANCE
}

fn glyph_uv(ch: u8) -> Option<[f32; 4]> {
    if !(32..=127).contains(&ch) {
        return None;
    }
    let i = u32::from(ch - 32);
    #[allow(clippy::cast_precision_loss)]
    let (col, row) = ((i % ATLAS_COLS) as f32, (i / ATLAS_COLS) as f32);
    #[allow(clippy::cast_precision_loss)]
    let (du, dv) = (1.0 / ATLAS_COLS as f32, 1.0 / ATLAS_ROWS as f32);
    Some([col * du, row * dv, du, dv])
}

/// Parse `#rrggbb` or `#rrggbbaa`.
pub fn hex(s: &str) -> Option<Rgba> {
    let h = s.trim().trim_start_matches('#');
    let byte = |i: usize| {
        u8::from_str_radix(h.get(i..i + 2)?, 16)
            .ok()
            .map(|b| f32::from(b) / 255.0)
    };
    match h.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 1.0]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_have_the_agreed_layout() {
        let mut f = Frame::new();
        f.rect(1.0, 2.0, 3.0, 4.0, [1.0, 0.0, 0.0, 1.0]);
        f.glyph(0.0, 0.0, 10.0, b'A', [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(f.len(), 2);
        assert_eq!(&f.floats()[..8], &[1.0, 2.0, 4.0, 2.0, 1.0, 6.0, 4.0, 6.0]);
        assert_eq!(f.floats()[14], 0.0);
        // 'A' is 65 - 32 = 33: column 1, row 2.
        let uv = &f.floats()[16 + 12..32];
        assert!((uv[0] - 1.0 / 16.0).abs() < 1e-6 && (uv[1] - 2.0 / 6.0).abs() < 1e-6);
        assert_eq!(f.to_bytes().len(), 2 * 16 * 4);
    }

    #[test]
    fn colours() {
        assert_eq!(hex("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(hex("00ff0080").map(|c| c[3] > 0.5), Some(true));
        assert_eq!(hex("nope"), None);
    }
}
