//! Original-sample-spacing chromatogram view. Rendering never alters calls.
// Signal arrays are bounded by the ABIF reader; display coordinates and
// viewport indices are clamped before conversion to GPU floats and indices.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use basecraft_io::abif::Trace;

use crate::frame::{Frame, Rgba};
use crate::theme::Theme;

const HEADER: f32 = 0.0;
const FOOTER: f32 = 32.0;

#[derive(Clone, Debug)]
pub struct SangerView {
    pub width: f32,
    pub height: f32,
    pub start: f64,
    pub samples_per_pixel: f64,
    pub selected: usize,
}

impl Default for SangerView {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 500.0,
            start: 0.0,
            samples_per_pixel: 1.0,
            selected: 0,
        }
    }
}

impl SangerView {
    pub fn fit(&mut self, trace: &Trace) {
        self.start = 0.0;
        self.samples_per_pixel =
            (trace.samples() as f64 / f64::from(self.width.max(1.0))).max(0.25);
    }

    pub fn fit_wrapped(&mut self, trace: &Trace) {
        self.fit(trace);
        self.samples_per_pixel = (self.samples_per_pixel / self.strip_count() as f64).max(0.25);
    }

    pub fn center_call(&mut self, trace: &Trace, index: usize) {
        let calls = trace.display_calls();
        self.selected = index.min(calls.bases.len().saturating_sub(1));
        let sample = f64::from(calls.positions[self.selected]);
        self.start = sample - f64::from(self.width) * self.samples_per_pixel * 0.5;
        self.clamp(trace);
    }

    fn clamp(&mut self, trace: &Trace) {
        let total = trace.samples() as f64;
        let span = f64::from(self.width.max(1.0)) * self.samples_per_pixel;
        self.start = self.start.clamp(0.0, (total - span).max(0.0));
    }

    pub fn zoom(&mut self, trace: &Trace, x: f32, factor: f64) {
        let mut axis = crate::zoom::Axis {
            start: self.start,
            per_px: self.samples_per_pixel,
        };
        axis.zoom_at(f64::from(x), factor, 0.25, 512.0);
        (self.start, self.samples_per_pixel) = (axis.start, axis.per_px);
        self.clamp(trace);
    }

    pub fn pan(&mut self, trace: &Trace, pixels: f32) {
        self.start -= f64::from(pixels) * self.samples_per_pixel;
        self.clamp(trace);
    }

    pub fn select_at(&mut self, trace: &Trace, x: f32) {
        let pos = (self.start + f64::from(x) * self.samples_per_pixel).max(0.0) as u32;
        let positions = &trace.display_calls().positions;
        let at = positions.partition_point(|&sample| sample < pos);
        let index = match at {
            0 => 0,
            n if n >= positions.len() => positions.len() - 1,
            n => {
                if pos - positions[n - 1] <= positions[n] - pos {
                    n - 1
                } else {
                    n
                }
            }
        };
        self.selected = index;
    }

    pub fn strip_count(&self) -> usize {
        (self.height / 80.0).ceil().max(1.0) as usize
    }

    fn strip_height(&self) -> f32 {
        self.height / self.strip_count() as f32
    }

    pub fn wrapped_scroll_size(&self, trace: &Trace) -> f64 {
        (f64::from(self.width) * self.samples_per_pixel * self.strip_count() as f64
            / trace.samples().max(1) as f64)
            .min(1.0)
    }

    pub fn scroll_wrapped(&mut self, trace: &Trace, fraction: f64) {
        let total = trace.samples() as f64;
        let limit = total * (1.0 - self.wrapped_scroll_size(trace));
        self.start = (fraction * total).clamp(0.0, limit.max(0.0));
    }

    pub fn select_wrapped(&mut self, trace: &Trace, x: f32, y: f32) {
        let strip = (y / self.strip_height()).floor().max(0.0) as usize;
        let mut lane = self.clone();
        lane.start += strip.min(self.strip_count() - 1) as f64
            * f64::from(self.width)
            * self.samples_per_pixel;
        lane.select_at(trace, x);
        self.selected = lane.selected;
    }

