//! The run a species analysis asks for, as the window, the CLI and MCP all
//! build it from the same request.

use basecraft_ops::Session;
use basecraft_ops::species_analysis::{Method, Request, run};
use std::collections::BTreeSet;
use std::path::Path;

const SPART: &str = include_str!("../../../../testdata/fixtures/delimitation/manual.spart");

fn declared(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn request<'a>(method: Method, input: &'a Path, settings: &str) -> Request<'a> {
    Request {
        method,
        input,
        rooted: true,
        settings: serde_json::from_str(settings).unwrap(),
        hypothesis: "",
        shown_tree: Path::new(""),
    }
}

#[test]
fn a_fit_passes_only_declared_settings_and_writes_beside_the_workspace() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("species.spart");
    std::fs::write(&path, SPART).unwrap();
    let tree = dir.path().join("tree.nwk");
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&path).unwrap();

    let asked = request(
        Method::Ptp,
        &tree,
        r#"{"seed": 7, "replicates": "", "colour": "red", "mapping": "m.tsv"}"#,
    );
    let ptp = run(
        &session,
        doc,
        &asked,
        &declared(&["rooted", "seed", "replicates"]),
    )
    .unwrap();
    assert_eq!(ptp.operation, "basecraft.delimitation/ptp@1");
    assert_eq!(ptp.input, tree);
    // An empty setting and undeclared ones are left out; a request's seed wins.
    assert_eq!(ptp.parameters.to_string(), r#"{"rooted":true,"seed":7}"#);
    assert_eq!(ptp.output.parent(), Some(dir.path()));
    assert_eq!(ptp.output.extension().unwrap(), "json");

    let shown = request(Method::DisplayUpgma, &tree, "{}");
    let display = run(&session, doc, &shown, &declared(&["method", "tree_role"])).unwrap();
    assert_eq!(display.operation, "basecraft.distance/tree@1");
    assert_eq!(display.parameters["method"], "upgma");
    assert_eq!(display.output.extension().unwrap(), "nwk");
}

#[test]
fn refused_requests_say_what_is_missing() {
    crate::isolated_profile::isolate!();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("species.spart");
    std::fs::write(&path, SPART).unwrap();
    let tree = dir.path().join("tree.nwk");
    let mut session = Session::unlogged(dir.path());
    let doc = session.open(&path).unwrap();
    let all = declared(&["groups_data", "deduplicate"]);

    let statistics = request(Method::GroupStatistics, &tree, "{}");
    assert!(matches!(
        run(&session, doc, &statistics, &all),
        Err(basecraft_ops::Error::MissingChoice(text)) if text == "Choose a hypothesis"
    ));
    let deduplicated = request(Method::GmycSingle, &tree, r#"{"deduplicate": true}"#);
    assert!(matches!(
        run(&session, doc, &deduplicated, &all),
        Err(basecraft_ops::Error::Unsupported(_))
    ));
    let alignment = request(Method::Asap, &tree, r#"{"deduplicate": true}"#);
    let asap = run(&session, doc, &alignment, &declared(&["unique_haplotypes"])).unwrap();
    assert_eq!(asap.parameters.to_string(), r#"{"unique_haplotypes":true}"#);
}
