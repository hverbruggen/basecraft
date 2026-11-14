//! Deterministic frame-output budgets, not timing or allocation benchmarks.
//! Run with ordinary `cargo test -p basecraft-viewmodel --test integration budgets::`.
//! Synthetic geometry fixtures deliberately repeat: growing the hidden document
//! must not increase output at the same viewport and zoom. They are not models
//! of biological variation. No renderer, filesystem or test counters are needed.

use std::sync::Arc;

use basecraft_docs::alignment::AlnRow;
use basecraft_docs::{AlignmentDoc, SequenceDoc};
use basecraft_protocol::{Feature, Location, Qualifier, Strand};
use basecraft_viewmodel::Frame;
use basecraft_viewmodel::alignment::AlignmentView;
use basecraft_viewmodel::frame::FLOATS_PER_INSTANCE;
use basecraft_viewmodel::genome::{GenomeView, Mode};
use basecraft_viewmodel::theme::Theme;

const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 400.0;

fn alignment(rows: usize, cols: usize) -> AlignmentDoc {
    AlignmentDoc::new(
        "frame-budget",
        (0..rows)
            .map(|row| {
                let residues: Vec<u8> = b"ACGTACGT--ACGTAC"
                    .iter()
                    .copied()
                    .cycle()
                    .take(cols)
                    .collect();
                // Same label width at every row and document size.
                AlnRow::new(format!("row{row:05}"), Arc::new(residues))
            })
            .collect(),
    )
}

fn genome(len: usize) -> SequenceDoc {
    let residues: Vec<u8> = b"ACGT".iter().copied().cycle().take(len).collect();
    let features = (0..u64::try_from(len).unwrap())
        .step_by(100)
        .flat_map(|base| {
            [(10, 50, Strand::Forward), (60, 90, Strand::Reverse)].map(|(start, end, strand)| {
                Feature {
                    kind: "gene".into(),
                    location: Location::simple(base + start, base + end, strand),
                    qualifiers: vec![Qualifier::new("gene", "marker")],
                }
            })
        })
        .collect();
    SequenceDoc::new("frame-budget", Arc::new(residues), features, false)
}

fn glyphs(frame: &Frame) -> usize {
    frame
        .floats()
        .as_chunks::<FLOATS_PER_INSTANCE>()
        .0
        .iter()
        .filter(|quad| quad[14] > 0.0)
        .count()
}

#[test]
fn alignment_frames_depend_on_visible_cells() {
    let theme = Theme::default();
    // Vary hidden rows and columns separately as well as together.
    // Measured before gating. Each row is first, quarter, middle, final page;
    // ruler digit lengths and partial rows account for the small differences.
    let cases = [
        (
            128,
            4096,
            [
                [2828, 2839, 2839, 2668],
                [14984, 14985, 14805, 14722],
                [10034, 9951, 9952, 9956],
            ],
        ),
        (
            512,
            4096,
            [
                [2828, 2839, 2839, 2668],
                [14984, 14985, 14990, 14722],
                [10034, 10031, 10032, 9956],
            ],
        ),
        (
            128,
            16384,
            [
                [2828, 2839, 2839, 2680],
                [14984, 14990, 14805, 14730],
                [10034, 9952, 9952, 9953],
            ],
        ),
        (
            512,
            16384,
            [
                [2828, 2839, 2839, 2680],
                [14984, 14990, 14990, 14730],
                [10034, 10032, 10032, 9953],
            ],
        ),
    ];
    for (rows, cols, baseline) in cases {
        let doc = alignment(rows, cols);
        // Letters, colour runs, then subpixel overview sampling.
        for ((zoom, ceiling), counts) in [(11.0, 3200), (3.0, 18500), (0.5, 11000)]
            .into_iter()
            .zip(baseline)
        {
            let mut view = AlignmentView::new(WIDTH, HEIGHT);
            view.px_per_col = zoom;
            view.sync(&doc);
            let mut frame = Frame::new();
            for (step, fraction) in [0.0, 0.25, 0.5, 1.0].into_iter().enumerate() {
                view.hscroll_to(fraction);
                view.vscroll_to(fraction);
                view.build(&doc, &theme, &mut frame);
                println!(
                    "alignment {rows}x{cols} zoom={zoom} step={step}: {} glyphs={}",
                    frame.len(),
                    glyphs(&frame)
                );
                assert_eq!(frame.len(), counts[step], "measured alignment baseline");
                // Independent geometry ceilings for a 630 px grid: at 11 px,
                // <=60 columns x22 rows, two quads/cell plus names/header;
                // at 3 px, <=212 columns x79 rows, one quad/cell plus labels;
                // at 0.5 px, <=82 occupied runs x122 rows plus the header.
                // The last bound uses this fixture's two-base gaps every 16 bp.
                assert!(frame.len() <= ceiling, "visible alignment output budget");
                if zoom > 7.0 {
                    assert!(glyphs(&frame) >= 1000, "visible residue grid must be drawn");
                }
                // Reusing a frame must replace its contents, not append a frame.
                let first = frame.floats().to_vec();
                view.build(&doc, &theme, &mut frame);
                assert_eq!(frame.floats(), first);
            }
        }
    }
}

