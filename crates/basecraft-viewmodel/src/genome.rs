//! The genome view: a sequence with its annotations, linear or circular.
//!
//! [`GenomeView`] holds the viewport and the interaction state, builds a
//! [`Frame`] for the visible region, and answers hit tests. It reads the
//! document but never edits it; edits go through operations.

// Pixel geometry mixes f32 screen coordinates with u64/f64 sequence
// coordinates; the conversions are deliberate and bounded by the viewport.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]

use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};

use basecraft_docs::{FeatureId, SequenceDoc};
use basecraft_protocol::{Feature, Strand};

use crate::frame::{Frame, Rgba, text_width};
use crate::theme::{Theme, alpha, mix};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The sequence wraps across lines, like text, scrolling vertically.
    Wrapped,
    /// One line, scrolling horizontally.
    Linear,
    Circular,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Ruler,
    Sequence,
    Features,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    Pan {
        last_x: f32,
        last_y: f32,
    },
    Select {
        anchor: u64,
    },
    /// Pressed on a feature: a click selects it; moving turns into a range
    /// selection from the press point.
    Feature {
        x: f32,
        y: f32,
    },
}

const RULER_H: f32 = 30.0;
const SEQ_Y: f32 = 38.0;
const SEQ_H: f32 = 20.0;
const LANE_H: f32 = 14.0;
const LANE_GAP: f32 = 9.0;
const LABEL_SIZE: f32 = 13.0;
/// Most pixels per base when zoomed in.
const MAX_PX_PER_BP: f64 = 28.0;
/// Zoom 100 %: bases comfortably readable.
const PX_PER_BP_AT_100: f64 = 6.1;
/// Pixels per base from which residue letters are drawn.
const LETTERS_FROM: f64 = 3.5;
/// Above this many visible features, draw their density instead.
const MAX_DRAWN_FEATURES: usize = 4000;

fn within(s: &basecraft_protocol::Segment, span: (u64, u64)) -> bool {
    s.start >= span.0 && s.end <= span.1
}

/// Bounds of the contiguous pieces a feature is drawn as on a linear view:
/// one, or two for a feature crossing the origin of a circular sequence.
fn pieces(f: &Feature, len: u64, circular: bool) -> Vec<(u64, u64)> {
    let segs = &f.location.segments;
    let mut groups: Vec<Vec<&basecraft_protocol::Segment>> = vec![Vec::new()];
    for (i, s) in segs.iter().enumerate() {
        if i > 0 && circular {
            let prev = &segs[i - 1];
            let wraps = match s.strand {
                Strand::Reverse => prev.start == 0 && s.end == len,
                _ => prev.end == len && s.start == 0,
            };
            if wraps {
                groups.push(Vec::new());
            }
        }
        if let Some(g) = groups.last_mut() {
            g.push(s);
        }
    }
    groups
        .into_iter()
        .filter(|g| !g.is_empty())
        .map(|g| {
            let start = g.iter().map(|s| s.start).min().unwrap_or(0);
            let end = g.iter().map(|s| s.end).max().unwrap_or(0);
            (start, end)
        })
        .collect()
}

fn is_transcript(kind: &str) -> bool {
    matches!(
        kind,
        "mRNA" | "transcript" | "tRNA" | "rRNA" | "ncRNA" | "lnc_RNA"
    )
}

/// Whole-sequence features that would only cover everything.
fn is_backdrop(f: &Feature) -> bool {
    matches!(f.kind.as_str(), "source" | "region" | "chromosome")
}

/// Space between wrapped lines.
const BLOCK_GAP: f32 = 16.0;
/// Most feature rows per wrapped line.
const WRAP_MAX_LANES: usize = 12;
/// Up to this many wrapped lines get exact heights; beyond, a fixed
/// estimate keeps layout cheap for chromosome-sized sequences.
const WRAP_EXACT_LINES: usize = 3000;
const WRAP_ESTIMATED_LANES: usize = 3;

/// Heights of the wrapped lines for one document version, line length and
/// width: `offsets[k]` is the top of line `k`, `offsets[n]` the total.
#[derive(Clone, Debug)]
struct WrapCache {
    version: u64,
    len: u64,
    line_bp: u64,
    width: f32,
    offsets: Vec<f32>,
}

/// Rows the density plot occupies when features are too many to draw.
const DENSE_ROWS: usize = 4;

enum FeatureLayout {
    Items {
        items: Vec<Item>,
        parts: HashMap<FeatureId, Vec<FeatureId>>,
    },
    Dense(usize),
}

/// A feature placed on a row of the linear view.
struct Item {
    id: FeatureId,
    /// The part of the feature this item draws: all of it, or one side of
    /// a feature that crosses the origin of a circular sequence.
    span: (u64, u64),
    x0: f32,
    x1: f32,
    label: String,
    label_inside: bool,
    /// Right edge including the arrowhead, for row packing.
    reach: f32,
}

#[derive(Clone, Debug)]
pub struct GenomeView {
    pub mode: Mode,
    /// Sequence coordinate at the left edge.
    pub start: f64,
    pub bp_per_px: f64,
    pub width: f32,
    pub height: f32,
    /// Insertion point, between residues: `0..=len`.
    pub cursor: Option<u64>,
    pub selection: Option<(u64, u64)>,
    /// The selection was made right to left (or is a reverse-strand
    /// feature's span); new features over it default to the reverse strand.
    pub selection_reverse: bool,
    pub selected: Option<FeatureId>,
    /// Features that didn't fit in the lanes on the last build.
    pub hidden: usize,
    drag: Drag,
    /// Feature boxes on the linear view, with the residue span each covers.
    hits: Vec<([f32; 4], FeatureId, (u64, u64))>,
    arc_hits: Vec<(f32, f32, f32, f32, FeatureId)>,
    /// Wrapped view: pixels scrolled down from the first line.
    scroll_y: f32,
    wrap: Option<WrapCache>,
    /// Wrapped view: after the next layout, put the line holding this
    /// position at this height. Used to keep the view steady across zoom
    /// and resize, when line breaks move.
    anchor: Option<(u64, f32)>,
}

