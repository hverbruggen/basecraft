//! Sanger review in one alignment coordinate system. Signal samples are warped
//! between Tracy event anchors for display; the evidence itself is immutable.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::{Frame, Theme, sanger_alignment::AlignmentContext, sanger_map};
use basecraft_docs::{
    AlignmentDoc,
    sanger::{Orientation, SangerDraft, SangerPlacement, SupportKind},
};
use basecraft_io::abif::Trace;

const NAMES: f32 = 180.0;
const ROW: f32 = 24.0;
const REFERENCE_ROW: f32 = 20.0;

#[derive(Clone, Debug)]
pub struct ReadLane {
    pub trace: Trace,
    /// Draft column -> Tracy basecall event in the original signal direction.
    pub column_events: Vec<Option<usize>>,
    pub gain: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ReviewColumn {
    pub source: Option<usize>,
    pub draft: Option<usize>,
}

pub struct SangerReviewView {
    pub draft: SangerDraft,
    pub lanes: Vec<ReadLane>,
    pub selected: usize,
    pub width: f32,
    pub height: f32,
    pub alignment: Option<AlignmentContext>,
    pub columns: Vec<ReviewColumn>,
    pub column_width: f32,
    pub left: f64,
    pub hovered: Option<usize>,
    draft_display: Vec<usize>,
    pub consensus: Vec<u8>,
    pub unresolved: Vec<bool>,
    pub masked: Vec<bool>,
    pub review_settings: basecraft_docs::sanger::ReviewSettings,
    pub reference_evidence: Option<basecraft_docs::sanger::ReviewReference>,
}

impl SangerReviewView {
    pub fn new(draft: SangerDraft, traces: Vec<Trace>) -> Result<Self, String> {
        draft.validate()?;
        if traces.len() != draft.inputs.len() {
            return Err("Trace count does not match draft".into());
        }
        let mut lanes = Vec::new();
        for (read, trace) in traces.into_iter().enumerate() {
            let calls = draft.inputs[read]
                .calls
                .as_ref()
                .ok_or("Tracy signal coordinates are missing; regenerate the draft")?;
            if calls
                .positions
                .last()
                .is_some_and(|&p| p as usize >= trace.samples())
            {
                return Err("Tracy event is outside the source signal".into());
            }
            let reverse = draft.inputs[read].orientation == Orientation::Reverse;
            let oriented = if reverse {
                sanger_map::reverse_complement(calls.sequence.as_bytes())
            } else {
                calls.sequence.as_bytes().to_vec()
            };
            let proposed = draft.proposed_read_calls(read + 1);
            let events = sanger_map::tracy_event_map(&proposed, &oriented);
            let mut index = 0;
            let column_events = draft
                .columns
                .iter()
                .map(|column| {
                    if !column
                        .support
                        .iter()
                        .any(|s| s.read == read + 1 && s.kind == SupportKind::Call)
                    {
                        return None;
                    }
                    let event =
                        events[index].map(|i| if reverse { oriented.len() - 1 - i } else { i });
                    index += 1;
                    event
                })
                .collect();
            lanes.push(ReadLane {
                trace,
                column_events,
                gain: 1.0,
                height: 80.0,
            });
        }
        let selected = draft.columns.iter().position(|c| c.review).unwrap_or(0);
        let columns = (0..draft.columns.len())
            .map(|i| ReviewColumn {
                source: None,
                draft: Some(i),
            })
            .collect();
        let draft_display = (0..draft.columns.len()).collect();
        let mut result = Self {
            consensus: draft.consensus.as_bytes().to_vec(),
            unresolved: vec![true; draft.columns.len()],
            masked: vec![false; draft.columns.len()],
            review_settings: basecraft_docs::sanger::ReviewSettings::default(),
            reference_evidence: None,
            draft,
            lanes,
            selected,
            width: 800.0,
            height: 650.0,
            alignment: None,
            columns,
            column_width: 12.0,
            left: 0.0,
            hovered: None,
            draft_display,
        };
        result.select_column(selected);
        Ok(result)
    }

    pub fn attach_alignment(
        &mut self,
        placement: SangerPlacement,
        alignment: AlignmentDoc,
    ) -> Result<(), String> {
        let context = AlignmentContext::new(&self.draft, placement, alignment)?;
        let mut columns = Vec::new();
        let mut source = 0;
        let first = context
            .columns
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or(0);
        // Leading draft overhangs are placed immediately before the first
        // mapped source column. Internal insertions get their own columns.
        for (i, &mapped) in context.columns.iter().enumerate() {
            let target = mapped.unwrap_or(if i == 0 { first } else { source });
            while source < target {
                columns.push(ReviewColumn {
                    source: Some(source),
                    draft: None,
                });
                source += 1;
            }
            columns.push(ReviewColumn {
                source: mapped,
                draft: Some(i),
            });
            if let Some(col) = mapped {
                source = col + 1;
            }
        }
        while source < context.alignment.width() as usize {
            columns.push(ReviewColumn {
                source: Some(source),
                draft: None,
            });
            source += 1;
        }
        self.draft_display = vec![0; self.draft.columns.len()];
        for (display, col) in columns.iter().enumerate() {
            if let Some(i) = col.draft {
                self.draft_display[i] = display;
            }
        }
        self.columns = columns;
        self.alignment = Some(context);
        self.select_column(self.selected);
        Ok(())
    }

    pub fn reference_mask_bounds(&self) -> Option<(usize, usize)> {
        let first = self.columns.iter().position(|c| c.source.is_some())?;
        let last = self.columns.iter().rposition(|c| c.source.is_some())?;
        let start = self.columns[first..=last]
            .iter()
            .filter_map(|c| c.draft)
            .min()?;
        let end = self.columns[first..=last]
            .iter()
            .filter_map(|c| c.draft)
            .max()?
            + 1;
        Some((start, end))
    }

