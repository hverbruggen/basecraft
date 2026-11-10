//! Real evidence ledgers and original FASTA bytes, served over the actual CLI/MCP.
use basecraft_ops::input_evidence;
use serde_json::json;
use std::{path::Path, process::Command};

#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;

#[test]
fn headless_save_conformance() {
    let Some(root) = isolated_profile::enter("headless_save_conformance") else {
        return;
    };
    let data = root.join("data");
    std::fs::create_dir(&data).unwrap();
    let original = b">gene\nACGT\n";
    let mut known = serde_json::Map::new();
    for name in [
        "known",
        "preservation",
        "writefail",
        "cli-known",
        "cli-preservation",
    ] {
        let source = data.join(format!("{name}.fasta"));
        std::fs::write(&source, original).unwrap();
        let folder = root.join(format!("analysis-{name}"));
        std::fs::create_dir(&folder).unwrap();
        let run = input_evidence::reference_input_with_dependencies(
            &folder.join("result.txt"),
            &source,
            &basecraft_io::hash_file(&source).unwrap(),
            &[],
            "adapter save regression",
        )
        .unwrap();
        known.insert(name.into(), json!(run.directory()));
    }
    let unrelated = root.join("unrelated.fasta");
    std::fs::write(&unrelated, original).unwrap();
    let damaged = input_evidence::reference_input_with_dependencies(
        &root.join("distant-result.txt"),
        &unrelated,
        &basecraft_io::hash_file(&unrelated).unwrap(),
        &[],
        "incomplete adapter evidence",
    )
    .unwrap();
    let ledger = damaged.artifact("input.json").unwrap();
    let good = std::fs::read(&ledger).unwrap();
    std::fs::write(&ledger, b"damaged ledger").unwrap();
    let config = root.join("fixture.json");
    std::fs::write(
        &config,
        json!({"known":known,"ledger":ledger,"good":String::from_utf8(good).unwrap()}).to_string(),
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/warned_save_conformance.py"))
        .arg(&data)
        .arg(&config)
        .env("BASECRAFT", env!("CARGO_BIN_EXE_basecraft"))
        .env(
            "PYTHONPATH",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn review_save_path_permission_code() {
    let Some(root) = isolated_profile::enter("review_save_path_permission_code") else {
        return;
    };
    let output = Command::new("python3")
        .arg("-c")
        .arg(r"
import json, os, pathlib, sys
from basecraft.session import Session, OperationError
root = pathlib.Path(sys.argv[1])
assert os.geteuid() != 0, 'probe requires ordinary user'
project = root / 'project'
project.mkdir()
original = b'>gene\nACGT\n'
source = project / 'gene.fasta'
source.write_bytes(original)
occupied = project / 'occupied.fasta'
occupied.write_bytes(original)
outside = root / 'outside.fasta'
outside.write_bytes(original)
# Canonical confinement must still reject a path through an in-project symlink.
(project / 'escape').symlink_to(root, target_is_directory=True)
blocked = project / 'blocked'
(blocked / 'inner').mkdir(parents=True)

with Session(project, writes=True) as session:
    doc = session.open('gene.fasta')['document']
    session.apply(doc, 0, {'op':'seq.replace', 'params':{'start':1,'end':2,'residues':'T'}})

    def refuses(route, action, expected):
        try:
            action()
        except OperationError as error:
            value = error.outcome
            assert error.code == expected, value
            assert value['schema'] == 1 and value['state'] == 'failed', value
            assert value['operation'] == route, value
            assert value['errors'][0]['operation'] == route, value
            assert value['errors'][0]['code'] == expected, value
            assert source.read_bytes() == original
            assert occupied.read_bytes() == original
            assert outside.read_bytes() == original
            return value
        raise AssertionError('unexpected successful path resolution: ' + route)

    blocked.chmod(0)
    outcomes = {}
    try:
        for route in ['save_check', 'save']:
            outcomes[route] = refuses(route, lambda: session.call(route, {
                'document':doc, 'path':'blocked/inner/new.fasta'}), 'permission_denied')
        # Both SDK convenience methods must expose the identical typed outcome.
        refuses('save_check', lambda: session.save_check(doc, 'blocked/inner/new.fasta'), 'permission_denied')
        refuses('save', lambda: session.save(doc, 'blocked/inner/new.fasta'), 'permission_denied')
        # Valid one-attempt consent cannot turn path resolution into a write bypass.
        check = session.save_check(doc)
        choice = {'token':check['token'], 'known_policy':'cancel', 'acknowledge_incomplete':True}
        refuses('save', lambda: session.save(doc, 'blocked/inner/new.fasta', decision=choice), 'permission_denied')
    finally:
        blocked.chmod(0o755)
    assert not (blocked / 'inner' / 'new.fasta').exists()

    for route in ['save_check', 'save']:
        for path in [42, None, {}, '../outside.fasta', 'escape/outside.fasta']:
            refuses(route, lambda: session.call(route, {'document':doc, 'path':path}), 'invalid_arguments')
        # Missing destination ancestry is an IO failure, not malformed JSON.
        refuses(route, lambda: session.call(route, {'document':doc, 'path':'missing/inner/new.fasta'}), 'io_error')
    assert not (project / 'missing').exists()

    check = session.save_check(doc, 'occupied.fasta')
    choice = {'token':check['token'], 'known_policy':'cancel', 'acknowledge_incomplete':True}
    refuses('save', lambda: session.save(doc, 'occupied.fasta', decision=choice), 'invalid_arguments')
    assert occupied.read_bytes() == original
    assert source.read_bytes() == original and session.state(doc)['can_undo']
    # Failed path checks leave edits intact; the permitted new destination works.
    check = session.save_check(doc, 'allowed.fasta')
    choice = {'token':check['token'], 'known_policy':'cancel', 'acknowledge_incomplete':False}
    saved = session.save(doc, 'allowed.fasta', decision=choice)
    assert saved['warnings'] == []
    assert (project / 'allowed.fasta').read_bytes() == b'>gene\nATGT\n'
    assert source.read_bytes() == original
    assert outside.read_bytes() == original
    print(json.dumps(outcomes), flush=True)
")
        .arg(&root)
        .env("BASECRAFT", env!("CARGO_BIN_EXE_basecraft"))
        .env("PYTHONPATH", Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python/src"))
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
