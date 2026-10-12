//! Editable SVG from the same quad geometry used by the screen renderer.
//! No raster images or glyph outlines are embedded.
// SVG matrix coefficients use the standard a–f names. Exact corner equality
// distinguishes rectangles emitted by Frame::rect from arbitrary polygons.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::float_cmp
)]
use crate::frame::{ATLAS_COLS, ATLAS_ROWS, FLOATS_PER_INSTANCE, Frame, Rgba};
use std::fmt::Write as _;

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn color(c: &[f32]) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (c[0].clamp(0., 1.) * 255.).round() as u8,
        (c[1].clamp(0., 1.) * 255.).round() as u8,
        (c[2].clamp(0., 1.) * 255.).round() as u8
    )
}

/// Export the visible frame, including its theme and font.
/// A single viewport clip matches the screen's bounds.
pub fn frame_svg(frame: &Frame, width: f32, height: f32, font: &str, background: Rgba) -> String {
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" font-family=\"{}\">\n<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>\n<defs><clipPath id=\"viewport\"><rect width=\"{width}\" height=\"{height}\"/></clipPath></defs>\n<g id=\"view\" clip-path=\"url(#viewport)\">\n",
        escape(font),
        color(&background)
    );
    let mut quads = frame
        .floats()
        .chunks_exact(FLOATS_PER_INSTANCE)
        .enumerate()
        .peekable();
    while let Some((i, q)) = quads.next() {
        let xs = [q[0], q[2], q[4], q[6]];
        let ys = [q[1], q[3], q[5], q[7]];
        if xs.iter().all(|&x| x >= width)
            || xs.iter().all(|&x| x <= 0.)
            || ys.iter().all(|&y| y >= height)
            || ys.iter().all(|&y| y <= 0.)
        {
            continue;
        }
        let fill = color(&q[8..12]);
        let opacity = q[11];
        let (a, b, c, d, e, f) = (
            q[2] - q[0],
            q[3] - q[1],
            q[4] - q[0],
            q[5] - q[1],
            q[0],
            q[1],
        );
        if q[14] > 0. {
            #[allow(clippy::cast_precision_loss)]
            let code = 32
                + (q[12] * ATLAS_COLS as f32).round() as u8
                + (q[13] * ATLAS_ROWS as f32).round() as u8 * 16;
            if code == 127 {
                let _ = writeln!(
                    svg,
                    "<ellipse transform=\"matrix({a} {b} {c} {d} {e} {f})\" cx=\"0.5\" cy=\"0.5\" rx=\"0.47\" ry=\"0.47\" fill=\"{fill}\" opacity=\"{opacity}\"/>"
                );
            } else {
                let mut label = char::from(code).to_string();
                let mut positions = String::from("0.5");
                let mut last_x = e;
                // Reassemble horizontal text runs. Residues with different colours
                // remain separate, while labels become editable words rather than letters.
                loop {
                    if a <= 0. || b != 0. || c != 0. {
                        break;
                    }
                    let Some((_, next)) = quads.peek() else { break };
                    if next[14] <= 0.
                        || next[8..12] != q[8..12]
                        || next[1] != f
                        || (next[2] - next[0] - a).abs() > 0.01
                        || (next[5] - next[1] - d).abs() > 0.01
                    {
                        break;
                    }
                    let gap = (next[0] - last_x) / (a * 0.5);
                    if !(0.99..=4.01).contains(&gap) || (gap - gap.round()).abs() > 0.01 {
                        break;
                    }
                    #[allow(clippy::cast_precision_loss)]
                    let next_code = 32
                        + (next[12] * ATLAS_COLS as f32).round() as u8
                        + (next[13] * ATLAS_ROWS as f32).round() as u8 * 16;
                    if next_code == 127 {
                        break;
                    }
                    for _ in 1..gap.round() as usize {
                        label.push(' ');
                        last_x += a * 0.5;
                        let _ = write!(positions, " {}", (last_x - e) / a + 0.5);
                    }
                    label.push(char::from(next_code));
                    last_x = next[0];
                    let _ = write!(positions, " {}", (last_x - e) / a + 0.5);
                    quads.next();
                }
                let text = escape(&label);
                let _ = writeln!(
                    svg,
                    "<text transform=\"matrix({a} {b} {c} {d} {e} {f})\" x=\"{positions}\" y=\"0.5\" xml:space=\"preserve\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"0.8\" fill=\"{fill}\" opacity=\"{opacity}\">{text}</text>"
                );
            }
        } else {
            let paint = if q[14] < 0. {
                let _ = writeln!(
                    svg,
                    "<defs><linearGradient id=\"g{i}\"><stop offset=\"0\" stop-color=\"{fill}\" stop-opacity=\"{}\"/><stop offset=\"1\" stop-color=\"{fill}\" stop-opacity=\"{}\"/></linearGradient></defs>",
                    q[12], q[13]
                );
                format!("url(#g{i})")
            } else {
                fill
            };
            if q[1] == q[3] && q[0] == q[4] && q[2] == q[6] && q[5] == q[7] {
                let _ = writeln!(
                    svg,
                    "<rect x=\"{e}\" y=\"{f}\" width=\"{a}\" height=\"{d}\" fill=\"{paint}\" opacity=\"{opacity}\"/>"
                );
            } else {
                let _ = writeln!(
                    svg,
                    "<polygon points=\"{},{} {},{} {},{} {},{}\" fill=\"{paint}\" opacity=\"{opacity}\"/>",
                    q[0], q[1], q[2], q[3], q[6], q[7], q[4], q[5]
                );
            }
        }
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editable_text_and_geometry_escape_xml() {
        let mut frame = Frame::new();
        frame.text(10., 20., 12., "A<&", [0., 0., 0., 1.]);
        frame.rect(1., 2., 3., 4., [1., 0., 0., 0.5]);
        frame.disc(10., 10., 3., [0., 0., 1., 1.]);
        let svg = frame_svg(&frame, 100., 80., "Test & Font", [1.; 4]);
        assert!(svg.contains("Test &amp; Font"));
        assert!(svg.contains("&lt;"));
        assert!(svg.contains("<ellipse"));
        assert!(svg.contains("width=\"3\" height=\"4\""));
        assert!(!svg.contains("<image"));
        assert!(svg.contains(">A&lt;&amp;</text>"));
    }
}