    fn available(&self) -> f64 {
        f64::from((self.width - NAMES).max(1.0))
    }
    fn clamp(&mut self) {
        self.left = self.left.clamp(
            0.0,
            (self.columns.len() as f64 - self.available() / f64::from(self.column_width)).max(0.0),
        );
    }
    pub fn scroll_size(&self) -> f64 {
        (self.available() / (self.columns.len() as f64 * f64::from(self.column_width))).min(1.0)
    }
    pub fn scroll_position(&self) -> f64 {
        self.left / self.columns.len().max(1) as f64
    }
    pub fn scroll_to(&mut self, fraction: f64) {
        self.left = fraction * self.columns.len() as f64;
        self.clamp();
        self.hovered = None;
    }
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self.clamp();
    }
    pub fn select_column(&mut self, column: usize) {
        self.selected = column.min(self.draft.columns.len() - 1);
        let display = self.draft_display[self.selected] as f64;
        let visible = self.available() / f64::from(self.column_width);
        if display < self.left || display + 1.0 > self.left + visible {
            self.left = display - visible * 0.5;
        }
        self.clamp();
    }
    pub fn display_at(&self, x: f32) -> Option<usize> {
        if x < NAMES || x >= self.width {
            return None;
        }
        let col =
            (self.left + f64::from(x - NAMES) / f64::from(self.column_width)).floor() as usize;
        (col < self.columns.len()).then_some(col)
    }
    pub fn select_at(&mut self, x: f32, _y: f32) {
        if let Some(i) = self.display_at(x).and_then(|c| self.columns[c].draft) {
            self.select_column(i);
        }
    }
    pub fn zoom(&mut self, x: f32, factor: f64) {
        // Columns per pixel, so 1 to 100 pixels a column.
        let mut axis = crate::zoom::Axis {
            start: self.left,
            per_px: 1.0 / f64::from(self.column_width),
        };
        axis.zoom_at(f64::from((x - NAMES).max(0.0)), factor, 0.01, 1.0);
        self.left = axis.start;
        self.column_width = (1.0 / axis.per_px) as f32;
        self.clamp();
        self.hovered = None;
    }
    pub fn pan(&mut self, pixels: f32) {
        self.left -= f64::from(pixels / self.column_width);
        self.clamp();
        self.hovered = None;
    }
    pub fn fit(&mut self) {
        self.left = 0.0;
        self.column_width = (self.available() / self.columns.len() as f64).clamp(0.1, 100.0) as f32;
        self.hovered = None;
    }
    /// Simultaneous channel amplitudes at the native Tracy call coordinate.
    /// Independent interval maxima can include adjacent bases and are not a
    /// valid secondary/primary ratio for this event.
    pub fn call_evidence(
        &self,
        read: usize,
        column: usize,
    ) -> Option<crate::sanger_triage::CallEvidence> {
        let lane = &self.lanes[read];
        let event = lane.column_events[column]?;
        let calls = self.draft.inputs[read].calls.as_ref()?;
        let position = calls.positions[event] as usize;
        let mut peaks = std::array::from_fn(|i| lane.trace.channels[i][position].max(0) as u32);
        let mut called = calls.sequence.as_bytes()[event];
        if self.draft.inputs[read].orientation == Orientation::Reverse {
            peaks.reverse();
            called = sanger_map::reverse_complement(&[called])[0];
        }
        Some(crate::sanger_triage::CallEvidence {
            called,
            score: calls.scores[event],
            peaks,
        })
    }
    pub fn frozen_reference(&self) -> Option<basecraft_docs::sanger::ReviewReference> {
        self.alignment
            .as_ref()
            .map(|context| basecraft_docs::sanger::ReviewReference {
                source_path: context.placement.source_path.clone(),
                source_sha256: context.placement.source_sha256.clone(),
                counts: context.stats.iter().map(|s| s.map(|s| s.acgt)).collect(),
            })
            .or_else(|| self.reference_evidence.clone())
    }
    pub fn assessment(&self, column: usize) -> crate::sanger_triage::Assessment {
        let col = &self.draft.columns[column];
        let mut evidence = Vec::new();
        for support in &col.support {
            if support.kind != SupportKind::Call {
                return crate::sanger_triage::Assessment::issue(
                    "Internal gap: review the alignment and traces",
                );
            }
            let Some(read) = self.call_evidence(support.read - 1, column) else {
                return crate::sanger_triage::Assessment::issue(
                    "Read event cannot be mapped uniquely to the original signal",
                );
            };
            evidence.push(read);
        }
        let counts = self
            .alignment
            .as_ref()
            .and_then(|c| c.stats[column].map(|s| s.acgt))
            .or_else(|| {
                self.reference_evidence
                    .as_ref()
                    .and_then(|r| r.counts[column])
            });
        crate::sanger_triage::assess(&self.review_settings, &evidence, counts)
    }
    pub fn requires_review(&self, column: usize) -> bool {
        self.assessment(column).call.is_none()
    }
    pub fn consensus_y(&self) -> f32 {
        ROW + self.alignment.as_ref().map_or(0.0, |a| {
            if a.representatives.is_empty() {
                REFERENCE_ROW
            } else {
                REFERENCE_ROW * a.representatives.len() as f32 + ROW
            }
        })
    }
    pub fn lane_top(&self, read: usize) -> f32 {
        self.consensus_y()
            + ROW
            + 6.0
            + self.lanes[..read]
                .iter()
                .map(|l| l.height + ROW + 8.0)
                .sum::<f32>()
    }
    pub fn content_height(&self) -> f32 {
        self.lane_top(self.lanes.len()) + 16.0
    }

    pub fn next_issue(&mut self, forward: bool) {
        let len = self.draft.columns.len();
        for offset in 1..=len {
            let i = if forward {
                (self.selected + offset) % len
            } else {
                (self.selected + len - offset % len) % len
            };
            if self.requires_review(i) {
                self.select_column(i);
                return;
            }
        }
    }
    pub fn selected_text(&self) -> String {
        self.site_text(self.draft_display[self.selected])
    }
    pub fn alignment_text(&self) -> String {
        self.alignment
            .as_ref()
            .map_or_else(String::new, |_| self.selected_text())
    }
    /// Compact position evidence for the document status bar.
    pub fn site_status(&self, display: usize) -> String {
        let col = self.columns[display];
        let mut parts = Vec::new();
        if let Some(source) = col.source {
            parts.push(format!("Alignment {}", source + 1));
        }
        if let Some(i) = col.draft {
            let state = if self.masked[i] {
                "masked"
            } else if self.unresolved[i] {
                "needs review"
            } else {
                "resolved"
            };
            parts.push(format!(
                "Consensus {}: {} · {state}",
                i + 1,
                self.consensus[i] as char
            ));
            for read in 0..self.lanes.len() {
                if let Some(evidence) = self.call_evidence(read, i) {
                    parts.push(format!(
                        "R{} {} score {} · secondary {:.0}%",
                        read + 1,
                        evidence.called as char,
                        evidence.score,
                        evidence.secondary_percent()
                    ));
                }
            }
        } else {
            parts.push("No trace coverage".into());
        }
        parts.join(" · ")
    }

