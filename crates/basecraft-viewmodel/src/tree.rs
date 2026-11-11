//! The tree editor's view: a rectangular layout, drawing as a list of
//! shapes that goes to the GPU frame or to SVG, hit testing and selection.
//!
//! Layout follows ape's `plot.phylo`: tips one row apart, each internal
//! node halfway between its first and last child, x by branch length
//! (or, without lengths, tips aligned and nodes by height).

// Pixel geometry mixes f32 screen coordinates with counts and f64 branch
// lengths; the conversions are deliberate and bounded by the tree's size.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]

use std::fmt::Write as _;

use basecraft_docs::TreeDoc;
use basecraft_protocol::{CladeAnnotation, Op, SupportPlacement};

use crate::frame::{Frame, GLYPH_ADVANCE, Rgba, hex};
use crate::theme::{Theme, alpha, mix};

const MARGIN: f32 = 16.0;
const LABEL_GAP: f32 = 5.0;
const SCALE_SPACE: f32 = 36.0;
const BAR_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
}

/// One thing to draw, in content coordinates (before scrolling). Text `y`
/// is the top of the line; `down` text runs top to bottom from `(x, y)`,
/// with `align` along its length.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Rgba,
    },
    /// A rectangle with fully rounded ends.
    Pill {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Rgba,
    },
    /// A rectangle whose opacity runs from `from` at its left edge to `to`
    /// at its right.
    Gradient {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Rgba,
        from: f32,
        to: f32,
    },
    Disc {
        cx: f32,
        cy: f32,
        r: f32,
        color: Rgba,
    },
    Text {
        x: f32,
        y: f32,
        size: f32,
        text: String,
        color: Rgba,
        align: Align,
        italic: bool,
        down: bool,
    },
}

/// A column of bars beside the tips. Clade annotations make one column per
/// nesting level; other views add their own, such as one per species
/// hypothesis.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TipColumn {
    /// The owner's id, reported by hit tests; empty for clade annotations.
    pub id: String,
    /// Pixels before the column.
    pub gap: f32,
    /// Pixels between one bar and the next down the column. Unset: a share
    /// of the row height, as for clade bars.
    pub spacing: Option<f32>,
    /// Rounded ends.
    pub round: bool,
    /// Write each bar's label beside it.
    pub labels: bool,
    /// Labels run top to bottom.
    pub vertical_labels: bool,
    /// Tip name to its bands, top to bottom. Bands sharing a row split it by
    /// `share`.
    pub tips: std::collections::HashMap<String, Vec<TipBand>>,
}

/// One tip's part in a bar.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TipBand {
    /// Touching rows with the same id draw as one bar.
    pub id: String,
    pub share: f32,
    pub width: f32,
    /// `#rrggbb`.
    pub color: String,
    pub label: String,
}

/// A bar as drawn: one run of touching rows of one band.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedBar {
    /// The column's owner id and the band id.
    pub column: String,
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: String,
    pub label: String,
    pub round: bool,
    /// Where the label goes, if it shows: x, and whether it runs down.
    pub label_at: Option<(f32, bool)>,
}

/// Colours for drawing.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub foreground: Rgba,
    pub muted: Rgba,
    pub background: Rgba,
    pub accent: Rgba,
}

impl Palette {
    pub fn from_theme(t: &Theme) -> Self {
        Self {
            foreground: t.foreground,
            muted: t.foreground_muted,
            background: t.background,
            accent: t.accent,
        }
    }

