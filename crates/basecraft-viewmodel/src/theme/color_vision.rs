//! Screening for the optional data palette, not a claim of accessibility.
//! Machado et al. full-severity protan/deutan matrices, in linear sRGB:
//! https://www.inf.ufrgs.br/~oliveira/pubs_files/CVD_Simulation/CVD_Simulation.html
//! Okabe–Ito source colours: https://jfly.uni-koeln.de/color/
use super::{Rgba, Theme};

// Identity, protanopia, deuteranopia. No new dependency or analysis operation.
const VISION: [[[f32; 3]; 3]; 3] = [
    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    [
        [0.152_286, 1.052_583, -0.204_868],
        [0.114_503, 0.786_281, 0.099_216],
        [-0.003_882, -0.048_116, 1.051_998],
    ],
    [
        [0.367_322, 0.860_646, -0.227_968],
        [0.280_085, 0.672_501, 0.047_413],
        [-0.011_820, 0.042_940, 0.968_881],
    ],
];
fn linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn encoded(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}
fn simulate(color: Rgba, vision: usize) -> [f32; 3] {
    let rgb = [linear(color[0]), linear(color[1]), linear(color[2])];
    VISION[vision].map(|row| {
        row.into_iter()
            .zip(rgb)
            .map(|(w, c)| w * c)
            .sum::<f32>()
            .clamp(0., 1.)
    })
}
fn luminance(rgb: [f32; 3]) -> f32 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
fn surfaces(theme: &Theme) -> [Rgba; 4] {
    [
        theme.background,
        theme.surface,
        theme.surface_strong,
        theme.selection,
    ]
}
fn contrast(color: Rgba, theme: &Theme) -> f32 {
    (0..VISION.len())
        .flat_map(|vision| {
            surfaces(theme).map(|surface| {
                let a = luminance(simulate(color, vision));
                let b = luminance(simulate(surface, vision));
                (a.max(b) + 0.05) / (a.min(b) + 0.05)
            })
        })
        .fold(f32::INFINITY, f32::min)
}
fn lab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let f = |v: f32| {
        if v > 216. / 24_389. {
            v.cbrt()
        } else {
            v * 841. / 108. + 4. / 29.
        }
    };
    let x = f((0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.95047);
    let y = f(0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b);
    let z = f((0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.08883);
    [116. * y - 16., 500. * (x - y), 200. * (y - z)]
}
fn separation(colors: [Rgba; 4]) -> f32 {
    (0..VISION.len())
        .flat_map(|vision| {
            let colors = colors.map(|c| lab(simulate(c, vision)));
            (0..4).flat_map(move |i| {
                (i + 1..4).map(move |j| {
                    colors[i]
                        .into_iter()
                        .zip(colors[j])
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f32>()
                        .sqrt()
                })
            })
        })
        .fold(f32::INFINITY, f32::min)
}
fn readable(color: Rgba, theme: &Theme) -> Rgba {
    // Preserve hue identities by mixing in linear light toward black/white.
    // A little headroom protects the 4.5 target when SVG species colours round.
    if contrast(color, theme) >= 4.5 {
        return color;
    }
    let rgb = [linear(color[0]), linear(color[1]), linear(color[2])];
    let end = if theme.dark { 1. } else { 0. };
    let mix = |amount| {
        let [r, g, b] = rgb.map(|c| encoded(c + (end - c) * amount));
        [r, g, b, 1.]
    };
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..24 {
        let mid = f32::midpoint(lo, hi);
        if contrast(mix(mid), theme) >= 4.6 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(hi)
}
pub(super) fn palette(theme: &Theme) -> [Rgba; 8] {
    // Stable roles: A green, C blue, G orange, T neutral, in both appearances.
    // The remaining amino/species categories retain their labels, not a promise
    // of eight uniquely distinguishable colours. User species overrides survive.
    let candidate = [
        theme.blue,
        theme.foreground,
        theme.green,
        theme.magenta,
        theme.red,
        theme.cyan,
        theme.orange,
        theme.yellow,
    ];
    let bases = [candidate[2], candidate[0], candidate[6], candidate[1]];
    // CIE76 >=20 is a conservative engineering heuristic, not a WCAG criterion.
    if bases.into_iter().all(|c| contrast(c, theme) >= 4.5) && separation(bases) >= 20. {
        return candidate.map(|c| readable(c, theme));
    }
    let byte = |r: u8, g: u8, b: u8| {
        [
            f32::from(r) / 255.,
            f32::from(g) / 255.,
            f32::from(b) / 255.,
            1.,
        ]
    };
    [
        byte(0, 114, 178),
        if theme.dark {
            [1.; 4]
        } else {
            [0., 0., 0., 1.]
        },
        byte(0, 158, 115),
        byte(204, 121, 167),
        byte(213, 94, 0),
        byte(86, 180, 233),
        byte(230, 159, 0),
        byte(240, 228, 66),
    ]
    .map(|c| readable(c, theme))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dark() -> Theme {
        Theme::from_omarchy(
            "mode = \"dark\"\nbackground = \"#171f25\"\ndark_background = \"#202b32\"\ndarker_background = \"#2b3840\"\nselection = \"#35454e\"\n",
        )
    }
    #[test]
    fn fallback_has_contrast_and_separation_in_both_appearances() {
        for theme in [Theme::default(), dark()] {
            let colors = palette(&theme);
            assert!(colors.into_iter().all(|c| contrast(c, &theme) >= 4.5));
            let bases = [colors[2], colors[0], colors[6], colors[1]];
            assert!(separation(bases) >= 20., "{}", separation(bases));
            let safe = theme.clone().with_color_blind_friendly(true);
            assert_eq!(safe.residue(b'A'), colors[2]);
            assert_eq!(safe.residue(b'C'), colors[0]);
            assert_eq!(safe.residue(b'G'), colors[6]);
            assert_eq!(safe.residue(b'T'), colors[1]);
            assert_eq!(safe.amino(b'R'), colors[1]);
            assert_eq!(safe.amino(b'D'), colors[4]);
            assert_eq!(safe.clone().with_color_blind_friendly(false), theme);
            assert_eq!(safe.foreground, theme.foreground);
        }
    }
    #[test]
    fn suitable_theme_colours_are_retained_and_confusable_ones_fall_back() {
        let mut theme = Theme::default();
        let safe = palette(&theme);
        theme.blue = safe[0];
        theme.foreground = safe[1];
        theme.green = safe[2];
        theme.orange = safe[6];
        let result = palette(&theme);
        for (actual, expected) in [result[0], result[1], result[2], result[6]]
            .into_iter()
            .zip([theme.blue, theme.foreground, theme.green, theme.orange])
        {
            assert_eq!(actual, expected);
        }
        theme.green = theme.blue;
        assert_eq!(palette(&theme), safe);
    }
}
