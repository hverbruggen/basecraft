//! Headless, warm-cache Session timings. Orchestrated by benches/run.py.
use basecraft_ops::{
    Session,
    files::{self, Kind},
};
use basecraft_platform::paths::AppPaths;
use basecraft_protocol::Op;
use serde_json::json;
use std::error::Error;
use std::path::Path;
use std::time::Instant;

#[path = "interaction/frame_cases.rs"]
mod frame_cases;
#[path = "interaction/tree_cases.rs"]
mod tree_cases;

#[path = "interaction/fixtures.rs"]
mod fixtures;
#[cfg(feature = "test-heap")]
#[path = "interaction/memory_cases.rs"]
mod memory_cases;
#[cfg(feature = "test-heap")]
#[global_allocator]
static ALLOCATOR: basecraft_io::heap::Allocator = basecraft_io::heap::Allocator;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const ROWS: usize = 32;
const WIDTH: usize = 4096;
const EDITS: usize = 1000;

fn verify(session: &Session, id: basecraft_ops::DocId, edited: bool) -> Result<()> {
    if !session.is_alignment(id) {
        return Err("fixture did not open as an alignment".into());
    }
    let doc = session.alignment(id);
    if doc.row_count() != ROWS || doc.width() != WIDTH as u64 {
        return Err("alignment dimensions changed".into());
    }
    for row in 0..ROWS {
        let expected: Vec<u8> = (0..WIDTH)
            .map(|col| {
                if edited && row == 0 && col < EDITS {
                    b'N'
                } else {
                    b"ACGT"[(row + col) % 4]
                }
            })
            .collect();
        if doc.row(row).name != format!("row{row:03}") || doc.read(row, 0, WIDTH as u64) != expected
        {
            return Err("alignment payload verification failed".into());
        }
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("expected SCENARIO WORK_DIRECTORY (use benches/run.py)".into());
    }
    let scenario = &args[0];
    if scenario.starts_with("heap-") {
        #[cfg(feature = "test-heap")]
        {
            println!("{}", memory_cases::measure(scenario, Path::new(&args[1]))?);
            return Ok(());
        }
        #[cfg(not(feature = "test-heap"))]
        return Err("heap scenarios require test-heap feature".into());
    }
    if cfg!(feature = "test-heap") {
        return Err("instrumented binary only supports heap scenarios; rebuild without test-heap for timings".into());
    }
    if scenario.starts_with("tree-") {
        return tree_cases::run(scenario, Path::new(&args[1]));
    }
    if scenario.starts_with("frame-") {
        return frame_cases::run(scenario, Path::new(&args[1]));
    }
    let bytes = match scenario.as_str() {
        "open-fasta-100mb" => Some(100_000_000),
        "open-fasta-1gb" => Some(1_000_000_000),
        "open-alignment" | "edit-alignment" | "undo-alignment" | "save-alignment" => None,
        _ => return Err("unknown scenario".into()),
    };
    let root = Path::new(&args[1]);
    let paths = AppPaths::discover()?;
    if paths.config != root.join("profile/config") || !root.is_absolute() {
        return Err("runner must supply an isolated absolute profile".into());
    }
    let file = root.join(if bytes.is_some() {
        "large.fasta"
    } else {
        "alignment.afa"
    });
    fixtures::fixture(&file, bytes)?;
    let fixture_bytes = file.metadata()?.len();
    // Explicit untimed warmup, including index/cache population; no cold-cache claim.
    let mut warm = Session::recoverable_with_paths(root, &paths)?;
    warm.open(&file)?;
    drop(warm);
    let mut session = Session::recoverable_with_paths(root, &paths)?;
    let start = Instant::now();
    let opened = session.open(&file);
    let open_ns = start.elapsed().as_nanos();
    let id = opened?;
    let mut duration_ns = open_ns;
    let mut operations = 1;
    if let Some(bytes) = bytes {
        if files::kind(&file)? != Kind::Sequence {
            return Err("large fixture did not open as a sequence".into());
        }
        let doc = session.doc(id);
        let payload_bytes = bytes - 7;
        let expected_len = payload_bytes - payload_bytes / 81;
        if doc.len() != expected_len {
            return Err("large FASTA length verification failed".into());
        }
        // Verify every residue, in bounded buffers, outside the timing interval.
        for offset in (0..expected_len).step_by(65536) {
            let actual = doc.read(offset, (offset + 65536).min(expected_len));
            if actual
                .iter()
                .enumerate()
                .any(|(i, b)| *b != b"ACGT"[((offset + i as u64) % 4) as usize])
            {
                return Err("large FASTA payload verification failed".into());
            }
        }
    } else {
        verify(&session, id, false)?;
        if scenario != "open-alignment" {
            let ops: Vec<Op> = (0..EDITS)
                .map(|col| Op::AlnReplace {
                    row: 0,
                    start: col as u64,
                    end: col as u64 + 1,
                    residues: "N".into(),
                })
                .collect();
            let mut edit_ns = 0;
            for op in ops {
                let start = Instant::now();
                let result = session.apply(id, op);
                edit_ns += start.elapsed().as_nanos();
                result?;
            }
            verify(&session, id, true)?;
            match scenario.as_str() {
                "edit-alignment" => {
                    duration_ns = edit_ns;
                    operations = EDITS;
                }
                "undo-alignment" => {
                    duration_ns = 0;
                    operations = EDITS;
                    for _ in 0..EDITS {
                        let start = Instant::now();
                        let result = session.undo(id);
                        duration_ns += start.elapsed().as_nanos();
                        result?;
                    }
                    verify(&session, id, false)?;
                }
                "save-alignment" => {
                    let start = Instant::now();
                    let result = session.save(id, None);
                    duration_ns = start.elapsed().as_nanos();
                    result?;
                    let mut reopened = Session::recoverable_with_paths(root, &paths)?;
                    let reopened_id = reopened.open(&file)?;
                    verify(&reopened, reopened_id, true)?;
                }
                _ => return Err("unknown alignment scenario".into()),
            }
        }
    }
    println!(
        "{}",
        json!({
            "scenario": scenario, "duration_ns": duration_ns, "operations": operations,
            "schema_version": 2, "verified": true, "fixture_bytes": fixture_bytes,
            "output_bytes": file.metadata()?.len(),
            "rows": if bytes.is_some() { 1 } else { ROWS },
            "columns": if bytes.is_some() { None } else { Some(WIDTH) },
            "edits": if scenario == "open-alignment" || bytes.is_some() { 0 } else { EDITS },
            "source_commit": option_env!("BASECRAFT_BENCH_REVISION").unwrap_or("unknown"),
            "debug_assertions": cfg!(debug_assertions)
        })
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("benchmark failed/refused: {error}");
        std::process::exit(1);
    }
}
