//! Bounded, presentation-only miniatures in normalized document coordinates.
//! No selection, operations, matching or durable state lives here.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use crate::{Rgba, Theme};

#[derive(Default, serde::Serialize)]
pub struct Miniature {
    pub shapes: Vec<Mark>,
}

#[derive(serde::Serialize)]
pub struct Mark {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub color: String,
    pub line: bool,
}

impl Miniature {
    pub fn rect(&mut self, bounds: [f64; 4], color: Rgba) {
        self.mark(bounds, color, false);
    }

    pub fn line(&mut self, endpoints: [f64; 4], color: Rgba) {
        self.mark(endpoints, color, true);
    }

    fn mark(&mut self, b: [f64; 4], color: Rgba, line: bool) {
        if b.iter().all(|v| v.is_finite()) {
            self.shapes.push(Mark {
                x: b[0],
                y: b[1],
                w: b[2],
                h: b[3],
                color: crate::svg::color(&color),
                line,
            });
        }
    }

    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{\"shapes\":[]}".into())
    }
}

/// Cache only miniature content; viewport motion never invalidates it.
#[derive(Default)]
pub struct Cache {
    key: String,
    content: String,
}

impl Cache {
    pub fn get(&mut self, key: String, build: impl FnOnce() -> Miniature) -> String {
        if self.key != key || self.content.is_empty() {
            self.content = build().json();
            self.key = key;
        }
        self.content.clone()
    }
}

/// Whole alignment sampled on a bounded grid, using the detail view's palette.
pub fn alignment(doc: &basecraft_docs::AlignmentDoc, theme: &Theme) -> Miniature {
    let mut out = Miniature::default();
    let (rows, cols) = (doc.row_count(), doc.width());
    let (nr, nc) = (rows.min(32), cols.min(512));
    let mut one = Vec::with_capacity(1);
    for r in 0..nr {
        for c in 0..nc {
            let col = c * cols / nc;
            one.clear();
            doc.read_into(r * rows / nr, col, col + 1, &mut one);
            if let Some(&b) = one
                .first()
                .filter(|&&b| !basecraft_docs::alignment::is_gap(b))
            {
                let color = if doc.is_protein() {
                    theme.amino(b)
                } else {
                    theme.residue(b)
                };
                out.rect(
                    [
                        c as f64 / nc as f64,
                        r as f64 / nr as f64,
                        1.0 / nc as f64,
                        0.85 / nr as f64,
                    ],
                    color,
                );
            }
        }
    }
    out
}

/// Features retain their real segments, including separate origin-crossing pieces.
pub fn sequence(doc: &basecraft_docs::SequenceDoc, theme: &Theme) -> Miniature {
    let mut out = Miniature::default();
    if doc.is_empty() {
        return out;
    }
    out.rect([0.0, 0.48, 1.0, 0.04], theme.foreground_muted);
    let stride = doc.feature_count().div_ceil(4096).max(1);
    for (_, f) in doc.features().step_by(stride) {
        for s in f.location.segments.iter().take(128) {
            if out.shapes.len() >= 8192 {
                return out;
            }
            let start = s.start.min(doc.len());
            let end = s.end.min(doc.len());
            if end > start {
                let y = if s.strand == basecraft_protocol::Strand::Reverse {
                    0.58
                } else {
                    0.12
                };
                out.rect(
                    [
                        start as f64 / doc.len() as f64,
                        y,
                        (end - start) as f64 / doc.len() as f64,
                        0.28,
                    ],
                    theme.feature(&f.kind),
                );
            }
        }
    }
    out
}

/// Whole native read. A bounded set of sample points; never scans the signal on pan.
pub fn trace(trace: &basecraft_io::abif::Trace, theme: &Theme) -> Miniature {
    let mut out = Miniature::default();
    let n = trace.samples();
    if n < 2 {
        return out;
    }
    let points = n.min(1024);
    let max = (0..points)
        .flat_map(|i| {
            let sample = i * (n - 1) / (points - 1);
            trace.channels.iter().map(move |c| c[sample])
        })
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    for (channel, color) in trace
        .channels
        .iter()
        .zip(crate::sanger::signal_colors(theme))
    {
        for i in 1..points {
            let a = (i - 1) * (n - 1) / (points - 1);
            let b = i * (n - 1) / (points - 1);
            out.line(
                [
                    (i - 1) as f64 / (points - 1) as f64,
                    0.95 - channel[a].max(0) as f64 / max * 0.9,
                    i as f64 / (points - 1) as f64,
                    0.95 - channel[b].max(0) as f64 / max * 0.9,
                ],
                color,
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_reuses_content_until_source_or_palette_changes() {
        let mut cache = Cache::default();
        let content = cache.get("document:1:light".into(), Miniature::default);
        assert_eq!(
            content,
            cache.get("document:1:light".into(), || panic!(
                "pan must reuse content"
            ))
        );
        let changed = cache.get("document:1:dark".into(), || {
            let mut result = Miniature::default();
            result.rect([0.0, 0.0, 1.0, 1.0], Theme::default().foreground);
            result
        });
        assert_ne!(content, changed);
    }

    #[test]
    fn trace_sampling_has_fixed_bound_and_handles_empty_signal() {
        let mut trace = basecraft_io::abif::Trace {
            channels: std::array::from_fn(|_| vec![10; 100_000]),
            call_sets: vec![],
            channel_order: *b"ACGT",
        };
        let result = super::trace(&trace, &Theme::default());
        assert_eq!(result.shapes.len(), 4 * 1023);
        assert!(
            result
                .shapes
                .iter()
                .all(|m| [m.x, m.y, m.w, m.h].iter().all(|v| v.is_finite()))
        );
        trace.channels = std::array::from_fn(|_| vec![]);
        assert!(super::trace(&trace, &Theme::default()).shapes.is_empty());
    }
}
