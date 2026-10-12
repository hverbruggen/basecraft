//! The alignment view: rows of residues under a ruler, a consensus line and
//! an identity plot, with the row names on the left.
//!
//! [`AlignmentView`] holds the viewport, the cursor and a block selection,
//! builds a [`Frame`] for what's visible, and turns input into alignment
//! operations. It reads the document but never edits it.

// Pixel geometry mixes f32 screen coordinates with u64/f64 alignment
// coordinates; the conversions are deliberate and bounded by the viewport.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]

use basecraft_docs::AlignmentDoc;
use basecraft_docs::alignment::is_gap;
use basecraft_protocol::Op;

use crate::frame::{Frame, Rgba};
use crate::genome::{format_bp, format_thousands, nice_step};
use crate::theme::{Theme, alpha, mix};

const RULER_H: f32 = 22.0;
const IDENTITY_H: f32 = 14.0;
const HEADER_GAP: f32 = 4.0;
const NAME_W: f32 = 170.0;
const NAME_PAD: f32 = 8.0;
const LABEL_SIZE: f32 = 12.0;
const MAX_PX_PER_COL: f64 = 28.0;
/// Zoom 100 %: residues comfortably readable.
const PX_PER_COL_AT_100: f64 = 11.0;
/// Pixels per column from which residue letters are drawn.
const LETTERS_FROM: f64 = 7.0;
/// Below this, the grid shows where residues are, not what they are.
const COLOURS_FROM: f64 = 1.5;
const MIN_ROW_H: f32 = 3.0;
const MAX_ROW_H: f32 = 26.0;
/// Most cells the consensus may read for one frame; past this, zoom in.
const CONSENSUS_BUDGET: u64 = 40_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Above the names: clicking selects everything.
    Corner,
    /// Ruler and consensus: clicking selects columns.
    Header,
    /// Row names: clicking selects rows.
    Names,
    Grid,
}

/// What a drag selection extends over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Span {
    Cells,
    Rows,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    Pan {
        last_x: f32,
        last_y: f32,
    },
    Select,
    /// Pressed inside the selection: moving slides the block; releasing
    /// without moving selects the pressed cell.
    Slide {
        origin: f64,
        applied: i64,
        moved: bool,
        cell: (usize, u64),
    },
}

/// Rows `rows.0..rows.1` by columns `cols.0..cols.1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub rows: (usize, usize),
    pub cols: (u64, u64),
}

impl Block {
    pub fn contains(&self, (row, col): (usize, u64)) -> bool {
        (self.rows.0..self.rows.1).contains(&row) && (self.cols.0..self.cols.1).contains(&col)
    }

    pub fn row_ranges(&self) -> Vec<(u32, u32)> {
        vec![(self.rows.0 as u32, self.rows.1 as u32)]
    }

    pub fn row_count(&self) -> usize {
        self.rows.1 - self.rows.0
    }

    pub fn col_count(&self) -> u64 {
        self.cols.1 - self.cols.0
    }
}

#[derive(Clone, Debug)]
pub struct AlignmentView {
    /// Column at the left edge of the grid.
    pub start: f64,
    pub px_per_col: f64,
    /// Pixels scrolled down from the first row.
    scroll_y: f32,
    pub width: f32,
    pub height: f32,
    pub name_width: f32,
    /// The cell typing goes to.
    pub cursor: (usize, u64),
    anchor: (usize, u64),
    span: Span,
    pub selection: Option<Block>,
    drag: Drag,
    /// The zoom was set by Fit and nothing since; resizing fits again.
    pub auto_fit: bool,
    /// Rows and columns as of the last [`AlignmentView::sync`].
    rows: usize,
    cols: u64,
}

