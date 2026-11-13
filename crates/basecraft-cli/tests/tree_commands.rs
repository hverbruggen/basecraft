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
