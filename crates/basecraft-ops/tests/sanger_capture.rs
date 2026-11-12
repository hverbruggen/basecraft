#![cfg(feature = "test-counters")]
use basecraft_ops::{
    records::Mutation,
    sanger_review::CapturedInput,
    sanger_review::{SangerRun, instrument_review},
};
#[path = "support/isolated_profile.rs"]
mod isolated_profile;

fn trace(root: &std::path::Path) -> std::path::PathBuf {
    let path = root.join("trace.ab1");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
        &path,
    )
    .unwrap();
    path
}
fn one_read(path: &std::path::Path) {
    let counts = basecraft_io::counters::snapshot();
    assert_eq!(
        counts.reads[&path.canonicalize().unwrap()].full_reads,
        1,
        "{counts:?}"
    );
}
#[test]
fn instrument_rendering_and_submission_share_one_trace_capture() {
    let Some(root) =
        isolated_profile::enter("instrument_rendering_and_submission_share_one_trace_capture")
    else {
        return;
    };
    let path = trace(&root);
    basecraft_io::counters::reset();
    let review = instrument_review(&path).unwrap();
    let mut session = basecraft_ops::Session::new(&root);
    let id = session.create_sanger_review(&review).unwrap();
    let review = session.sanger_review(id).unwrap();
    let capture = review.captured_inputs().unwrap().remove(0);
    assert_eq!(
        capture.sha256(),
        basecraft_io::sha256_bytes(capture.bytes())
    );
    assert_eq!(capture.hash(), basecraft_io::hash_bytes(capture.bytes()));
    assert_eq!(
        review.working_draft().unwrap().inputs[0].sha256,
        capture.sha256()
    );
    let rendered = basecraft_io::abif::parse(capture.bytes()).unwrap();
    assert_eq!(
        rendered.display_calls().bases,
        review.draft.consensus.as_bytes()
    );
    let frozen = root.join("caller.ab1");
    capture.write(&frozen).unwrap();
    Mutation::begin()
        .unwrap()
        .reference_captured(
            &root.join("analysis"),
            &capture,
            &[],
            "fixture caller",
            None,
        )
        .unwrap();
    one_read(&path);
    assert_eq!(std::fs::read(&frozen).unwrap(), capture.bytes());
    let before = capture.bytes().to_vec();
    std::fs::write(&path, b"replaced during review").unwrap();
    assert_eq!(capture.bytes(), before);
    assert!(capture.check_current().is_err());
    assert!(review.working_draft().is_err());
}
#[test]
fn explicit_sanger_run_registers_both_identities_in_one_read() {
    let Some(root) =
        isolated_profile::enter("explicit_sanger_run_registers_both_identities_in_one_read")
    else {
        return;
    };
    let path = trace(&root);
    basecraft_io::counters::reset();
    let run = SangerRun::prepare(
        &root.join("analysis"),
        "consensus",
        std::slice::from_ref(&path),
        None,
    )
    .unwrap();
    one_read(&path);
    assert_eq!(
        run.captured_inputs()[0].sha256(),
        CapturedInput::open(&path).unwrap().sha256()
    );
}

#[test]
fn large_alignment_context_still_registers_as_streamed_evidence() {
    use std::io::{Read, Write};
    let Some(root) =
        isolated_profile::enter("large_alignment_context_still_registers_as_streamed_evidence")
    else {
        return;
    };
    let path = root.join("reference.afa");
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(b">reference\n").unwrap();
    std::io::copy(
        &mut std::io::repeat(b'A').take(129 * 1024 * 1024),
        &mut file,
    )
    .unwrap();
    file.write_all(b"\n").unwrap();
    drop(file);
    let run = SangerRun::prepare(
        &root.join("placement"),
        "placement",
        std::slice::from_ref(&path),
        None,
    )
    .unwrap();
    let request = run.artifact("request.json").unwrap();
    assert!(request.is_file());
    let input = basecraft_ops::records::read_input(run.directory()).unwrap();
    assert_ne!(input.submitted_hash, "");
}