#[test]
fn genome_frames_depend_on_visible_tracks() {
    let theme = Theme::default();
    // 40..1000 annotated 100-base blocks, all built with the real document
    // interval index and (for Wrapped) the real wrap layout/cache.
    // Linear letters/colours, then wrapped letters/colours; measured totals.
    let cases = [
        (
            4000,
            [
                [261, 273, 273, 257],
                [504, 511, 511, 486],
                [1062, 1092, 1092, 1092],
                [2030, 2553, 2044, 2044],
            ],
        ),
        (
            20000,
            [
                [261, 273, 285, 267],
                [504, 511, 516, 490],
                [1062, 1092, 1140, 1140],
                [2030, 2555, 2064, 2064],
            ],
        ),
        (
            100_000,
            [
                [261, 285, 285, 268],
                [504, 516, 516, 491],
                [1062, 1140, 1140, 1141],
                [2030, 2580, 2064, 2065],
            ],
        ),
    ];
    for (len, baseline) in cases {
        let doc = genome(len);
        for (mode_index, mode) in [Mode::Linear, Mode::Wrapped].into_iter().enumerate() {
            for (zoom_index, px_per_base) in [8.0, 2.0].into_iter().enumerate() {
                let mut view = GenomeView::new(WIDTH, HEIGHT);
                view.mode = mode;
                view.bp_per_px = 1.0 / px_per_base;
                let mut frame = Frame::new();
                for (step, fraction) in [0.0, 0.25, 0.5, 1.0].into_iter().enumerate() {
                    // First frame initializes wrapped layout; subsequent frames
                    // use the scrollbar API, including its clamped final page.
                    if step > 0 {
                        view.scroll_to(fraction, doc.len());
                    }
                    view.build(&doc, &theme, &mut frame);
                    println!(
                        "genome {len} {mode:?} px={px_per_base} step={step}: {} glyphs={}",
                        frame.len(),
                        glyphs(&frame)
                    );
                    assert_eq!(
                        frame.len(),
                        baseline[mode_index * 2 + zoom_index][step],
                        "measured genome baseline"
                    );
                    // A track spans 100 (letters) or 400 (colours) bases and
                    // at most eight of our features. Allow 600 quads per track
                    // for residues, arrows, six-letter labels and ruler ticks.
                    // Four hundred vertical pixels admit at most five tracks,
                    // including partially visible tracks at both edges.
                    let tracks = if mode == Mode::Linear { 1 } else { 5 };
                    assert!(frame.len() <= 600 * tracks, "visible genome output budget");
                    if zoom_index == 0 {
                        let cells = frame
                            .floats()
                            .as_chunks::<FLOATS_PER_INSTANCE>()
                            .0
                            .iter()
                            .filter(|q| {
                                q[14] == 0.0
                                    && (q[2] - q[0] - 8.0).abs() < 0.01
                                    && (q[5] - q[1] - 20.0).abs() < 0.01
                            })
                            .count();
                        // The final linear page allows a 5% blank trailing margin.
                        assert!(cells >= 95, "actual residue backgrounds must be present");
                        assert!(glyphs(&frame) >= 100, "actual residues must be labelled");
                    }
                    assert_eq!(view.hidden, 0, "fixture annotations must fit");
                    let first = frame.floats().to_vec();
                    view.build(&doc, &theme, &mut frame);
                    assert_eq!(frame.floats(), first);
                }
            }
        }
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn residue_rects(frame: &Frame, width: f32, height: f32, top: f32) -> usize {
    frame
        .floats()
        .as_chunks::<FLOATS_PER_INSTANCE>()
        .0
        .iter()
        .filter(|q| {
            q[14] == 0.0 && q[1] >= top && close(q[2] - q[0], width) && close(q[5] - q[1], height)
        })
        .count()
}

// Compare actual atlas payloads with independently specified fixture residues.
// Size/vertical position separate residues from names, rulers and feature labels.
fn assert_residue_glyphs(frame: &Frame, size: f32, top: f32, expected: &[u8]) {
    let mut actual: Vec<_> = frame
        .floats()
        .as_chunks::<FLOATS_PER_INSTANCE>()
        .0
        .iter()
        .filter(|q| q[14] > 0.0 && q[1] >= top && close(q[2] - q[0], size))
        .map(|q| {
            [
                q[12].to_bits(),
                q[13].to_bits(),
                q[14].to_bits(),
                q[15].to_bits(),
            ]
        })
        .collect();
    let mut reference = Frame::new();
    for &base in expected {
        reference.glyph(0.0, 0.0, size, base, [1.0; 4]);
    }
    let mut expected: Vec<_> = reference
        .floats()
        .as_chunks::<FLOATS_PER_INSTANCE>()
        .0
        .iter()
        .map(|q| {
            [
                q[12].to_bits(),
                q[13].to_bits(),
                q[14].to_bits(),
                q[15].to_bits(),
            ]
        })
        .collect();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected, "visible scientific glyph payload");
}

#[test]
fn alignment_pan_and_zoom_keep_exact_visible_residue_budgets() {
    let theme = Theme::default();
    for (rows, cols) in [(128, 4096), (512, 16384)] {
        let doc = alignment(rows, cols);
        let mut view = AlignmentView::new(WIDTH, HEIGHT);
        let mut frame = Frame::new();
        // Source geometry: 630 px grid, one extra column/row for overscan.
        // 11 px: 59 columns x22 rows (17.05 px high, 57.05 px header).
        // 22 px: 30 columns x14 rows (26 px high, 66 px header).
        // 5.5 px: 116 columns x44 rows (8.525 px high, 40 px header).
        // Ceilings include grid output, eight-character row names, consensus/
        // identity bars and rulers: <=840+112+248 at 22 px, and
        // <=5104+352+744 at 5.5 px. Existing 11 px ceiling is retained.
        // A 16-column pan preserves this fixture's 16-base phase; returning
        // past the left edge must clamp back to the original page.
        for (step, (first, columns, visible_rows, ceiling)) in [
            (0, 59, 22, 3200),
            (16, 59, 22, 3200),
            (16, 30, 14, 1200),
            (16, 116, 44, 6200),
            (0, 116, 44, 6200),
        ]
        .into_iter()
        .enumerate()
        {
            match step {
                1 => view.pan(176.0),
                2 => view.zoom_at(view.name_width, 0.0, 2.0),
                3 => view.zoom_at(view.name_width, 0.0, 0.25),
                4 => view.pan(-1_000_000.0),
                _ => {}
            }
            view.build(&doc, &theme, &mut frame);
            assert_eq!(view.start, f64::from(first));
            let residues: Vec<_> = b"ACGTACGT--ACGTAC"
                .iter()
                .copied()
                .cycle()
                .skip(usize::try_from(first).unwrap())
                .take(columns)
                .collect();
            let occupied = residues.iter().filter(|&&b| b != b'-').count();
            let (width, height) = match step {
                2 => (21.0, 25.0), // one-pixel padding at large letter zoom
                3 | 4 => (5.5, 7.525),
                _ => (11.0, 17.05),
            };
            assert_eq!(
                residue_rects(
                    &frame,
                    width,
                    height,
                    view.header_height() - view.row_height(),
                ),
                occupied * visible_rows,
                "exact grid backgrounds at step {step}"
            );
            if step < 3 {
                assert_residue_glyphs(
                    &frame,
                    if step == 2 { 23.0 } else { 14.05 },
                    view.header_height() - view.row_height(),
                    &residues.repeat(visible_rows),
                );
            }
            assert!(frame.len() <= ceiling, "alignment transition step {step}");
        }
    }
}

#[test]
fn genome_pan_and_zoom_rebuild_visible_tracks() {
    let theme = Theme::default();
    for len in [4000, 20_000] {
        let doc = genome(len);
        for mode in [Mode::Linear, Mode::Wrapped] {
            let mut view = GenomeView::new(WIDTH, HEIGHT);
            view.mode = mode;
            view.bp_per_px = 1.0 / 8.0;
            let mut frame = Frame::new();
            // Annotated wrapped tracks are 106 px high: 76 px feature top,
            // one 14 px lane, 16 px block gap. Four tracks intersect 400 px;
            // scrolling half a track admits five, including both edge tracks.
            // 800 px holds 100/50/400 bases at 8/16/2 px per base.
            for (step, (bases_per_track, cell_width, tracks)) in [
                (100, 8.0, 4),
                (100, 8.0, 4),
                (50, 15.0, 4),
                (400, 2.0, 4),
                (400, 2.0, 5),
            ]
            .into_iter()
            .enumerate()
            {
                match step {
                    1 if mode == Mode::Wrapped => view.scroll_by(106.0),
                    1 => view.pan(-128.0, doc.len()), // 16 bases to the right
                    2 => view.zoom_at(0.0, 2.0, doc.len()),
                    3 => view.zoom_at(0.0, 0.125, doc.len()),
                    4 if mode == Mode::Wrapped => view.scroll_by(53.0),
                    4 => view.pan(1_000_000.0, doc.len()), // clamped 5% margin
                    _ => {}
                }
                view.build(&doc, &theme, &mut frame);
                // Wrapped build leaves start at the last drawn track; linear
                // zoom at x=0 retains the panned left boundary until clamped.
                let start = if mode == Mode::Wrapped {
                    [300.0, 400.0, 250.0, 1200.0, 1600.0][step]
                } else {
                    [0.0, 16.0, 16.0, 16.0, -20.0][step]
                };
                assert_eq!(view.start, start, "{mode:?} transition step {step}");
                let tracks = if mode == Mode::Linear { 1 } else { tracks };
                let cells = if mode == Mode::Linear && step == 4 {
                    380 // 400-base span minus the 20-base leading margin
                } else {
                    bases_per_track * tracks
                };
                assert_eq!(
                    residue_rects(&frame, cell_width, 20.0, f32::NEG_INFINITY),
                    cells,
                    "exact sequence cells: {mode:?} step {step}"
                );
                if step < 3 {
                    assert_residue_glyphs(
                        &frame,
                        if step == 2 { 17.0 } else { 16.0 },
                        f32::NEG_INFINITY,
                        &b"ACGT"
                            .iter()
                            .copied()
                            .cycle()
                            .take(cells)
                            .collect::<Vec<_>>(),
                    );
                }
                assert_eq!(view.hidden, 0);
                assert!(frame.len() <= 600 * tracks, "{mode:?} step {step}");
            }
        }
    }
}
