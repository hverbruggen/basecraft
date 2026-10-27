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
use crate::frame::{ATLAS_COLS, ATLAS_ROWS, FLOATS_PER_INSTANCE, Frame, Rgba, TextRun};
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

fn source_text(svg: &mut String, run: &TextRun) {
    let text = escape(&run.text);
    let fill = color(&run.color);
    let size = run.size * 0.8;
    // Frame::text advances by UTF-8 byte, so keep that allocated width.
    // Character-count width would shift the following atlas geometry.
    #[allow(clippy::cast_precision_loss)]
    let length = run.text.len() as f32 * run.size * 0.5;
    let opacity = run.color[3];
    if run.vertical {
        let _ = writeln!(
            svg,
            "<text transform=\"translate({} {}) rotate(90)\" x=\"0\" y=\"{}\" font-size=\"{size}\" textLength=\"{length}\" lengthAdjust=\"spacing\" xml:space=\"preserve\" fill=\"{fill}\" opacity=\"{opacity}\">{text}</text>",
            run.x + run.size,
            run.y,
            run.size * 0.5,
        );
    } else {
        let _ = writeln!(
            svg,
            "<text x=\"{}\" y=\"{}\" font-size=\"{size}\" textLength=\"{length}\" lengthAdjust=\"spacing\" dominant-baseline=\"central\" xml:space=\"preserve\" fill=\"{fill}\" opacity=\"{opacity}\">{text}</text>",
            run.x,
            run.y + run.size * 0.5,
        );
    }
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
    let mut runs = frame.text_runs().iter().peekable();
    while let Some((i, q)) = quads.next() {
        let mut replaced = false;
        while runs.peek().is_some_and(|run| run.start == i) {
            let Some(run) = runs.next() else { break };
            source_text(&mut svg, run);
            if run.end > i {
                for _ in i + 1..run.end {
                    quads.next();
                }
                replaced = true;
            }
        }
        if replaced {
            continue;
        }
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
                    let Some((next_index, next)) = quads.peek() else {
                        break;
                    };
                    // A retained Unicode source run replaces its own atlas
                    // quads. Let the outer loop emit that run in source order.
                    if runs.peek().is_some_and(|run| run.start == *next_index) {
                        break;
                    }
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
    for run in runs {
        source_text(&mut svg, run);
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

    #[test]
    fn unicode_text_survives_atlas_and_translated_frames() {
        let mut strip = Frame::new();
        strip.text(10., 20., 12., "Aé&北", [0., 0., 0., 1.]);
        strip.text_down(30., 40., 12., "β", [0., 0., 0., 1.]);
        let mut frame = Frame::new();
        frame.append_translated(&strip, 5., 7.);
        let svg = frame_svg(&frame, 200., 100., "Test", [1.; 4]);
        assert!(svg.contains("Aé&amp;北"));
        assert!(svg.contains(">β</text>"));
        assert!(svg.contains("x=\"15\" y=\"33\""));
        assert!(svg.contains("translate(47 47) rotate(90)"));
        assert_eq!(svg.matches("<text ").count(), 2);
        assert!(!svg.contains("<image"));
    }

    #[test]
    fn adjacent_ascii_and_mixed_text_keep_separate_editable_runs() {
        let mut frame = Frame::new();
        frame.text(10., 20., 10., "A", [0., 0., 0., 1.]);
        frame.text(15., 20., 10., "Bé", [0., 0., 0., 1.]);
        let svg = frame_svg(&frame, 100., 50., "Test", [1.; 4]);
        assert_eq!(svg.matches("<text ").count(), 2);
        let ascii = svg.find(">A</text>").expect("first ASCII run");
        let mixed = svg.find(">Bé</text>").expect("retained mixed run");
        assert!(ascii < mixed);
    }

    #[test]
    fn zero_quad_unicode_run_keeps_its_position_between_text_calls() {
        let mut frame = Frame::new();
        frame.text(10., 20., 10., "A", [0., 0., 0., 1.]);
        frame.text(15., 20., 10., "β", [0., 0., 0., 1.]);
        frame.text(25., 20., 10., "Bé", [0., 0., 0., 1.]);
        let svg = frame_svg(&frame, 100., 50., "Test", [1.; 4]);
        assert_eq!(svg.matches("<text ").count(), 3);
        let ascii = svg.find(">A</text>").expect("first ASCII run");
        let unicode = svg.find(">β</text>").expect("zero-quad Unicode run");
        let mixed = svg.find(">Bé</text>").expect("last mixed run");
        assert!(ascii < unicode && unicode < mixed);
    }
}
