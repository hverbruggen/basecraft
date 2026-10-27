//! PNG is a rasterization of the editable export SVG, never a second layout.
//!
//! The usvg tree is private to rendering: serializing it would outline text and
//! destroy the editability of the original SVG.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use resvg::{tiny_skia, usvg};
use usvg::fontdb::{Family, Query};

const MAX_SIDE: u32 = 10_000;
const MAX_PIXELS: u64 = 40_000_000;

fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        // Includes user-installed fonts and Fontconfig's generic family aliases.
        db.load_system_fonts();
        // Some Fontconfig aliases name fonts that are not installed. Resolve
        // those generics to installed faces instead of silently dropping text.
        for (generic, candidates) in [
            (Family::Serif, ["DejaVu Serif", "Liberation Serif"]),
            (Family::SansSerif, ["DejaVu Sans", "Liberation Sans"]),
            (Family::Monospace, ["DejaVu Sans Mono", "Liberation Mono"]),
        ] {
            if db
                .query(&Query {
                    families: &[generic],
                    ..Query::default()
                })
                .is_some()
            {
                continue;
            }
            let family = candidates
                .into_iter()
                .find(|name| {
                    db.query(&Query {
                        families: &[Family::Name(name)],
                        ..Query::default()
                    })
                    .is_some()
                })
                .map(str::to_owned)
                .or_else(|| db.faces().next()?.families.first().map(|f| f.0.clone()));
            if let Some(family) = family {
                match generic {
                    Family::Serif => db.set_serif_family(family),
                    Family::SansSerif => db.set_sans_serif_family(family),
                    Family::Monospace => db.set_monospace_family(family),
                    _ => {}
                }
            }
        }
        Arc::new(db)
    }))
}

fn checked_width(width: f64) -> Result<u32, String> {
    if !width.is_finite() || width.fract() != 0.0 || !(1.0..=f64::from(MAX_SIDE)).contains(&width) {
        return Err(format!(
            "PNG width must be a whole number from 1 to {MAX_SIDE} pixels"
        ));
    }
    // Finite, positive integer bounded by MAX_SIDE above.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok(width as u32)
}

fn dimensions(width: u32, size: usvg::Size) -> Result<(u32, u32), String> {
    let height = (f64::from(size.height()) * f64::from(width) / f64::from(size.width())).round();
    if !height.is_finite() || !(1.0..=f64::from(MAX_SIDE)).contains(&height) {
        return Err(format!(
            "PNG height must be from 1 to {MAX_SIDE} pixels; choose a different width or figure region"
        ));
    }
    // Checked before conversion and before any output pixel allocation.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let height = height as u32;
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| (pixels <= MAX_PIXELS).then_some(pixels)?.checked_mul(4));
    if bytes
        .and_then(|bytes| usize::try_from(bytes).ok())
        .is_none()
    {
        return Err(
            "PNG exceeds 40 million pixels; reduce its width or choose a smaller figure region"
                .into(),
        );
    }
    Ok((width, height))
}

