//! Tree edits by name (D1): each is the same typed edit `basecraft edit`
//! applies, and a missing choice (outgroup, order) is refused by name.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::path::Path;
use std::process::{Command, Output};

const TREE: &str = "((a:1,b:1):1,(c:1,(d:1,e:1):1):1);\n";

fn basecraft(profile: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_basecraft"))
        .args(args)
        .env("BASECRAFT_PROFILE_DIR", profile)
        .output()
        .unwrap()
}

#[test]
fn tree_commands_make_the_same_edits_as_typed_operations() {
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    for (name, command, operation) in [
        (
            "reroot",
            vec!["reroot", "--outgroup", "c"],
            r#"{"op":"tree.reroot","params":{"clade":["c"]}}"#,
        ),
        (
            "ladderize",
            vec!["ladderize", "--order", "largest-first"],
            r#"{"op":"tree.ladderize","params":{"small_first":false}}"#,
        ),
        (
            "rotate",
            vec!["rotate", "--clade", "d", "--clade", "e"],
            r#"{"op":"tree.rotate","params":{"clade":["d","e"]}}"#,
        ),
    ] {
        let by_name = temp.path().join(format!("{name}-named.nwk"));
        let typed = temp.path().join(format!("{name}-typed.nwk"));
        std::fs::write(&by_name, TREE).unwrap();
        std::fs::write(&typed, TREE).unwrap();
        let mut args = vec!["tree", command[0], by_name.to_str().unwrap()];
        args.extend(&command[1..]);
        let named = basecraft(&profile, &args);
        assert!(
            named.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&named.stderr)
        );
        let edited = basecraft(&profile, &["edit", typed.to_str().unwrap(), operation]);
        assert!(
            edited.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&edited.stderr)
        );
        let result = std::fs::read_to_string(&by_name).unwrap();
        assert_ne!(result, TREE, "{name} changed nothing");
        assert_eq!(result, std::fs::read_to_string(&typed).unwrap(), "{name}");
    }
    // A missing choice is refused by name, and the file is left alone.
    let file = temp.path().join("untouched.nwk");
    std::fs::write(&file, TREE).unwrap();
    for (args, missing) in [
        (vec!["tree", "reroot", file.to_str().unwrap()], "--outgroup"),
        (vec!["tree", "ladderize", file.to_str().unwrap()], "--order"),
    ] {
        let refused = basecraft(&profile, &args);
        assert!(!refused.status.success());
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains(missing),
            "{missing}"
        );
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), TREE);
}

#[test]
fn tips_are_labelled_from_the_projects_specimen_table() {
    use basecraft_protocol::metadata::{ChangeSet, RecordLink};
    crate::isolated_profile::isolate!();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let tree = project.join("tree.nwk");
    std::fs::write(&tree, "(a:1,b:1,c:1);\n").unwrap();
    let path = |p: &Path| p.to_str().unwrap().to_owned();
    let labels = |fields: &str| {
        basecraft(
            &profile,
            &[
                "--project",
                &path(&project),
                "tree",
                "labels",
                &path(&tree),
                "--fields",
                fields,
            ],
        )
    };
    let none = labels("specimen");
    assert!(!none.status.success());
    assert!(String::from_utf8_lossy(&none.stderr).contains("no specimen metadata"));
    let without = basecraft(
        &profile,
        &["tree", "labels", &path(&tree), "--fields", "specimen"],
    );
    assert!(!without.status.success());
    assert!(String::from_utf8_lossy(&without.stderr).contains("needs a project"));

    let mut session = basecraft_ops::Session::new(&project);
    let metadata = basecraft_ops::metadata::open(&mut session, true)
        .unwrap()
        .unwrap();
    let link = |record: &str, specimen: &str| RecordLink {
        document: "tree.nwk".into(),
        record: record.into(),
        specimen: specimen.into(),
    };
    basecraft_ops::metadata::apply_plan(
        &mut session,
        metadata,
        ChangeSet {
            specimens: vec!["S1".into(), "S2".into()],
            links: vec![link("a", "S1"), link("b", "S2")],
            ..ChangeSet::default()
        },
    )
    .unwrap();
    session.save(metadata, None).unwrap();
    drop(session);

    let wrong = labels("colour");
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("not a label field"));
    let done = labels("specimen");
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&done.stdout).unwrap();
    assert_eq!(value["kept_own_name"], 1, "{value}");
    // A tree outside the project is refused: the table links files inside it.
    let outside = temp.path().join("outside.nwk");
    std::fs::write(&outside, "(a:1,b:1,c:1);\n").unwrap();
    let refused = basecraft(
        &profile,
        &[
            "--project",
            &path(&project),
            "tree",
            "labels",
            &path(&outside),
            "--fields",
            "specimen",
        ],
    );
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("outside the project"));
    let mut reader = basecraft_ops::Session::unlogged(&project);
    let opened = reader.open(&tree).unwrap();
    let style = reader.tree(opened).unwrap().style();
    assert_eq!(style.tip_labels["a"], "S1");
    assert_eq!(style.tip_labels["b"], "S2");
}
