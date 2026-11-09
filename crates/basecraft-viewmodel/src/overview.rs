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

/// Whole native read as a bounded min/max envelope. Computed once per source
/// and palette by the controller cache, so narrow peaks survive downsampling.
pub fn trace(trace: &basecraft_io::abif::Trace, theme: &Theme) -> Miniature {
    let mut out = Miniature::default();
    let n = trace.samples();
    if n == 0 {
        return out;
    }
    let points = n.min(1024);
    let maximum = f64::from(
        trace
            .channels
            .iter()
            .flatten()
            .copied()
            .max()
            .unwrap_or(1)
            .max(1),
    );
    for (channel, color) in trace
        .channels
        .iter()
        .zip(crate::sanger::signal_colors(theme))
    {
        for i in 0..points {
            let a = i * n / points;
            let b = (i + 1) * n / points;
            let bucket = &channel[a..b];
            let low = f64::from(bucket.iter().copied().min().unwrap_or(0).max(0));
            let high = f64::from(bucket.iter().copied().max().unwrap_or(0).max(0));
            let x = (i as f64 + 0.5) / points as f64;
            out.line(
                [
                    x,
                    0.95 - low / maximum * 0.9,
                    x,
                    0.95 - high / maximum * 0.9,
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
    fn overview_match_spans_and_trace_calls_keep_native_coordinates() {
        assert!(MatchMark::span(0, 0, 20, 0.0, 1.0).is_none());
        assert!(MatchMark::span(1, 4, 3, 0.0, 1.0).is_none());
        let trace = basecraft_io::abif::Trace {
            channels: std::array::from_fn(|_| vec![0; 100]),
            call_sets: vec![basecraft_io::abif::CallSet {
                number: 1,
                bases: b"ACG".to_vec(),
                positions: vec![10, 20, 90],
                qualities: None,
            }],
            channel_order: *b"ACGT",
        };
        let mark = trace_match(&trace, 0, 2).unwrap();
        assert!((mark.x - 0.1).abs() < 1e-6);
        assert!((mark.width - 0.11).abs() < 1e-6);
        assert!(trace_match(&trace, 0, 4).is_none());
        assert!(trace_match(&trace, 3, 3).is_none());
    }

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
        assert_eq!(result.shapes.len(), 4 * 1024);
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

/// Optional Find mark, ready for DocumentOverview.matchMarks. Match identity,
/// ordering, reverse-strand semantics and current-result state stay in Find.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct MatchMark {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl MatchMark {
    /// A zero-based half-open span. Empty/missing spans have no invented mark.
    pub fn span(start: u64, end: u64, total: u64, y: f64, height: f64) -> Option<Self> {
        if total == 0
            || start >= end
            || end > total
            || !y.is_finite()
            || !height.is_finite()
            || y < 0.0
            || height <= 0.0
            || y + height > 1.000_001
        {
            return None;
        }
        Some(Self {
            x: start as f64 / total as f64,
            y,
            width: (end - start) as f64 / total as f64,
            height,
        })
    }
}

/// Find on a single trace reports display-call indices, never raw sample indices.
/// Missing positions produce no mark. Inclusive last event becomes half-open.
pub fn trace_match(trace: &basecraft_io::abif::Trace, start: u64, end: u64) -> Option<MatchMark> {
    if start >= end {
        return None;
    }
    let calls = trace.display_calls();
    let a = *calls.positions.get(usize::try_from(start).ok()?)?;
    let b = *calls
        .positions
        .get(usize::try_from(end.checked_sub(1)?).ok()?)?;
    MatchMark::span(
        u64::from(a),
        u64::from(b) + 1,
        trace.samples() as u64,
        0.0,
        1.0,
    )
}