    pub fn build_wrapped(&self, trace: &Trace, theme: &Theme, frame: &mut Frame) {
        frame.clear();
        frame.rect(0.0, 0.0, self.width, self.height, theme.background);
        let mut lane = self.clone();
        lane.height = (self.strip_height() - 4.0).max(40.0);
        let mut strip_frame = Frame::new();
        for strip in 0..self.strip_count() {
            lane.start = self.start + strip as f64 * f64::from(self.width) * self.samples_per_pixel;
            if lane.start >= trace.samples() as f64 {
                break;
            }
            lane.build(trace, theme, &mut strip_frame);
            frame.append_translated(&strip_frame, 0.0, strip as f32 * self.strip_height());
        }
    }

    pub fn build(&self, trace: &Trace, theme: &Theme, frame: &mut Frame) {
        frame.clear();
        let w = self.width.max(1.0);
        let h = self.height.max(1.0);
        frame.rect(0.0, 0.0, w, h, theme.background);
        frame.rect(0.0, HEADER, w, h - HEADER - FOOTER, theme.surface);
        let colors = signal_colors(theme);
        let signal_h = (h - HEADER - FOOTER - 12.0).max(1.0);
        let bottom = HEADER + signal_h;
        let first = self.start.max(0.0) as usize;
        let last = ((self.start + f64::from(w) * self.samples_per_pixel).ceil() as usize)
            .min(trace.samples());
        if first >= last {
            return;
        }
        let maxval = trace
            .channels
            .iter()
            .flat_map(|channel| &channel[first..last])
            .copied()
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        for (channel, color) in trace.channels.iter().zip(colors) {
            if self.samples_per_pixel <= 1.0 {
                for sample in first..last.saturating_sub(1) {
                    let x0 = ((sample as f64 - self.start) / self.samples_per_pixel) as f32;
                    let x1 = ((sample as f64 + 1.0 - self.start) / self.samples_per_pixel) as f32;
                    let y0 = bottom - channel[sample].max(0) as f32 / maxval * signal_h;
                    let y1 = bottom - channel[sample + 1].max(0) as f32 / maxval * signal_h;
                    signal_segment(frame, x0, y0, x1, y1, color);
                }
            } else {
                let mut previous = None;
                for x in 0..w.ceil() as usize {
                    let a = (self.start + x as f64 * self.samples_per_pixel)
                        .floor()
                        .max(0.0) as usize;
                    if a >= trace.samples() {
                        break;
                    }
                    let b = ((self.start + (x + 1) as f64 * self.samples_per_pixel).ceil()
                        as usize)
                        .max(a + 1)
                        .min(trace.samples());
                    let lo = channel[a..b].iter().copied().min().unwrap_or(0).max(0) as f32;
                    let hi = channel[a..b].iter().copied().max().unwrap_or(0).max(0) as f32;
                    let y0 = bottom - hi / maxval * signal_h;
                    let y1 = bottom - lo / maxval * signal_h;
                    frame.rect(x as f32, y0, 1.0, (y1 - y0).max(1.0), color);
                    if let Some(y) = previous {
                        signal_segment(frame, x as f32 - 1.0, y, x as f32, y0, color);
                    }
                    previous = Some(y0);
                }
            }
        }
        let calls = trace.display_calls();
        let at = calls
            .positions
            .partition_point(|&sample| f64::from(sample) < self.start);
        let end = calls.positions.partition_point(|&sample| {
            f64::from(sample) < self.start + f64::from(w) * self.samples_per_pixel
        });
        for i in at..end {
            let x = ((f64::from(calls.positions[i]) - self.start) / self.samples_per_pixel) as f32;
            let color = match calls.bases[i] {
                b'A' => colors[0],
                b'C' => colors[1],
                b'G' => colors[2],
                b'T' => colors[3],
                _ => theme.foreground_muted,
            };
            if i == self.selected {
                frame.rect(x - 1.0, HEADER, 2.0, signal_h, theme.accent);
            }
            frame.rect(x, bottom + 3.0, 1.0, 7.0, theme.foreground_muted);
            if self.samples_per_pixel <= 8.0 {
                frame.glyph(x - 5.0, bottom + 13.0, 15.0, calls.bases[i], color);
            }
        }
    }
}

