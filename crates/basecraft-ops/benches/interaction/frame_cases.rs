//! Test-only clients of the public, Qt-free viewmodel API. No layout copies.
use super::Result;
use basecraft_docs::{AlignmentDoc, SequenceDoc, alignment::AlnRow};
use basecraft_protocol::{Feature, Location, Qualifier, Strand};
use basecraft_viewmodel::{
    Frame,
    alignment::AlignmentView,
    frame::FLOATS_PER_INSTANCE,
    genome::{GenomeView, Mode},
    theme::Theme,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{hint::black_box, path::Path, sync::Arc, time::Instant};

const WIDTH: f32 = 3840.0;
const HEIGHT: f32 = 2160.0;
const ZOOM: f32 = 8.0;
const FRAMES: usize = 200;

#[derive(Serialize, Deserialize)]
struct Input {
    rows: Vec<(String, Vec<u8>)>,
    features: Vec<Feature>,
}

enum Document {
    Alignment(AlignmentDoc),
    Genome(SequenceDoc, Mode),
}

enum View {
    Alignment(AlignmentView),
    Genome(GenomeView),
}

impl Document {
    fn view(&self) -> View {
        match self {
            Self::Alignment(doc) => {
                let mut view = AlignmentView::new(WIDTH, HEIGHT);
                view.px_per_col = f64::from(ZOOM);
                view.sync(doc);
                View::Alignment(view)
            }
            Self::Genome(_, mode) => {
                let mut view = GenomeView::new(WIDTH, HEIGHT);
                view.mode = *mode;
                view.bp_per_px = 1.0 / f64::from(ZOOM);
                View::Genome(view)
            }
        }
    }

    fn build(&self, view: &mut View, theme: &Theme, frame: &mut Frame) {
        match (self, view) {
            (Self::Alignment(doc), View::Alignment(view)) => view.build(doc, theme, frame),
            (Self::Genome(doc, _), View::Genome(view)) => view.build(doc, theme, frame),
            _ => unreachable!("bench document/view pairing"),
        }
        black_box(frame);
    }

    fn scroll(&self, view: &mut View, fraction: f64) {
        match (self, view) {
            (Self::Alignment(_), View::Alignment(view)) => {
                view.hscroll_to(fraction);
                view.vscroll_to(fraction);
            }
            (Self::Genome(doc, _), View::Genome(view)) => view.scroll_to(fraction, doc.len()),
            _ => unreachable!("bench document/view pairing"),
        }
    }
}

fn verify(frame: &Frame, view: &View) -> Result<()> {
    let quads = frame.floats().as_chunks::<FLOATS_PER_INSTANCE>().0;
    let glyphs = quads.iter().filter(|q| q[14] > 0.0).count();
    let residue_height = match view {
        View::Alignment(view) => view.row_height(),
        View::Genome(_) => 20.0,
    };
    let residues = quads
        .iter()
        .filter(|q| {
            q[14] == 0.0
                && (q[2] - q[0] - ZOOM).abs() < 0.01
                && (q[5] - q[1] - residue_height).abs() < 0.01
        })
        .count();
    if glyphs < 400 || residues < 400 || frame.floats().iter().any(|v| !v.is_finite()) {
        return Err("frame lacks finite labelled residue geometry".into());
    }
    if let View::Genome(view) = view
        && view.hidden != 0
    {
        return Err("fixture annotations unexpectedly hidden".into());
    }
    Ok(())
}

pub fn run(scenario: &str, root: &Path) -> Result<()> {
    let name = scenario
        .strip_prefix("frame-")
        .ok_or("invalid frame scenario")?;
    let (phase, case) = name.split_once('-').ok_or("invalid frame phase")?;
    let (rows, columns, mode) = match case {
        "alignment" => (256, 4096, None),
        "alignment-hidden" => (1024, 4096, None),
        "genome-linear" => (1, 100_000, Some(Mode::Linear)),
        "genome-wrapped" => (1, 100_000, Some(Mode::Wrapped)),
        "genome-wrapped-hidden" => (1, 1_000_000, Some(Mode::Wrapped)),
        _ => return Err("unknown frame fixture".into()),
    };
    let frames = match phase {
        "first" | "warm" => 1,
        "scroll" => FRAMES,
        _ => return Err("unknown frame phase".into()),
    };
    let features = if mode.is_some() {
        (0..columns as u64)
            .step_by(100)
            .flat_map(|base| {
                [(10, 50, Strand::Forward), (60, 90, Strand::Reverse)].map(
                    |(start, end, strand)| Feature {
                        kind: "gene".into(),
                        location: Location::simple(base + start, base + end, strand),
                        qualifiers: vec![Qualifier::new("gene", "marker")],
                    },
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    let input = Input {
        rows: (0..rows)
            .map(|row| {
                (
                    format!("row{row:05}"),
                    (0..columns).map(|col| b"ACGT"[col % 4]).collect(),
                )
            })
            .collect(),
        features,
    };
    // Actual generated input bytes, captured before construction/timing. JSON is
    // only an untimed synthetic fixture, never an application persistence format.
    let path = root.join("frame-input.json");
    serde_json::to_writer(
        std::io::BufWriter::new(std::fs::File::create(&path)?),
        &input,
    )?;
    drop(input);
    let fixture_bytes = path.metadata()?.len();
    let input: Input =
        serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(&path)?))?;
    if input.rows.len() != rows
        || input.rows.iter().enumerate().any(|(i, (name, seq))| {
            name != &format!("row{i:05}")
                || seq.len() != columns
                || seq.iter().enumerate().any(|(j, b)| *b != b"ACGT"[j % 4])
        })
        || input.features.len() != if mode.is_some() { columns / 50 } else { 0 }
    {
        return Err("frame fixture payload mismatch".into());
    }
    let feature_count = input.features.len();
    let doc = if let Some(mode) = mode {
        Document::Genome(
            SequenceDoc::new(
                "frame-timing",
                Arc::new(input.rows[0].1.clone()),
                input.features,
                false,
            ),
            mode,
        )
    } else {
        Document::Alignment(AlignmentDoc::new(
            "frame-timing",
            input
                .rows
                .into_iter()
                .map(|(name, seq)| AlnRow::new(name, Arc::new(seq)))
                .collect(),
        ))
    };
    let theme = Theme::default();
    let (mut view, mut frame, mut duration_ns) = if phase == "first" {
        // Includes fresh view/sync, empty Frame and first cache/layout construction.
        // Documents, interval indexes, theme and input decoding already exist.
        let start = Instant::now();
        let mut view = doc.view();
        let mut frame = Frame::new();
        doc.build(&mut view, &theme, &mut frame);
        let elapsed = start.elapsed().as_nanos();
        (view, frame, elapsed)
    } else {
        let mut view = doc.view();
        let mut frame = Frame::new();
        doc.build(&mut view, &theme, &mut frame);
        verify(&frame, &view)?;
        (view, frame, 0)
    };
    let mut output_counts = Vec::with_capacity(frames);
    for index in 0..frames {
        let fraction = f64::from(u32::try_from(index)?) / f64::from(u32::try_from(FRAMES - 1)?);
        if phase != "first" {
            let start = Instant::now();
            if phase == "scroll" {
                doc.scroll(&mut view, fraction);
            }
            doc.build(&mut view, &theme, &mut frame);
            duration_ns += start.elapsed().as_nanos();
        }
        verify(&frame, &view)?;
        output_counts.push(frame.len());
        // Untimed independent fresh-view replay at the same position checks the
        // entire output, and detects stale caches or accumulating frame buffers.
        let mut reference_view = doc.view();
        let mut reference = Frame::new();
        doc.build(&mut reference_view, &theme, &mut reference);
        if phase == "scroll" {
            doc.scroll(&mut reference_view, fraction);
            doc.build(&mut reference_view, &theme, &mut reference);
        }
        if frame.floats() != reference.floats() {
            return Err("frame differs from untimed fresh-view replay".into());
        }
    }
    println!(
        "{}",
        json!({
            "schema_version": 2, "scenario": scenario, "verified": true,
            "duration_ns": duration_ns, "operations": frames, "fixture_bytes": fixture_bytes,
            "output_bytes": path.metadata()?.len(), "rows": rows, "columns": columns, "edits": 0,
            "work": {"protocol": "headless-frame-v1", "phase": phase, "width": WIDTH, "height": HEIGHT,
                "zoom_px": ZOOM, "theme": "Theme::default", "mode": case, "features": feature_count,
                "frames": frames, "scroll_path": if phase == "scroll" { "i/199; full clamped range; alignment both axes" } else { "origin" },
                "fixture": "JSON rows rowNNNNN, repeated ACGT; genome paired gene marker every 100bp"},
            "output_counts": output_counts,
            "source_commit": option_env!("BASECRAFT_BENCH_REVISION").unwrap_or("unknown"),
            "debug_assertions": cfg!(debug_assertions)
        })
    );
    Ok(())
}
