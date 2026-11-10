//! Deterministic frame-output budgets, not timing or allocation benchmarks.
//! Run with ordinary `cargo test -p basecraft-viewmodel --test budgets`.
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