fn signal_segment(frame: &mut Frame, x0: f32, y0: f32, x1: f32, y1: f32, color: Rgba) {
    let length = (x1 - x0).hypot(y1 - y0).max(0.001);
    let dx = -(y1 - y0) / length * 0.7;
    let dy = (x1 - x0) / length * 0.7;
    frame.quad(
        [
            [x0 + dx, y0 + dy],
            [x1 + dx, y1 + dy],
            [x0 - dx, y0 - dy],
            [x1 - dx, y1 - dy],
        ],
        color,
        [0.0; 4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_io::abif::CallSet;

    #[test]
    fn selection_and_zoom_keep_original_sample_coordinates() {
        let trace = Trace {
            channels: std::array::from_fn(|_| vec![0; 100]),
            call_sets: vec![CallSet {
                number: 1,
                bases: b"ACG".to_vec(),
                positions: vec![10, 40, 90],
                qualities: None,
            }],
            channel_order: *b"ACGT",
        };
        let mut view = SangerView {
            width: 100.0,
            samples_per_pixel: 1.0,
            ..SangerView::default()
        };
        view.select_at(&trace, 39.0);
        assert_eq!(view.selected, 1);
        view.zoom(&trace, 40.0, 2.0);
        assert_eq!(view.start, 20.0);
        assert_eq!(trace.display_calls().positions[view.selected], 40);
    }
    #[test]
    fn wrapped_selection_and_scroll_cover_original_samples() {
        let trace = Trace {
            channels: std::array::from_fn(|_| vec![0; 1000]),
            call_sets: vec![CallSet {
                number: 1,
                bases: b"ACG".to_vec(),
                positions: vec![10, 140, 290],
                qualities: None,
            }],
            channel_order: *b"ACGT",
        };
        let mut view = SangerView {
            width: 100.0,
            height: 600.0,
            samples_per_pixel: 1.0,
            ..SangerView::default()
        };
        view.select_wrapped(&trace, 40.0, 90.0);
        assert_eq!(view.selected, 1);
        assert!((view.wrapped_scroll_size(&trace) - 0.8).abs() < 0.0001);
        view.scroll_wrapped(&trace, 1.0);
        assert!((view.start - 200.0).abs() < 0.0001);
        view.fit_wrapped(&trace);
        assert!((view.wrapped_scroll_size(&trace) - 1.0).abs() < 0.0001);
    }

    #[test]
    fn native_samples_draw_connected_signal_segments() {
        let trace = Trace {
            channels: std::array::from_fn(|_| vec![0, 10, 20, 10, 0]),
            call_sets: vec![CallSet {
                number: 1,
                bases: b"A".to_vec(),
                positions: vec![2],
                qualities: None,
            }],
            channel_order: *b"ACGT",
        };
        let view = SangerView {
            width: 20.0,
            samples_per_pixel: 0.5,
            ..SangerView::default()
        };
        let mut frame = Frame::new();
        view.build(&trace, &Theme::default(), &mut frame);
        let segments = frame
            .floats()
            .as_chunks::<{ crate::frame::FLOATS_PER_INSTANCE }>()
            .0
            .iter()
            .filter(|q| {
                q[14] == 0.0
                    && (q[0] - q[2]).abs() > f32::EPSILON
                    && (q[1] - q[3]).abs() > f32::EPSILON
            })
            .count();
        assert_eq!(segments, 16);
    }
}

/// Palette shared by detail traces, review lanes and miniatures.
pub(crate) fn signal_colors(theme: &Theme) -> [Rgba; 4] {
    (*b"ACGT").map(|base| crate::sanger_review::base_color(base, theme))
}