    /// Black on white, for figures.
    pub fn print() -> Self {
        Self {
            foreground: [0.0, 0.0, 0.0, 1.0],
            muted: [0.45, 0.45, 0.45, 1.0],
            background: [1.0, 1.0, 1.0, 1.0],
            accent: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// Positions of everything, in content pixels.
#[derive(Clone, Debug, Default)]
struct Layout {
    x: Vec<f32>,
    y: Vec<f32>,
    row: f32,
    /// Pixels per unit of branch length.
    unit: f32,
    lengths: bool,
    show_tips: bool,
    /// Where tip labels end: bars start beyond.
    label_end: f32,
    /// Every bar beside the tips, clade annotations' and other columns'.
    bars: Vec<PlacedBar>,
    /// Columns with an owner: its id, x and bar width.
    columns: Vec<(String, f32, f32)>,
    width: f32,
    height: f32,
    tips_bottom: f32,
    /// The root's x.
    left: f32,
}

pub struct TreeView {
    /// Exact counterpart tips highlighted by a linked alignment selection.
    pub linked_taxa: std::collections::HashSet<String>,
    pub multi_taxa: Option<std::collections::HashSet<String>>,
    width: f32,
    height: f32,
    /// Row spacing and tree width relative to fitting the window.
    row_zoom: f32,
    width_zoom: f32,
    scroll_x: f32,
    scroll_y: f32,
    pub selected: Option<usize>,
    /// Screen pixels per layout pixel; lines snap to screen pixels.
    pixel_ratio: f32,
    layout: Layout,
    /// Bar columns after the clade annotations' own.
    columns: Vec<TipColumn>,
    columns_revision: u64,
    /// What the layout was computed for: document version, columns, size, zooms.
    layout_key: Option<(u64, u64, u32, u32, u32, u32)>,
}

impl TreeView {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            linked_taxa: std::collections::HashSet::new(),
            multi_taxa: None,
            width,
            height,
            row_zoom: 1.0,
            width_zoom: 1.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            selected: None,
            pixel_ratio: 1.0,
            layout: Layout::default(),
            columns: Vec::new(),
            columns_revision: 0,
            layout_key: None,
        }
    }

    /// Show these bar columns after any clade annotation bars.
    pub fn set_columns(&mut self, columns: Vec<TipColumn>) {
        if self.columns != columns {
            self.columns = columns;
            self.columns_revision += 1;
        }
    }

    /// Find Name hits carry node identity, not a column-zero sequence span.
    pub fn overview_match(&self, node: usize) -> Option<crate::overview::MatchMark> {
        let (w, h) = (
            f64::from(self.layout.width.max(1.0)),
            f64::from(self.layout.height.max(1.0)),
        );
        let x = f64::from(*self.layout.x.get(node)?) / w;
        let y = f64::from(*self.layout.y.get(node)? - self.layout.row / 2.0) / h;
        let height = f64::from(self.layout.row) / h;
        if !(0.0..=1.0).contains(&x) || y < 0.0 || y + height > 1.0 {
            return None;
        }
        Some(crate::overview::MatchMark {
            x,
            y,
            width: 1.0 - x,
            height,
        })
    }

    /// Simplified current layout, including the active owned species bands.
    pub fn overview(&self, doc: &TreeDoc, theme: &Theme) -> crate::overview::Miniature {
        let mut out = crate::overview::Miniature::default();
        let (w, h) = (self.layout.width.max(1.0), self.layout.height.max(1.0));
        let n = doc.nodes().len();
        let stride = n.div_ceil(4096).max(1);
        for i in (0..n).step_by(stride) {
            if let Some(parent) = doc.node(i).parent {
                let (x, y) = (self.layout.x[i], self.layout.y[i]);
                let (px, py) = (self.layout.x[parent], self.layout.y[parent]);
                out.line(
                    [
                        f64::from(px / w),
                        f64::from(py / h),
                        f64::from(px / w),
                        f64::from(y / h),
                    ],
                    theme.foreground_muted,
                );
                out.line(
                    [
                        f64::from(px / w),
                        f64::from(y / h),
                        f64::from(x / w),
                        f64::from(y / h),
                    ],
                    theme.foreground_muted,
                );
            }
        }
        let stride = self.layout.bars.len().div_ceil(4096).max(1);
        for b in self.layout.bars.iter().step_by(stride) {
            if let Some(color) = crate::frame::hex(&b.color) {
                out.rect(
                    [
                        f64::from(b.x / w),
                        f64::from(b.y / h),
                        f64::from(b.w / w),
                        f64::from(b.h / h),
                    ],
                    color,
                );
            }
        }
        out
    }

    /// Reuse the layout's existing invalidation key, excluding scroll position.
    pub fn overview_key(&self) -> String {
        format!("{:?}", self.layout_key)
    }

    /// Every bar beside the tips, in content coordinates.
    pub fn bars(&self) -> &[PlacedBar] {
        &self.layout.bars
    }

    /// The bar under a point, if any.
    pub fn bar_at(&self, x: f32, y: f32) -> Option<&PlacedBar> {
        let (x, y) = (x + self.scroll_x, y + self.scroll_y);
        self.layout
            .bars
            .iter()
            .find(|b| x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h)
    }

    /// Columns with an owner, on screen: id, x and bar width.
    pub fn column_positions(&self) -> Vec<(String, f32, f32)> {
        self.layout
            .columns
            .iter()
            .map(|(id, x, w)| (id.clone(), x - self.scroll_x, *w))
            .collect()
    }

    pub fn set_pixel_ratio(&mut self, ratio: f32) {
        self.pixel_ratio = if ratio > 0.0 { ratio } else { 1.0 };
    }

    /// Visible-row geometry for aligned, independent overlays using this canvas.
    pub fn tip_rows(&self, doc: &TreeDoc) -> Vec<(String, f32, f32)> {
        doc.tips_in_order()
            .iter()
            .map(|&i| {
                (
                    doc.node(i).name.clone(),
                    self.layout.y[i] - self.scroll_y,
                    self.layout.row,
                )
            })
            .collect()
    }

    pub fn node_position(&self, node: usize) -> Option<(f32, f32)> {
        Some((
            *self.layout.x.get(node)? - self.scroll_x,
            *self.layout.y.get(node)? - self.scroll_y,
        ))
    }

    pub fn size(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// Recompute the layout if the document or the view changed.
    pub fn sync(&mut self, doc: &TreeDoc) {
        if self.selected.is_some_and(|s| s >= doc.nodes().len()) {
            self.selected = None;
        }
        let key = (
            doc.version(),
            self.columns_revision,
            self.width.to_bits(),
            self.height.to_bits(),
            self.row_zoom.to_bits(),
            self.width_zoom.to_bits(),
        );
        if self.layout_key != Some(key) {
            self.layout = layout_with(
                doc,
                &self.columns,
                self.width,
                self.height,
                self.row_zoom,
                self.width_zoom,
            );
            self.layout_key = Some(key);
            self.clamp_scroll();
        }
    }

    /// Lay out afresh on the next sync. Needed when a different tree replaces
    /// the document: its version can equal the old one's.
    pub fn forget_layout(&mut self) {
        self.layout_key = None;
    }

    fn clamp_scroll(&mut self) {
        self.scroll_y = self
            .scroll_y
            .clamp(0.0, (self.layout.height - self.height).max(0.0));
        self.scroll_x = self
            .scroll_x
            .clamp(0.0, (self.layout.width - self.width).max(0.0));
    }

    // Zoom and scroll.

    pub fn fit(&mut self) {
        self.row_zoom = 1.0;
        self.width_zoom = 1.0;
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
    }

    /// Zoom rows (`vertical`) or the tree's width by `factor`, keeping the
    /// point under `(x, y)` in place.
    pub fn zoom_at(&mut self, doc: &TreeDoc, x: f32, y: f32, factor: f32, vertical: bool) {
        self.sync(doc);
        if vertical {
            let before = self.row_zoom;
            self.row_zoom = (self.row_zoom * factor).clamp(1.0, 200.0);
            let f = self.row_zoom / before;
            let content_y = y + self.scroll_y;
            self.sync(doc);
            self.scroll_y = (content_y - MARGIN) * f + MARGIN - y;
        } else {
            let before = self.width_zoom;
            self.width_zoom = (self.width_zoom * factor).clamp(0.2, 50.0);
            let f = self.width_zoom / before;
            let content_x = x + self.scroll_x;
            self.sync(doc);
            self.scroll_x = (content_x - MARGIN) * f + MARGIN - x;
        }
        self.clamp_scroll();
    }

    /// Zoom rows and width together about the middle of the view.
    pub fn zoom_both(&mut self, doc: &TreeDoc, factor: f32) {
        let (x, y) = (self.width / 2.0, self.height / 2.0);
        self.zoom_at(doc, x, y, factor, true);
        self.zoom_at(doc, x, y, factor, false);
    }

    /// The side of the root branch to measure the root's position from:
    /// the selection if it's one of the root's two children, else the
    /// first. With the branch's length and that side's share of it.
    pub fn root_side(&self, doc: &TreeDoc) -> Option<(usize, usize, f64, f64)> {
        let (a, b, length) = doc.root_branch()?;
        let (side, other) = if self.selected == Some(b) {
            (b, a)
        } else {
            (a, b)
        };
        let share = doc.node(side).length? / length.max(f64::MIN_POSITIVE);
        Some((side, other, length, share))
    }

    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        self.scroll_x += dx;
        self.scroll_y += dy;
        self.clamp_scroll();
    }

    /// Scroll bar position and size, as fractions.
    pub fn vscroll(&self) -> (f32, f32) {
        fraction(self.scroll_y, self.height, self.layout.height)
    }

    pub fn hscroll(&self) -> (f32, f32) {
        fraction(self.scroll_x, self.width, self.layout.width)
    }

    pub fn vscroll_to(&mut self, f: f32) {
        self.scroll_y = f * self.layout.height;
        self.clamp_scroll();
    }

    pub fn hscroll_to(&mut self, f: f32) {
        self.scroll_x = f * self.layout.width;
        self.clamp_scroll();
    }

    pub fn zoom_percent(&self) -> f64 {
        f64::from(self.row_zoom) * 100.0
    }

    pub fn zoom_text(&self) -> String {
        crate::zoom::percent_text(self.zoom_percent())
    }

    /// Scroll so `node` is in view.
    pub fn reveal(&mut self, node: usize) {
        let Some(&y) = self.layout.y.get(node) else {
            return;
        };
        if y < self.scroll_y + MARGIN {
            self.scroll_y = y - MARGIN * 2.0;
        } else if y > self.scroll_y + self.height - MARGIN {
            self.scroll_y = y - self.height + MARGIN * 2.0;
        }
        self.clamp_scroll();
    }

    // Selection.

    /// The node under a point: a tip by its label, a clade by the branch
    /// above it or its vertical line.
    pub fn node_at(&self, doc: &TreeDoc, x: f32, y: f32) -> Option<usize> {
        let (x, y) = (x + self.scroll_x, y + self.scroll_y);
        let l = &self.layout;
        let tol = (l.row * 0.45).clamp(3.0, 8.0);
        let mut best: Option<(f32, usize)> = None;
        let mut offer = |d: f32, n: usize| {
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, n));
            }
        };
        for (i, node) in doc.nodes().iter().enumerate() {
            let (nx, ny) = (l.x[i], l.y[i]);
            if node.is_tip() && l.show_tips {
                // The label's box.
                if x >= nx && x <= l.label_end && (y - ny).abs() <= l.row / 2.0 {
                    offer(0.0, i);
                }
            }
            let px = node.parent.map_or(nx - 10.0, |p| l.x[p]);
            if x >= px - 2.0 && x <= nx + 2.0 && (y - ny).abs() <= tol {
                offer((y - ny).abs(), i);
            }
            if !node.is_tip() {
                let (a, b) = (
                    l.y[node.children[0]],
                    l.y[*node.children.last().unwrap_or(&node.children[0])],
                );
                if (x - nx).abs() <= tol && y >= a - 2.0 && y <= b + 2.0 {
                    offer((x - nx).abs(), i);
                }
            }
        }
        best.map(|(_, n)| n)
    }

    pub fn select_at(&mut self, doc: &TreeDoc, x: f32, y: f32) -> Option<usize> {
        self.multi_taxa = None;
        self.selected = self.node_at(doc, x, y);
        self.selected
    }

    pub fn selected_names(&self, doc: &TreeDoc) -> Vec<String> {
        if let Some(names) = &self.multi_taxa {
            return doc
                .nodes()
                .iter()
                .filter(|n| n.is_tip() && names.contains(&n.name))
                .map(|n| n.name.clone())
                .collect();
        }
        let mut stack: Vec<_> = self.selected.into_iter().collect();
        let mut names = Vec::new();
        while let Some(n) = stack.pop() {
            if doc.node(n).is_tip() {
                names.push(doc.node(n).name.clone());
            } else {
                stack.extend(&doc.node(n).children);
            }
        }
        names
    }

    /// Maximal clades entirely contained in the selected taxon set.
    pub fn selected_roots(&self, doc: &TreeDoc) -> Vec<usize> {
        let Some(names) = &self.multi_taxa else {
            return self.selected.into_iter().collect();
        };
        let mut included = vec![false; doc.nodes().len()];
        let order = doc.preorder();
        for &n in order.iter().rev() {
            included[n] = if doc.node(n).is_tip() {
                names.contains(&doc.node(n).name)
            } else {
                doc.node(n).children.iter().all(|&c| included[c])
            };
        }
        order
            .into_iter()
            .filter(|&n| included[n] && doc.node(n).parent.is_none_or(|p| !included[p]))
            .collect()
    }

    /// Ctrl-click adds/removes a clade's exact tip set, without filling in its MRCA.
    pub fn toggle_clade(&mut self, doc: &TreeDoc, node: usize) {
        let previous: std::collections::HashSet<_> = self.selected_names(doc).into_iter().collect();
        self.multi_taxa = None;
        self.selected = Some(node);
        let clicked = self.selected_names(doc);
        let mut union = previous;
        if clicked.iter().all(|n| union.contains(n)) {
            for name in clicked {
                union.remove(&name);
            }
        } else {
            union.extend(clicked);
        }
        if union.is_empty() {
            self.selected = None;
        }
        self.multi_taxa = Some(union);
    }

    /// Move the selection: to the parent, the first child, or a sibling.
    pub fn select_parent(&mut self, doc: &TreeDoc) {
        self.multi_taxa = None;
        if let Some(p) = self.selected.and_then(|s| doc.node(s).parent) {
            self.selected = Some(p);
            self.reveal(p);
        }
    }

    pub fn select_child(&mut self, doc: &TreeDoc) {
        self.multi_taxa = None;
        let s = self.selected.unwrap_or(doc.root());
        if let Some(&c) = doc.node(s).children.first() {
            self.selected = Some(c);
            self.reveal(c);
        } else {
            self.selected = Some(s);
        }
    }

    pub fn select_sibling(&mut self, doc: &TreeDoc, by: i32) {
        self.multi_taxa = None;
        let Some(s) = self.selected else {
            self.selected = Some(doc.root());
            return;
        };
        let Some(p) = doc.node(s).parent else { return };
        let kids = &doc.node(p).children;
        let i = kids.iter().position(|&c| c == s).unwrap_or(0) as i32 + by;
        if let Some(&c) = usize::try_from(i).ok().and_then(|i| kids.get(i)) {
            self.selected = Some(c);
            self.reveal(c);
        }
    }

    // Operations on the selection.

    /// Whole-tree commands do not depend on the current selection.
    pub fn can_auto_root(doc: &TreeDoc) -> bool {
        doc.tip_count() >= 2 && doc.has_lengths()
    }

    pub fn reroot_op(&self, doc: &TreeDoc) -> Option<Op> {
        let s = self.selected.filter(|&s| s != doc.root())?;
        Some(Op::TreeReroot {
            clade: doc.clade_ref(s),
        })
    }

    /// Flip the selected subtree; for a tip, its parent's.
    pub fn rotate_op(&self, doc: &TreeDoc) -> Option<Op> {
        let s = self.selected?;
        let s = if doc.node(s).is_tip() {
            doc.node(s).parent?
        } else {
            s
        };
        Some(Op::TreeRotate {
            clade: doc.clade_ref(s),
        })
    }

    /// The annotation on the selected clade, if any, and its id.
    pub fn selected_annotation(&self, doc: &TreeDoc) -> Option<(u32, CladeAnnotation)> {
        let s = self.selected?;
        doc.annotations()
            .iter()
            .find(|(_, a)| doc.resolve(&a.clade).ok() == Some(s))
            .map(|(&id, a)| (id, a.clone()))
    }

    // Text.

    pub fn hover_text(&self, doc: &TreeDoc, x: f32, y: f32) -> Option<String> {
        if let Some(bar) = self.bar_at(x, y) {
            return Some(bar.label.clone());
        }
        let n = self.node_at(doc, x, y)?;
        Some(describe_node(doc, n))
    }

    pub fn status(&self, doc: &TreeDoc) -> String {
        // Name the support sets, so the numbers on the branches say what they are.
        let support = if doc.supports().is_empty() {
            "no support values".to_string()
        } else {
            let names: Vec<&str> = doc.supports().iter().map(|s| s.name.as_str()).collect();
            format!("support: {}", names.join(", "))
        };
        let mut parts = vec![format!("{} tips · {support}", doc.tip_count())];
        if let Some(s) = self.selected {
            parts.push(describe_node(doc, s));
        }
        let moved: Vec<String> = doc
            .displaced_annotations()
            .iter()
            .map(|id| format!("{:?}", doc.annotations()[id].label))
            .collect();
        if !moved.is_empty() {
            parts.push(format!(
                "moved labels, no longer on the group they were made for: {}",
                moved.join(", ")
            ));
        }
        parts.join("   ·   ")
    }

    // Drawing.

    /// Draw the visible part of the tree.
    pub fn build(&mut self, doc: &TreeDoc, theme: &Theme, frame: &mut Frame) {
        self.sync(doc);
        frame.clear();
        let pal = Palette::from_theme(theme);
        let clip = (self.scroll_y - 40.0, self.scroll_y + self.height + 40.0);
        let shapes = shapes(doc, &self.layout, &pal, self.selected, Some(clip));
        let (dx, dy) = (-self.scroll_x, -self.scroll_y);
        for s in &shapes {
            draw(frame, s, dx, dy, self.pixel_ratio);
        }
        for (i, node) in doc.nodes().iter().enumerate() {
            if node.is_tip()
                && (self.linked_taxa.contains(&node.name)
                    || self
                        .multi_taxa
                        .as_ref()
                        .is_some_and(|names| names.contains(&node.name)))
            {
                frame.disc(
                    self.layout.x[i] + dx,
                    self.layout.y[i] + dy,
                    4.0,
                    theme.accent,
                );
            }
        }
    }
}

