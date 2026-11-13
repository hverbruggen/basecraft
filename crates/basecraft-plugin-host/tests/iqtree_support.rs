//! IQ-TREE support semantics survive both registered publication forms: a
//! whole-collection export and a single exported tree file. A process-local
//! fake executable supplies known Newick bytes without an ML run.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{Registry, RegistryConfig},
};
use basecraft_protocol::{SupportKind, operation::OperationRequest};
use serde_json::json;

fn plugins() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn check_tree(path: &Path, method: &str, multi: bool) {
    let raw = std::fs::read_to_string(path).unwrap();
    if multi {
        assert!(raw.contains("85/72"), "{raw}");
    } else {
        assert!(raw.contains("0.95"), "{raw}");
    }
    let tree = basecraft_ops::files::load_tree(path).unwrap().doc;
    if multi {
        assert_eq!(tree.supports().len(), 2);
        assert_eq!(tree.supports()[0].name, "Support 1");
        assert_eq!(tree.supports()[1].name, "Support 2");
        assert!(
            tree.supports()
                .iter()
                .all(|set| set.kind == SupportKind::Unknown)
        );
        // The companion states only what the branch lengths measure.
        assert_eq!(
            tree.branch_lengths().map(|fact| &fact.value),
            Some(&basecraft_protocol::facts::BranchLengths::Substitutions)
        );
    } else {
        assert_eq!(tree.supports().len(), 1);
        let support = &tree.supports()[0];
        assert_eq!(support.name, method);
        assert_eq!(support.kind, SupportKind::Bootstrap);
        assert!(
            !support.proportions,
            "IQ-TREE bootstrap is percentage scale"
        );
        assert!(basecraft_ops::files::tree_sidecar(path).is_file());
    }
}

fn run_cases(root: &Path) {
    let input = root.join("input.afa");
    std::fs::write(
        &input,
        b">a\nACGTACGT\n>b\nACGTACGA\n>c\nACGTTCGT\n>d\nTCGTACGT\n",
    )
    .unwrap();
    let registry = Registry::load(&RegistryConfig {
        bundled: plugins(),
        packages: root.join("profile/plugins"),
        index: root.join("profile/index.json"),
    })
    .unwrap();
    assert!(registry.operations.contains_key("basecraft.iqtree/tree@1"));

    for (support, seed, label, multi) in [
        ("ufboot", 1, "Ultrafast bootstrap (UFBoot)", false),
        ("bootstrap", 1, "Standard bootstrap", false),
        ("ufboot", 42, "", true),
    ] {
        let suffix = if multi { "multi" } else { support };
        let destination = format!("registered-{suffix}");
        let request: OperationRequest = serde_json::from_value(json!({
            "operation":"basecraft.iqtree/tree@1",
            "inputs":{"alignment":[{"kind":"file","path":"input.afa"}]},
            "parameters":{"model":"JC","threads":1,"support":support,"replicates":if support == "ufboot" { 1000 } else { 100 },"seed":seed},
            "destinations":{"artifacts":destination,"artifacts/tree.nwk":format!("file-{suffix}.nwk")}
        }))
        .unwrap();
        let mut jobs = JobManager::default();
        let accepted = jobs
            .submit(
                &registry,
                request,
                SessionPolicy::new(root, true).unwrap(),
                "IQ-TREE support regression",
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let completed = loop {
            let current = jobs.get(&accepted.run_id).unwrap();
            if current.state.terminal() {
                break current;
            }
            assert!(Instant::now() < deadline, "registered job timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            completed.state,
            basecraft_protocol::operation::RuntimeState::Completed,
            "{completed:?}"
        );
        let exported = root.join(&destination).join("tree.nwk");
        check_tree(&exported, label, multi);
        check_tree(&root.join(format!("file-{suffix}.nwk")), label, multi);
        let retained = Path::new(completed.evidence.as_ref().unwrap()).join("artifacts/tree.nwk");
        check_tree(&retained, label, multi);
        let moved = root.join(format!("copied-{suffix}"));
        std::fs::create_dir(&moved).unwrap();
        let moved_tree = moved.join("tree.nwk");
        std::fs::copy(&exported, &moved_tree).unwrap();
        std::fs::copy(
            exported.with_file_name("tree.nwk.basecraft.json"),
            moved.join("tree.nwk.basecraft.json"),
        )
        .unwrap();
        check_tree(&moved_tree, label, multi);
        assert_eq!(
            basecraft_ops::records::verify(&retained, None).unwrap(),
            Vec::<String>::new()
        );
    }
}

#[test]
fn known_support_survives_both_publication_forms() {
    crate::isolated_profile::isolate!();
    if std::env::var_os("BASECRAFT_IQTREE_SUPPORT_CHILD").is_some() {
        run_cases(Path::new(
            &std::env::var("BASECRAFT_IQTREE_SUPPORT_ROOT").unwrap(),
        ));
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let executable = temporary.path().join("iqtree-double");
    std::fs::write(
        &executable,
        r"#!/usr/bin/env python3
import pathlib, sys
if '--version' in sys.argv:
    print('IQ-TREE test double')
    sys.exit(0)
prefix = pathlib.Path(sys.argv[sys.argv.index('-pre') + 1])
seed = int(sys.argv[sys.argv.index('-seed') + 1])
label = '85/72' if seed == 42 else '0.95'
prefix.with_suffix('.treefile').write_text(f'((t0:1,t1:1){label}:1,(t2:1,t3:1)99:1);\n')
",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "known_support_survives_both_publication_forms",
            "--nocapture",
        ])
        .env("BASECRAFT_IQTREE_SUPPORT_CHILD", "1")
        .env("BASECRAFT_IQTREE_SUPPORT_ROOT", temporary.path())
        .env("BASECRAFT_IQTREE", &executable)
        .env("BASECRAFT_PROFILE_DIR", temporary.path().join("profile"))
        .status()
        .unwrap();
    assert!(status.success());
}