impl AlignmentView {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            start: 0.0,
            px_per_col: PX_PER_COL_AT_100,
            scroll_y: 0.0,
            width: width.max(1.0),
            height: height.max(1.0),
            name_width: NAME_W,
            cursor: (0, 0),
            anchor: (0, 0),
            span: Span::Cells,
            selection: None,
            drag: Drag::None,
            auto_fit: false,
            rows: 0,
            cols: 0,
        }
    }

    /// Take the document's size, and keep the cursor and selection inside
    /// it. Call after opening and after every edit.
    pub fn sync(&mut self, doc: &AlignmentDoc) {
        self.rows = doc.row_count();
        self.cols = doc.width();
        let last = (self.rows.saturating_sub(1), self.cols.saturating_sub(1));
        self.cursor = (self.cursor.0.min(last.0), self.cursor.1.min(last.1));
        self.anchor = (self.anchor.0.min(last.0), self.anchor.1.min(last.1));
        if self.selection.is_some() {
            self.update_block();
        }
        self.clamp();
    }

    /// Keep cursor, selection and viewport attached to surviving columns.
    pub fn map_deleted_columns(&mut self, runs: &[(u64, u64)]) {
        let map = |position: u64| {
            position
                - runs
                    .iter()
                    .map(|&(a, b)| position.min(b).saturating_sub(a))
                    .sum::<u64>()
        };
        self.cursor.1 = map(self.cursor.1);
        self.anchor.1 = map(self.anchor.1);
        self.start = map(self.start.max(0.0) as u64) as f64;
        if let Some(block) = &mut self.selection {
            block.cols = (map(block.cols.0), map(block.cols.1));
        }
    }

    // Geometry.

    pub fn row_height(&self) -> f32 {
        ((self.px_per_col * 1.55) as f32).clamp(MIN_ROW_H, MAX_ROW_H)
    }

    fn letters(&self) -> bool {
        self.px_per_col >= LETTERS_FROM
    }

    fn consensus_h(&self) -> f32 {
        if self.letters() {
            self.row_height()
        } else {
            0.0
        }
    }

    pub fn header_height(&self) -> f32 {
        RULER_H + self.consensus_h() + IDENTITY_H + HEADER_GAP
    }

    fn grid_width(&self) -> f32 {
        (self.width - self.name_width).max(1.0)
    }

    fn grid_height(&self) -> f32 {
        (self.height - self.header_height()).max(1.0)
    }

    pub fn x_of(&self, col: f64) -> f32 {
        self.name_width + ((col - self.start) * self.px_per_col) as f32
    }

    pub fn col_at(&self, x: f32) -> f64 {
        self.start + f64::from(x - self.name_width) / self.px_per_col
    }

    pub fn y_of(&self, row: usize) -> f32 {
        self.header_height() + row as f32 * self.row_height() - self.scroll_y
    }

    fn row_at_f(&self, y: f32) -> f32 {
        (y - self.header_height() + self.scroll_y) / self.row_height()
    }

    pub fn region_at(&self, x: f32, y: f32) -> Region {
        match (x < self.name_width, y < self.header_height()) {
            (true, true) => Region::Corner,
            (false, true) => Region::Header,
            (true, false) => Region::Names,
            (false, false) => Region::Grid,
        }
    }

    /// The cell under a point, clamped into the alignment.
    pub fn cell_at(&self, x: f32, y: f32) -> (usize, u64) {
        let row = self.row_at_f(y).max(0.0) as usize;
        let col = self.col_at(x).max(0.0) as u64;
        (
            row.min(self.rows.saturating_sub(1)),
            col.min(self.cols.saturating_sub(1)),
        )
    }

    /// The cell under a point, if there is one.
    fn cell_hit(&self, x: f32, y: f32) -> Option<(usize, u64)> {
        let (r, c) = (self.row_at_f(y), self.col_at(x));
        (r >= 0.0 && c >= 0.0 && (r as usize) < self.rows && (c as u64) < self.cols)
            .then_some((r as usize, c as u64))
    }

    fn visible_rows(&self) -> std::ops::Range<usize> {
        let h = self.row_height();
        let first = (self.scroll_y / h).floor().max(0.0) as usize;
        let last = ((self.scroll_y + self.grid_height()) / h).ceil() as usize + 1;
        first.min(self.rows)..last.min(self.rows)
    }

    fn visible_cols(&self) -> (u64, u64) {
        let c0 = self.start.floor().max(0.0) as u64;
        let c1 = (self.start + f64::from(self.grid_width()) / self.px_per_col).ceil() as u64 + 1;
        (c0.min(self.cols), c1.min(self.cols))
    }

    // Viewport.

    fn min_px_per_col(&self) -> f64 {
        (f64::from(self.grid_width()) / self.cols.max(1) as f64).min(1.0)
    }

    fn clamp(&mut self) {
        self.px_per_col = self.px_per_col.clamp(self.min_px_per_col(), MAX_PX_PER_COL);
        let span = f64::from(self.grid_width()) / self.px_per_col;
        let cols = self.cols as f64;
        self.start = if span >= cols {
            0.0
        } else {
            self.start.clamp(0.0, cols - span)
        };
        let total = self.rows as f32 * self.row_height();
        self.scroll_y = self
            .scroll_y
            .clamp(0.0, (total - self.grid_height()).max(0.0));
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        if self.auto_fit {
            // The pane often settles into its final size after a fit.
            self.fit();
        }
        self.clamp();
    }

    /// The whole width of the alignment in view, but no closer than 100 %.
    pub fn fit(&mut self) {
        self.px_per_col =
            (f64::from(self.grid_width()) / self.cols.max(1) as f64).min(PX_PER_COL_AT_100);
        self.start = 0.0;
        self.scroll_y = 0.0;
        self.clamp();
        self.auto_fit = true;
    }

    /// 100 % zoom from the top left.
    pub fn reset_zoom(&mut self) {
        self.auto_fit = false;
        self.px_per_col = PX_PER_COL_AT_100;
        self.start = 0.0;
        self.scroll_y = 0.0;
        self.clamp();
    }

    /// Zoom by `factor` (> 1 zooms in), keeping the cell under `(x, y)` put.
    pub fn zoom_at(&mut self, x: f32, y: f32, factor: f64) {
        self.auto_fit = false;
        let col = self.col_at(x);
        let row = self.row_at_f(y);
        self.px_per_col *= factor;
        self.clamp();
        self.start = col - f64::from(x - self.name_width) / self.px_per_col;
        self.scroll_y = row * self.row_height() - (y - self.header_height());
        self.clamp();
    }

    /// Zoom so the selection's columns fill the width, and show its top row.
    pub fn zoom_to_selection(&mut self) {
        self.auto_fit = false;
        let b = self.block();
        self.px_per_col = f64::from(self.grid_width()) / b.col_count().max(1) as f64;
        self.clamp();
        self.start = b.cols.0 as f64;
        self.clamp();
        self.reveal((b.rows.0, b.cols.0));
    }

    /// Zoom around the middle of the grid.
    pub fn zoom_step(&mut self, factor: f64) {
        let x = self.name_width + self.grid_width() / 2.0;
        let y = self.header_height() + self.grid_height() / 2.0;
        self.zoom_at(x, y, factor);
    }

    pub fn zoom_text(&self) -> String {
        format!("{:.0} %", self.px_per_col / PX_PER_COL_AT_100 * 100.0)
    }

    pub fn scroll_by(&mut self, dy: f32) {
        self.auto_fit = false;
        self.scroll_y += dy;
        self.clamp();
    }

    /// Move the view right by `dx` pixels.
    pub fn pan(&mut self, dx: f32) {
        self.auto_fit = false;
        self.start += f64::from(dx) / self.px_per_col;
        self.clamp();
    }

    /// Horizontal scroll bar position and size, as fractions.
    pub fn hscroll(&self) -> (f64, f64) {
        let cols = self.cols.max(1) as f64;
        let span = f64::from(self.grid_width()) / self.px_per_col;
        (self.start / cols, (span / cols).min(1.0))
    }

    pub fn vscroll(&self) -> (f64, f64) {
        let total = (self.rows as f32 * self.row_height()).max(1.0);
        (
            f64::from(self.scroll_y / total),
            f64::from((self.grid_height() / total).min(1.0)),
        )
    }

    pub fn hscroll_to(&mut self, fraction: f64) {
        self.auto_fit = false;
        self.start = fraction * self.cols as f64;
        self.clamp();
    }

    pub fn vscroll_to(&mut self, fraction: f64) {
        self.auto_fit = false;
        self.scroll_y = fraction as f32 * self.rows as f32 * self.row_height();
        self.clamp();
    }

    /// Scroll so a cell is in view, with a little room around it.
    pub fn reveal(&mut self, (row, col): (usize, u64)) {
        let span = f64::from(self.grid_width()) / self.px_per_col;
        let margin = (span * 0.1).min(8.0);
        let c = col as f64;
        if c < self.start + margin {
            self.start = c - margin;
        } else if c + 1.0 > self.start + span - margin {
            self.start = c + 1.0 - span + margin;
        }
        let h = self.row_height();
        let top = row as f32 * h;
        if top < self.scroll_y {
            self.scroll_y = top;
        } else if top + h > self.scroll_y + self.grid_height() {
            self.scroll_y = top + h - self.grid_height();
        }
        self.clamp();
    }

    /// While a selection drag is near or past an edge, scroll and extend the
    /// selection; returns whether the view moved.
    pub fn edge_scroll(&mut self, x: f32, y: f32) -> bool {
        if self.drag != Drag::Select {
            return false;
        }
        let edge = 24.0;
        let speed = |d: f32| (d / edge).clamp(0.0, 3.0) * 12.0;
        let (start, scroll) = (self.start, self.scroll_y);
        if self.span != Span::Rows {
            if x < self.name_width + edge {
                self.pan(-speed(self.name_width + edge - x));
            } else if x > self.width - edge {
                self.pan(speed(x - (self.width - edge)));
            }
        }
        if self.span != Span::Columns {
            if y < self.header_height() + edge {
                self.scroll_by(-speed(self.header_height() + edge - y));
            } else if y > self.height - edge {
                self.scroll_by(speed(y - (self.height - edge)));
            }
        }
        let moved = (start - self.start).abs() > f64::EPSILON
            || (scroll - self.scroll_y).abs() > f32::EPSILON;
        if moved {
            self.drag_to(x, y);
        }
        moved
    }

    // Selection.

    fn update_block(&mut self) {
        if self.rows == 0 || self.cols == 0 {
            self.selection = None;
            return;
        }
        let (a, c) = (self.anchor, self.cursor);
        let rows = (a.0.min(c.0), a.0.max(c.0) + 1);
        let cols = (a.1.min(c.1), a.1.max(c.1) + 1);
        self.selection = Some(match self.span {
            Span::Cells => Block { rows, cols },
            Span::Rows => Block {
                rows,
                cols: (0, self.cols),
            },
            Span::Columns => Block {
                rows: (0, self.rows),
                cols,
            },
        });
    }

    /// The selection, or the cursor's cell.
    pub fn block(&self) -> Block {
        self.selection.unwrap_or(Block {
            rows: (self.cursor.0, self.cursor.0 + 1),
            cols: (self.cursor.1, self.cursor.1 + 1),
        })
    }

    pub fn select_all(&mut self) {
        if self.rows == 0 {
            return;
        }
        self.anchor = (0, 0);
        self.cursor = (self.rows - 1, self.cursor.1);
        self.span = Span::Rows;
        self.update_block();
    }

    /// Select whole rows `a..b`.
    pub fn select_rows(&mut self, a: usize, b: usize) {
        if a >= b || b > self.rows {
            return;
        }
        self.anchor = (a, 0);
        self.cursor = (b - 1, self.cursor.1);
        self.span = Span::Rows;
        self.update_block();
    }

    /// Select rows `r0..r1` by columns `c0..c1`, cursor at the top left.
    pub fn select_block(&mut self, (r0, r1): (usize, usize), (c0, c1): (u64, u64)) {
        if r0 >= r1 || c0 >= c1 || r1 > self.rows || c1 > self.cols {
            return;
        }
        self.anchor = (r1 - 1, c1 - 1);
        self.cursor = (r0, c0);
        self.span = Span::Cells;
        self.update_block();
        self.reveal(self.cursor);
    }

    /// Back to just the cursor's cell.
    pub fn collapse(&mut self) {
        self.anchor = self.cursor;
        self.span = Span::Cells;
        self.update_block();
    }

    /// Move the cursor; with `extend`, the selection grows from its anchor.
    pub fn move_cursor(&mut self, rows: i64, cols: i64, extend: bool) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let r = (self.cursor.0 as i64 + rows).clamp(0, self.rows as i64 - 1);
        let c = (self.cursor.1 as i64 + cols).clamp(0, self.cols as i64 - 1);
        self.cursor = (r as usize, c as u64);
        if !extend {
            self.anchor = self.cursor;
            self.span = Span::Cells;
        }
        self.update_block();
        self.reveal(self.cursor);
    }

    /// Rows a page holds, for Page Up and Page Down.
    pub fn page_rows(&self) -> i64 {
        ((self.grid_height() / self.row_height()) as i64 - 1).max(1)
    }

    /// Move the selection and cursor along by `by` columns, after an edit
    /// moved what they cover.
    pub fn offset_columns(&mut self, by: i64) {
        let move_col = |c: u64| c.saturating_add_signed(by);
        self.cursor.1 = move_col(self.cursor.1);
        self.anchor.1 = move_col(self.anchor.1);
        if let Drag::Slide { applied, .. } = &mut self.drag {
            *applied += by;
        }
        self.update_block();
    }

    /// After typing, move the selection right by its width, staying in the
    /// alignment.
    pub fn advance(&mut self) {
        let width = self.block().col_count();
        let last = self.cols.saturating_sub(1);
        let move_col = |c: u64| c.saturating_add(width).min(last);
        self.cursor.1 = move_col(self.cursor.1);
        self.anchor.1 = move_col(self.anchor.1);
        self.update_block();
        self.reveal(self.cursor);
    }

    /// Likewise by `by` rows, after rows moved.
    pub fn offset_rows(&mut self, by: i64) {
        let move_row = |r: usize| r.saturating_add_signed(by as isize);
        self.cursor.0 = move_row(self.cursor.0);
        self.anchor.0 = move_row(self.anchor.0);
        self.update_block();
        self.reveal(self.cursor);
    }

    // Pointer input.

    pub fn press(&mut self, x: f32, y: f32, shift: bool) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let cell = self.cell_at(x, y);
        match self.region_at(x, y) {
            Region::Corner => {
                self.select_all();
                self.drag = Drag::None;
                return;
            }
            Region::Grid => {
                if !shift
                    && let Some(b) = self.selection
                    && b.contains(cell)
                {
                    self.drag = Drag::Slide {
                        origin: self.col_at(x),
                        applied: 0,
                        moved: false,
                        cell,
                    };
                    return;
                }
                self.cursor = cell;
                if !shift || self.span != Span::Cells {
                    self.span = Span::Cells;
                    if !shift {
                        self.anchor = cell;
                    }
                }
            }
            Region::Names => {
                if !shift || self.span != Span::Rows {
                    self.anchor = (if shift { self.anchor.0 } else { cell.0 }, 0);
                }
                self.cursor = (cell.0, self.cursor.1);
                self.span = Span::Rows;
            }
            Region::Header => {
                if !shift || self.span != Span::Columns {
                    self.anchor = (0, if shift { self.anchor.1 } else { cell.1 });
                }
                self.cursor = (self.cursor.0, cell.1);
                self.span = Span::Columns;
            }
        }
        self.update_block();
        self.drag = Drag::Select;
    }

    /// Start panning (middle button).
    pub fn pan_press(&mut self, x: f32, y: f32) {
        self.drag = Drag::Pan {
            last_x: x,
            last_y: y,
        };
    }

    /// Pointer moved with a button down. When sliding a block, returns how
    /// many more columns the pointer asks it to move; the caller applies
    /// that as shift operations and reports each step with
    /// [`AlignmentView::offset_columns`].
    pub fn drag_to(&mut self, x: f32, y: f32) -> Option<i64> {
        match &mut self.drag {
            Drag::None => None,
            Drag::Pan { last_x, last_y } => {
                let (dx, dy) = (*last_x - x, *last_y - y);
                *last_x = x;
                *last_y = y;
                self.pan(dx);
                self.scroll_by(dy);
                None
            }
            Drag::Select => {
                let cell = self.cell_at(x, y);
                match self.span {
                    Span::Cells => self.cursor = cell,
                    Span::Rows => self.cursor.0 = cell.0,
                    Span::Columns => self.cursor.1 = cell.1,
                }
                self.update_block();
                None
            }
            Drag::Slide {
                origin,
                applied,
                moved,
                ..
            } => {
                let target = (self.start + f64::from(x - self.name_width) / self.px_per_col
                    - *origin)
                    .round() as i64;
                let want = target - *applied;
                if want != 0 {
                    *moved = true;
                }
                (want != 0).then_some(want)
            }
        }
    }

    pub fn release(&mut self) {
        if let Drag::Slide {
            moved: false, cell, ..
        } = self.drag
        {
            self.cursor = cell;
            self.collapse();
        }
        self.drag = Drag::None;
    }

    /// Right-click: select the cell, row or column under the pointer unless
    /// it's already selected. Returns the region.
    pub fn context_press(&mut self, x: f32, y: f32) -> Region {
        let region = self.region_at(x, y);
        let cell = self.cell_at(x, y);
        let inside = self.selection.is_some_and(|b| match region {
            Region::Grid => b.contains(cell),
            Region::Names => (b.rows.0..b.rows.1).contains(&cell.0) && b.cols == (0, self.cols),
            Region::Header => (b.cols.0..b.cols.1).contains(&cell.1) && b.rows == (0, self.rows),
            Region::Corner => true,
        });
        if !inside && region != Region::Corner {
            self.press(x, y, false);
            self.drag = Drag::None;
        }
        region
    }

    // Operations for the current selection. The caller applies them, and
    // on success moves the view along as each says.

    /// Type `residue` (a gap for `-`) into the selection: insert as many as
    /// it is wide before it, or type over every cell of it. Either way the
    /// selection then moves right by its width.
    pub fn type_op(&self, residue: char, insert: bool) -> Op {
        let b = self.block();
        let width = b.col_count();
        if insert && residue == '-' {
            Op::AlnInsertGaps {
                rows: b.row_ranges(),
                col: b.cols.0,
                count: width,
            }
        } else if insert {
            Op::AlnInsert {
                rows: b.row_ranges(),
                col: b.cols.0,
                residues: residue.to_string().repeat(width as usize),
            }
        } else if b.row_count() == 1 {
            // One row stays a replace, so typing along it merges.
            Op::AlnReplace {
                row: b.rows.0 as u32,
                start: b.cols.0,
                end: b.cols.1,
                residues: residue.to_string().repeat(width as usize),
            }
        } else {
            Op::AlnFill {
                rows: b.row_ranges(),
                start: b.cols.0,
                end: b.cols.1,
                residue: residue.to_string(),
            }
        }
    }

    /// A gap before the selection in its rows; then it moves right one.
    pub fn insert_gap_op(&self) -> Op {
        let b = self.block();
        Op::AlnInsertGaps {
            rows: b.row_ranges(),
            col: b.cols.0,
            count: 1,
        }
    }

    /// Backspace: remove the cells just left of the selection, pulling it
    /// left one. In overwrite mode only gaps go, and a residue there
    /// refuses; in insert mode residues go too.
    pub fn remove_left_op(&self, doc: &AlignmentDoc, insert: bool) -> Result<Op, String> {
        let b = self.block();
        if b.cols.0 == 0 {
            return Err("Nothing to the left of the selection".into());
        }
        let col = b.cols.0 - 1;
        for r in b.rows.0..b.rows.1 {
            if insert {
                break;
            }
            if !doc
                .read(r, col, col + 1)
                .first()
                .copied()
                .is_some_and(is_gap)
            {
                return Err(format!(
                    "{} has {} at column {}; Backspace only removes gaps",
                    doc.row(r).name,
                    if doc.is_protein() {
                        "an amino acid"
                    } else {
                        "a base"
                    },
                    format_thousands(col + 1)
                ));
            }
        }
        Ok(Op::AlnDelete {
            rows: b.row_ranges(),
            start: col,
            end: col + 1,
        })
    }

    /// Delete the selected cells; the rest of each row moves left.
    pub fn delete_op(&self) -> Op {
        let b = self.block();
        Op::AlnDelete {
            rows: b.row_ranges(),
            start: b.cols.0,
            end: b.cols.1,
        }
    }

    /// Delete the selected columns from every row.
    pub fn delete_columns_op(&self) -> Op {
        let b = self.block();
        Op::AlnDelete {
            rows: vec![(0, self.rows as u32)],
            start: b.cols.0,
            end: b.cols.1,
        }
    }

    /// A gap column before the selection, in every row.
    pub fn insert_column_op(&self) -> Op {
        Op::AlnInsertGaps {
            rows: vec![(0, self.rows as u32)],
            col: self.block().cols.0,
            count: 1,
        }
    }

    /// Slide the selection by `by` columns.
    pub fn shift_op(&self, by: i64) -> Op {
        let b = self.block();
        Op::AlnShift {
            rows: b.row_ranges(),
            start: b.cols.0,
            end: b.cols.1,
            by,
        }
    }

    /// Move the selected rows up or down one; `None` at the edge.
    pub fn move_rows_op(&self, up: bool) -> Option<Op> {
        let b = self.block();
        let to = if up {
            b.rows.0.checked_sub(1)?
        } else if b.rows.1 < self.rows {
            b.rows.0 + 1
        } else {
            return None;
        };
        Some(Op::AlnMoveRows {
            rows: b.row_ranges(),
            to: to as u32,
        })
    }

    /// Move the selected rows to the top.
    pub fn rows_to_top_op(&self) -> Op {
        Op::AlnMoveRows {
            rows: self.block().row_ranges(),
            to: 0,
        }
    }

    pub fn remove_rows_op(&self) -> Op {
        Op::AlnRemoveRows {
            rows: self.block().row_ranges(),
        }
    }

    /// The selected block as FASTA, for the clipboard.
    pub fn selection_fasta(&self, doc: &AlignmentDoc) -> String {
        let b = self.block();
        let mut out = String::new();
        for r in b.rows.0..b.rows.1.min(doc.row_count()) {
            out.push('>');
            out.push_str(&doc.row(r).name);
            out.push('\n');
            out.push_str(&String::from_utf8_lossy(&doc.read(r, b.cols.0, b.cols.1)));
            out.push('\n');
        }
        out
    }

    // Drawing.

    pub fn build(&mut self, doc: &AlignmentDoc, theme: &Theme, frame: &mut Frame) {
        frame.clear();
        self.sync(doc);
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let colour = |b: u8| {
            if doc.is_protein() {
                theme.amino(b)
            } else {
                theme.residue(b)
            }
        };
        self.draw_grid(doc, theme, &colour, frame);
        self.draw_selection(theme, frame);
        self.draw_names(doc, theme, frame);
        self.draw_header(doc, theme, &colour, frame);
    }

    fn draw_grid(
        &self,
        doc: &AlignmentDoc,
        theme: &Theme,
        colour: &dyn Fn(u8) -> Rgba,
        frame: &mut Frame,
    ) {
        let (c0, c1) = self.visible_cols();
        if c0 >= c1 {
            return;
        }
        let h = self.row_height();
        let ppc = self.px_per_col as f32;
        let mut buf = Vec::new();
        for r in self.visible_rows() {
            let y = self.y_of(r);
            if self.letters() {
                buf.clear();
                doc.read_into(r, c0, c1, &mut buf);
                let size = (h - 3.0).min(ppc * 1.9);
                let pad = if ppc >= 12.0 { 1.0 } else { 0.0 };
                for (i, &b) in buf.iter().enumerate() {
                    let x = self.x_of((c0 + i as u64) as f64);
                    let gx = x + (ppc - size * crate::frame::GLYPH_ADVANCE) / 2.0;
                    let gy = y + (h - size) / 2.0;
                    if is_gap(b) {
                        frame.glyph(gx, gy, size, b'-', alpha(theme.foreground_muted, 0.35));
                        continue;
                    }
                    let c = colour(b);
                    frame.rect(
                        x + pad / 2.0,
                        y + pad / 2.0,
                        ppc - pad,
                        h - pad,
                        mix(theme.background, c, 0.22),
                    );
                    frame.glyph(gx, gy, size, b, c);
                }
            } else if self.px_per_col >= COLOURS_FROM {
                buf.clear();
                doc.read_into(r, c0, c1, &mut buf);
                let cell_h = if h > 4.0 { h - 1.0 } else { h };
                // Runs of one colour make one quad.
                let mut run: Option<(usize, Rgba)> = None;
                for i in 0..=buf.len() {
                    let c = buf.get(i).filter(|b| !is_gap(**b)).map(|&b| colour(b));
                    if run.is_some_and(|(_, rc)| Some(rc) == c) {
                        continue;
                    }
                    if let Some((s, rc)) = run.take() {
                        let x = self.x_of((c0 + s as u64) as f64);
                        frame.rect(x, y, (i - s) as f32 * ppc, cell_h, alpha(rc, 0.85));
                    }
                    run = c.map(|c| (i, c));
                }
            } else {
                self.draw_overview_row(doc, r, y, theme, &mut buf, frame);
            }
        }
    }

    /// One row when columns are narrower than a pixel or two: where it has
    /// residues, sampled once per pixel.
    fn draw_overview_row(
        &self,
        doc: &AlignmentDoc,
        r: usize,
        y: f32,
        theme: &Theme,
        buf: &mut Vec<u8>,
        frame: &mut Frame,
    ) {
        let (c0, c1) = self.visible_cols();
        let x0 = self.x_of(c0 as f64).max(self.name_width);
        let x1 = self.x_of(c1 as f64).min(self.width);
        let pixels = (x1 - x0).max(0.0) as usize;
        let cols_per_px = 1.0 / self.px_per_col;
        // Narrow enough to read the whole range; otherwise one read a pixel.
        let contiguous = cols_per_px <= 16.0;
        if contiguous {
            buf.clear();
            doc.read_into(r, c0, c1, buf);
        }
        let mut one = Vec::with_capacity(1);
        let h = self.row_height();
        let cell_h = if h > 4.0 { h - 1.0 } else { h };
        let colour = alpha(theme.foreground_muted, 0.55);
        let mut run: Option<usize> = None;
        for p in 0..=pixels {
            let present = p < pixels && {
                let col = (self.col_at(x0 + p as f32 + 0.5) as u64).min(c1.saturating_sub(1));
                let b = if contiguous {
                    buf.get((col - c0) as usize).copied()
                } else {
                    one.clear();
                    doc.read_into(r, col, col + 1, &mut one);
                    one.first().copied()
                };
                b.is_some_and(|b| !is_gap(b))
            };
            match (present, run) {
                (true, None) => run = Some(p),
                (false, Some(s)) => {
                    frame.rect(x0 + s as f32, y, (p - s) as f32, cell_h, colour);
                    run = None;
                }
                _ => {}
            }
        }
    }

    fn draw_selection(&self, theme: &Theme, frame: &mut Frame) {
        let gx0 = self.name_width;
        let gy0 = self.header_height();
        let clip = |x0: f32, y0: f32, x1: f32, y1: f32| {
            (
                x0.max(gx0),
                y0.max(gy0),
                x1.min(self.width),
                y1.min(self.height),
            )
        };
        if let Some(b) = self.selection {
            let (x0, y0, x1, y1) = clip(
                self.x_of(b.cols.0 as f64),
                self.y_of(b.rows.0),
                self.x_of(b.cols.1 as f64),
                self.y_of(b.rows.1),
            );
            if x1 > x0 && y1 > y0 {
                frame.rect(x0, y0, x1 - x0, y1 - y0, alpha(theme.accent, 0.16));
                outline(frame, x0, y0, x1 - x0, y1 - y0, 2.0, theme.accent);
            }
        }
        let (r, c) = self.cursor;
        let (x0, y0, x1, y1) = clip(
            self.x_of(c as f64),
            self.y_of(r),
            self.x_of(c as f64 + 1.0),
            self.y_of(r + 1),
        );
        if x1 > x0 && y1 > y0 {
            outline(frame, x0, y0, x1 - x0, y1 - y0, 1.5, theme.foreground);
        }
    }

    fn draw_names(&self, doc: &AlignmentDoc, theme: &Theme, frame: &mut Frame) {
        let panel = mix(theme.background, theme.surface, 0.6);
        let top = self.header_height();
        frame.rect(0.0, top, self.name_width, self.height - top, panel);
        let h = self.row_height();
        let size = (h - 2.0).min(LABEL_SIZE);
        let selected = self.selection.map(|b| b.rows);
        for r in self.visible_rows() {
            let y = self.y_of(r);
            if selected.is_some_and(|(a, b)| (a..b).contains(&r)) {
                frame.rect(
                    0.0,
                    y.max(top),
                    self.name_width,
                    h.min(y + h - top),
                    alpha(theme.accent, 0.18),
                );
            }
            if size < 6.0 || y < top - 1.0 {
                continue;
            }
            let name = &doc.row(r).name;
            let fits = ((self.name_width - 2.0 * NAME_PAD) / (size * crate::frame::GLYPH_ADVANCE))
                as usize;
            let shown = if name.chars().count() > fits && fits > 1 {
                let mut short: String = name.chars().take(fits - 1).collect();
                short.push('~');
                short
            } else {
                name.clone()
            };
            frame.text(
                NAME_PAD,
                y + (h - size) / 2.0,
                size,
                &shown,
                theme.foreground,
            );
        }
        frame.rect(
            self.name_width - 1.0,
            top,
            1.0,
            self.height - top,
            alpha(theme.foreground_muted, 0.3),
        );
    }

    fn draw_header(
        &self,
        doc: &AlignmentDoc,
        theme: &Theme,
        colour: &dyn Fn(u8) -> Rgba,
        frame: &mut Frame,
    ) {
        let hh = self.header_height();
        frame.rect(0.0, 0.0, self.width, hh, theme.background);
        frame.rect(
            0.0,
            0.0,
            self.name_width,
            hh,
            mix(theme.background, theme.surface, 0.6),
        );
        let (c0, c1) = self.visible_cols();
        self.draw_ruler(c0, c1, theme, frame);
        let cons_y = RULER_H;
        let id_y = RULER_H + self.consensus_h();
        let label = |frame: &mut Frame, y: f32, h: f32, text: &str| {
            let size = (h - 2.0).min(11.0);
            if size >= 6.0 {
                frame.text(
                    NAME_PAD,
                    y + (h - size) / 2.0,
                    size,
                    text,
                    theme.foreground_muted,
                );
            }
        };
        if self.letters() {
            label(frame, cons_y, self.consensus_h(), "Consensus");
        }
        label(frame, id_y, IDENTITY_H, "Identity");
        let cells = (c1 - c0).saturating_mul(self.rows as u64);
        if cells > CONSENSUS_BUDGET {
            frame.text(
                self.name_width + 6.0,
                id_y + 1.0,
                11.0,
                "zoom in to see the consensus",
                theme.foreground_muted,
            );
        } else if c1 > c0 {
            let ppc = self.px_per_col as f32;
            if self.letters() {
                let cons = doc.consensus(c0, c1);
                let h = self.consensus_h();
                let size = (h - 3.0).min(ppc * 1.9);
                for (i, c) in cons.iter().enumerate() {
                    let x = self.x_of((c0 + i as u64) as f64);
                    if c.residue != b'-' {
                        let col = colour(c.residue);
                        frame.rect(
                            x,
                            cons_y,
                            ppc,
                            h,
                            mix(theme.background, col, 0.22 * c.agreement),
                        );
                        frame.glyph(
                            x + (ppc - size * crate::frame::GLYPH_ADVANCE) / 2.0,
                            cons_y + (h - size) / 2.0,
                            size,
                            c.residue,
                            col,
                        );
                    }
                    identity_bar(frame, x, id_y, ppc, c.agreement, theme);
                }
            } else {
                // One bar per two pixels, from the column at its middle.
                let x0 = self.x_of(c0 as f64).max(self.name_width);
                let x1 = self.x_of(c1 as f64).min(self.width);
                let mut x = x0;
                while x < x1 {
                    let col = (self.col_at(x + 1.0) as u64).min(c1 - 1);
                    if let Some(c) = doc.consensus_at(col) {
                        identity_bar(frame, x, id_y, 2.0, c.agreement, theme);
                    }
                    x += 2.0;
                }
            }
        }
        frame.rect(
            0.0,
            hh - 1.0,
            self.width,
            1.0,
            alpha(theme.foreground_muted, 0.3),
        );
        frame.rect(
            self.name_width - 1.0,
            0.0,
            1.0,
            hh,
            alpha(theme.foreground_muted, 0.3),
        );
    }

    fn draw_ruler(&self, c0: u64, c1: u64, theme: &Theme, frame: &mut Frame) {
        let base_y = RULER_H - 4.0;
        let step = nice_step(110.0 / self.px_per_col);
        let minor = step / 5.0;
        let centre = |col: f64| self.x_of(col - 1.0) + self.px_per_col as f32 / 2.0;
        // Columns are numbered from 1.
        let (lo, hi) = (c0 as f64 + 1.0, c1 as f64);
        if minor * self.px_per_col >= 7.0 {
            let mut t = (lo / minor).ceil() * minor;
            while t <= hi {
                frame.rect(centre(t), base_y - 3.0, 1.0, 3.0, theme.foreground_muted);
                t += minor;
            }
        }
        let mut t = (lo / step).ceil() * step;
        while t <= hi {
            let x = centre(t);
            if x >= self.name_width {
                frame.rect(x, base_y - 7.0, 1.0, 7.0, theme.foreground);
                frame.text(
                    x + 3.0,
                    2.0,
                    11.0,
                    &format_bp(t as u64, step),
                    theme.foreground_muted,
                );
            }
            t += step;
        }
    }

    // Text.

    /// What's under the pointer, for a tooltip.
    pub fn hover_text(&self, x: f32, y: f32, doc: &AlignmentDoc) -> Option<String> {
        match self.region_at(x, y) {
            Region::Grid => {
                let (r, c) = self.cell_hit(x, y)?;
                let row = doc.row(r);
                let b = *doc.read(r, c, c + 1).first()?;
                let what = if is_gap(b) {
                    "gap".to_string()
                } else {
                    let before = doc.read(r, 0, c).iter().filter(|&&b| !is_gap(b)).count();
                    format!(
                        "{} {} ({})",
                        if doc.is_protein() {
                            "amino acid"
                        } else {
                            "base"
                        },
                        format_thousands(before as u64 + 1),
                        b as char
                    )
                };
                Some(format!(
                    "{} · column {} · {what}",
                    row.name,
                    format_thousands(c + 1)
                ))
            }
            Region::Header => {
                let c = self.col_at(x);
                if c < 0.0 || c as u64 >= self.cols {
                    return None;
                }
                let c = c as u64;
                if (self.visible_cols().1 - self.visible_cols().0).saturating_mul(self.rows as u64)
                    > CONSENSUS_BUDGET
                {
                    return Some(format!("column {}", format_thousands(c + 1)));
                }
                let cons = doc.consensus_at(c)?;
                Some(format!(
                    "column {} · consensus {} · {:.0} % of rows",
                    format_thousands(c + 1),
                    cons.residue as char,
                    cons.agreement * 100.0
                ))
            }
            Region::Names => {
                let r = self.row_at_f(y);
                if r < 0.0 || r as usize >= self.rows {
                    return None;
                }
                let row = doc.row(r as usize);
                Some(if row.description.is_empty() {
                    row.name.clone()
                } else {
                    format!("{} {}", row.name, row.description)
                })
            }
            Region::Corner => None,
        }
    }

    /// One line describing the alignment, the cursor and the selection.
    pub fn status(&self, doc: &AlignmentDoc) -> String {
        let mut parts = vec![format!(
            "{} rows × {} columns · {}",
            format_thousands(doc.row_count() as u64),
            format_thousands(doc.width()),
            if doc.is_protein() {
                "protein"
            } else {
                "nucleotide"
            }
        )];
        if doc.row_count() > 0 {
            let (r, c) = self.cursor;
            parts.push(format!(
                "{} column {}",
                doc.row(r.min(doc.row_count() - 1)).name,
                format_thousands(c + 1)
            ));
        }
        if let Some(b) = self.selection
            && (b.row_count() > 1 || b.col_count() > 1)
        {
            parts.push(format!(
                "selected {} × {} (columns {}..{})",
                format_thousands(b.row_count() as u64),
                format_thousands(b.col_count()),
                format_thousands(b.cols.0 + 1),
                format_thousands(b.cols.1)
            ));
        }
        parts.join("   ·   ")
    }
}