fn fraction(pos: f32, view: f32, content: f32) -> (f32, f32) {
    if content <= view {
        (0.0, 1.0)
    } else {
        (pos / content, view / content)
    }
}

/// A node as people read it, for tooltips and the status line.
fn describe_node(doc: &TreeDoc, n: usize) -> String {
    let node = doc.node(n);
    let mut parts = Vec::new();
    if node.is_tip() {
        parts.push(tip_label(doc, &node.name));
    } else {
        let tips = doc.tip_counts()[n];
        parts.push(if node.name.is_empty() {
            format!("clade of {tips} tips")
        } else {
            format!("{} ({tips} tips)", node.name)
        });
        let values: Vec<String> = doc
            .supports()
            .iter()
            .filter_map(|s| Some(format!("{} {}", s.name, s.formatted(n, doc.style())?)))
            .collect();
        if !values.is_empty() {
            parts.push(values.join(", "));
        }
    }
    if let Some(l) = node.length.filter(|_| n != doc.root()) {
        parts.push(format!("branch {}", format_length(l)));
    }
    parts.join(" · ")
}

/// Underscores in names read as spaces, as in most tree viewers.
pub fn display_name(name: &str) -> String {
    name.replace('_', " ")
}

fn format_length(l: f64) -> String {
    if l == 0.0 {
        "0".into()
    } else if l.abs() > 0.001 {
        l.to_string()
    } else {
        format!("{l:.2e}")
    }
}

fn text_w(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * GLYPH_ADVANCE
}

fn shown_supports(doc: &TreeDoc) -> Vec<&basecraft_docs::SupportSet> {
    if doc.style().support_shown.is_empty() {
        doc.supports().iter().collect()
    } else {
        doc.style()
            .support_shown
            .iter()
            .filter_map(|name| doc.support(name))
            .collect()
    }
}

fn support_legend(doc: &TreeDoc) -> Option<String> {
    let shown = shown_supports(doc);
    (doc.style().support_legend && shown.len() > 1).then(|| {
        shown
            .iter()
            .map(|set| set.name.as_str())
            .collect::<Vec<_>>()
            .join(" / ")
    })
}

fn footer_space(doc: &TreeDoc) -> f32 {
    if (doc.has_lengths() && doc.style().scale_bar) || support_legend(doc).is_some() {
        SCALE_SPACE.max(doc.style().support_font as f32 + 28.0)
    } else {
        0.0
    }
}

/// Display settings never alter Newick names or matching keys.
pub fn tip_label(doc: &TreeDoc, name: &str) -> String {
    doc.style()
        .tip_labels
        .get(name)
        .cloned()
        .unwrap_or_else(|| display_name(name))
}

impl TreeView {
    pub fn reveal_find(&mut self, node: usize) {
        self.reveal(node);
        // Include the label even when a wide tree was horizontally panned.
        if let Some(&x) = self.layout.x.get(node)
            && (x < self.scroll_x || self.layout.label_end > self.scroll_x + self.width)
        {
            self.scroll_x = x - self.width * 0.25;
            self.clamp_scroll();
        }
    }
    pub fn draw_find(&self, find: &crate::find::Session, theme: &Theme, frame: &mut Frame) {
        for (i, hit) in find.results.hits.iter().enumerate() {
            let Some((x, y)) = self.node_position(hit.row) else {
                continue;
            };
            let h = self.layout.row;
            if y < 0.0 || y > self.height {
                continue;
            }
            let x = x.max(0.0);
            let end = (self.layout.label_end - self.scroll_x).min(self.width);
            crate::find::outline(
                frame,
                (x, y - h * 0.5, end - x, h),
                theme,
                i == find.current,
            );
        }
    }
}