    pub fn site_text(&self, display: usize) -> String {
        let col = self.columns[display];
        let mut parts = Vec::new();
        if let Some(source) = col.source {
            parts.push(format!("Alignment column {}", source + 1));
        } else if self.alignment.is_some() {
            let first = self
                .columns
                .iter()
                .position(|c| c.source.is_some())
                .unwrap_or(0);
            let last = self
                .columns
                .iter()
                .rposition(|c| c.source.is_some())
                .unwrap_or(0);
            parts.push(
                if display < first {
                    "5′ extension beyond reference"
                } else if display > last {
                    "3′ extension beyond reference"
                } else {
                    "Insertion relative to reference"
                }
                .into(),
            );
        }
        if let Some(i) = col.draft {
            parts.push(format!(
                "Consensus: {}{}",
                self.consensus[i] as char,
                if self.draft.columns[i].conflict {
                    " · reads disagree"
                } else {
                    ""
                }
            ));
            parts.push(format!("Consensus column {}", i + 1));
            let assessment = self.assessment(i);
            parts.push(format!("Threshold assessment: {}", assessment.reason));
            if let Some(base) = assessment.suggested {
                parts.push(format!("Signal-supported candidate: {base}"));
            }
            if self.masked[i] {
                parts.push("Masked · excluded from consensus exports".into());
            }
            for (read, lane) in self.lanes.iter().enumerate() {
                let base = self.draft.columns[i]
                    .support
                    .iter()
                    .find(|s| s.read == read + 1)
                    .map_or("no coverage".into(), |s| s.base.to_string());
                let score = lane.column_events[i].and_then(|event| {
                    self.draft.inputs[read]
                        .calls
                        .as_ref()
                        .map(|calls| calls.scores[event])
                });
                if let Some(evidence) = self.call_evidence(read, i) {
                    parts.push(format!("Read {} native Tracy call {} · event signal A:{} C:{} G:{} T:{} · secondary/primary {:.1}% (ambiguous at ≥{}%)",
                        read + 1, evidence.called as char, evidence.peaks[0], evidence.peaks[1], evidence.peaks[2], evidence.peaks[3], evidence.secondary_percent(), self.review_settings.ambiguity_percent));
                }
                parts.push(format!(
                    "Read {}: {}{}",
                    read + 1,
                    base,
                    score.map_or(String::new(), |q| format!(" · Tracy score {q}"))
                ));
            }
        }
        if let (Some(context), Some(source)) = (&self.alignment, col.source) {
            if let Some(stats) = context.alignment.column_stats(source as u64) {
                parts.push(format!(
                    "Reference A:{} C:{} G:{} T:{} gaps:{}",
                    stats.acgt[0], stats.acgt[1], stats.acgt[2], stats.acgt[3], stats.gap
                ));
            }
        }
        parts.join("\n")
    }
    fn x(&self, column: f64) -> f32 {
        NAMES + ((column - self.left) * f64::from(self.column_width)) as f32
    }
    fn glyph(&self, frame: &mut Frame, display: usize, y: f32, base: u8, theme: &Theme) {
        let x = self.x(display as f64 + 0.5);
        if x < NAMES || x >= self.width {
            return;
        }
        let color = base_color(base, theme);
        if self.column_width >= 8.0 {
            frame.glyph(x - 4.0, y + 3.0, 16.0, base, color);
        } else if base != b'-' && base != b' ' {
            frame.rect(x, y + 5.0, self.column_width.max(1.0), 12.0, color);
        }
    }
    fn bold_glyph(&self, frame: &mut Frame, display: usize, y: f32, base: u8, theme: &Theme) {
        self.glyph(frame, display, y, base, theme);
        // Synthetic bold with the same atlas and cell advance.
        let x = self.x(display as f64 + 0.5);
        if self.column_width >= 8.0 && x >= NAMES && x < self.width {
            frame.glyph(x - 3.35, y + 3.0, 16.0, base, base_color(base, theme));
        }
    }
    fn label(frame: &mut Frame, y: f32, text: &str, theme: &Theme) {
        // Leave sequence cells clear even for long filenames.
        let label: String = text.chars().take(24).collect();
        frame.text(8.0, y + 4.0, 12.0, &label, theme.foreground);
    }
    fn bold_label(frame: &mut Frame, y: f32, text: &str, theme: &Theme) {
        Self::label(frame, y, text, theme);
        frame.text(8.55, y + 4.0, 12.0, text, theme.foreground);
    }
    pub fn build(&self, theme: &Theme, frame: &mut Frame) {
        frame.clear();
        frame.rect(0.0, 0.0, self.width, self.height, theme.background);
        let first = (self.left.floor() as usize).min(self.columns.len());
        let end = (first + (self.available() / f64::from(self.column_width)).ceil() as usize + 2)
            .min(self.columns.len());
        let mut y = ROW;
        if let Some(context) = &self.alignment {
            Self::bold_label(frame, y, "Reference consensus", theme);
            for display in first..end {
                let base = self.columns[display]
                    .source
                    .and_then(|col| context.alignment.consensus_at(col as u64))
                    .map_or(b'-', |c| c.residue);
                self.bold_glyph(frame, display, y, base.to_ascii_uppercase(), theme);
            }
            y += REFERENCE_ROW;
            for (row, representative) in context.representatives.iter().enumerate() {
                Self::label(
                    frame,
                    y,
                    &context.alignment.row(representative.row).name,
                    theme,
                );
                for display in first..end {
                    let base = self.columns[display].source.map_or(b'-', |col| {
                        context
                            .alignment
                            .read(representative.row, col as u64, col as u64 + 1)[0]
                    });
                    self.glyph(frame, display, y, base.to_ascii_uppercase(), theme);
                }
                y += if row + 1 == context.representatives.len() {
                    ROW
                } else {
                    REFERENCE_ROW
                };
            }
        }
        if self.alignment.is_some() {
            frame.rect(
                8.0,
                y - 2.0,
                (self.width - 16.0).max(0.0),
                1.0,
                [
                    theme.foreground[0],
                    theme.foreground[1],
                    theme.foreground[2],
                    0.18,
                ],
            );
        }
        Self::bold_label(frame, y, "Trace consensus", theme);
        for display in first..end {
            if let Some(i) = self.columns[display].draft {
                if self.unresolved[i] {
                    let x = self.x(display as f64).max(NAMES);
                    frame.rect(
                        x,
                        y,
                        self.column_width.min(self.width - x).max(0.0),
                        ROW,
                        [theme.accent[0], theme.accent[1], theme.accent[2], 0.12],
                    );
                }
                self.bold_glyph(frame, display, y, self.consensus[i], theme);
            } else {
                self.glyph(frame, display, y, b'-', theme);
            }
        }
        y += ROW + 6.0;
        for (read, lane) in self.lanes.iter().enumerate() {
            let name = self.draft.inputs[read]
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            let lane_height = lane.height + ROW + 8.0;
            let sequence_y = y + lane.height + 8.0;
            Self::label(frame, sequence_y, &name, theme);
            for display in first..end {
                let base = self.columns[display]
                    .draft
                    .and_then(|i| {
                        self.draft.columns[i]
                            .support
                            .iter()
                            .find(|s| s.read == read + 1)
                    })
                    .map_or(b' ', |s| s.base as u8);
                self.glyph(frame, display, sequence_y, base, theme);
            }
            self.signal(read, lane, y, lane_height - ROW - 8.0, theme, frame);
            y += lane_height;
        }
        // Masked evidence stays visible; exports use the same mask state.
        for display in first..end {
            if self.columns[display].draft.is_some_and(|i| self.masked[i]) {
                let x = self.x(display as f64).max(NAMES);
                let width = self.column_width.min(self.width - x).max(0.0);
                frame.rect(
                    x,
                    ROW,
                    width,
                    (self.content_height() - ROW - 8.0).max(0.0),
                    [
                        theme.background[0],
                        theme.background[1],
                        theme.background[2],
                        0.65,
                    ],
                );
                frame.rect(
                    x,
                    self.consensus_y() + ROW * 0.5,
                    width,
                    1.0,
                    [
                        theme.foreground[0],
                        theme.foreground[1],
                        theme.foreground[2],
                        0.25,
                    ],
                );
            }
        }
        // Coordinate ruler belongs to the same grid as every row and peak.
        for display in first..end {
            if let Some(source) = self.columns[display].source {
                if source % 10 == 0 && self.column_width >= 6.0 {
                    let x = self.x(display as f64 + 0.5);
                    if x >= NAMES && x < self.width - 24.0 {
                        frame.text(
                            x - 3.0,
                            3.0,
                            10.0,
                            &(source + 1).to_string(),
                            theme.foreground_muted,
                        );
                    }
                }
            }
        }
        if let Some(display) = self.hovered {
            let x = self.x(display as f64 + 0.5);
            if x >= NAMES && x < self.width {
                frame.rect(x, 0.0, 1.0, self.height, theme.accent);
            }
        }
        let selected_x = self.x(self.draft_display[self.selected] as f64 + 0.5);
        if selected_x >= NAMES && selected_x < self.width {
            let left = (selected_x - self.column_width * 0.5).max(NAMES);
            let right = (selected_x + self.column_width * 0.5).min(self.width);
            let bottom = self.content_height().min(self.height) - 8.0;
            let color = [theme.accent[0], theme.accent[1], theme.accent[2], 0.35];
            frame.rect(left, ROW, right - left, 1.0, color);
            frame.rect(left, bottom, right - left, 1.0, color);
            frame.rect(left, ROW, 1.0, bottom - ROW, color);
            frame.rect(right - 1.0, ROW, 1.0, bottom - ROW, color);
            frame.rect(selected_x - 2.0, 19.0, 4.0, 3.0, theme.accent);
            frame.rect(
                selected_x - 5.0,
                self.consensus_y() + ROW - 2.0,
                10.0,
                2.0,
                theme.accent,
            );
        }
    }
    fn signal(
        &self,
        read: usize,
        lane: &ReadLane,
        top: f32,
        height: f32,
        theme: &Theme,
        frame: &mut Frame,
    ) {
        let calls = self.draft.inputs[read]
            .calls
            .as_ref()
            .expect("validated Tracy calls");
        let reverse = self.draft.inputs[read].orientation == Orientation::Reverse;
        let samples = lane.trace.samples();
        let mut anchors: Vec<(f64, f64)> = lane
            .column_events
            .iter()
            .enumerate()
            .filter_map(|(i, event)| {
                event.map(|event| {
                    let original = calls.positions[event] as usize;
                    let sample = if reverse {
                        samples - 1 - original
                    } else {
                        original
                    };
                    (sample as f64, self.draft_display[i] as f64 + 0.5)
                })
            })
            .collect();
        if anchors.is_empty() {
            return;
        }
        let spacing = if anchors.len() > 1 {
            (anchors[anchors.len() - 1].0 - anchors[0].0) / (anchors.len() - 1) as f64
        } else {
            12.0
        };
        let first = anchors[0];
        let last = anchors[anchors.len() - 1];
        anchors.insert(0, ((first.0 - spacing * 0.5).max(0.0), first.1 - 0.5));
        anchors.push((
            (last.0 + spacing * 0.5).min((samples - 1) as f64),
            last.1 + 0.5,
        ));
        let visible: Vec<_> = anchors
            .windows(2)
            .filter(|a| self.x(a[1].1) >= NAMES && self.x(a[0].1) <= self.width && a[1].0 > a[0].0)
            .collect();
        let mut maximum = 1;
        for pair in &visible {
            for sample in pair[0].0 as usize..=(pair[1].0.ceil() as usize).min(samples - 1) {
                let original = if reverse {
                    samples - 1 - sample
                } else {
                    sample
                };
                for channel in &lane.trace.channels {
                    maximum = maximum.max(channel[original]);
                }
            }
        }
        for pair in visible {
            let (s0, c0) = pair[0];
            let (s1, c1) = pair[1];
            for sample in s0.ceil() as usize..(s1.ceil() as usize).min(samples - 1) {
                let x0 = self.x(c0 + (sample as f64 - s0) / (s1 - s0) * (c1 - c0));
                let x1 = self.x(c0 + (sample as f64 + 1.0 - s0) / (s1 - s0) * (c1 - c0));
                if x1 <= NAMES || x0 >= self.width {
                    continue;
                }
                // Alignment gaps have no call event and do not acquire a peak.
                let col = ((c0 + (sample as f64 + 0.5 - s0) / (s1 - s0) * (c1 - c0)).floor()
                    as usize)
                    .min(self.columns.len() - 1);
                if !self.columns[col].draft.is_some_and(|i| {
                    lane.column_events[i].is_some()
                        && self.draft.columns[i]
                            .support
                            .iter()
                            .any(|s| s.read == read + 1 && s.kind == SupportKind::Call)
                }) {
                    continue;
                }
                for channel in 0..4 {
                    let actual_channel = if reverse { 3 - channel } else { channel };
                    let original0 = if reverse {
                        samples - 1 - sample
                    } else {
                        sample
                    };
                    let original1 = if reverse {
                        samples - 2 - sample
                    } else {
                        sample + 1
                    };
                    let y0 = top
                        + height
                            * (1.0
                                - lane.trace.channels[actual_channel][original0].max(0) as f32
                                    / maximum as f32
                                    * lane.gain);
                    let y1 = top
                        + height
                            * (1.0
                                - lane.trace.channels[actual_channel][original1].max(0) as f32
                                    / maximum as f32
                                    * lane.gain);
                    let left = x0.max(NAMES);
                    let right = x1.min(self.width);
                    let a = (left - x0) / (x1 - x0);
                    let b = (right - x0) / (x1 - x0);
                    let mut left = left;
                    let mut right = right;
                    let mut ya = y0 + (y1 - y0) * a;
                    let mut yb = y0 + (y1 - y0) * b;
                    if ya < top && yb < top {
                        continue;
                    }
                    if ya < top {
                        left += (right - left) * (top - ya) / (yb - ya);
                        ya = top;
                    }
                    if yb < top {
                        right = left + (right - left) * (top - ya) / (yb - ya);
                        yb = top;
                    }
                    segment(
                        frame,
                        left,
                        ya,
                        right,
                        yb,
                        base_color(b"ACGT"[channel], theme),
                    );
                }
            }
        }
    }
}