impl GenomeView {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            mode: Mode::Wrapped,
            start: 0.0,
            bp_per_px: 1.0,
            width: width.max(1.0),
            height: height.max(1.0),
            cursor: None,
            selection: None,
            selection_reverse: false,
            selected: None,
            hidden: 0,
            drag: Drag::None,
            hits: Vec::new(),
            arc_hits: Vec::new(),
            scroll_y: 0.0,
            wrap: None,
            anchor: None,
        }
    }

    // Viewport.

    pub fn resize(&mut self, width: f32, height: f32, len: u64) {
        if self.mode == Mode::Wrapped {
            self.anchor_top_line();
            self.width = width.max(1.0);
            self.height = height.max(1.0);
            self.clamp(len);
            return;
        }
        let centre = self.start + f64::from(self.width) * self.bp_per_px / 2.0;
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self.start = centre - f64::from(self.width) * self.bp_per_px / 2.0;
        self.clamp(len);
    }

    pub fn fit(&mut self, len: u64) {
        self.bp_per_px = (len.max(1) as f64 / f64::from(self.width)).max(1.0 / MAX_PX_PER_BP);
        self.start = 0.0;
        self.anchor = Some((0, 0.0));
        self.clamp(len);
    }

    /// 100 % zoom from the start of the sequence: the opening view.
    pub fn reset_zoom(&mut self, len: u64) {
        self.bp_per_px = 1.0 / PX_PER_BP_AT_100;
        self.start = 0.0;
        self.scroll_y = 0.0;
        self.anchor = Some((0, 0.0));
        self.clamp(len);
    }

    fn span(&self) -> f64 {
        f64::from(self.width) * self.bp_per_px
    }

    fn clamp(&mut self, len: u64) {
        let len = len as f64;
        let max_bpp = (len / f64::from(self.width)).max(1.0 / MAX_PX_PER_BP);
        self.bp_per_px = self.bp_per_px.clamp(1.0 / MAX_PX_PER_BP, max_bpp);
        let span = self.span();
        if span >= len {
            self.start = (len - span) / 2.0;
        } else {
            let margin = span * 0.05;
            self.start = self.start.clamp(-margin, len - span + margin);
        }
    }

    /// Zoom by `factor` (> 1 zooms in), keeping the position under `x` fixed.
    pub fn zoom_at(&mut self, x: f32, factor: f64, len: u64) {
        if self.mode == Mode::Wrapped {
            // Line breaks move when zooming; keep the top line's start put.
            self.anchor_top_line();
            self.bp_per_px /= factor;
            self.clamp(len);
            return;
        }
        let anchor = self.bp_at(x);
        self.bp_per_px /= factor;
        self.clamp(len);
        self.start = anchor - f64::from(x) * self.bp_per_px;
        self.clamp(len);
    }

    /// Wrapped view: scroll down by `dy` pixels (negative scrolls up).
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll_y += dy;
        self.clamp_scroll();
    }

    pub fn pan(&mut self, dx: f32, len: u64) {
        self.start -= f64::from(dx) * self.bp_per_px;
        self.clamp(len);
    }

    /// Show `start..end` with a small margin on both sides.
    pub fn zoom_to(&mut self, start: u64, end: u64, len: u64) {
        let span = (end.saturating_sub(start)).max(1) as f64;
        let margin = span * 0.05;
        if self.mode == Mode::Circular {
            self.mode = Mode::Linear;
        }
        self.bp_per_px = (span + 2.0 * margin) / f64::from(self.width);
        self.start = start as f64 - margin;
        self.anchor = Some((start, 8.0));
        self.clamp(len);
    }

    /// Right-click: select the feature under the pointer, or move the cursor
    /// there unless the click falls inside the current selection, which a
    /// context menu then acts on.
    pub fn context_press(&mut self, x: f32, y: f32, len: u64) {
        self.drag = Drag::None;
        if let Some(id) = self.feature_at(x, y, len) {
            self.selected = Some(id);
            return;
        }
        let p = self.position_at(x, y, len);
        let inside = self.selection.is_some_and(|(a, b)| p >= a && p <= b);
        if !inside {
            self.selection = None;
            self.cursor = Some(p);
        }
        self.selected = None;
    }

    /// Zoom level for display: pixels per base when bases are wider than a
    /// pixel, else bases per pixel, plus how much sequence is shown.
    pub fn zoom_text(&self, len: u64) -> String {
        let percent = self.px_per_bp() / PX_PER_BP_AT_100 * 100.0;
        let level = if percent >= 10.0 {
            format!("{percent:.0}%")
        } else {
            // Two significant digits: 2.5%, 0.16%, 0.0016%.
            let digits = (1.0 - percent.log10().floor()).max(1.0) as usize;
            format!("{percent:.digits$}%")
        };
        let shown = match (self.mode, &self.wrap) {
            (Mode::Wrapped, Some(w)) => {
                let lines = self.visible_lines().len() as u64;
                (lines * w.line_bp).min(len)
            }
            _ => self.span().min(len as f64) as u64,
        };
        format!("{level} · {} shown", format_length(shown))
    }

    /// Scrollbar position and size, as fractions of the sequence.
    pub fn scroll_fraction(&self, len: u64) -> (f64, f64) {
        if self.mode == Mode::Wrapped {
            let total = f64::from(self.wrap_total().max(1.0));
            let size = (f64::from(self.height) / total).min(1.0);
            return (
                (f64::from(self.scroll_y) / total).clamp(0.0, 1.0 - size),
                size,
            );
        }
        let len = len.max(1) as f64;
        let size = (self.span() / len).min(1.0);
        let pos = (self.start / len).clamp(0.0, (1.0 - size).max(0.0));
        (pos, size)
    }

    pub fn scroll_to(&mut self, fraction: f64, len: u64) {
        if self.mode == Mode::Wrapped {
            self.scroll_y = (fraction * f64::from(self.wrap_total())) as f32;
            self.clamp_scroll();
            return;
        }
        self.start = fraction * len as f64;
        self.clamp(len);
    }

    /// While a selection drag is past either edge of the view, scroll that
    /// way, faster the further out the pointer is, and extend the selection.
    /// Returns whether it scrolled, so the caller keeps ticking.
    pub fn edge_scroll(&mut self, x: f32, y: f32, len: u64) -> bool {
        const EDGE: f32 = 24.0;
        if !matches!(self.drag, Drag::Select { .. }) || self.mode == Mode::Circular {
            return false;
        }
        if self.mode == Mode::Wrapped {
            let over = if y < EDGE {
                y - EDGE
            } else if y > self.height - EDGE {
                y - (self.height - EDGE)
            } else {
                0.0
            };
            if over == 0.0 {
                return false;
            }
            let before = self.scroll_y;
            self.scroll_by(over.signum() * (2.0 + over.abs() * 0.6));
            self.drag_to(x, y.clamp(0.0, self.height), len);
            return (self.scroll_y - before).abs() > f32::EPSILON;
        }
        let over = if x < EDGE {
            x - EDGE
        } else if x > self.width - EDGE {
            x - (self.width - EDGE)
        } else {
            0.0
        };
        if over == 0.0 {
            return false;
        }
        let before = self.start;
        // Pixels per tick: a slow crawl near the edge, fast far beyond it.
        self.pan(-(over.signum() * (2.0 + over.abs() * 0.6)), len);
        self.drag_to(x.clamp(0.0, self.width), y, len);
        (self.start - before).abs() > f64::EPSILON
    }

    /// Scroll so that `pos` is visible.
    pub fn reveal(&mut self, pos: u64, len: u64) {
        if self.mode == Mode::Wrapped {
            if let Some(w) = &self.wrap {
                let k = ((pos / w.line_bp) as usize).min(w.offsets.len().saturating_sub(2));
                let top = w.offsets[k] - self.scroll_y;
                let bottom = w.offsets[k + 1] - self.scroll_y;
                if top < 0.0 || bottom > self.height {
                    self.scroll_y = w.offsets[k] - self.height / 3.0;
                    self.clamp_scroll();
                }
            } else {
                self.anchor = Some((pos, self.height / 3.0));
            }
            return;
        }
        let p = pos as f64;
        let span = self.span();
        if p < self.start || p > self.start + span {
            self.start = p - span / 2.0;
            self.clamp(len);
        }
    }

    pub fn x_of(&self, bp: f64) -> f32 {
        ((bp - self.start) / self.bp_per_px) as f32
    }

    pub fn bp_at(&self, x: f32) -> f64 {
        self.start + f64::from(x) * self.bp_per_px
    }

    /// The residue boundary nearest to `x`, for placing the cursor.
    pub fn boundary_at(&self, x: f32, len: u64) -> u64 {
        self.bp_at(x).round().clamp(0.0, len as f64) as u64
    }

    fn px_per_bp(&self) -> f64 {
        1.0 / self.bp_per_px
    }

    fn letters(&self) -> bool {
        self.px_per_bp() >= LETTERS_FROM
    }

    #[allow(clippy::unused_self)] // Will depend on track settings.
    fn feature_top(&self) -> f32 {
        SEQ_Y + SEQ_H + 18.0
    }

    pub fn region_at(&self, y: f32) -> Region {
        if y < SEQ_Y - 2.0 {
            Region::Ruler
        } else if y < self.feature_top() - 6.0 {
            Region::Sequence
        } else {
            Region::Features
        }
    }

    // Circular geometry: origin at the top, clockwise.

    fn centre(&self) -> (f32, f32) {
        (self.width / 2.0, self.height / 2.0 + 8.0)
    }

    fn radius(&self) -> f32 {
        (self.width.min(self.height) * 0.30).max(40.0)
    }

    fn angle(bp: f64, len: u64) -> f32 {
        -PI / 2.0 + TAU * (bp / len.max(1) as f64) as f32
    }

    fn bp_at_angle(a: f32, len: u64) -> f64 {
        let t = ((a + PI / 2.0).rem_euclid(TAU)) / TAU;
        f64::from(t) * len as f64
    }

    // Hit tests.

    pub fn feature_at(&self, x: f32, y: f32, len: u64) -> Option<FeatureId> {
        match self.mode {
            Mode::Linear | Mode::Wrapped => self.feature_hit_at(x, y).map(|(id, _)| id),
            Mode::Circular => {
                let (cx, cy) = self.centre();
                let (dx, dy) = (x - cx, y - cy);
                let r = (dx * dx + dy * dy).sqrt();
                let bp = Self::bp_at_angle(dy.atan2(dx), len) as f32;
                self.arc_hits
                    .iter()
                    .find(|(b0, b1, r0, r1, _)| {
                        bp >= *b0 && bp <= *b1 && r >= *r0 - 2.0 && r <= *r1 + 2.0
                    })
                    .map(|h| h.4)
            }
        }
    }

    /// The feature box under a point on the linear view, with its span.
    fn feature_hit_at(&self, x: f32, y: f32) -> Option<(FeatureId, (u64, u64))> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _, _)| x >= r[0] - 2.0 && x <= r[2] + 2.0 && y >= r[1] && y <= r[3])
            .map(|(_, id, span)| (*id, *span))
    }

    fn position_at(&self, x: f32, y: f32, len: u64) -> u64 {
        match self.mode {
            Mode::Linear => self.boundary_at(x, len),
            Mode::Wrapped => {
                let Some(w) = &self.wrap else { return 0 };
                let k = self.line_at(y);
                let b0 = k as u64 * w.line_bp;
                let b1 = (b0 + w.line_bp).min(len);
                (b0 as f64 + f64::from(x) * self.bp_per_px)
                    .round()
                    .clamp(b0 as f64, b1 as f64) as u64
            }
            Mode::Circular => {
                let (cx, cy) = self.centre();
                Self::bp_at_angle((y - cy).atan2(x - cx), len)
                    .round()
                    .clamp(0.0, len as f64) as u64
            }
        }
    }

    // Input.

    /// Mouse press. Clicking a feature selects it; clicking the sequence or
    /// ruler places the cursor and starts a selection; anywhere else starts
    /// a pan.
    /// Left press. A plain click always replaces the selection: on a
    /// feature it selects the feature and its residues; anywhere else it
    /// places the cursor and starts a new range selection, which dragging
    /// extends. Shift-click extends the selection from the cursor instead.
    pub fn press(&mut self, x: f32, y: f32, shift: bool, len: u64) {
        let hit = match self.mode {
            Mode::Linear | Mode::Wrapped => self.feature_hit_at(x, y),
            Mode::Circular => self
                .feature_at(x, y, len)
                .and_then(|id| self.arc_hits.iter().find(|h| h.4 == id))
                .map(|h| (h.4, (h.0 as u64, h.1 as u64))),
        };
        if let Some((id, span)) = hit {
            self.selected = Some(id);
            self.selection = Some(span);
            self.cursor = Some(span.1);
            self.drag = Drag::Feature { x, y };
            return;
        }
        self.selected = None;
        let p = self.position_at(x, y, len);
        let anchor = match (shift, self.cursor) {
            (true, Some(c)) => c,
            _ => p,
        };
        self.cursor = Some(p);
        self.selection = (anchor != p).then(|| (anchor.min(p), anchor.max(p)));
        self.selection_reverse = p < anchor;
        self.drag = Drag::Select { anchor };
    }

    /// Start panning with the pointer (middle button).
    pub fn pan_press(&mut self, x: f32, y: f32) {
        self.drag = Drag::Pan {
            last_x: x,
            last_y: y,
        };
    }

    pub fn drag_to(&mut self, x: f32, y: f32, len: u64) {
        match self.drag {
            Drag::None => {}
            Drag::Pan { last_x, last_y } => {
                match self.mode {
                    Mode::Linear => self.pan(x - last_x, len),
                    Mode::Wrapped => self.scroll_by(last_y - y),
                    Mode::Circular => {}
                }
                self.drag = Drag::Pan {
                    last_x: x,
                    last_y: y,
                };
            }
            Drag::Select { anchor } => {
                let p = self.position_at(x, y, len);
                self.cursor = Some(p);
                self.selection = (anchor != p).then(|| (anchor.min(p), anchor.max(p)));
                self.selection_reverse = p < anchor;
            }
            Drag::Feature { x: px, y: py } => {
                if (x - px).abs() > 4.0 {
                    let anchor = self.position_at(px, py, len);
                    self.selected = None;
                    self.drag = Drag::Select { anchor };
                    self.drag_to(x, y, len);
                }
            }
        }
    }

    pub fn release(&mut self) {
        self.drag = Drag::None;
    }

    /// Move the cursor by `delta` residues; with `extend`, grow the selection.
    pub fn move_cursor(&mut self, delta: i64, extend: bool, len: u64) {
        let c = self.cursor.unwrap_or(0);
        let next = (c as i64 + delta).clamp(0, len as i64) as u64;
        if extend {
            let anchor = match self.selection {
                Some((a, b)) if b == c => a,
                Some((a, b)) if a == c => b,
                _ => c,
            };
            self.selection = (anchor != next).then(|| (anchor.min(next), anchor.max(next)));
            self.selection_reverse = next < anchor;
        } else {
            self.selection = None;
        }
        self.cursor = Some(next);
        if self.mode != Mode::Circular {
            self.reveal(next, len);
        }
    }

    // Drawing.

    pub fn build(&mut self, doc: &SequenceDoc, theme: &Theme, frame: &mut Frame) {
        frame.clear();
        self.hits.clear();
        self.arc_hits.clear();
        self.hidden = 0;
        match self.mode {
            Mode::Linear => self.build_linear(doc, theme, frame),
            Mode::Wrapped => self.build_wrapped(doc, theme, frame),
            Mode::Circular => self.build_circular(doc, theme, frame),
        }
    }

    fn visible_range(&self, len: u64) -> (u64, u64) {
        let b0 = self.start.floor().max(0.0) as u64;
        let b1 = ((self.start + self.span()).ceil().max(0.0) as u64).min(len);
        (b0.min(len), b1)
    }

    // Wrapped layout.

    /// Residues per wrapped line at the current zoom and width.
    pub fn line_bp(&self) -> u64 {
        (f64::from(self.width) * self.bp_per_px).floor().max(1.0) as u64
    }

    fn line_height(&self, lanes: usize) -> f32 {
        if lanes == 0 {
            self.feature_top() - 10.0 + BLOCK_GAP
        } else {
            self.feature_top() + lanes as f32 * (LANE_H + LANE_GAP) - LANE_GAP + BLOCK_GAP
        }
    }

    fn wrap_total(&self) -> f32 {
        self.wrap
            .as_ref()
            .and_then(|w| w.offsets.last().copied())
            .unwrap_or(0.0)
    }

    fn clamp_scroll(&mut self) {
        let max = (self.wrap_total() - self.height).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max);
    }

    /// The wrapped line under screen height `y`.
    fn line_at(&self, y: f32) -> usize {
        let Some(w) = &self.wrap else { return 0 };
        let lines = w.offsets.len().saturating_sub(1).max(1);
        w.offsets
            .partition_point(|&o| o <= y + self.scroll_y)
            .saturating_sub(1)
            .min(lines - 1)
    }

    /// Wrapped lines at least partly on screen.
    fn visible_lines(&self) -> std::ops::Range<usize> {
        let Some(w) = &self.wrap else { return 0..0 };
        let lines = w.offsets.len().saturating_sub(1);
        let first = self.line_at(0.0);
        let end = w
            .offsets
            .partition_point(|&o| o < self.scroll_y + self.height)
            .min(lines);
        first..end.max(first + 1).min(lines)
    }

    /// Keep the top visible line's first residue at its current height
    /// through the next relayout.
    fn anchor_top_line(&mut self) {
        if self.anchor.is_some() {
            return;
        }
        if let Some(w) = &self.wrap {
            let k = self.line_at(0.0);
            self.anchor = Some((k as u64 * w.line_bp, w.offsets[k] - self.scroll_y));
        }
    }

    /// Lay out wrapped lines for this document, zoom and width, unless the
    /// cached layout still holds.
    fn ensure_wrap(&mut self, doc: &SequenceDoc) {
        let (line_bp, len) = (self.line_bp(), doc.len());
        if let Some(w) = &self.wrap
            && w.version == doc.version()
            && w.len == len
            && w.line_bp == line_bp
            && (w.width - self.width).abs() < 0.5
        {
            return;
        }
        let lines = (len.div_ceil(line_bp) as usize).max(1);
        let saved = self.start;
        let mut offsets = Vec::with_capacity(lines + 1);
        let mut acc = 0.0;
        offsets.push(acc);
        for k in 0..lines {
            let lanes = if lines <= WRAP_EXACT_LINES {
                let b0 = k as u64 * line_bp;
                self.start = b0 as f64;
                self.lanes_needed(doc, b0, (b0 + line_bp).min(len))
                    .min(WRAP_MAX_LANES)
            } else {
                WRAP_ESTIMATED_LANES
            };
            acc += self.line_height(lanes);
            offsets.push(acc);
        }
        self.start = saved;
        self.wrap = Some(WrapCache {
            version: doc.version(),
            len,
            line_bp,
            width: self.width,
            offsets,
        });
    }

    fn build_wrapped(&mut self, doc: &SequenceDoc, theme: &Theme, frame: &mut Frame) {
        self.ensure_wrap(doc);
        let len = doc.len();
        let Some(w) = self.wrap.clone() else { return };
        if let Some((pos, y)) = self.anchor.take() {
            let k = ((pos / w.line_bp) as usize).min(w.offsets.len() - 2);
            self.scroll_y = w.offsets[k] - y;
        }
        self.clamp_scroll();
        for k in self.visible_lines() {
            let b0 = k as u64 * w.line_bp;
            let b1 = (b0 + w.line_bp).min(len);
            let oy = w.offsets[k] - self.scroll_y;
            let height = w.offsets[k + 1] - w.offsets[k] - BLOCK_GAP;
            self.start = b0 as f64;
            self.draw_track(doc, theme, frame, (b0, b1), oy, height, WRAP_MAX_LANES);
        }
        if self.hidden > 0 {
            frame.text(
                self.width - 220.0,
                self.height - 18.0,
                LABEL_SIZE,
                &format!("+{} features not shown", self.hidden),
                theme.foreground_muted,
            );
        }
    }

    fn build_linear(&mut self, doc: &SequenceDoc, theme: &Theme, frame: &mut Frame) {
        let len = doc.len();
        let (b0, b1) = self.visible_range(len);
        let x_end = self.x_of(len as f64);
        let x_start = self.x_of(0.0);

        // Beyond the sequence ends.
        frame.rect(0.0, 0.0, x_start.max(0.0), self.height, theme.surface);
        frame.rect(x_end, 0.0, self.width - x_end, self.height, theme.surface);

        let top = self.feature_top();
        let max_lanes = (((self.height - top) / (LANE_H + LANE_GAP)).floor() as usize).max(1);
        self.draw_track(doc, theme, frame, (b0, b1), 0.0, self.height, max_lanes);
        if self.hidden > 0 {
            frame.text(
                8.0,
                self.height - 18.0,
                LABEL_SIZE,
                &format!("+{} features not shown", self.hidden),
                theme.foreground_muted,
            );
        }
    }

    /// One track: ruler, residues and features for `b0..b1`, drawn from
    /// `oy` down, `height` tall, using the current `start` for x.
    #[allow(clippy::too_many_arguments)]
    fn draw_track(
        &mut self,
        doc: &SequenceDoc,
        theme: &Theme,
        frame: &mut Frame,
        (b0, b1): (u64, u64),
        oy: f32,
        height: f32,
        max_lanes: usize,
    ) {
        self.draw_ruler(b0, b1, oy, theme, frame);
        if let Some((a, b)) = self.selection {
            let (a, b) = (a.max(b0), b.min(b1));
            if a < b {
                let (xa, xb) = (self.x_of(a as f64), self.x_of(b as f64));
                frame.rect(
                    xa,
                    oy + RULER_H,
                    (xb - xa).max(1.0),
                    height - RULER_H,
                    alpha(theme.selection, 0.55),
                );
            }
        }
        self.draw_sequence(doc, b0, b1, oy, theme, frame);
        self.draw_features(doc, b0, b1, oy, max_lanes, theme, frame);
        if let Some(c) = self.cursor
            && c >= b0
            && c <= b1
        {
            let x = self.x_of(c as f64);
            let bottom = oy + self.feature_top() - 12.0;
            frame.rect(
                x - 1.0,
                oy + SEQ_Y - 5.0,
                2.0,
                bottom - oy - SEQ_Y + 5.0,
                theme.accent,
            );
        }
    }

    fn draw_ruler(&self, b0: u64, b1: u64, oy: f32, theme: &Theme, frame: &mut Frame) {
        let base_y = oy + RULER_H - 6.0;
        let x0 = self.x_of(b0 as f64).max(0.0);
        let x1 = self.x_of(b1 as f64).min(self.width);
        frame.rect(x0, base_y, x1 - x0, 1.0, theme.foreground_muted);
        let step = nice_step(self.bp_per_px * 110.0);
        let minor = step / 5.0;
        let (lo, hi) = (b0 as f64, b1 as f64);
        if minor / self.bp_per_px >= 7.0 {
            let mut t = (lo / minor).ceil() * minor;
            while t <= hi {
                frame.rect(self.x_of(t), base_y - 3.0, 1.0, 3.0, theme.foreground_muted);
                t += minor;
            }
        }
        let mut t = (lo / step).ceil() * step;
        while t <= hi {
            let x = self.x_of(t);
            frame.rect(x, base_y - 7.0, 1.0, 7.0, theme.foreground);
            frame.text(
                x + 3.0,
                oy + 2.0,
                12.0,
                &format_bp(t as u64, step),
                theme.foreground_muted,
            );
            t += step;
        }
    }

    fn draw_sequence(
        &self,
        doc: &SequenceDoc,
        b0: u64,
        b1: u64,
        oy: f32,
        theme: &Theme,
        frame: &mut Frame,
    ) {
        if b1 <= b0 {
            return;
        }
        let seq_y = oy + SEQ_Y;
        let ppb = self.px_per_bp() as f32;
        if self.px_per_bp() >= 1.0 {
            let residues = doc.read(b0, b1);
            let letters = self.letters();
            for (i, &b) in residues.iter().enumerate() {
                let x = self.x_of((b0 + i as u64) as f64);
                let c = theme.residue(b);
                if letters {
                    // The forward strand only; leave a gap between cells
                    // once there's room for one.
                    let size = (SEQ_H - 3.0).min(ppb * 2.0);
                    let gx = x + (ppb - size * crate::frame::GLYPH_ADVANCE) / 2.0;
                    let gap = if ppb >= 9.0 { 1.0 } else { 0.0 };
                    frame.rect(
                        x + gap / 2.0,
                        seq_y,
                        ppb - gap,
                        SEQ_H,
                        mix(theme.background, c, 0.22),
                    );
                    frame.glyph(gx, seq_y + (SEQ_H - size) / 2.0, size, b, c);
                } else {
                    frame.rect(x, seq_y, ppb.max(1.0), SEQ_H, alpha(c, 0.85));
                }
            }
            return;
        }
        // Zoomed out: GC content per bin, scaled to the visible range so
        // local variation shows; colour runs from AT-rich to GC-rich.
        let bins = (self.x_of(b1 as f64) - self.x_of(b0 as f64)).max(1.0) as usize / 2;
        let bins = bins.max(1);
        // Summary bins are 256 bp; below that, a sliding window over the
        // residues themselves gives a continuous curve.
        let gc = if b1 - b0 <= 4_000_000 && (b1 - b0) / (bins as u64) < 512 {
            window_gc(doc, b0, b1, bins)
        } else {
            doc.summary().gc(b0, b1, bins)
        };
        let bin_w = (self.x_of(b1 as f64) - self.x_of(b0 as f64)) / gc.len() as f32;
        let x0 = self.x_of(b0 as f64);
        let finite = gc.iter().copied().filter(|v| !v.is_nan());
        let (lo, hi) = finite.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let (lo, hi) = if hi - lo < 0.02 {
            (f32::midpoint(lo, hi) - 0.01, f32::midpoint(lo, hi) + 0.01)
        } else {
            (lo, hi)
        };
        frame.rect(
            x0,
            seq_y,
            bin_w * gc.len() as f32,
            SEQ_H,
            alpha(theme.surface, 0.6),
        );
        for (i, &v) in gc.iter().enumerate() {
            if v.is_nan() {
                continue;
            }
            let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
            let h = 2.0 + t * (SEQ_H - 2.0);
            let x = x0 + i as f32 * bin_w;
            frame.rect(
                x,
                seq_y + SEQ_H - h,
                bin_w.max(1.0),
                h,
                alpha(mix(theme.orange, theme.blue, t), 0.8),
            );
        }
        let label = format!("GC {:.0}-{:.0}%", lo * 100.0, hi * 100.0);
        frame.text(
            self.width - text_width(&label, 11.0) - 6.0,
            seq_y + SEQ_H + 2.0,
            11.0,
            &label,
            theme.foreground_muted,
        );
    }

    /// Features overlapping `b0..b1` as items ready for row packing, with
    /// exon/intron/CDS children grouped under their transcripts, or just a
    /// count when there are too many to draw.
    fn feature_layout(&self, doc: &SequenceDoc, b0: u64, b1: u64) -> FeatureLayout {
        let mut ids = doc.features_in(b0, b1);
        ids.retain(|id| doc.feature(*id).is_some_and(|f| !is_backdrop(f)));
        if ids.len() > MAX_DRAWN_FEATURES {
            return FeatureLayout::Dense(ids.len());
        }

        // Gene models: exons and CDS whose parent transcript is in view are
        // drawn inside the transcript's glyph, not on rows of their own.
        let mut by_gff_id: HashMap<&str, FeatureId> = HashMap::new();
        for &id in &ids {
            if let Some(gid) = doc.feature(id).and_then(|f| f.qualifier("ID")) {
                by_gff_id.insert(gid, id);
            }
        }
        let mut parts: HashMap<FeatureId, Vec<FeatureId>> = HashMap::new();
        let mut absorbed: HashSet<FeatureId> = HashSet::new();
        for &id in &ids {
            let Some(f) = doc.feature(id) else { continue };
            if !matches!(
                f.kind.as_str(),
                "exon" | "CDS" | "five_prime_UTR" | "three_prime_UTR"
            ) {
                continue;
            }
            let parent = f
                .qualifier("Parent")
                .and_then(|p| by_gff_id.get(p))
                .copied();
            if let Some(pid) = parent
                && doc.feature(pid).is_some_and(|p| is_transcript(&p.kind))
            {
                parts.entry(pid).or_default().push(id);
                absorbed.insert(id);
            }
        }
        // GenBank has no Parent links: group exon, intron and CDS features
        // under the transcript (or CDS) of the same /gene.
        let rank = |kind: &str| match kind {
            "mRNA" | "transcript" => 3,
            "tRNA" | "rRNA" | "ncRNA" | "misc_RNA" => 2,
            "CDS" => 1,
            _ => 0,
        };
        let mut by_gene: HashMap<&str, FeatureId> = HashMap::new();
        for &id in &ids {
            let Some(f) = doc.feature(id) else { continue };
            let (Some(g), r) = (f.qualifier("gene"), rank(&f.kind)) else {
                continue;
            };
            if r > 0
                && by_gene
                    .get(g)
                    .and_then(|o| doc.feature(*o))
                    .is_none_or(|o| rank(&o.kind) < r)
            {
                by_gene.insert(g, id);
            }
        }
        for &id in &ids {
            let Some(f) = doc.feature(id) else { continue };
            if absorbed.contains(&id) || f.qualifier("Parent").is_some() {
                continue;
            }
            if !matches!(f.kind.as_str(), "exon" | "intron" | "CDS") {
                continue;
            }
            let Some(&pid) = f.qualifier("gene").and_then(|g| by_gene.get(g)) else {
                continue;
            };
            let parent_rank = doc.feature(pid).map_or(0, |p| rank(&p.kind));
            if pid != id && parent_rank > rank(&f.kind) {
                parts.entry(pid).or_default().push(id);
                absorbed.insert(id);
            }
        }
        ids.retain(|id| !absorbed.contains(id));

        let mut items: Vec<Item> = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(f) = doc.feature(id) else { continue };
            for (s, e) in pieces(f, doc.len(), doc.circular) {
                let x0 = self.x_of(s as f64);
                let x1 = self.x_of(e as f64).max(x0 + 1.0);
                let label = f.label().unwrap_or(&f.kind).to_string();
                let lw = text_width(&label, LABEL_SIZE);
                // Measured on the visible part, so long features keep a name.
                // Gene models have lines and boxes inside, so no name there;
                // their gene row carries it.
                let visible = x1.min(self.width) - x0.max(0.0);
                let label_inside = visible >= lw + 8.0 && !parts.contains_key(&id);
                // Room for the strand arrowhead.
                let reach = x1 + 8.0;
                items.push(Item {
                    id,
                    span: (s, e),
                    x0,
                    x1,
                    label,
                    label_inside,
                    reach,
                });
            }
        }
        items.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        FeatureLayout::Items { items, parts }
    }

    /// Rows needed to pack the features of `b0..b1` without hiding any.
    fn lanes_needed(&self, doc: &SequenceDoc, b0: u64, b1: u64) -> usize {
        match self.feature_layout(doc, b0, b1) {
            FeatureLayout::Dense(_) => DENSE_ROWS,
            FeatureLayout::Items { items, .. } => {
                let mut ends: Vec<f32> = Vec::new();
                for item in &items {
                    match ends.iter().position(|&end| end + 4.0 < item.x0) {
                        Some(l) => ends[l] = item.reach,
                        None => ends.push(item.reach),
                    }
                }
                ends.len()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_features(
        &mut self,
        doc: &SequenceDoc,
        b0: u64,
        b1: u64,
        oy: f32,
        max_lanes: usize,
        theme: &Theme,
        frame: &mut Frame,
    ) {
        let top = oy + self.feature_top();
        let (items, parts) = match self.feature_layout(doc, b0, b1) {
            FeatureLayout::Items { items, parts } => (items, parts),
            FeatureLayout::Dense(count) => {
                let bins = (self.width / 3.0) as usize;
                let counts = doc.index().density(b0, b1, bins);
                let max = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
                let (x0, x1) = (self.x_of(b0 as f64), self.x_of(b1 as f64));
                let w = (x1 - x0) / bins as f32;
                for (i, &c) in counts.iter().enumerate() {
                    let h = 60.0 * c as f32 / max;
                    frame.rect(
                        x0 + i as f32 * w,
                        top + 60.0 - h,
                        w.max(1.0),
                        h,
                        alpha(theme.green, 0.7),
                    );
                }
                frame.text(
                    8.0,
                    top + 68.0,
                    LABEL_SIZE,
                    &format!("{count} features here; zoom in to see them"),
                    theme.foreground_muted,
                );

                return;
            }
        };
        let mut lane_ends: Vec<f32> = Vec::new();
        // (lane, item) in drawing order, for placing labels afterwards.
        let mut placed: Vec<(usize, usize)> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let lane = lane_ends.iter().position(|&end| end + 4.0 < item.x0);
            let lane = match lane {
                Some(l) => l,
                None if lane_ends.len() < max_lanes => {
                    lane_ends.push(f32::NEG_INFINITY);
                    lane_ends.len() - 1
                }
                None => {
                    self.hidden += 1;
                    continue;
                }
            };
            lane_ends[lane] = item.reach;
            placed.push((lane, index));
            let y = top + lane as f32 * (LANE_H + LANE_GAP);
            let Some(f) = doc.feature(item.id) else {
                continue;
            };
            let children: Vec<&Feature> = parts
                .get(&item.id)
                .map(|c| c.iter().filter_map(|id| doc.feature(*id)).collect())
                .unwrap_or_default();
            self.draw_feature(f, &children, item.span, item.x0, item.x1, y, theme, frame);
            if self.selected == Some(item.id) {
                outline(
                    frame,
                    item.x0 - 2.0,
                    y - 2.0,
                    item.x1 - item.x0 + 4.0,
                    LANE_H + 4.0,
                    theme.accent,
                );
            }
            self.hits
                .push(([item.x0, y, item.x1, y + LANE_H], item.id, item.span));
        }
        // Names only inside features wide enough for them; hovering shows
        // the rest.
        for &(lane, index) in &placed {
            let item = &items[index];
            if item.label_inside {
                let y = top + lane as f32 * (LANE_H + LANE_GAP);
                let text_y = y + (LANE_H - LABEL_SIZE) / 2.0;
                frame.text(
                    item.x0.max(0.0) + 7.0,
                    text_y,
                    LABEL_SIZE,
                    &item.label,
                    theme.background,
                );
            }
        }
    }

    /// One feature on one row. A transcript with `parts` (its exons and
    /// CDS) is drawn as a gene model: a line for introns, thin boxes for
    /// exons and full-height boxes for coding segments.
    #[allow(clippy::too_many_arguments)]
    fn draw_feature(
        &self,
        f: &Feature,
        parts: &[&Feature],
        span: (u64, u64),
        x0: f32,
        x1: f32,
        y: f32,
        theme: &Theme,
        frame: &mut Frame,
    ) {
        let color = theme.feature(&f.kind);
        let segs = &f.location.segments;
        let mid = y + LANE_H / 2.0;
        if parts.is_empty() {
            if segs.len() > 1 {
                frame.rect(x0, mid - 1.0, x1 - x0, 2.0, alpha(color, 0.6));
            }
            for s in segs.iter().filter(|s| within(s, span)) {
                let sx0 = self.x_of(s.start as f64);
                let sx1 = self.x_of(s.end as f64).max(sx0 + 1.0);
                frame.rect(sx0, y, sx1 - sx0, LANE_H, color);
            }
        } else {
            frame.rect(x0, mid - 1.0, x1 - x0, 2.0, alpha(color, 0.7));
            // The parent's own segments, when they carry the structure (a
            // CDS or tRNA with join(), or a spliced mRNA); a single-span
            // transcript relies on its exon children instead.
            let own = segs.len() > 1 || !is_transcript(&f.kind);
            if own {
                let h = if f.kind == "CDS" {
                    LANE_H
                } else {
                    LANE_H * 0.75
                };
                for s in segs.iter().filter(|s| within(s, span)) {
                    let sx0 = self.x_of(s.start as f64);
                    let sx1 = self.x_of(s.end as f64).max(sx0 + 1.0);
                    frame.rect(sx0, mid - h / 2.0, sx1 - sx0, h, color);
                }
            }
            for p in parts {
                let (h, c) = match p.kind.as_str() {
                    "CDS" => (LANE_H, theme.feature("CDS")),
                    "intron" => continue,
                    _ if own => continue,
                    _ => (LANE_H * 0.55, color),
                };
                for s in p.location.segments.iter().filter(|s| within(s, span)) {
                    let sx0 = self.x_of(s.start as f64);
                    let sx1 = self.x_of(s.end as f64).max(sx0 + 1.0);
                    frame.rect(sx0, mid - h / 2.0, sx1 - sx0, h, c);
                }
            }
        }
        // Incomplete ends look torn: a zigzag cut in the background colour.
        let loc = &f.location;
        let reverse = loc.strand() == Strand::Reverse;
        let low_partial = if reverse {
            loc.partial_3()
        } else {
            loc.partial_5()
        };
        let high_partial = if reverse {
            loc.partial_5()
        } else {
            loc.partial_3()
        };
        if low_partial && x0 > -10.0 {
            torn_edge(frame, x0, y, true, theme.background);
        }
        if high_partial && x1 < self.width + 10.0 {
            torn_edge(frame, x1, y, false, theme.background);
        }
        // Strand arrowhead at the 3' end, when the feature is wide enough
        // for it not to swamp the feature itself, and the end is complete.
        if x1 - x0 < 10.0 || loc.partial_3() {
            return;
        }
        let h = LANE_H;
        match f.location.strand() {
            Strand::Forward => frame.quad(
                [[x1, y], [x1 + 6.0, mid], [x1, y + h], [x1 + 6.0, mid]],
                color,
                [0.0; 4],
            ),
            Strand::Reverse => frame.quad(
                [[x0 - 6.0, mid], [x0, y], [x0 - 6.0, mid], [x0, y + h]],
                color,
                [0.0; 4],
            ),
            Strand::Unstranded => {}
        }
    }

    // Circular.

    fn build_circular(&mut self, doc: &SequenceDoc, theme: &Theme, frame: &mut Frame) {
        let len = doc.len().max(1);
        let (cx, cy) = self.centre();
        let r = self.radius();

        if let Some((a, b)) = self.selection {
            arc(
                frame,
                (cx, cy),
                Self::angle(a as f64, len),
                Self::angle(b as f64, len),
                r * 0.45,
                r + 70.0,
                alpha(theme.selection, 0.6),
            );
        }

        // GC ring.
        let gc = doc.summary().gc(0, len, 360);
        let baseline = mean_gc(doc);
        let gc_r = r * 0.62;
        for (i, &v) in gc.iter().enumerate() {
            if v.is_nan() {
                continue;
            }
            let a0 = -PI / 2.0 + TAU * i as f32 / 360.0;
            let a1 = a0 + TAU / 360.0;
            let dev = ((v - baseline) * r * 1.5).clamp(-r * 0.12, r * 0.12);
            let (r0, r1, c) = if dev >= 0.0 {
                (gc_r, gc_r + dev, theme.blue)
            } else {
                (gc_r + dev, gc_r, theme.orange)
            };
            arc(frame, (cx, cy), a0, a1, r0, r1.max(r0 + 0.5), alpha(c, 0.7));
        }

        // Backbone and ticks.
        arc(
            frame,
            (cx, cy),
            0.0,
            TAU,
            r - 1.0,
            r + 1.0,
            theme.foreground,
        );
        let step = nice_step(len as f64 / 12.0);
        let mut t = 0.0;
        while t < len as f64 {
            let a = Self::angle(t, len);
            radial(frame, (cx, cy), a, r + 2.0, r + 8.0, 1.0, theme.foreground);
            let label = format_bp(t as u64, step);
            let (lx, ly) = (cx + a.cos() * (r + 14.0), cy + a.sin() * (r + 14.0));
            let w = text_width(&label, 12.0);
            let lx = if a.cos() < -0.1 {
                lx - w
            } else if a.cos() < 0.1 {
                lx - w / 2.0
            } else {
                lx
            };
            let ly = if a.sin() < 0.0 { ly - 12.0 } else { ly };
            frame.text(lx, ly, 12.0, &label, theme.foreground_muted);
            t += step;
        }

        // Features: forward strand outside the backbone, reverse inside.
        let mut feats: Vec<(FeatureId, &Feature)> =
            doc.features().filter(|(_, f)| !is_backdrop(f)).collect();
        if feats.len() > MAX_DRAWN_FEATURES {
            let counts = doc.index().density(0, len, 360);
            let max = counts.iter().copied().max().unwrap_or(1).max(1) as f32;
            for (i, &c) in counts.iter().enumerate() {
                let a0 = -PI / 2.0 + TAU * i as f32 / 360.0;
                arc(
                    frame,
                    (cx, cy),
                    a0,
                    a0 + TAU / 360.0,
                    r + 30.0,
                    r + 30.0 + 40.0 * c as f32 / max,
                    alpha(theme.green, 0.7),
                );
            }
            feats.clear();
        }
        feats.sort_by_key(|(_, f)| f.location.bounds().map_or(0, |b| b.0));
        let lane_w = 9.0;
        let mut lanes: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
        let mut labels: Vec<(f32, String, f32)> = Vec::new();
        for (id, f) in &feats {
            let reverse = f.location.strand() == Strand::Reverse;
            let side = usize::from(reverse);
            let Some((s, e)) = f.location.bounds() else {
                continue;
            };
            // A feature crossing the origin has segments at both ends; its
            // packing extent is the whole circle, which is rare and fine.
            let (s, e) = (s as f64, e as f64);
            let lane = lanes[side]
                .iter()
                .position(|&end| end < s)
                .unwrap_or_else(|| {
                    lanes[side].push(0.0);
                    lanes[side].len() - 1
                });
            if lane >= 5 {
                self.hidden += 1;
                continue;
            }
            lanes[side][lane] = e + len as f64 * 0.002;
            let (r0, r1) = if reverse {
                let outer = r - 6.0 - lane as f32 * (lane_w + 3.0);
                (outer - lane_w, outer)
            } else {
                let inner = r + 26.0 + lane as f32 * (lane_w + 3.0);
                (inner, inner + lane_w)
            };
            let mut color = theme.feature(&f.kind);
            if self.selected == Some(*id) {
                color = theme.accent;
            }
            for seg in &f.location.segments {
                let (a0, a1) = (
                    Self::angle(seg.start as f64, len),
                    Self::angle(seg.end as f64, len),
                );
                arc(frame, (cx, cy), a0, a1.max(a0 + 0.002), r0, r1, color);
                self.arc_hits
                    .push((seg.start as f32, seg.end as f32, r0, r1, *id));
            }
            if matches!(f.kind.as_str(), "gene" | "tRNA" | "rRNA") || self.selected == Some(*id) {
                let mid = Self::angle(f64::midpoint(s, e), len);
                labels.push((
                    mid,
                    f.label().unwrap_or(&f.kind).to_string(),
                    if reverse { r * 0.40 } else { r + 96.0 },
                ));
            }
        }
        // Labels, skipping any that would overlap one already placed.
        let mut placed: Vec<[f32; 4]> = Vec::new();
        for (a, text, lr) in labels {
            let inner = lr < r;
            let (x, y) = (cx + a.cos() * lr, cy + a.sin() * lr);
            let w = text_width(&text, 11.0);
            let flip = if inner { a.cos() > 0.0 } else { a.cos() < 0.0 };
            let x = if flip { x - w } else { x };
            let rect = [x - 2.0, y - 7.0, x + w + 2.0, y + 7.0];
            if placed
                .iter()
                .any(|p| rect[0] < p[2] && rect[2] > p[0] && rect[1] < p[3] && rect[3] > p[1])
            {
                continue;
            }
            placed.push(rect);
            frame.text(x, y - 6.0, 11.0, &text, theme.foreground_muted);
        }

        if let Some(c) = self.cursor {
            radial(
                frame,
                (cx, cy),
                Self::angle(c as f64, len),
                r * 0.45,
                r + 80.0,
                2.0,
                theme.accent,
            );
        }

        let name = &doc.name;
        frame.text(
            cx - text_width(name, 18.0) / 2.0,
            cy - 20.0,
            18.0,
            name,
            theme.foreground,
        );
        let size = format!("{} bp", format_thousands(len));
        frame.text(
            cx - text_width(&size, 13.0) / 2.0,
            cy + 4.0,
            13.0,
            &size,
            theme.foreground_muted,
        );
    }

    /// What to show when hovering over a feature: name, type, location
    /// (1-based) and length.
    pub fn hover_text(&self, x: f32, y: f32, doc: &SequenceDoc) -> Option<String> {
        let id = self.feature_at(x, y, doc.len())?;
        let f = doc.feature(id)?;
        let (s, e) = f.location.bounds()?;
        let strand = match f.location.strand() {
            Strand::Forward => " (+)",
            Strand::Reverse => " (−)",
            Strand::Unstranded => "",
        };
        let name = f.label().map_or(String::new(), |l| format!("{l} · "));
        let exons = if f.location.segments.len() > 1 {
            format!(" · {} parts", f.location.segments.len())
        } else {
            String::new()
        };
        let partial = match (f.location.partial_5(), f.location.partial_3()) {
            (true, true) => " · both ends incomplete",
            (true, false) => " · 5′ incomplete",
            (false, true) => " · 3′ incomplete",
            (false, false) => "",
        };
        Some(format!(
            "{name}{} · {}..{}{strand} · {}{exons}{partial}",
            f.kind,
            format_thousands(s + 1),
            format_thousands(e),
            format_length(f.location.len())
        ))
    }

    /// One line describing the view, the cursor, the selection and the
    /// selected feature.
    pub fn status(&self, doc: &SequenceDoc) -> String {
        let mut parts = vec![format!(
            "{}  {} bp, {} features",
            doc.name,
            format_thousands(doc.len()),
            format_thousands(doc.feature_count() as u64)
        )];
        if let Some(c) = self.cursor {
            parts.push(format!("cursor {}", format_thousands(c)));
        }
        if let Some((a, b)) = self.selection {
            parts.push(format!(
                "selected {}..{} ({} bp)",
                format_thousands(a + 1),
                format_thousands(b),
                format_thousands(b - a)
            ));
        }
        if let Some(f) = self.selected.and_then(|id| doc.feature(id)) {
            let (s, e) = f.location.bounds().unwrap_or((0, 0));
            let strand = match f.location.strand() {
                Strand::Forward => "+",
                Strand::Reverse => "-",
                Strand::Unstranded => ".",
            };
            parts.push(format!(
                "{} {} {}..{} {strand}",
                f.kind,
                f.label().unwrap_or(""),
                format_thousands(s + 1),
                format_thousands(e)
            ));
        }
        parts.join("   ·   ")
    }
}

/// GC fraction in `bins` bins over `start..end`, each averaged over a
/// window of at least 100 bp centred on the bin.
fn window_gc(doc: &SequenceDoc, start: u64, end: u64, bins: usize) -> Vec<f32> {
    let span = end - start;
    let bin = span as f64 / bins as f64;
    let half = (100.0f64.max(bin) / 2.0).ceil() as u64;
    let lo = start.saturating_sub(half);
    let hi = (end + half).min(doc.len());
    let residues = doc.read(lo, hi);
    // Prefix sums of GC and ACGT counts.
    let mut gc = Vec::with_capacity(residues.len() + 1);
    let mut acgt = Vec::with_capacity(residues.len() + 1);
    let (mut g, mut a) = (0u32, 0u32);
    gc.push(0);
    acgt.push(0);
    for &b in &residues {
        match b | 0x20 {
            b'g' | b'c' | b's' => {
                g += 1;
                a += 1;
            }
            b'a' | b't' | b'u' | b'w' => a += 1,
            _ => {}
        }
        gc.push(g);
        acgt.push(a);
    }
    (0..bins)
        .map(|i| {
            let centre = start as f64 + (i as f64 + 0.5) * bin;
            let w0 = ((centre as u64).saturating_sub(half)).max(lo) - lo;
            let w1 = ((centre as u64 + half).min(hi)) - lo;
            let (w0, w1) = (w0 as usize, (w1 as usize).max(w0 as usize));
            let n = acgt[w1] - acgt[w0];
            if n == 0 {
                f32::NAN
            } else {
                (gc[w1] - gc[w0]) as f32 / n as f32
            }
        })
        .collect()
}

/// Mean GC fraction over the whole sequence, the baseline the circular GC
/// ring deviates from; 0.5 if there are no informative residues.
fn mean_gc(doc: &SequenceDoc) -> f32 {
    let v = doc.summary().gc(0, doc.len().max(1), 1);
    v.first().copied().filter(|x| !x.is_nan()).unwrap_or(0.5)
}

/// A zigzag notch at `x`, cut into a feature box spanning `y..y + LANE_H`;
/// `left` puts the teeth on the box's left edge.
fn torn_edge(frame: &mut Frame, x: f32, y: f32, left: bool, background: Rgba) {
    let teeth = 2;
    let h = LANE_H / teeth as f32;
    let depth = 6.0;
    let d = if left { depth } else { -depth };
    for i in 0..teeth {
        let top = y + i as f32 * h;
        // A triangle: point inside the box, base on the edge.
        frame.quad(
            [
                [x, top],
                [x + d, top + h / 2.0],
                [x, top + h],
                [x + d, top + h / 2.0],
            ],
            background,
            [0.0; 4],
        );
    }
}

fn outline(frame: &mut Frame, x: f32, y: f32, w: f32, h: f32, color: Rgba) {
    frame.rect(x, y, w, 2.0, color);
    frame.rect(x, y + h - 2.0, w, 2.0, color);
    frame.rect(x, y, 2.0, h, color);
    frame.rect(x + w - 2.0, y, 2.0, h, color);
}

/// A ring segment from angle `a0` to `a1` between radii `r0` and `r1`.
fn arc(frame: &mut Frame, c: (f32, f32), a0: f32, a1: f32, r0: f32, r1: f32, color: Rgba) {
    let (a0, a1) = if a1 < a0 { (a1, a0) } else { (a0, a1) };
    let steps = ((a1 - a0) / (PI / 180.0)).ceil().max(1.0) as usize;
    let da = (a1 - a0) / steps as f32;
    let p = |a: f32, r: f32| [c.0 + a.cos() * r, c.1 + a.sin() * r];
    for i in 0..steps {
        let (s, e) = (a0 + da * i as f32, a0 + da * (i + 1) as f32);
        frame.quad([p(s, r1), p(e, r1), p(s, r0), p(e, r0)], color, [0.0; 4]);
    }
}

/// A radial line of the given width at angle `a`.
fn radial(frame: &mut Frame, c: (f32, f32), a: f32, r0: f32, r1: f32, width: f32, color: Rgba) {
    let (dx, dy) = (a.cos(), a.sin());
    let (nx, ny) = (-dy * width / 2.0, dx * width / 2.0);
    let p = |r: f32, s: f32| [c.0 + dx * r + nx * s, c.1 + dy * r + ny * s];
    frame.quad(
        [p(r1, -1.0), p(r1, 1.0), p(r0, -1.0), p(r0, 1.0)],
        color,
        [0.0; 4],
    );
}

/// The smallest 1, 2 or 5 × 10^k that is at least `raw`.
pub fn nice_step(raw: f64) -> f64 {
    let raw = raw.max(1.0);
    let mag = 10f64.powf(raw.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * mag >= raw {
            return m * mag;
        }
    }
    10.0 * mag
}

/// Compact position labels: 950, 12k, 1.5M, with enough decimals that
/// labels `step` apart differ.
pub fn format_bp(n: u64, step: f64) -> String {
    let (unit, suffix) = if n >= 1_000_000 {
        (1e6, "M")
    } else if n >= 10_000 {
        (1e3, "k")
    } else {
        return n.to_string();
    };
    let decimals = if step >= unit {
        0
    } else {
        (unit / step).log10().ceil() as usize
    };
    if decimals > 3 {
        // Base-pair zoom: full positions read better than 1.000123M.
        return format_thousands(n);
    }
    format!("{:.*}{suffix}", decimals, n as f64 / unit)
}

/// A length with a unit: 950 bp, 12.5 kb, 1.50 Mb.
pub fn format_length(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2} Mb", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1} kb", n as f64 / 1e3)
    } else {
        format!("{n} bp")
    }
}