fn identity_bar(frame: &mut Frame, x: f32, y: f32, w: f32, agreement: f32, theme: &Theme) {
    let h = 2.0 + agreement * (IDENTITY_H - 4.0);
    frame.rect(
        x,
        y + IDENTITY_H - 1.0 - h,
        w,
        h,
        alpha(mix(theme.orange, theme.green, agreement), 0.8),
    );
}

fn outline(frame: &mut Frame, x: f32, y: f32, w: f32, h: f32, t: f32, color: Rgba) {
    frame.rect(x, y, w, t, color);
    frame.rect(x, y + h - t, w, t, color);
    frame.rect(x, y, t, h, color);
    frame.rect(x + w - t, y, t, h, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_docs::alignment::AlnRow;
    use std::sync::Arc;

    fn doc(rows: usize, cols: usize) -> AlignmentDoc {
        AlignmentDoc::new(
            "t",
            (0..rows)
                .map(|r| {
                    let seq: Vec<u8> = (0..cols)
                        .map(|c| {
                            if (r + c) % 7 == 0 {
                                b'-'
                            } else {
                                b"ACGT"[(r * 3 + c) % 4]
                            }
                        })
                        .collect();
                    AlnRow::new(format!("row{r}"), Arc::new(seq))
                })
                .collect(),
        )
    }

    fn view(d: &AlignmentDoc) -> AlignmentView {
        let mut v = AlignmentView::new(800.0, 400.0);
        v.sync(d);
        v
    }

    #[test]
    fn cells_hit_test_both_ways() {
        let d = doc(30, 500);
        let mut v = view(&d);
        let h = v.row_height();
        let cell = (4, 17);
        let x = v.x_of(17.5);
        let y = v.y_of(4) + h / 2.0;
        assert_eq!(v.cell_at(x, y), cell);
        v.scroll_by(h * 3.0);
        assert_eq!(v.cell_at(x, y), (7, 17));
        assert_eq!(v.region_at(10.0, y), Region::Names);
        assert_eq!(v.region_at(x, 5.0), Region::Header);
    }

    #[test]
    fn drag_selects_blocks_rows_and_columns() {
        let d = doc(30, 500);
        let mut v = view(&d);
        let h = v.row_height();
        let at =
            |v: &AlignmentView, r: usize, c: u64| (v.x_of(c as f64 + 0.5), v.y_of(r) + h / 2.0);
        let (x, y) = at(&v, 2, 10);
        v.press(x, y, false);
        let (x, y) = at(&v, 5, 4);
        v.drag_to(x, y);
        v.release();
        assert_eq!(
            v.selection,
            Some(Block {
                rows: (2, 6),
                cols: (4, 11)
            })
        );
        // Row names select whole rows.
        let (_, y) = at(&v, 3, 0);
        v.press(20.0, y, false);
        v.release();
        assert_eq!(
            v.selection,
            Some(Block {
                rows: (3, 4),
                cols: (0, 500)
            })
        );
        // The ruler selects whole columns; shift extends.
        v.press(v.x_of(7.5), 5.0, false);
        v.release();
        v.press(v.x_of(9.5), 5.0, true);
        v.release();
        assert_eq!(
            v.selection,
            Some(Block {
                rows: (0, 30),
                cols: (7, 10)
            })
        );
    }

    #[test]
    fn dragging_inside_the_selection_slides_it() {
        let d = doc(10, 100);
        let mut v = view(&d);
        let h = v.row_height();
        let y = v.y_of(1) + h / 2.0;
        v.press(v.x_of(20.5), y, false);
        v.drag_to(v.x_of(24.5), y);
        v.release();
        // Press inside, move three columns left.
        v.press(v.x_of(22.5), y, false);
        assert_eq!(v.drag_to(v.x_of(19.5), y), Some(-3));
        v.offset_columns(-1);
        assert_eq!(v.drag_to(v.x_of(19.5), y), Some(-2));
        v.release();
        assert_eq!(v.selection.unwrap().cols, (19, 24));
        // A click inside without moving selects that cell.
        v.press(v.x_of(21.5), y, false);
        v.release();
        assert_eq!(v.selection.unwrap().cols, (21, 22));
    }

    #[test]
    fn deleted_columns_map_selection() {
        let d = doc(10, 100);
        let mut v = view(&d);
        v.select_block((2, 5), (20, 40));
        v.map_deleted_columns(&[(1, 3), (25, 30)]);
        assert_eq!(v.block().cols, (18, 33));
    }

    #[test]
    fn ops_follow_the_selection() {
        let d = doc(10, 100);
        let mut v = view(&d);
        v.cursor = (2, 30);
        v.collapse();
        v.move_cursor(2, 3, true);
        assert_eq!(
            v.shift_op(-1),
            Op::AlnShift {
                rows: vec![(2, 5)],
                start: 30,
                end: 34,
                by: -1
            }
        );
        assert_eq!(
            v.move_rows_op(true),
            Some(Op::AlnMoveRows {
                rows: vec![(2, 5)],
                to: 1
            })
        );
        assert_eq!(
            v.delete_columns_op(),
            Op::AlnDelete {
                rows: vec![(0, 10)],
                start: 30,
                end: 34
            }
        );
        // Row 2 has a residue at column 29: only insert mode removes it.
        assert!(v.remove_left_op(&d, false).is_err());
        assert_eq!(
            v.remove_left_op(&d, true),
            Ok(Op::AlnDelete {
                rows: vec![(2, 5)],
                start: 29,
                end: 30
            })
        );
        // Typing applies to the whole 3 × 4 selection.
        assert_eq!(
            v.type_op('-', true),
            Op::AlnInsertGaps {
                rows: vec![(2, 5)],
                col: 30,
                count: 4
            }
        );
        assert_eq!(
            v.type_op('A', true),
            Op::AlnInsert {
                rows: vec![(2, 5)],
                col: 30,
                residues: "AAAA".into()
            }
        );
        assert_eq!(
            v.type_op('-', false),
            Op::AlnFill {
                rows: vec![(2, 5)],
                start: 30,
                end: 34,
                residue: "-".into()
            }
        );
        v.advance();
        assert_eq!((v.block().rows, v.block().cols), ((2, 5), (34, 38)));
        v.collapse();
        assert!(matches!(
            v.type_op('A', true),
            Op::AlnInsert { col: 37, .. }
        ));
        assert!(matches!(
            v.type_op('A', false),
            Op::AlnReplace { start: 37, .. }
        ));
        v.select_rows(0, 10);
        assert_eq!(v.move_rows_op(false), None);
    }

    #[test]
    fn every_zoom_draws_and_fit_shows_everything() {
        let d = doc(40, 3000);
        let mut v = view(&d);
        let theme = Theme::default();
        let mut f = Frame::new();
        v.build(&d, &theme, &mut f);
        let letters = f.len();
        assert!(letters > 1000, "{letters}");
        v.zoom_step(0.4);
        v.build(&d, &theme, &mut f);
        assert!(f.len() > 100);
        v.fit();
        assert!(v.px_per_col < COLOURS_FROM);
        assert!((v.hscroll().1 - 1.0).abs() < 1e-9);
        // Still fitted after the pane narrows, until the user zooms.
        v.resize(700.0, 400.0);
        assert!((v.hscroll().1 - 1.0).abs() < 1e-9);
        v.build(&d, &theme, &mut f);
        assert!(f.len() > 100);
        assert!(v.zoom_text().ends_with('%'));
    }

    #[test]
    fn hover_and_status() {
        let d = doc(5, 50);
        let mut v = view(&d);
        let y = v.y_of(1) + v.row_height() / 2.0;
        // Row 1 is "CTAG-..." from column 0; column 2 is the third residue.
        let t = v.hover_text(v.x_of(2.5), y, &d).unwrap();
        assert!(t.starts_with("row1 · column 3 · base 3"), "{t}");
        let t = v.hover_text(v.x_of(6.5), y, &d).unwrap();
        assert!(t.ends_with("gap"), "{t}");
        assert!(
            v.hover_text(v.x_of(2.5), 30.0, &d)
                .unwrap()
                .contains("consensus")
        );
        v.select_all();
        assert!(v.status(&d).contains("selected 5 × 50"));
    }
}