#[cfg(test)]
fn layout(doc: &TreeDoc, width: f32, height: f32, row_zoom: f32, width_zoom: f32) -> Layout {
    layout_with(doc, &[], width, height, row_zoom, width_zoom)
}

/// A clade label as drawn: marked when it no longer names the group it was
/// made for, as after a reroot.
fn shown_label(displaced: &[u32], id: u32, label: &str) -> String {
    if label.is_empty() || !displaced.contains(&id) {
        label.to_owned()
    } else {
        format!("{label} (moved)")
    }
}

/// One column per nesting level of the clade annotations with bars: a bar
/// that contains others goes outside them.
fn annotation_columns(doc: &TreeDoc, tips: &[usize]) -> Vec<TipColumn> {
    let displaced = doc.displaced_annotations();
    let ranges: Vec<(u32, &CladeAnnotation, usize, usize)> = doc
        .annotations()
        .iter()
        .filter(|(_, a)| a.bar)
        .filter_map(|(&id, a)| {
            let node = doc.resolve(&a.clade).ok()?;
            let (f, l) = doc.tip_range(node);
            let pos = |t| tips.iter().position(|&x| x == t).unwrap_or(0);
            Some((id, a, pos(f), pos(l)))
        })
        .collect();
    let mut level = vec![0usize; ranges.len()];
    let mut by_size: Vec<usize> = (0..ranges.len()).collect();
    by_size.sort_by_key(|&k| ranges[k].3 - ranges[k].2);
    for &k in &by_size {
        let (_, _, a0, a1) = ranges[k];
        level[k] = by_size
            .iter()
            .filter(|&&j| j != k)
            .filter(|&&j| {
                let (_, _, b0, b1) = ranges[j];
                b0 >= a0 && b1 <= a1 && (b1 - b0) < (a1 - a0)
            })
            .map(|&j| level[j] + 1)
            .max()
            .unwrap_or(0);
    }
    let levels = level.iter().copied().max().map_or(0, |c| c + 1);
    let mut columns = vec![
        TipColumn {
            gap: BAR_GAP,
            labels: true,
            vertical_labels: doc.style().vertical_bar_labels,
            ..TipColumn::default()
        };
        levels
    ];
    for (k, &(id, a, first, last)) in ranges.iter().enumerate() {
        for &t in &tips[first..=last.max(first)] {
            columns[level[k]]
                .tips
                .entry(doc.node(t).name.clone())
                .or_default()
                .push(TipBand {
                    id: id.to_string(),
                    share: 1.0,
                    width: a.bar_width as f32,
                    color: a.colour.clone(),
                    label: shown_label(&displaced, id, &a.label),
                });
        }
    }
    columns
}

/// A column's width: its widest bar and, when labelled, room for the labels.
fn column_width(column: &TipColumn, font: f32) -> f32 {
    let bands = || column.tips.values().flatten();
    let bar = bands().map(|b| b.width).fold(0.0, f32::max);
    if !column.labels {
        return bar;
    }
    let label = if column.vertical_labels {
        font + 4.0
    } else {
        bands().map(|b| text_w(&b.label, font)).fold(0.0, f32::max) + 6.0
    };
    bar + 4.0 + label
}

/// Lay the columns out from `start`: one bar per run of touching rows of a
/// band, less the column's spacing. Returns the bars and the owned columns.
fn place_bars(
    doc: &TreeDoc,
    tips: &[usize],
    columns: &[&TipColumn],
    y: &[f32],
    row: f32,
    start: f32,
) -> (Vec<PlacedBar>, Vec<(String, f32, f32)>) {
    let font = doc.style().tip_font as f32;
    let mut bars: Vec<PlacedBar> = Vec::new();
    let mut owned = Vec::new();
    let mut x = start;
    for column in columns {
        x += column.gap;
        let first = bars.len();
        for &t in tips {
            let Some(bands) = column.tips.get(&doc.node(t).name) else {
                continue;
            };
            let total: f32 = bands.iter().map(|b| b.share.max(0.0)).sum();
            let mut top = y[t] - row / 2.0;
            for band in bands {
                let h = if total > 0.0 {
                    row * band.share.max(0.0) / total
                } else {
                    0.0
                };
                let in_column = bars.len() > first;
                match bars.last_mut() {
                    Some(last)
                        if in_column && last.id == band.id && top <= last.y + last.h + 0.5 =>
                    {
                        last.h = top + h - last.y;
                    }
                    _ => bars.push(PlacedBar {
                        column: column.id.clone(),
                        id: band.id.clone(),
                        x,
                        y: top,
                        w: band.width,
                        h,
                        color: band.color.clone(),
                        label: band.label.clone(),
                        round: column.round,
                        label_at: column
                            .labels
                            .then_some((x + band.width + 4.0, column.vertical_labels)),
                    }),
                }
                top += h;
            }
        }
        // Space between successive bars; clade bars keep a share of a row.
        for bar in &mut bars[first..] {
            let inset = match column.spacing {
                Some(spacing) => (spacing / 2.0).min((bar.h - 2.0).max(0.0) / 2.0),
                None => (row * 0.2).min(4.0),
            };
            bar.y += inset;
            bar.h = (bar.h - 2.0 * inset).max(1.0);
        }
        let width = column_width(column, font);
        if !column.id.is_empty() {
            let bar = column
                .tips
                .values()
                .flatten()
                .map(|b| b.width)
                .fold(0.0, f32::max);
            owned.push((column.id.clone(), x, bar));
        }
        x += width;
    }
    (bars, owned)
}