pub fn format_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_protocol::{Location, Qualifier};
    use std::sync::Arc;

    fn doc() -> SequenceDoc {
        let seq: Vec<u8> = b"ACGT".iter().copied().cycle().take(10_000).collect();
        let feats = (0..20)
            .map(|i| Feature {
                kind: "gene".into(),
                location: Location::simple(
                    i * 500,
                    i * 500 + 300,
                    if i % 2 == 0 {
                        Strand::Forward
                    } else {
                        Strand::Reverse
                    },
                ),
                qualifiers: vec![Qualifier::new("gene", format!("g{i}"))],
            })
            .collect();
        SequenceDoc::new("t", Arc::new(seq), feats, true)
    }

    #[test]
    fn zoom_keeps_anchor_and_clamps() {
        let mut v = GenomeView::new(1000.0, 600.0);
        v.mode = Mode::Linear;
        v.fit(10_000);
        assert!((v.bp_per_px - 10.0).abs() < 1e-9);
        let before = v.bp_at(250.0);
        v.zoom_at(250.0, 4.0, 10_000);
        assert!((v.bp_at(250.0) - before).abs() < 1e-6);
        v.zoom_at(250.0, 1e9, 10_000);
        assert!((v.px_per_bp() - MAX_PX_PER_BP).abs() < 1e-9);
        v.zoom_at(250.0, 1e-9, 10_000);
        assert!((v.bp_per_px - 10.0).abs() < 1e-9);
    }

    #[test]
    fn linear_build_draws_and_hit_tests_features() {
        let d = doc();
        let mut v = GenomeView::new(1000.0, 600.0);
        v.mode = Mode::Linear;
        v.fit(d.len());
        let mut f = Frame::new();
        v.build(&d, &Theme::default(), &mut f);
        assert!(f.len() > 40);
        assert_eq!(v.hits.len(), 20);
        let (r, id, _) = v.hits[3];
        assert_eq!(
            v.feature_at(
                f32::midpoint(r[0], r[2]),
                f32::midpoint(r[1], r[3]),
                d.len()
            ),
            Some(id)
        );
        // Zoomed in to letters: residues are read and drawn as glyphs.
        v.zoom_at(10.0, 1000.0, d.len());
        v.build(&d, &Theme::default(), &mut f);
        assert!(v.letters());
        assert!(f.len() > 100);
    }

    #[test]
    fn circular_hit_test_finds_arc() {
        let d = doc();
        let mut v = GenomeView::new(800.0, 800.0);
        v.mode = Mode::Circular;
        let mut f = Frame::new();
        v.build(&d, &Theme::default(), &mut f);
        // Gene 0 is forward, 0..300, on the outer lane, right after the top.
        let (cx, cy) = v.centre();
        let a = GenomeView::angle(150.0, d.len());
        let rr = v.radius() + 30.0;
        assert_eq!(
            v.feature_at(cx + a.cos() * rr, cy + a.sin() * rr, d.len()),
            Some(FeatureId(0))
        );
    }

    #[test]
    fn press_and_drag_select_a_range() {
        let d = doc();
        let mut v = GenomeView::new(1000.0, 600.0);
        v.mode = Mode::Linear;
        v.fit(d.len());
        v.press(100.0, SEQ_Y + 5.0, false, d.len());
        v.drag_to(200.0, SEQ_Y + 5.0, d.len());
        v.release();
        assert_eq!(v.selection, Some((1000, 2000)));
        assert_eq!(v.cursor, Some(2000));
        v.move_cursor(5, true, d.len());
        assert_eq!(v.selection, Some((1000, 2005)));
    }

    #[test]
    fn dragging_past_the_edge_scrolls_and_extends() {
        let d = doc();
        let mut v = GenomeView::new(1000.0, 600.0);
        v.mode = Mode::Linear;
        v.zoom_to(5000, 6000, d.len());
        v.press(500.0, SEQ_Y + 5.0, false, d.len());
        let start = v.start;
        assert!(v.edge_scroll(-50.0, SEQ_Y + 5.0, d.len()));
        assert!(v.start < start);
        let (a, _) = v.selection.unwrap();
        assert!(a as f64 <= v.start + 1.0);
        // Not dragging: no scrolling.
        v.release();
        assert!(!v.edge_scroll(-50.0, SEQ_Y + 5.0, d.len()));
    }

    #[test]
    fn clicks_replace_the_selection() {
        let d = doc();
        let mut v = GenomeView::new(1000.0, 600.0);
        v.mode = Mode::Linear;
        v.fit(d.len());
        let mut f = Frame::new();
        v.build(&d, &Theme::default(), &mut f);
        // A manual selection, then a click on gene 3 (1500..1800).
        v.press(100.0, SEQ_Y + 5.0, false, d.len());
        v.drag_to(200.0, SEQ_Y + 5.0, d.len());
        v.release();
        let (r, id, _) = v.hits[3];
        v.press(
            f32::midpoint(r[0], r[2]),
            f32::midpoint(r[1], r[3]),
            false,
            d.len(),
        );
        assert_eq!(v.selected, Some(id));
        assert_eq!(v.selection, Some((1500, 1800)));
        // A plain click on the sequence starts over.
        v.press(400.0, SEQ_Y + 5.0, false, d.len());
        assert_eq!((v.selection, v.selected), (None, None));
        // A click on empty space clears too, and dragging from there selects.
        v.selection = Some((1, 2));
        v.press(990.0, 590.0, false, d.len());
        assert_eq!(v.selection, None);
        v.drag_to(900.0, 590.0, d.len());
        assert_eq!(v.selection, Some((9000, 9900)));
        v.release();
        // Dragging that starts on a feature becomes a range selection.
        v.press(
            f32::midpoint(r[0], r[2]),
            f32::midpoint(r[1], r[3]),
            false,
            d.len(),
        );
        v.drag_to(
            f32::midpoint(r[0], r[2]) + 50.0,
            f32::midpoint(r[1], r[3]),
            d.len(),
        );
        assert_eq!(v.selected, None);
        assert_eq!(v.selection.map(|(a, b)| b - a), Some(500));
    }

    #[test]
    fn zoom_percentages() {
        let mut v = GenomeView::new(1000.0, 600.0);
        v.bp_per_px = 1.0 / 6.1;
        assert!(v.zoom_text(10_000).starts_with("100%"));
        v.bp_per_px = 100.0;
        assert!(
            v.zoom_text(1_000_000).starts_with("0.16%"),
            "{}",
            v.zoom_text(1_000_000)
        );
        v.bp_per_px = 10_000.0;
        assert!(
            v.zoom_text(100_000_000).starts_with("0.0016%"),
            "{}",
            v.zoom_text(100_000_000)
        );
    }

    #[test]
    fn wrapped_lines_hit_test_and_select_across_lines() {
        let d = doc();
        let mut v = GenomeView::new(1000.0, 400.0);
        v.reset_zoom(d.len());
        let mut f = Frame::new();
        v.build(&d, &Theme::default(), &mut f);
        let line = v.line_bp();
        assert_eq!(line, (1000.0 * (1.0 / PX_PER_BP_AT_100)).floor() as u64);
        let w = v.wrap.clone().unwrap();
        assert_eq!(w.offsets.len() - 1, d.len().div_ceil(line) as usize);
        // Drag from the first line into the second selects across the break.
        let y0 = SEQ_Y + 5.0;
        let y1 = w.offsets[1] + SEQ_Y + 5.0;
        v.press(500.0, y0, false, d.len());
        v.drag_to(100.0, y1, d.len());
        let (a, b) = v.selection.unwrap();
        assert!(a < line && b > line, "{a}..{b} across {line}");
        // Scrolling moves line 1 to the top; positions follow.
        v.release();
        v.scroll_by(w.offsets[1]);
        v.build(&d, &Theme::default(), &mut f);
        assert_eq!(v.line_at(1.0), 1);
        assert_eq!(v.position_at(0.0, SEQ_Y, d.len()), line);
        // Zoom keeps the top line's start in place.
        v.zoom_at(500.0, 2.0, d.len());
        v.build(&d, &Theme::default(), &mut f);
        let top = v.line_at(1.0) as u64 * v.line_bp();
        assert!(top <= line && line < top + v.line_bp());
    }

    #[test]
    fn labels() {
        assert_eq!(nice_step(130.0), 200.0);
        assert_eq!(nice_step(0.3), 1.0);
        assert_eq!(format_bp(950, 50.0), "950");
        assert_eq!(format_bp(12_000, 1000.0), "12k");
        assert_eq!(format_bp(1_500_000, 500_000.0), "1.5M");
        assert_eq!(format_bp(1_050_000, 50_000.0), "1.05M");
        assert_eq!(format_bp(1_000_420, 20.0), "1,000,420");
        assert_eq!(format_thousands(1_234_567), "1,234,567");
    }
}
