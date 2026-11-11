#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_ops::{analysis_store::ArtifactRun, input_evidence as evidence};
use basecraft_protocol::input_records::EvidenceState;

#[test]
fn submitted_input_ancestry_and_source_identity() {
    crate::isolated_profile::isolate!();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.fa");
    std::fs::write(&source, b">a\nACGT\n").unwrap();
    let hash = basecraft_io::hash_file(&source).unwrap();
    let original = evidence::reference_input_with_dependencies(
        &root.path().join("result"),
        &source,
        &hash,
        &[],
        "source",
    )
    .unwrap();
    let generated = ArtifactRun::create(&root.path().join("derived"), "generated").unwrap();
    evidence::link_generated_input(
        &generated,
        &[original.directory().into()],
        b">a\nAC\n",
        "canonical-fasta-v1",
        "derived",
    )
    .unwrap();
    let input = evidence::read_input(generated.directory()).unwrap();
    assert_eq!(input.state, EvidenceState::Generated);
    assert_eq!(input.upstream.len(), 1);
    assert_eq!(input.upstream[0].id, original.id());
    assert_eq!(input.submitted_hash, basecraft_io::hash_bytes(b">a\nAC\n"));
    assert_eq!(
        evidence::verify_ancestors(generated.directory()).unwrap(),
        Vec::<String>::new()
    );
    std::fs::write(&source, b">a\nAAAA\n").unwrap();
    assert_ne!(
        evidence::verify_ancestors(generated.directory()).unwrap(),
        Vec::<String>::new()
    );
}
