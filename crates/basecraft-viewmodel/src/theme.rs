//! Colours. Reads an Omarchy theme (`colors.toml`) when present; otherwise
//! uses a built-in light palette.

use crate::frame::{Rgba, hex};

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub background: Rgba,
    pub surface: Rgba,
    pub surface_strong: Rgba,
    pub foreground: Rgba,
    pub foreground_muted: Rgba,
    pub accent: Rgba,
    pub selection: Rgba,
    pub muted: Rgba,
    pub red: Rgba,
    pub orange: Rgba,
    pub yellow: Rgba,
    pub green: Rgba,
    pub cyan: Rgba,
    pub blue: Rgba,
    pub magenta: Rgba,
    pub brown: Rgba,
}

impl Default for Theme {
    fn default() -> Self {
        let c = |s| hex(s).unwrap_or([0.5, 0.5, 0.5, 1.0]);
        Self {
            dark: false,
            background: c("#f4f1ea"),
            surface: c("#e8e3d8"),
            surface_strong: c("#d9d2c3"),
            foreground: c("#2b2620"),
            foreground_muted: c("#6f665a"),
            accent: c("#9a4a24"),
            selection: c("#d8c8a6"),
            muted: c("#8a7f6e"),
            red: c("#b03a2e"),
            orange: c("#a8571c"),
            yellow: c("#8a6a12"),
            green: c("#3f7a4c"),
            cyan: c("#2f6f74"),
            blue: c("#3d5a8a"),
            magenta: c("#8f3c60"),
            brown: c("#6e4c2e"),
        }
    }
}

impl Theme {
    /// Explicit publication palette; screen exports retain the current theme by default.
    #[must_use]
    pub fn publication() -> Self {
        Self {
            background: [1.; 4],
            foreground: [0., 0., 0., 1.],
            ..Self::default()
        }
    }

    /// The theme colour whose hue is nearest `hue` (degrees: 0 red, 120
    /// green), among its saturated palette colours. Status messages use it
    /// so that success and failure read as green and red in any theme.
    pub fn nearest_hue(&self, hue: f32) -> Rgba {
        let palette = [
            self.red,
            self.orange,
            self.yellow,
            self.green,
            self.cyan,
            self.blue,
            self.magenta,
            self.brown,
            self.accent,
        ];
        let distance = |c: &Rgba| {
            let (h, s) = hue_saturation(*c);
            if s < 0.15 {
                return f32::MAX;
            }
            let d = (h - hue).rem_euclid(360.0);
            d.min(360.0 - d)
        };
        palette
            .into_iter()
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            .unwrap_or(self.foreground)
    }

    /// Parse an Omarchy `colors.toml`. Unknown or missing keys keep their
    /// defaults, so a partial theme still works.
    pub fn from_omarchy(text: &str) -> Self {
        let mut t = Theme::default();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            let Some((key, rest)) = trimmed.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = rest.trim();
            if key == "mode" {
                t.dark = value.trim_matches('"') == "dark";
                continue;
            }
            // `"#rrggbb"  # comment`: take the quoted part.
            let Some(quoted) = value.strip_prefix('"').and_then(|v| v.split('"').next()) else {
                continue;
            };
            let Some(color) = hex(quoted) else { continue };
            let slot = match key {
                "background" => &mut t.background,
                "dark_background" => &mut t.surface,
                "darker_background" => &mut t.surface_strong,
                "foreground" => &mut t.foreground,
                "dark_foreground" => &mut t.foreground_muted,
                "accent" => &mut t.accent,
                "selection" => &mut t.selection,
                "muted" => &mut t.muted,
                "red" => &mut t.red,
                "orange" => &mut t.orange,
                "yellow" => &mut t.yellow,
                "green" => &mut t.green,
                "cyan" => &mut t.cyan,
                "blue" => &mut t.blue,
                "magenta" => &mut t.magenta,
                "brown" => &mut t.brown,
                _ => continue,
            };
            *slot = color;
        }
        t
    }

    /// Residue colour: the usual A green, C blue, G amber, T red.
    pub fn residue(&self, b: u8) -> Rgba {
        match b.to_ascii_uppercase() {
            b'A' => self.green,
            b'C' => self.blue,
            b'G' => self.yellow,
            b'T' | b'U' => self.red,
            b'-' => self.background,
            _ => self.muted,
        }
    }

    /// Amino acid colour, by side-chain class as in Clustal X: hydrophobic
    /// blue, positive red, negative magenta, polar green, and their own
    /// colours for cysteine, glycine, proline and the aromatic H and Y.
    pub fn amino(&self, b: u8) -> Rgba {
        match b.to_ascii_uppercase() {
            b'A' | b'I' | b'L' | b'M' | b'F' | b'W' | b'V' => self.blue,
            b'K' | b'R' => self.red,
            b'E' | b'D' => self.magenta,
            b'N' | b'Q' | b'S' | b'T' => self.green,
            b'C' => self.orange,
            b'G' => self.yellow,
            b'P' => self.brown,
            b'H' | b'Y' => self.cyan,
            b'-' | b'.' => self.background,
            _ => self.muted,
        }
    }

    pub fn feature(&self, kind: &str) -> Rgba {
        match kind {
            "gene" => self.green,
            "CDS" => self.yellow,
            "mRNA" | "transcript" => self.blue,
            "exon" => self.cyan,
            "tRNA" => self.magenta,
            "rRNA" | "ncRNA" => self.orange,
            "repeat_region" | "mobile_element" => self.brown,
            _ => self.muted,
        }
    }
}

/// `color` with alpha multiplied by `a`.
pub fn alpha(color: Rgba, a: f32) -> Rgba {
    [color[0], color[1], color[2], color[3] * a]
}

/// Linear mix of two colours.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_omarchy_colours() {
        let t = Theme::from_omarchy(
            "# comment\nmode = \"dark\"\naccent = \"#ff0000\"  # vermilion\nhyprland_active_border = \"rgba(8a4a2cee)\"\n",
        );
        assert!(t.dark);
        assert_eq!(t.accent, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(t.green, Theme::default().green);
    }
}

/// Hue in degrees and HSL saturation of a colour.
fn hue_saturation([red, green, blue, _]: Rgba) -> (f32, f32) {
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let delta = max - min;
    if delta < 1e-6 {
        return (0.0, 0.0);
    }
    let lightness = f32::midpoint(max, min);
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(1e-6);
    let hue = if (max - red).abs() < 1e-6 {
        60.0 * ((green - blue) / delta).rem_euclid(6.0)
    } else if (max - green).abs() < 1e-6 {
        60.0 * ((blue - red) / delta + 2.0)
    } else {
        60.0 * ((red - green) / delta + 4.0)
    };
    (hue, saturation)
}

#[cfg(test)]
mod hue_tests {
    use super::*;

    #[test]
    fn nearest_hue_picks_green_and_red() {
        let mut t = Theme {
            green: [0.2, 0.7, 0.3, 1.0],
            red: [0.8, 0.2, 0.2, 1.0],
            ..Theme::default()
        };
        assert_eq!(t.nearest_hue(120.0), t.green);
        assert_eq!(t.nearest_hue(0.0), t.red);
        // A theme whose "green" is grey falls back to the nearest real hue.
        t.green = [0.5, 0.5, 0.5, 1.0];
        t.cyan = [0.1, 0.7, 0.6, 1.0];
        assert_eq!(t.nearest_hue(120.0), t.cyan);
    }
}