fn rasterize(svg: &str, width: f64) -> Result<tiny_skia::Pixmap, String> {
    let width = checked_width(width)?;
    let missing_font = AtomicBool::new(false);
    let missing_glyph = AtomicU32::new(0);
    let select_font = usvg::FontResolver::default_font_selector();
    let select_fallback = usvg::FontResolver::default_fallback_selector();
    let options = usvg::Options {
        font_family: "sans-serif".into(),
        fontdb: fonts(),
        font_resolver: usvg::FontResolver {
            select_font: Box::new(|font, db| {
                let result = select_font(font, db);
                if result.is_none() {
                    missing_font.store(true, Ordering::Relaxed);
                }
                result
            }),
            select_fallback: Box::new(|character, excluded, db| {
                let result = select_fallback(character, excluded, db);
                if result.is_none() {
                    missing_glyph.store(u32::from(character), Ordering::Relaxed);
                }
                result
            }),
        },
        // Figures are self-contained vectors; never read an external image.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|e| format!("Cannot render figure SVG: {e}"))?;
    if missing_font.load(Ordering::Relaxed) {
        return Err("Cannot render figure text: install the figure font or a system fallback font, then reopen Basecraft".into());
    }
    let glyph = missing_glyph.load(Ordering::Relaxed);
    if glyph != 0 {
        return Err(format!(
            "Cannot render figure character U+{glyph:04X}: install a font supporting it, then reopen Basecraft"
        ));
    }
    let (width, height) = dimensions(width, tree.size())?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or("Could not allocate PNG image; reduce the export width")?;
    #[allow(clippy::cast_precision_loss)] // Both dimensions are bounded by 10,000.
    let scale = width as f32 / tree.size().width();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

/// Return complete encoded bytes before the caller opens its atomic export writer.
pub(crate) fn render(svg: &str, width: f64) -> Result<Vec<u8>, String> {
    rasterize(svg, width)?
        .encode_png()
        .map_err(|e| format!("Could not encode PNG: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect x="10" y="10" width="40" height="20" fill="#c02030"/></svg>"##;

    #[test]
    fn proportional_size_and_alpha() {
        for width in [1500, 320] {
            let png = render(RECT, f64::from(width)).unwrap();
            let image = tiny_skia::Pixmap::decode_png(&png).unwrap();
            assert_eq!((image.width(), image.height()), (width, width / 2));
            assert_eq!(image.pixel(0, 0).unwrap().alpha(), 0);
            let pixel = image.pixel(width / 4, width / 4).unwrap();
            assert_eq!(
                (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()),
                (192, 32, 48, 255)
            );
        }
    }

    #[test]
    fn invalid_width_and_output_bounds() {
        for width in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.0,
            0.0,
            0.5,
            1500.5,
            10001.0,
            f64::MAX,
        ] {
            assert!(render(RECT, width).is_err(), "{width}");
        }
        for (w, h, output) in [(1, 10000, 1500.0), (10000, 1, 1.0), (100, 100, 10000.0)] {
            let svg =
                format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"/>"#);
            assert!(render(&svg, output).is_err());
        }
        assert!(render("not SVG", 1500.0).is_err());
        assert_eq!(
            dimensions(8000, usvg::Size::from_wh(8.0, 5.0).unwrap()).unwrap(),
            (8000, 5000)
        );
    }

    #[test]
    fn fractional_transformed_text_matches_normal_text() {
        let make = |text: &str| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60">{text}</svg>"#
            )
        };
        let fractional = make(
            r#"<text font-family="DejaVu Sans Mono" font-size="0.8" transform="matrix(30 0 0 30 10 40)">Aβé</text>"#,
        );
        let normal =
            make(r#"<text font-family="DejaVu Sans Mono" font-size="24" x="10" y="40">Aβé</text>"#);
        let a = rasterize(&fractional, 1500.0).unwrap();
        let b = rasterize(&normal, 1500.0).unwrap();
        let ink = a.pixels().iter().filter(|p| p.alpha() > 0).count();
        assert!(ink > 1000, "text must be visible");
        // Floating-point transformation can move an antialiased boundary a
        // fraction of a pixel. Compare coverage, not PNG compression bytes.
        let difference: u64 = a
            .data()
            .iter()
            .zip(b.data())
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        assert!(
            difference < 10000,
            "equivalent text geometry diverged: {difference}"
        );
        assert!(fractional.contains("<text"));
        assert!(!fractional.contains("<path"));
    }

    #[test]
    fn clipping_strokes_rotation_multiline_and_unicode() {
        let svg = include_str!("../tests/fixtures/figure-raster.svg");
        let image = rasterize(svg, 600.0).unwrap();
        assert_eq!((image.width(), image.height()), (600, 360));
        assert_eq!(image.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(image.pixel(50, 50).unwrap().alpha(), 128);
        assert_eq!(
            image.pixel(150, 50).unwrap().alpha(),
            0,
            "clip must exclude rectangle"
        );
        assert!(image.pixels().iter().filter(|p| p.alpha() > 0).count() > 3000);
        // Optional review artifacts from the exact production renderer.
        if let Some(folder) = std::env::var_os("BASECRAFT_FIGURE_EVIDENCE") {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                std::path::Path::new(&folder).join("figure-resvg.png"),
                image.encode_png().unwrap(),
            )
            .unwrap();
            for (label, text) in [
                (
                    "fractional",
                    r#"<text font-family="DejaVu Sans Mono" font-size="0.8" transform="matrix(30 0 0 30 10 40)">Aβé</text>"#,
                ),
                (
                    "normal",
                    r#"<text font-family="DejaVu Sans Mono" font-size="24" x="10" y="40">Aβé</text>"#,
                ),
            ] {
                let xml = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60">{text}</svg>"#
                );
                std::fs::write(
                    std::path::Path::new(&folder).join(format!("{label}-resvg.png")),
                    render(&xml, 1500.0).unwrap(),
                )
                .unwrap();
            }
        }
    }
}
