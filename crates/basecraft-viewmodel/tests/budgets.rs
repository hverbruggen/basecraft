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
        .chunks_exact(FLOATS_PER_INSTANCE)
        .filter(|quad| quad[14] > 0.0)
        .count()
}

#[test]
fn alignment_frames_depend_on_visible_cells() {
    let theme = Theme::default();
    // Vary hidden rows and columns separately as well as together.
    for (rows, cols) in [(128, 4096), (512, 4096), (128, 16384), (512, 16384)] {
        let doc = alignment(rows, cols);
        // Letters, colour runs, then subpixel overview sampling.
        for zoom in [11.0, 3.0, 0.5] {
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
                assert!(!frame.is_empty());
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
    for len in [4000, 20000, 100000] {
        let doc = genome(len);
        for mode in [Mode::Linear, Mode::Wrapped] {
            for px_per_base in [8.0, 2.0] {
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
                    assert!(!frame.is_empty());
                    assert_eq!(view.hidden, 0, "fixture annotations must fit");
                    let first = frame.floats().to_vec();
                    view.build(&doc, &theme, &mut frame);
                    assert_eq!(frame.floats(), first);
                }
            }
        }
    }
}