fn layout_with(
    doc: &TreeDoc,
    extra: &[TipColumn],
    width: f32,
    height: f32,
    row_zoom: f32,
    width_zoom: f32,
) -> Layout {
    let style = doc.style();
    let n = doc.nodes().len();
    let order = doc.preorder();
    let tips = doc.tips_in_order();
    let lengths = doc.has_lengths();
    // Depth along x, in branch length units (or levels).
    let mut depth = vec![0.0f64; n];
    if lengths {
        for &i in &order {
            if let Some(p) = doc.node(i).parent {
                depth[i] = depth[p] + doc.node(i).length.unwrap_or(0.0).max(0.0);
            }
        }
    } else {
        let mut height_of = vec![0.0f64; n];
        for &i in order.iter().rev() {
            let node = doc.node(i);
            height_of[i] = node
                .children
                .iter()
                .map(|&c| height_of[c] + 1.0)
                .fold(0.0, f64::max);
        }
        let top = height_of[doc.root()];
        for i in 0..n {
            depth[i] = top - height_of[i];
        }
    }
    let max_depth = depth.iter().copied().fold(0.0, f64::max);
    let scale = footer_space(doc);
    let avail_h = height - 2.0 * MARGIN - scale;
    let row = (avail_h / tips.len().max(1) as f32).max(0.5) * row_zoom;
    let tip_font = style.tip_font as f32;
    let show_tips = row >= tip_font * 0.7;
    let label_w = if show_tips {
        tips.iter()
            .map(|&t| text_w(&tip_label(doc, &doc.node(t).name), tip_font))
            .fold(0.0, f32::max)
            + LABEL_GAP
    } else {
        0.0
    };
    let columns = annotation_columns(doc, &tips);
    let columns: Vec<&TipColumn> = columns.iter().chain(extra).collect();
    let column_w: Vec<f32> = columns.iter().map(|c| column_width(c, tip_font)).collect();
    let bars_w: f32 = columns.iter().zip(&column_w).map(|(c, w)| c.gap + w).sum();
    // Room at the left for support values beside branches near the root.
    let support_chars = (0..n)
        .filter(|&i| i != doc.root())
        .map(|i| {
            doc.supports()
                .iter()
                .map(|s| s.formatted(i, style).map_or(1, |v| v.len()) + 1)
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let left = MARGIN + support_chars as f32 * style.support_font as f32 * GLYPH_ADVANCE;
    let avail_w = (width - MARGIN - left - label_w - bars_w).max(60.0);
    let unit = if max_depth > 0.0 {
        (f64::from(avail_w * width_zoom) / max_depth) as f32
    } else {
        0.0
    };
    let mut x = vec![0.0f32; n];
    for i in 0..n {
        x[i] = left + (depth[i] as f32) * unit;
    }
    let mut y = vec![0.0f32; n];
    for (k, &t) in tips.iter().enumerate() {
        y[t] = MARGIN + row * (k as f32 + 0.5);
    }
    for &i in order.iter().rev() {
        let node = doc.node(i);
        if let (Some(&a), Some(&b)) = (node.children.first(), node.children.last()) {
            y[i] = f32::midpoint(y[a], y[b]);
        }
    }
    let tree_right = left + max_depth as f32 * unit;
    let label_end = tree_right + label_w;
    let (bars, placed_columns) = place_bars(doc, &tips, &columns, &y, row, label_end);
    let tips_bottom = MARGIN + row * tips.len() as f32;
    Layout {
        x,
        y,
        row,
        unit,
        lengths,
        show_tips,
        label_end,
        bars,
        columns: placed_columns,
        width: (label_end + bars_w + MARGIN).max(support_legend(doc).map_or(0.0, |text| {
            let scale_width = if lengths && style.scale_bar {
                144.0
            } else {
                0.0
            };
            left + scale_width + text_w(&text, style.support_font as f32) + MARGIN
        })),
        height: tips_bottom + scale + MARGIN,
        tips_bottom,
        left,
    }
}

/// Everything to draw, back to front. `clip` limits it to a band of y.
fn shapes(
    doc: &TreeDoc,
    l: &Layout,
    pal: &Palette,
    selected: Option<usize>,
    clip: Option<(f32, f32)>,
) -> Vec<Shape> {
    let style = doc.style();
    let mut out = Vec::new();
    let visible = |a: f32, b: f32| clip.is_none_or(|(c0, c1)| b >= c0 && a <= c1);
    let tip_font = style.tip_font as f32;
    let small = style.support_font as f32;
    let bw = (style.branch_width as f32).max(0.5);
    let half_row = l.row / 2.0;
    // Which nodes are in the selected clade.
    let mut in_selection = vec![false; doc.nodes().len()];
    if let Some(s) = selected {
        let mut stack = vec![s];
        while let Some(i) = stack.pop() {
            in_selection[i] = true;
            stack.extend(&doc.node(i).children);
        }
    }

    // Fields behind everything.
    for (id, a) in doc.annotations().iter().filter(|(_, a)| a.field) {
        let Ok(node) = doc.resolve(&a.clade) else {
            continue;
        };
        let (f, t) = doc.tip_range(node);
        let (y0, y1) = (l.y[f] - half_row, l.y[t] + half_row);
        if !visible(y0, y1) {
            continue;
        }
        let x0 = l.x[node] - 6.0;
        let id = id.to_string();
        let right = l
            .bars
            .iter()
            .find(|b| b.column.is_empty() && b.id == id)
            .map_or(l.label_end + 4.0, |b| {
                b.x + b.w
                    + 6.0
                    + if style.vertical_bar_labels {
                        tip_font
                    } else {
                        text_w(&a.label, tip_font)
                    }
            });
        out.push(Shape::Gradient {
            x: x0,
            y: y0,
            w: right - x0,
            h: y1 - y0,
            color: hex(&a.colour).unwrap_or(pal.muted),
            from: f32::from(a.opacity_left.min(100)) / 100.0,
            to: f32::from(a.opacity_right.min(100)) / 100.0,
        });
    }

    // Selection band behind the selected clade's tips.
    if let Some(s) = selected {
        let (f, t) = doc.tip_range(s);
        let (y0, y1) = (l.y[f] - half_row, l.y[t] + half_row);
        if visible(y0, y1) {
            out.push(Shape::Rect {
                x: l.x[s] - 4.0,
                y: y0,
                w: l.label_end - l.x[s] + 8.0,
                h: y1 - y0,
                color: alpha(pal.accent, 0.12),
            });
        }
    }

    // Branches.
    for (i, node) in doc.nodes().iter().enumerate() {
        let color = if in_selection[i] {
            pal.accent
        } else {
            pal.foreground
        };
        if let Some(p) = node.parent
            && visible(l.y[i] - bw, l.y[i] + bw)
        {
            let x0 = l.x[p] - bw / 2.0;
            out.push(Shape::Rect {
                x: x0,
                y: l.y[i] - bw / 2.0,
                w: l.x[i] - x0,
                h: bw,
                color,
            });
        }
        if let (Some(&a), Some(&b)) = (node.children.first(), node.children.last()) {
            let (y0, y1) = (l.y[a], l.y[b]);
            if visible(y0, y1) {
                out.push(Shape::Rect {
                    x: l.x[i] - bw / 2.0,
                    y: y0 - bw / 2.0,
                    w: bw,
                    h: y1 - y0 + bw,
                    color,
                });
            }
        }
    }

    // Support: circles at nodes, then values beside branches.
    let shown = shown_supports(doc);
    let symbols = style
        .symbols
        .as_ref()
        .and_then(|s| Some((s, doc.support(&s.set)?)));
    let r = (l.row * 0.32).clamp(2.5, 7.0);
    // Values clear the circles when there are any.
    let gap = if symbols.is_some() { r + 2.0 } else { 3.0 };
    for (i, node) in doc.nodes().iter().enumerate() {
        let Some(p) = node.parent else { continue };
        if node.is_tip() || !visible(l.y[i] - small * 1.5, l.y[i] + small * 1.5) {
            continue;
        }
        if let Some((sym, set)) = symbols
            && let Some(v) = set.display_percent(i, style)
        {
            out.push(Shape::Disc {
                cx: l.x[i],
                cy: l.y[i],
                r,
                color: pal.foreground,
            });
            let fill = if v >= f64::from(sym.high) {
                None
            } else if v >= f64::from(sym.low) {
                Some(mix(pal.foreground, pal.background, 0.55))
            } else {
                Some(pal.background)
            };
            if let Some(color) = fill {
                out.push(Shape::Disc {
                    cx: l.x[i],
                    cy: l.y[i],
                    r: r - 1.2,
                    color,
                });
            }
        }
        if shown.is_empty() || l.row < small * 0.6 {
            continue;
        }
        let parts: Vec<String> = shown
            .iter()
            .map(|s| {
                let min = f64::from(
                    style
                        .support_minimums
                        .get(&s.name)
                        .copied()
                        .unwrap_or(style.support_min),
                );
                match (s.formatted(i, style), s.display_percent(i, style)) {
                    (Some(v), Some(pct)) if pct >= min => v,
                    (Some(v), None) if min == 0.0 => v,
                    _ => "–".into(),
                }
            })
            .collect();
        if parts.len() == 1 && parts[0] == "–" {
            continue;
        }
        let up = match style.support_placement {
            SupportPlacement::Above => true,
            SupportPlacement::Below => false,
            SupportPlacement::Natural => l.y[i] <= l.y[p],
        };
        let y = if up {
            l.y[i] - small - bw / 2.0 - 1.0
        } else {
            l.y[i] + bw / 2.0 + 1.0
        };
        out.push(Shape::Text {
            x: l.x[i] - gap,
            y,
            size: small,
            text: parts.join("/"),
            color: pal.muted,
            align: Align::Right,
            italic: false,
            down: false,
        });
    }

    // Tip labels.
    if l.show_tips {
        for (i, node) in doc.nodes().iter().enumerate() {
            if !node.is_tip() || !visible(l.y[i] - tip_font, l.y[i] + tip_font) {
                continue;
            }
            out.push(Shape::Text {
                x: l.x[i] + LABEL_GAP,
                y: l.y[i] - tip_font / 2.0,
                size: tip_font,
                text: tip_label(doc, &node.name),
                color: if in_selection[i] {
                    pal.accent
                } else {
                    pal.foreground
                },
                align: Align::Left,
                italic: true,
                down: false,
            });
        }
    }

    // Bars beside the tips, with their labels.
    for b in &l.bars {
        if !visible(b.y, b.y + b.h) {
            continue;
        }
        let color = hex(&b.color).unwrap_or(pal.foreground);
        let (x, y, w, h) = (b.x, b.y, b.w, b.h);
        out.push(if b.round {
            Shape::Pill { x, y, w, h, color }
        } else {
            Shape::Rect { x, y, w, h, color }
        });
        if let Some((tx, down)) = b.label_at {
            let mid = y + h / 2.0;
            out.push(Shape::Text {
                x: tx,
                y: if down { mid } else { mid - tip_font / 2.0 },
                size: tip_font,
                text: b.label.clone(),
                color: pal.foreground,
                align: if down { Align::Center } else { Align::Left },
                italic: false,
                down,
            });
        }
    }

    // Names on stem branches.
    let displaced = doc.displaced_annotations();
    for (&id, a) in doc.annotations() {
        let Ok(node) = doc.resolve(&a.clade) else {
            continue;
        };
        let colour = hex(&a.colour).unwrap_or(pal.foreground);
        if a.on_branch && !a.label.is_empty() && visible(l.y[node] - 20.0, l.y[node] + 20.0) {
            let px = doc.node(node).parent.map_or(l.x[node] - 40.0, |p| l.x[p]);
            out.push(Shape::Text {
                x: f32::midpoint(px, l.x[node]),
                y: l.y[node] - small - bw / 2.0 - 2.0,
                size: small + 1.0,
                text: shown_label(&displaced, id, &a.label),
                color: if a.bar || a.field {
                    pal.foreground
                } else {
                    colour
                },
                align: Align::Center,
                italic: false,
                down: false,
            });
        }
    }

    // Shared footer: support legend follows the scalebar without overlap.
    let mut legend_x = l.left;
    // Scale bar.
    if l.lengths && style.scale_bar && l.unit > 0.0 {
        let target = f64::from(120.0 / l.unit);
        let mag = 10f64.powf(target.log10().floor());
        let nice = [1.0, 2.0, 5.0, 10.0]
            .iter()
            .map(|m| m * mag)
            .rfind(|&v| v <= target)
            .unwrap_or(mag);
        let w = nice as f32 * l.unit;
        legend_x += w.max(text_w(&format_length(nice), small)) + 24.0;
        let y = l.tips_bottom + 14.0;
        if visible(y - 4.0, y + 20.0) {
            out.push(Shape::Rect {
                x: l.left,
                y,
                w,
                h: bw.max(1.0),
                color: pal.foreground,
            });
            out.push(Shape::Text {
                x: l.left + w / 2.0,
                y: y + 4.0,
                size: small,
                text: format_length(nice),
                color: pal.foreground,
                align: Align::Center,
                italic: false,
                down: false,
            });
        }
    }
    if let Some(text) = support_legend(doc) {
        let y = l.tips_bottom + 18.0;
        if visible(y, y + small) {
            out.push(Shape::Text {
                x: legend_x,
                y,
                size: small,
                text,
                color: pal.foreground,
                align: Align::Left,
                italic: false,
                down: false,
            });
        }
    }
    out
}

/// Draw one shape into the frame, shifted by `(dx, dy)`. Rectangles (branches,
/// bars) snap to whole screen pixels at `ratio` screen pixels per layout
/// pixel, so every line of a width draws equally thick.
fn draw(frame: &mut Frame, s: &Shape, dx: f32, dy: f32, ratio: f32) {
    match s {
        Shape::Rect { x, y, w, h, color } => {
            let snap = |v: f32| (v * ratio).round() / ratio;
            let (x0, y0) = (snap(x + dx), snap(y + dy));
            // At least one screen pixel, and the width rounded on its own,
            // not by where the edges fall.
            let w = (w * ratio).round().max(1.0) / ratio;
            let h = (h * ratio).round().max(1.0) / ratio;
            frame.rect(x0, y0, w, h, *color);
        }
        Shape::Gradient {
            x,
            y,
            w,
            h,
            color,
            from,
            to,
        } => {
            frame.gradient(x + dx, y + dy, *w, *h, *color, (*from, *to));
        }
        Shape::Disc { cx, cy, r, color } => frame.disc(cx + dx, cy + dy, *r, *color),
        // A rectangle between two discs, along its longer side.
        Shape::Pill { x, y, w, h, color } => {
            let r = w.min(*h) / 2.0;
            let (x, y) = (x + dx, y + dy);
            if h >= w {
                frame.rect(x, y + r, *w, h - 2.0 * r, *color);
                frame.disc(x + r, y + r, r, *color);
                frame.disc(x + r, y + h - r, r, *color);
            } else {
                frame.rect(x + r, y, w - 2.0 * r, *h, *color);
                frame.disc(x + r, y + r, r, *color);
                frame.disc(x + w - r, y + r, r, *color);
            }
        }
        Shape::Text {
            x,
            y,
            size,
            text,
            color,
            align,
            down,
            ..
        } => {
            let w = text_w(text, *size);
            let shift = match align {
                Align::Left => 0.0,
                Align::Right => -w,
                Align::Center => -w / 2.0,
            };
            if *down {
                frame.text_down(x + dx, y + dy + shift, *size, text, *color);
            } else {
                frame.text(x + dx + shift, y + dy, *size, text, *color);
            }
        }
    }
}

/// The whole tree as an SVG figure, black on white, `width` pixels wide
/// and rows `row` pixels apart.
pub fn to_svg(doc: &TreeDoc, width: f32, row: f32) -> String {
    figure(
        doc,
        &[],
        width,
        row,
        Palette::print(),
        "Helvetica, Arial, sans-serif",
    )
    .svg
}

/// Full-tree figure preserving screen colours and typography.
pub fn to_svg_with_style(doc: &TreeDoc, width: f32, row: f32, theme: &Theme, font: &str) -> String {
    figure(doc, &[], width, row, Palette::from_theme(theme), font).svg
}

/// A tree figure with bar columns, and where its owned columns and rows are.
pub struct Figure {
    pub svg: String,
    /// Owned columns: id, x and bar width.
    pub columns: Vec<(String, f32, f32)>,
    pub width: f32,
    pub height: f32,
}

/// Full-tree figure in screen colours with `columns` beside the tips, as the
/// canvas draws them.
pub fn figure_with_columns(
    doc: &TreeDoc,
    columns: &[TipColumn],
    width: f32,
    row: f32,
    theme: &Theme,
    font: &str,
) -> Figure {
    figure(doc, columns, width, row, Palette::from_theme(theme), font)
}

/// Independent selected clades arranged vertically without inventing connecting edges.
pub fn forest_svg(trees: &[TreeDoc], width: f32, row: f32, theme: &Theme, font: &str) -> String {
    let mut figures = Vec::new();
    let mut total = 0.0;
    let mut widest = width;
    for (i, tree) in trees.iter().enumerate() {
        let scale = footer_space(tree);
        let height = tree.tip_count().max(1) as f32 * row + 2.0 * MARGIN + scale;
        let geometry = layout_with(tree, &[], width, height, 1.0, 1.0);
        widest = widest.max(geometry.width);
        let figure = to_svg_with_style(tree, width, row, theme, font)
            .replacen("<svg ", &format!("<svg y=\"{total}\" "), 1)
            .replace("id=\"g", &format!("id=\"clade{i}-g"))
            .replace("url(#g", &format!("url(#clade{i}-g"));
        figures.push(format!("<g id=\"clade-{i}\">{figure}</g>"));
        total += geometry.height;
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{widest}\" height=\"{total}\" viewBox=\"0 0 {widest} {total}\">\n{}\n</svg>\n",
        figures.join("\n")
    )
}

fn figure(
    doc: &TreeDoc,
    columns: &[TipColumn],
    width: f32,
    row: f32,
    pal: Palette,
    font: &str,
) -> Figure {
    let tips = doc.tip_count().max(1) as f32;
    let scale = footer_space(doc);
    let height = tips * row + 2.0 * MARGIN + scale;
    let l = layout_with(doc, columns, width, height, 1.0, 1.0);
    let shapes = shapes(doc, &l, &pal, None, None);
    let (w, h) = (l.width.max(width), l.height);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" \
         viewBox=\"0 0 {w:.1} {h:.1}\" font-family=\"{}\">\n\
         <rect width=\"100%\" height=\"100%\" fill=\"{}\"/>\n",
        xml_escape(font),
        css(pal.background)
    );
    let mut gradients = 0;
    for s in &shapes {
        match s {
            Shape::Pill { x, y, w, h, color } => {
                let _ = writeln!(
                    svg,
                    "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" rx=\"{:.2}\" {}/>",
                    w.min(*h) / 2.0,
                    fill(*color)
                );
            }
            Shape::Rect { x, y, w, h, color } => {
                let _ = writeln!(
                    svg,
                    "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" {}/>",
                    fill(*color)
                );
            }
            Shape::Gradient {
                x,
                y,
                w,
                h,
                color,
                from,
                to,
            } => {
                gradients += 1;
                let c = css(*color);
                let _ = writeln!(
                    svg,
                    "<defs><linearGradient id=\"g{gradients}\"><stop offset=\"0\" stop-color=\"{c}\" \
                     stop-opacity=\"{:.3}\"/><stop offset=\"1\" stop-color=\"{c}\" \
                     stop-opacity=\"{:.3}\"/></linearGradient></defs>\n\
                     <rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\" \
                     fill=\"url(#g{gradients})\"/>",
                    from * color[3],
                    to * color[3]
                );
            }
            Shape::Disc { cx, cy, r, color } => {
                let _ = writeln!(
                    svg,
                    "<circle cx=\"{cx:.2}\" cy=\"{cy:.2}\" r=\"{r:.2}\" {}/>",
                    fill(*color)
                );
            }
            Shape::Text {
                x,
                y,
                size,
                text,
                color,
                align,
                italic,
                down,
            } => {
                let anchor = match align {
                    Align::Left => "start",
                    Align::Right => "end",
                    Align::Center => "middle",
                };
                let style = if *italic {
                    " font-style=\"italic\""
                } else {
                    ""
                };
                let font_size = size * 0.8;
                let text_length = crate::frame::text_width(text, *size);
                let text = xml_escape(text);
                if *down {
                    // Rotated a quarter turn clockwise about its start.
                    let _ = writeln!(
                        svg,
                        "<text transform=\"translate({:.2},{y:.2}) rotate(90)\" font-size=\"{font_size}\" textLength=\"{text_length}\" lengthAdjust=\"spacingAndGlyphs\" \
                         text-anchor=\"{anchor}\"{style} {}>{text}</text>",
                        x + size * 0.2,
                        fill(*color)
                    );
                } else {
                    let _ = writeln!(
                        svg,
                        "<text x=\"{x:.2}\" y=\"{:.2}\" font-size=\"{font_size}\" textLength=\"{text_length}\" lengthAdjust=\"spacingAndGlyphs\" text-anchor=\"{anchor}\"{style} {}>{text}</text>",
                        y + size * 0.8,
                        fill(*color)
                    );
                }
            }
        }
    }
    svg.push_str("</svg>\n");
    Figure {
        svg,
        columns: l.columns,
        width: w,
        height: h,
    }
}

fn css(c: Rgba) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

fn fill(c: Rgba) -> String {
    if c[3] >= 0.999 {
        format!("fill=\"{}\"", css(c))
    } else {
        format!("fill=\"{}\" fill-opacity=\"{:.3}\"", css(c), c[3])
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use basecraft_protocol::{SupportSymbols, TreeStyle};

    fn tree(text: &str) -> TreeDoc {
        TreeDoc::from_newick("t", text).unwrap()
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    fn texts(shapes: &[Shape]) -> Vec<(String, f32, f32)> {
        shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Text { text, x, y, .. } => Some((text.clone(), *x, *y)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_label_a_reroot_moved_is_marked_and_named_in_the_status() {
        let mut d = tree("((a,b),(c,d),e);");
        for (id, clade, label) in [(1, ["a", "b"], "AB"), (2, ["c", "d"], "CD")] {
            let annotation = Some(CladeAnnotation::new(clade.map(String::from).into(), label));
            d.apply_edit(&Op::TreeAnnotate { id, annotation }).unwrap();
        }
        let view = TreeView::new(400.0, 300.0);
        let labels = |d: &TreeDoc| -> Vec<String> {
            annotation_columns(d, &d.tips_in_order())
                .iter()
                .flat_map(|column| column.tips.values().flatten().map(|band| band.label.clone()))
                .collect()
        };
        assert!(!view.status(&d).contains("moved"));
        assert!(labels(&d).iter().all(|label| !label.contains("moved")));
        d.apply_edit(&Op::TreeReroot {
            clade: vec!["a".into()],
        })
        .unwrap();
        assert!(view.status(&d).contains("moved labels, no longer on the group they were made for: \"AB\""));
        assert!(labels(&d).contains(&"AB (moved)".to_owned()));
        assert!(labels(&d).contains(&"CD".to_owned()));
    }

    #[test]
    fn overview_uses_current_layout_and_owned_band_colours() {
        let d = tree("((A:1,B:2):1,C:3);");
        let mut v = TreeView::new(400.0, 180.0);
        v.set_columns(vec![TipColumn {
            id: "species".into(),
            tips: [(
                "A".into(),
                vec![TipBand {
                    id: "group".into(),
                    share: 1.0,
                    width: 20.0,
                    color: "#abcdef".into(),
                    label: String::new(),
                }],
            )]
            .into(),
            ..TipColumn::default()
        }]);
        v.sync(&d);
        let before = v.overview(&d, &Theme::default()).json();
        assert!(before.contains("#abcdef"));
        let key = v.overview_key();
        assert!(v.overview_match(usize::MAX).is_none());
        assert!(v.overview_match(d.root()).is_some());
        v.hscroll_to(1.0);
        v.vscroll_to(1.0);
        assert_eq!(v.overview_key(), key);
        assert_eq!(v.overview(&d, &Theme::default()).json(), before);
        v.zoom_both(&d, 3.0);
        assert_ne!(v.overview_key(), key);
        assert!(
            v.overview(&d, &Theme::default())
                .shapes
                .iter()
                .all(|s| [s.x, s.y, s.w, s.h].iter().all(|v| v.is_finite()))
        );
    }

    #[test]
    fn metadata_labels_draw_without_replacing_identity() {
        let mut doc = tree("(scientific_ID:1,other:2);");
        let mut style = doc.style().clone();
        style.tip_labels.insert(
            "scientific_ID".into(),
            "Caulerpa taxifolia voucher 123".into(),
        );
        doc.set_style(style);
        let mut view = TreeView::new(800.0, 400.0);
        view.sync(&doc);
        let drawn = texts(&shapes(&doc, &view.layout, &Palette::print(), None, None));
        assert!(
            drawn
                .iter()
                .any(|(text, _, _)| text == "Caulerpa taxifolia voucher 123")
        );
        assert!(doc.nodes().iter().any(|n| n.name == "scientific_ID"));
        assert!(
            view.tip_rows(&doc)
                .iter()
                .any(|(name, _, _)| name == "scientific_ID")
        );
        assert!(
            crate::accessible::tree(&doc, 0)
                .text
                .contains("Caulerpa taxifolia voucher 123")
        );
    }

    #[test]
    fn a_replacement_tree_is_laid_out_afresh() {
        let mut view = TreeView::new(400.0, 300.0);
        let first = tree("((a,b),(c,d));");
        view.sync(&first);
        let flipped = tree("((c,d),(a,b));");
        view.forget_layout();
        view.sync(&flipped);
        let rows = view.tip_rows(&flipped);
        let order: Vec<_> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(order, ["c", "d", "a", "b"]);
        assert!(rows.windows(2).all(|w| w[0].1 < w[1].1), "{rows:?}");
    }

    #[test]
    fn scale_labels_use_decimals_strictly_above_one_thousandth() {
        assert_eq!(format_length(0.001), "1.00e-3");
        assert_eq!(format_length(0.000_999_999_9), "1.00e-3");
        for value in [
            0.001_000_000_01,
            0.0011,
            0.002,
            0.01,
            1.0,
            1000.0,
            1_234.567_89,
            1e20,
            f64::MAX,
        ] {
            let label = format_length(value);
            assert!(!label.contains(['e', 'E']), "{value}: {label}");
            assert_eq!(label.parse::<f64>().unwrap(), value);
        }
        assert_eq!(format_length(0.0), "0");
        assert_ne!(format_length(f64::MIN_POSITIVE), "0");
    }

    #[test]
    fn support_canvas_svg_tooltip_and_accessible_text_share_precision() {
        let mut t = tree("((A:1,B:1)78.654321/0.987654321:1,C:1);");
        let mut style = t.style().clone();
        style.support_decimals.insert("Support 1".into(), 1);
        style.support_decimals.insert("Support 2".into(), 3);
        t.set_style(style);
        let l = layout(&t, 800.0, 600.0, 1.0, 1.0);
        let labels = texts(&shapes(&t, &l, &Palette::print(), None, None));
        assert!(labels.iter().any(|(text, _, _)| text == "78.7/0.988"));
        let node = t.resolve(&names(&["A", "B"])).unwrap();
        assert!(describe_node(&t, node).contains("Support 1 78.7, Support 2 0.988"));
        assert!(crate::accessible::tree(&t, 0).text.contains("78.7\t0.988"));
        let svg = to_svg(&t, 800.0, 20.0);
        assert!(svg.contains("78.7/0.988"));
        assert!(t.to_newick().contains("78.654321/0.987654321"));
    }

    #[test]
    fn multiple_clades_keep_exact_taxa_and_export_separate_figures() {
        let t = tree("((A:1,B:1)90:1,(C:1,D:1)80:1);");
        let mut v = TreeView::new(800., 600.);
        v.selected = t.find_tip("A");
        v.toggle_clade(&t, t.find_tip("C").unwrap());
        assert_eq!(v.selected_names(&t), names(&["A", "C"]));
        let roots = v.selected_roots(&t);
        assert_eq!(roots.len(), 2);
        let figures: Vec<_> = roots.into_iter().map(|n| t.subtree(n).unwrap()).collect();
        let svg = forest_svg(&figures, 500., 20., &Theme::default(), "monospace");
        assert_eq!(svg.matches("<g id=\"clade-").count(), 2);
        assert!(svg.contains(">A</text>"));
        assert!(svg.contains(">C</text>"));
        assert!(!svg.contains(">B</text>"));
        v.toggle_clade(&t, t.find_tip("C").unwrap());
        assert_eq!(v.selected_names(&t), names(&["A"]));
    }

    #[test]
    fn layout_places_tips_in_rows_and_nodes_between() {
        let t = tree("((A:1,B:2)90:1,C:1);");
        let l = layout(&t, 600.0, 300.0, 1.0, 1.0);
        let tips = t.tips_in_order();
        assert!(l.y[tips[0]] < l.y[tips[1]] && l.y[tips[1]] < l.y[tips[2]]);
        let ab = t.resolve(&names(&["A", "B"])).unwrap();
        assert_eq!(l.y[ab], f32::midpoint(l.y[tips[0]], l.y[tips[1]]));
        // B is deepest: 3 units, at the right of the tree area.
        assert!(l.x[tips[1]] > l.x[tips[0]]);
        assert!((l.x[tips[1]] - l.left - 3.0 * l.unit).abs() < 0.01);
        // Room left of the root for the support value beside it.
        assert!(l.left > MARGIN);
        assert!(l.width <= 600.0 + 0.5);
    }

    #[test]
    fn support_sits_above_upper_branches_and_below_lower_ones() {
        let mut t = tree("(((A:1,B:1)90:1,(C:1,D:1)80:1):1,E:1);");
        let l = layout(&t, 600.0, 300.0, 1.0, 1.0);
        let ab = t.resolve(&names(&["A", "B"])).unwrap();
        let cd = t.resolve(&names(&["C", "D"])).unwrap();
        let find =
            |shapes: &[Shape], s: &str| texts(shapes).into_iter().find(|(t, ..)| t == s).unwrap().2;
        let shapes = shapes(&t, &l, &Palette::print(), None, None);
        assert!(find(&shapes, "90") < l.y[ab], "upper child: above");
        assert!(find(&shapes, "80") > l.y[cd], "lower child: below");
        t.set_style(TreeStyle {
            support_placement: SupportPlacement::Above,
            support_min: 85,
            ..TreeStyle::default()
        });
        let shapes = super::shapes(&t, &l, &Palette::print(), None, None);
        assert!(find(&shapes, "90") < l.y[ab]);
        assert!(
            !texts(&shapes).iter().any(|(t, ..)| t == "80"),
            "below the minimum"
        );
    }

    #[test]
    fn ordered_support_thresholds_and_legend_share_figure_geometry() {
        let mut t = tree("((A:1,B:1)95/0.8:1,C:1);");
        let mut style = t.style().clone();
        style.support_shown = names(&["Support 2", "Support 1"]);
        style.support_minimums.insert("Support 2".into(), 90);
        style.support_minimums.insert("Support 1".into(), 90);
        t.set_style(style);
        let l = layout(&t, 600.0, 300.0, 1.0, 1.0);
        let rendered = shapes(&t, &l, &Palette::print(), None, None);
        let text = texts(&rendered);
        assert!(text.iter().any(|(s, ..)| s == "–/95"));
        let legend = rendered
            .iter()
            .find_map(|shape| match shape {
                Shape::Text { x, y, text, .. } if text == "Support 2 / Support 1" => Some((*x, *y)),
                _ => None,
            })
            .unwrap();
        assert!(legend.0 > l.left + 24.0);
        assert!(legend.1 > l.tips_bottom);
        assert!(legend.1 + (t.style().support_font as f32) < l.height);
        let svg = to_svg(&t, 600.0, 20.0);
        assert!(svg.contains("–/95"));
        assert!(svg.contains("Support 2 / Support 1"));
        let mut style = t.style().clone();
        style.scale_bar = false;
        t.set_style(style);
        let l = layout(&t, 600.0, 300.0, 1.0, 1.0);
        let rendered = shapes(&t, &l, &Palette::print(), None, None);
        assert!(rendered.iter().any(|s| matches!(s, Shape::Text {x, text, ..} if text == "Support 2 / Support 1" && *x == l.left)));
        let mut style = t.style().clone();
        style.support_legend = false;
        t.set_style(style);
        assert!(!to_svg(&t, 600.0, 20.0).contains("Support 2 / Support 1"));
    }

    #[test]
    fn bars_nest_outward_and_fields_get_gradients() {
        let mut t = tree("(((A:1,B:1):1,C:1):1,D:1);");
        let mut inner = CladeAnnotation::new(names(&["A", "B"]), "Inner");
        inner.field = true;
        inner.opacity_left = 0;
        inner.opacity_right = 40;
        t.annotate(1, Some(inner)).unwrap();
        t.annotate(2, Some(CladeAnnotation::new(names(&["A", "C"]), "Outer")))
            .unwrap();
        let l = layout(&t, 600.0, 300.0, 1.0, 1.0);
        let x = |id: u32| l.bars.iter().find(|b| b.id == id.to_string()).unwrap().x;
        assert!(x(2) > x(1), "the containing clade's bar sits outside");
        let shapes = shapes(&t, &l, &Palette::print(), None, None);
        assert!(
            shapes.iter().any(
                |s| matches!(s, Shape::Gradient { from, to, .. } if *from == 0.0 && *to == 0.4)
            )
        );
        let mut f = Frame::new();
        for s in &shapes {
            draw(&mut f, s, 0.0, 0.0, 1.0);
        }
        let gradients = f
            .floats()
            .chunks(16)
            .filter(|i| i[14] < 0.0 && i[12] == 0.0 && (i[13] - 0.4).abs() < 1e-6)
            .count();
        assert_eq!(gradients, 1, "the field is one gradient quad");
    }

    #[test]
    fn zooming_both_ways_widens_and_spreads() {
        let t = tree("((A:1,B:2)90:1,C:1);");
        let mut v = TreeView::new(600.0, 300.0);
        v.sync(&t);
        let (w0, h0) = (v.layout.width, v.layout.height);
        let x0 = v.layout.x[t.tips_in_order()[1]];
        v.zoom_both(&t, 1.25);
        v.sync(&t);
        assert!(v.layout.height > h0 * 1.1, "{} vs {h0}", v.layout.height);
        assert!(v.layout.width > w0 * 1.1, "{} vs {w0}", v.layout.width);
        assert!(v.layout.x[t.tips_in_order()[1]] > x0 * 1.1);
    }

    #[test]
    fn lines_snap_to_whole_screen_pixels() {
        // At 1.92 screen pixels per layout pixel, a 1 px line must cover
        // two screen pixels wherever it falls.
        let mut f = Frame::new();
        for k in 0..20 {
            let shape = Shape::Rect {
                x: 3.0,
                y: 10.0 + k as f32 * 0.37,
                w: 50.0,
                h: 1.0,
                color: [0.0, 0.0, 0.0, 1.0],
            };
            draw(&mut f, &shape, 0.0, 0.0, 1.92);
        }
        for i in f.floats().chunks(16) {
            let (top, bottom) = (i[1] * 1.92, i[5] * 1.92);
            assert!((top - top.round()).abs() < 1e-3, "{top}");
            assert!((bottom - top - 2.0).abs() < 1e-3, "{}", bottom - top);
        }
    }

    #[test]
    fn clicking_a_branch_selects_its_clade() {
        let t = tree("((A:1,B:1)90:1,C:1);");
        let mut v = TreeView::new(600.0, 300.0);
        v.sync(&t);
        let ab = t.resolve(&names(&["A", "B"])).unwrap();
        let (x, y) = (f32::midpoint(v.layout.x[0], v.layout.x[ab]), v.layout.y[ab]);
        assert_eq!(v.select_at(&t, x, y + 1.0), Some(ab));
        assert!(matches!(v.rotate_op(&t), Some(Op::TreeRotate { .. })));
        assert!(v.hover_text(&t, x, y).unwrap().contains("Support 90"));
        v.select_parent(&t);
        assert_eq!(v.selected, Some(t.root()));
        assert_eq!(v.reroot_op(&t), None);
    }

    #[test]
    fn svg_has_italic_tips_and_symbols() {
        let mut t = tree("((A_one:1,B:1)95:1,(C:1,D:1)60:1);");
        t.set_style(TreeStyle {
            symbols: Some(SupportSymbols {
                set: "Support".into(),
                high: 90,
                low: 70,
            }),
            ..TreeStyle::default()
        });
        let svg = to_svg(&t, 500.0, 20.0);
        assert!(svg.starts_with("<svg"));
        let tip = svg.lines().find(|l| l.ends_with(">A one</text>")).unwrap();
        assert!(tip.contains("font-style=\"italic\""), "{tip}");
        assert!(svg.matches("<circle").count() >= 2);
    }
}