/// Infer an editable specimen ID from the source read names.
pub fn suggested_name(paths: &[std::path::PathBuf]) -> String {
    let names: Vec<String> = paths
        .iter()
        .map(|path| {
            let mut name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if let Some((prefix, suffix)) = name.rsplit_once(['_', '-', '.']) {
                if ["forward", "reverse", "fwd", "rev", "f", "r"]
                    .contains(&suffix.to_ascii_lowercase().as_str())
                {
                    name = prefix.to_owned();
                }
            }
            name
        })
        .collect();
    let Some(first) = names.first() else {
        return "specimen".into();
    };
    let common: String = first
        .chars()
        .enumerate()
        .take_while(|(i, c)| names.iter().all(|n| n.chars().nth(*i) == Some(*c)))
        .map(|(_, c)| c)
        .collect();
    let name = if common.len() >= 3 {
        common.trim_end_matches(['_', '-', '.'])
    } else {
        first
    };
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "specimen".into()
    } else {
        name
    }
}

fn base_color(base: u8, theme: &Theme) -> [f32; 4] {
    match base {
        b'A' => [0.21, 0.72, 0.36, 1.0],
        b'C' => [0.26, 0.53, 0.93, 1.0],
        b'G' => theme.foreground,
        b'T' => [0.91, 0.30, 0.27, 1.0],
        _ => theme.foreground_muted,
    }
}
fn segment(frame: &mut Frame, x0: f32, y0: f32, x1: f32, y1: f32, color: [f32; 4]) {
    let length = (x1 - x0).hypot(y1 - y0).max(0.001);
    let dx = -(y1 - y0) / length * 0.65;
    let dy = (x1 - x0) / length * 0.65;
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
    use basecraft_docs::alignment::AlnRow;
    use basecraft_io::abif::CallSet;
    use std::sync::Arc;

    fn review(reverse: bool) -> SangerReviewView {
        let raw = serde_json::json!({
            "status":"draft_requires_review", "engine":"Tracy", "version_output":"test",
            "engine_sha256":"a".repeat(64), "mode":"consensus",
            "inputs":[
                {"read":1,"path":"forward.ab1","sha256":"a".repeat(64),"orientation":"forward", "calls":{"sequence":"AC","positions":[10,60],"scores":[30,20]}},
                {"read":2,"path":"reverse.ab1","sha256":"b".repeat(64),"orientation":if reverse {"reverse"} else {"forward"},"calls":{"sequence":if reverse {"GT"} else {"AC"},"positions":[10,60],"scores":[25,35]}}
            ],
            "consensus":"AC", "consensus_length":2,
            "minimum_overlap":1, "minimum_identity":0.8, "overlaps":[], "aligned_columns":2, "conflict_columns":0,
            "columns":[
                {"column":1,"consensus":"A","support":[{"read":1,"base":"A","kind":"call"},{"read":2,"base":"A","kind":"call"}],"conflict":false,"review":false},
                {"column":2,"consensus":"C","support":[{"read":1,"base":"C","kind":"call"},{"read":2,"base":"C","kind":"call"}],"conflict":false,"review":true}
            ]
        });
        let traces = (0..2)
            .map(|read| {
                let mut channels = std::array::from_fn(|_| vec![0; 100]);
                channels[if read == 1 && reverse { 3 } else { 0 }]
                    [if read == 1 && reverse { 60 } else { 10 }] = 100;
                channels[if read == 1 && reverse { 2 } else { 1 }]
                    [if read == 1 && reverse { 10 } else { 60 }] = 100;
                Trace {
                    channels,
                    call_sets: vec![CallSet {
                        number: 1,
                        bases: b"NN".to_vec(),
                        positions: vec![5, 75],
                        qualities: None,
                    }],
                    channel_order: *b"ACGT",
                }
            })
            .collect();
        SangerReviewView::new(serde_json::from_value(raw).unwrap(), traces).unwrap()
    }
    fn attach(view: &mut SangerReviewView) {
        let rows = vec![
            AlnRow::new("reference-a", Arc::new(b"A-GC".to_vec())),
            AlnRow::new("reference-b", Arc::new(b"A--C".to_vec())),
        ];
        let placement = SangerPlacement {
            status: "candidate_requires_review".into(),
            engine: "MAFFT".into(),
            version: "test".into(),
            source_path: "source.afa".into(),
            source_sha256: "a".repeat(64),
            source_rows: 2,
            source_width: 4,
            fragment: "AC".into(),
            map: vec![Some(0), Some(3)],
            aligned_probe: "A--C".into(),
            insertions: 0,
            parameters: vec![],
        };
        view.attach_alignment(placement, AlignmentDoc::new("reference", rows))
            .unwrap();
    }
    #[test]
    fn tracy_events_are_used_even_when_vendor_calls_disagree() {
        let view = review(false);
        assert_eq!(view.lanes[0].column_events, vec![Some(0), Some(1)]);
        assert!(view.selected_text().contains("Tracy score 20"));
        assert!(!view.selected_text().contains("original"));
    }
    #[test]
    fn status_summary_tracks_review_masks_and_reference_only_columns() {
        let mut view = review(false);
        attach(&mut view);
        view.unresolved[0] = true;
        let status = view.site_status(0);
        assert!(status.contains("Alignment 1"));
        assert!(status.contains("Consensus 1: A · needs review"));
        assert!(status.contains("R1 A score 30"));
        assert!(!status.contains('\n'));
        assert!(!status.contains("event signal"));
        view.masked[0] = true;
        assert!(view.site_status(0).contains("masked"));
        view.masked[0] = false;
        view.unresolved[0] = false;
        assert!(view.site_status(0).contains("resolved"));
        assert_eq!(view.site_status(1), "Alignment 2 · No trace coverage");
    }

    #[test]
    fn reference_gaps_remain_in_the_shared_grid() {
        let mut view = review(true);
        attach(&mut view);
        assert_eq!(view.draft_display, vec![0, 3]);
        assert_eq!(view.columns[1].source, Some(1));
        assert_eq!(view.columns[1].draft, None);
        let x = view.x(3.5);
        view.select_at(x, 500.0);
        assert_eq!(view.selected, 1);
        assert_eq!(view.display_at(x), Some(3));
        assert!(view.site_text(3).contains("Alignment column 4"));
    }
    #[test]
    fn reverse_trace_peaks_have_the_complementary_color_and_shared_x() {
        let view = review(true);
        assert_eq!(view.lanes[1].column_events, vec![Some(1), Some(0)]);
        let mut forward = Frame::new();
        let mut reverse = Frame::new();
        view.signal(
            0,
            &view.lanes[0],
            0.0,
            100.0,
            &Theme::default(),
            &mut forward,
        );
        view.signal(
            1,
            &view.lanes[1],
            0.0,
            100.0,
            &Theme::default(),
            &mut reverse,
        );
        for frame in [&forward, &reverse] {
            // The green A peak must reach the same aligned column centre.
            let peak = frame
                .floats()
                .chunks_exact(crate::frame::FLOATS_PER_INSTANCE)
                .find(|q| q[8..12] == base_color(b'A', &Theme::default()) && q[1] < 1.0)
                .unwrap();
            assert!((peak[0] - view.x(0.5)).abs() < 1.0);
        }
    }
    #[test]
    fn insertion_columns_do_not_consume_a_reference_column() {
        let mut view = review(false);
        let rows = vec![
            AlnRow::new("a", Arc::new(b"A".to_vec())),
            AlnRow::new("b", Arc::new(b"A".to_vec())),
        ];
        let placement = SangerPlacement {
            status: "candidate_requires_review".into(),
            engine: "MAFFT".into(),
            version: "test".into(),
            source_path: "source.afa".into(),
            source_sha256: "a".repeat(64),
            source_rows: 2,
            source_width: 1,
            fragment: "AC".into(),
            map: vec![Some(0), None],
            aligned_probe: "A".into(),
            insertions: 1,
            parameters: vec![],
        };
        view.attach_alignment(placement, AlignmentDoc::new("ref", rows))
            .unwrap();
        assert_eq!(view.columns.len(), 2);
        assert_eq!(view.columns[1].source, None);
        assert!(view.site_text(1).contains("extension beyond reference"));
    }
    #[test]
    fn zoom_scroll_and_hover_keep_one_coordinate_system() {
        let mut view = review(false);
        attach(&mut view);
        view.column_width = 26.0;
        view.resize(232.0, 500.0);
        view.scroll_to(1.0);
        assert!((view.scroll_position() - 0.5).abs() < 0.001);
        let anchor = view.left + 0.5;
        let x = view.x(anchor);
        view.zoom(x, 2.0);
        assert!(
            (view.left + f64::from(x - NAMES) / f64::from(view.column_width) - anchor).abs()
                < 0.001
        );
        view.fit();
        assert!((view.scroll_size() - 1.0).abs() < 0.001);
    }
    #[test]
    fn triage_accepts_clear_support_but_retains_real_uncertainty() {
        let mut view = review(false);
        assert!(!view.requires_review(0));
        assert!(!view.requires_review(1)); // two matching score-20+ calls
        view.draft.columns[0].support.pop();
        assert!(!view.requires_review(0)); // strong single coverage is sufficient
        view.draft.inputs[0].calls.as_mut().unwrap().scores[0] = 10;
        assert!(view.requires_review(0));
        view.draft.inputs[0].calls.as_mut().unwrap().scores[0] = 40;
        view.lanes[0].column_events[0] = None;
        assert!(view.requires_review(0));
        view.lanes[0].column_events[0] = Some(0);
        view.draft.columns[0].conflict = true;
        assert_eq!(view.assessment(0).call, Some('A')); // evidence overrides a stale draft flag
        view.draft.columns[0].conflict = false;
        view.draft.columns[0].consensus = 'N';
        assert_eq!(view.assessment(0).call, Some('A')); // strong clear evidence resolves an ambiguous draft
    }
    #[test]
    fn neighbouring_peaks_are_not_mistaken_for_ambiguity_at_this_call() {
        let mut view = review(false);
        for lane in &mut view.lanes {
            lane.trace.channels[2][25] = 300;
        }
        assert_eq!(view.assessment(0).call, Some('A'));
        for lane in &mut view.lanes {
            lane.trace.channels[2][10] = 100;
        }
        assert!(
            view.assessment(0).call.is_none(),
            "a genuine secondary signal at the call still requires review"
        );
    }
    #[test]
    fn peak_evidence_is_complemented_into_the_shared_orientation() {
        for reverse in [false, true] {
            let view = review(reverse);
            for column in 0..2 {
                let left = view.call_evidence(0, column).unwrap();
                let right = view.call_evidence(1, column).unwrap();
                assert_eq!(left.peaks, right.peaks);
                assert_eq!(left.called, right.called);
                assert_eq!(
                    view.assessment(column).call,
                    Some(view.draft.columns[column].consensus)
                );
            }
        }
    }
    #[test]
    fn edits_render_and_vertical_gain_stays_inside_trace_lane() {
        let mut view = review(false);
        view.consensus[0] = b'T';
        assert!(view.site_text(0).contains("Consensus: T"));
        let mut frame = Frame::new();
        let theme = Theme::default();
        view.build(&theme, &mut frame);
        let mut expected = Frame::new();
        expected.glyph(0.0, 0.0, 16.0, b'T', base_color(b'T', &theme));
        assert!(
            frame
                .floats()
                .chunks_exact(16)
                .any(|q| q[12..16] == expected.floats()[12..16]
                    && (q[1] - view.consensus_y() - 3.0).abs() < 0.01)
        );
        frame.clear();
        view.lanes[0].gain = 0.5;
        view.signal(0, &view.lanes[0], 100.0, 100.0, &theme, &mut frame);
        let low = frame
            .floats()
            .chunks_exact(16)
            .flat_map(|q| [q[1], q[3], q[5], q[7]])
            .fold(f32::INFINITY, f32::min);
        assert!(low > 149.0);
        frame.clear();
        view.lanes[0].gain = 5.0;
        view.signal(0, &view.lanes[0], 100.0, 100.0, &theme, &mut frame);
        assert!(frame.len() > 10);
        let high = frame
            .floats()
            .chunks_exact(16)
            .flat_map(|q| [q[1], q[3], q[5], q[7]])
            .fold(f32::INFINITY, f32::min);
        assert!(high < 101.0);
        for q in frame.floats().chunks_exact(16) {
            for corner in 0..4 {
                assert!(q[corner * 2 + 1] >= 99.0);
                assert!(q[corner * 2 + 1] <= 201.0);
            }
        }
    }

    #[test]
    fn trace_height_is_independent_of_gain_and_viewport() {
        let mut view = review(false);
        attach(&mut view);
        assert_eq!(view.lanes[0].height, 80.0);
        assert!((view.lane_top(1) - view.lane_top(0) - 112.0).abs() < 0.01);
        let original = view.content_height();
        view.resize(900.0, 1200.0);
        assert!((view.content_height() - original).abs() < 0.01);
        view.lanes[0].height = 320.0;
        view.lanes[0].gain = 4.0;
        assert!((view.content_height() - original - 240.0).abs() < 0.01);
        assert_eq!(view.lanes[1].height, 80.0);
        // Three representatives are packed 20 px apart, with a 24 px gap
        // after the last one before the editable consensus.
        let rows = view.alignment.as_ref().unwrap().representatives.len();
        assert!((view.consensus_y() - (ROW + REFERENCE_ROW * rows as f32 + ROW)).abs() < 0.01);
    }
    #[test]
    fn context_includes_closest_farthest_and_two_distinct_extras() {
        let view = review(false);
        let rows = ["AC", "AA", "CC", "AT", "GC", "TT"]
            .iter()
            .enumerate()
            .map(|(i, sequence)| {
                AlnRow::new(format!("row{i}"), Arc::new(sequence.as_bytes().to_vec()))
            })
            .collect();
        let placement = SangerPlacement {
            status: "candidate_requires_review".into(),
            engine: "MAFFT".into(),
            version: "test".into(),
            source_path: "ref.afa".into(),
            source_sha256: "a".repeat(64),
            source_rows: 6,
            source_width: 2,
            fragment: "AC".into(),
            map: vec![Some(0), Some(1)],
            aligned_probe: "AC".into(),
            insertions: 0,
            parameters: vec![],
        };
        let context =
            AlignmentContext::new(&view.draft, placement, AlignmentDoc::new("ref", rows)).unwrap();
        assert_eq!(context.representatives.len(), 3);
        assert_eq!(context.representatives[0].row, 0);
        assert_eq!(context.representatives[1].row, 5);
        let mut selected: Vec<_> = context.representatives.iter().map(|r| r.row).collect();
        selected.sort_unstable();
        selected.dedup();
        assert_eq!(selected.len(), 3);
    }
    #[test]
    fn names_are_inferred_from_read_names() {
        let paths = |names: &[&str]| {
            names
                .iter()
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            suggested_name(&paths(&[
                "/reads/RBNII397-13_forward.ab1",
                "/reads/RBNII397-13_reverse.ab1"
            ])),
            "RBNII397-13"
        );
        assert_eq!(
            suggested_name(&paths(&["sample-F.ab1", "sample-R.ab1"])),
            "sample"
        );
        assert_eq!(
            suggested_name(&paths(&["sample_F1.ab1", "sample_R1.ab1"])),
            "sample"
        );
        assert_eq!(suggested_name(&paths(&["my sample.ab1"])), "my_sample");
        assert_eq!(suggested_name(&[]), "specimen");
    }
    #[test]
    fn real_pair_alignment_renders_when_demo_evidence_is_supplied() {
        let Ok(path) = std::env::var("BASECRAFT_SANGER_EVIDENCE") else {
            return;
        };
        let mut draft = SangerDraft::read(std::path::Path::new(&path)).unwrap();
        if std::env::var("BASECRAFT_SANGER_REVERSE").as_deref() == Ok("1") {
            draft = draft.reverse_complemented();
        }
        let traces = draft
            .inputs
            .iter()
            .map(|r| basecraft_io::abif::open(&r.path).unwrap())
            .collect();
        let mut view = SangerReviewView::new(draft, traces).unwrap();
        for lane in &view.lanes {
            assert!(lane.column_events.iter().flatten().count() > 500);
        }
        let source = std::env::var("BASECRAFT_SANGER_REFERENCE").unwrap();
        let placement = SangerPlacement::read(std::path::Path::new(
            &std::env::var("BASECRAFT_SANGER_PLACEMENT").unwrap(),
        ))
        .unwrap();
        let indexed =
            Arc::new(basecraft_io::cache::open_fasta(std::path::Path::new(&source)).unwrap());
        view.attach_alignment(placement, AlignmentDoc::from_fasta("demo", &indexed))
            .unwrap();
        let context = view.alignment.as_ref().unwrap();
        let comparable = context
            .placement
            .map
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.map(|c| (i, c)))
            .collect::<Vec<_>>();
        let matches = comparable
            .iter()
            .filter(|&&(i, c)| {
                context
                    .alignment
                    .consensus_at(c as u64)
                    .unwrap()
                    .residue
                    .to_ascii_uppercase()
                    == context.placement.fragment.as_bytes()[i]
            })
            .count();
        assert!(
            matches * 100 / comparable.len() > 95,
            "demo draft must face the reference alignment"
        );
        let (keep_start, keep_end) = view.reference_mask_bounds().unwrap();
        let pending = (keep_start..keep_end)
            .filter(|&i| view.requires_review(i))
            .count();
        view.masked = (0..view.draft.columns.len())
            .map(|i| i < keep_start || i >= keep_end)
            .collect();
        println!(
            "Reference mask retains columns {}..{}; {} terminal columns masked",
            keep_start + 1,
            keep_end,
            view.draft.columns.len() - (keep_end - keep_start)
        );
        println!(
            "Real demo triage: {pending}/{} unresolved",
            view.draft.columns.len()
        );
        if let Ok(path) = std::env::var("BASECRAFT_SANGER_DIAGNOSTICS") {
            let diagnostics: Vec<_> = (keep_start..keep_end).map(|i| {
                let col = &view.draft.columns[i];
                let reads: Vec<_> = col.support.iter().filter_map(|support| {
                    let read = support.read - 1;
                    let evidence = view.call_evidence(read,i)?;
                    let event = view.lanes[read].column_events[i]?;
                    let calls = view.draft.inputs[read].calls.as_ref()?;
                    let sample = calls.positions[event] as usize;
                    let mut point: [i32;4] = std::array::from_fn(|c| view.lanes[read].trace.channels[c][sample]);
                    if view.draft.inputs[read].orientation == Orientation::Reverse { point.reverse(); }
                    Some(serde_json::json!({"called":evidence.called as char,"score":evidence.score,"peaks":evidence.peaks,"point":point,"support":support.base,"kind":format!("{:?}",support.kind)}))
                }).collect();
                let score_pass = reads.iter().any(|r| r["score"].as_i64().unwrap()>=30)
                    || (reads.len()>=2 && reads.iter().all(|r| r["score"].as_i64().unwrap()>=20));
                let original_pass = "ACGT".contains(col.consensus) && !col.conflict && !reads.is_empty()
                    && reads.len()==col.support.len() && col.support.iter().all(|s| s.kind==SupportKind::Call && s.base==col.consensus)
                    && reads.iter().all(|r| r["called"]==col.consensus.to_string()) && score_pass;
                serde_json::json!({"column":i+1,"consensus":col.consensus,"old_issue":!original_pass,"new_issue":view.requires_review(i),"reason":view.assessment(i).reason,"reads":reads})
            }).collect();
            std::fs::write(path, serde_json::to_vec_pretty(&diagnostics).unwrap()).unwrap();
        }
        // The example has no curated truth labels. Validate configurable
        // evidence filtering, rather than prescribing a biological error count.
        let defaults = view.review_settings.clone();
        view.review_settings.strong_score = 100;
        view.review_settings.paired_score = 100;
        let strict = (keep_start..keep_end)
            .filter(|&i| view.requires_review(i))
            .count();
        assert!(
            strict > pending,
            "stricter score thresholds must flag more of these demo calls"
        );
        view.review_settings = defaults;
        let live_calls: Vec<_> = (0..view.draft.columns.len())
            .map(|i| view.assessment(i).call)
            .collect();
        view.reference_evidence = view.frozen_reference();
        let live = view.alignment.take();
        let frozen_calls: Vec<_> = (0..view.draft.columns.len())
            .map(|i| view.assessment(i).call)
            .collect();
        assert_eq!(
            live_calls, frozen_calls,
            "stored whole-alignment counts reproduce triage without the MSA"
        );
        view.alignment = live;
        view.unresolved = (0..view.draft.columns.len())
            .map(|i| !view.masked[i] && view.requires_review(i))
            .collect();
        view.resize(1200.0, 650.0);
        view.selected = 200;
        view.select_column(200);
        view.hovered = Some(view.draft_display[200]);
        let mut frame = Frame::new();
        view.build(&Theme::default(), &mut frame);
        assert!(frame.len() > 1000);
        if let Ok(path) = std::env::var("BASECRAFT_SANGER_SVG") {
            std::fs::write(
                path,
                crate::svg::frame_svg(
                    &frame,
                    view.width,
                    view.height,
                    "monospace",
                    Theme::default().background,
                ),
            )
            .unwrap();
        }
    }
}
