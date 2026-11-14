//! Independent retention regression probes: no production implementation changes.
use std::process::Command;

fn retained(input: &str, method: &str) -> (tempfile::TempDir, std::process::Output) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("input.fa"), input).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_basecraft-distance"))
        .current_dir(dir.path())
        .args([
            "--input",
            "input.fa",
            "--model",
            "p",
            "--method",
            method,
            "--threads",
            "1",
            "--tree-threads",
            "1",
            "--tree",
            "tree.nwk",
            "--bootstrap",
            "4",
            "--seed",
            "71",
            "--bootstrap-trees",
            "trees.nex",
        ])
        .output()
        .unwrap();
    (dir, output)
}

#[test]
fn retained_quoted_taxa_and_root_markers_round_trip() {
    let names = ["a'one", "b[comment],(:)=", "c_é"];
    let input = format!(
        ">{}\nAAAAAC\n>{}\nAAAAAT\n>{}\nACAAAC\n",
        names[0], names[1], names[2]
    );
    for (method, marker) in [("sorted", "[&U]"), ("upgma", "[&R]")] {
        let (dir, output) = retained(&input, method);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = std::fs::read_to_string(dir.path().join("trees.nex")).unwrap();
        assert_eq!(text.matches(marker).count(), 4);
        let parsed = basecraft_io::newick::parse_trees(&text).unwrap();
        assert_eq!(parsed.len(), 4);
        for (index, tree) in parsed.iter().enumerate() {
            assert_eq!(tree.name, format!("replicate_{}", index + 1));
            let mut tips: Vec<_> = tree
                .nodes
                .iter()
                .filter(|n| n.children.is_empty())
                .map(|n| n.name.as_str())
                .collect();
            tips.sort_unstable();
            assert_eq!(tips, names);
        }
    }
}

#[test]
fn retained_taxon_containing_end_token_reopens() {
    for method in ["sorted", "upgma"] {
        let (dir, output) = retained(">weekend;sample\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n", method);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = std::fs::read_to_string(dir.path().join("trees.nex")).unwrap();
        let trees = basecraft_io::newick::parse_trees(&text).expect(
            "a successfully retained collection must reopen, including quoted end; in taxa",
        );
        assert_eq!(trees.len(), 4);
        assert!(
            trees[0]
                .nodes
                .iter()
                .any(|node| node.name == "weekend;sample")
        );
    }
}

#[test]
fn late_native_primary_failure_reports_published_collection() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("input.fa"),
        ">a\nAAAAAC\n>b\nAAAAAT\n>c\nACAAAC\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("primary.nwk")).unwrap();
    let collection = dir.path().join("trees.nex");
    let output = Command::new(env!("CARGO_BIN_EXE_basecraft-distance"))
        .current_dir(dir.path())
        .args([
            "--input",
            "input.fa",
            "--model",
            "p",
            "--threads",
            "1",
            "--tree",
            "primary.nwk",
            "--bootstrap",
            "4",
            "--bootstrap-trees",
        ])
        .arg(&collection)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(collection.is_file());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains(collection.to_str().unwrap()),
        "published collection is absent from the failure diagnostic: {diagnostic}"
    );
}
