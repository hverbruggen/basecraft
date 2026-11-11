//! Runtime grants are explicit, recipient-pinned and absent from durable evidence.
#[path = "../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
use basecraft_plugin_host::{
    jobs::JobManager,
    policy::SessionPolicy,
    registry::{self, Registry, RegistryConfig},
};
use basecraft_protocol::operation::{OperationOutcome, RuntimeState};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const SCRIPT: &str = r"
import json, os, sys
from pathlib import Path
from basecraft.plugin import Plugin
plugin = Plugin('org.example.credentials', '1.0.0')
print('initialize=' + os.environ.get('BASECRAFT_ZENODO_TOKEN', 'absent'), flush=True)
def inspect(snapshot, parameters, context):
    token = os.environ.get('BASECRAFT_ZENODO_TOKEN', '')
    assert os.environ.get('BASECRAFT_UNRELATED_SETTING') == 'preserved'
    print('stdout=' + token, flush=True)
    print('stderr=' + token, file=sys.stderr, flush=True)
    if parameters['fail']:
        raise ValueError('remote error=' + token)
    Path(context['outputs']['report']).write_text(json.dumps(dict(seen=bool(token))))
    return dict(ops=[], data=dict(seen=bool(token),echo=token), artifacts=[dict(role='report',path='.')])
plugin.context_operation('inspect')(inspect)
plugin.context_operation('other')(inspect)
plugin.run()
";

fn fixture_registry(root: &Path, bundled: bool, title: &str) -> Registry {
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("plugin.toml"), "schema=2\ncontract='operations.json'\nid='org.example.credentials'\nname='Credential fixture'\nversion='1.0.0'\n[entry]\noperation='python:plugin.py'\n").unwrap();
    std::fs::write(source.join("plugin.py"), SCRIPT).unwrap();
    let descriptor = |name: &str| {
        json!({"name":name,"version":"1.0.0","implementation":name,"title":title,"description":"Synthetic credential boundary fixture","family":"file_analysis",
        "parameters":{"type":"object","additionalProperties":false,"properties":{"fail":{"type":"boolean","default":false}}},
        "result":{"type":"object","additionalProperties":false,"properties":{"seen":{"type":"boolean"},"echo":{"type":"string"}},"required":["seen","echo"]},
        "inputs":{"source":{"view":"bytes","media_type":"application/octet-stream","min":1,"max":1}},
        "outputs":{"report":{"media_type":"application/json","min":1,"max":1,"visibility":"result"}},
        "effects":{"document_write":false,"artifact_write":true},
        "credentials":{"access":"BASECRAFT_ZENODO_TOKEN"},"required_features":["operation-contract-v1"]})
    };
    std::fs::write(
        source.join("operations.json"),
        json!({"schema":1,"operations":[descriptor("inspect"),descriptor("other")]}).to_string(),
    )
    .unwrap();
    let config = RegistryConfig {
        bundled: root.join("bundle"),
        packages: root.join("packages"),
        index: root.join("index.json"),
    };
    if bundled {
        std::fs::create_dir_all(&config.bundled).unwrap();
        std::fs::rename(source, config.bundled.join("fixture")).unwrap();
    } else {
        registry::install(&config, &source).unwrap();
    }
    Registry::load(&config).unwrap()
}
fn run(
    registry: &Registry,
    root: &Path,
    policy: SessionPolicy,
    operation: &str,
    fail: bool,
) -> OperationOutcome {
    std::fs::write(root.join("input"), b"synthetic").unwrap();
    let request=serde_json::from_value(json!({"operation":operation,"inputs":{"source":[{"kind":"file","path":"input"}]},"parameters":{"fail":fail}})).unwrap();
    let mut jobs = JobManager::default();
    let accepted = jobs
        .submit(registry, request, policy, "credential regression")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let outcome = jobs.get(&accepted.run_id).unwrap();
        if outcome.state.terminal() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "{outcome:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn evidence(outcome: &OperationOutcome) -> String {
    let execution = std::fs::read_to_string(
        Path::new(outcome.evidence.as_ref().unwrap()).join("execution.json"),
    )
    .unwrap();
    assert!(!execution.contains("synthetic-ambient-secret"));
    assert!(!execution.contains("synthetic-explicit-secret"));
    assert!(
        !serde_json::to_string(outcome)
            .unwrap()
            .contains("synthetic-explicit-secret")
    );
    execution
}
#[test]
fn credential_scope_and_redaction() {
    crate::isolated_profile::isolate!();
    // Spawn our own test child with fake ambient inputs; never mutate this
    // process's shared environment or require real user credentials.
    if std::env::var_os("BASECRAFT_CREDENTIAL_TEST_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "credential_scope_and_redaction", "--nocapture"])
            .env("BASECRAFT_CREDENTIAL_TEST_CHILD", "1")
            .env("BASECRAFT_ZENODO_TOKEN", "synthetic-ambient-secret")
            .env("BASECRAFT_UNRELATED_SETTING", "preserved")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    for bundled in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let registry = fixture_registry(root, bundled, "Original");
        let operation = registry
            .resolve("org.example.credentials/inspect@1", None)
            .unwrap();
        // Declarations (even bundled ones) do not authorize ambient credentials.
        let ordinary = run(
            &registry,
            root,
            SessionPolicy::new(root, true).unwrap(),
            &operation.id,
            false,
        );
        assert_eq!(ordinary.state, RuntimeState::Completed, "{ordinary:?}");
        assert_eq!(ordinary.data.as_ref().unwrap()["seen"], false);
        assert!(evidence(&ordinary).contains("initialize=absent"));
        let granted = SessionPolicy::new(root, true)
            .unwrap()
            .with_runtime_credential(operation, "access", "synthetic-explicit-secret".into())
            .unwrap();
        assert!(!format!("{granted:?}").contains("synthetic-explicit-secret"));
        let intended = run(&registry, root, granted.clone(), &operation.id, false);
        assert_eq!(intended.state, RuntimeState::Completed, "{intended:?}");
        assert_eq!(intended.data.as_ref().unwrap()["seen"], true);
        assert_eq!(intended.data.as_ref().unwrap()["echo"], "[redacted]");
        let execution: Value = serde_json::from_str(&evidence(&intended)).unwrap();
        assert!(
            execution["stdout"]
                .as_str()
                .unwrap()
                .contains("stdout=[redacted]")
        );
        assert!(
            execution["stderr"]
                .as_str()
                .unwrap()
                .contains("stderr=[redacted]")
        );
        // An operation in the same package declaring the same credential is
        // still a different recipient and cannot borrow a grant.
        let other = run(
            &registry,
            root,
            granted.clone(),
            "org.example.credentials/other@1",
            false,
        );
        assert_eq!(other.data.as_ref().unwrap()["seen"], false);
        evidence(&other);
        let failed = run(&registry, root, granted.clone(), &operation.id, true);
        assert_eq!(failed.state, RuntimeState::Failed);
        assert!(failed.errors[0].message.contains("[redacted]"));
        evidence(&failed);
        // Direct/legacy process spawn also strips ambient credentials before
        // initialize, even when a package declares the same credential.
        let mut direct = basecraft_plugin_host::PluginProcess::spawn_with(
            &operation.package.manifest,
            operation.package.interpreter.as_deref().unwrap(),
        )
        .unwrap();
        direct.finish().unwrap();
        assert!(direct.captured_output().0.contains("initialize=absent"));
        // Same operation ID, replaced accepted code: old grants do not transfer.
        let replacement = tempfile::tempdir().unwrap();
        // A genuinely different package digest with the same public operation ID.
        let changed = fixture_registry(replacement.path(), bundled, "Replacement");
        let outcome = run(
            &changed,
            replacement.path(),
            SessionPolicy::new(replacement.path(), true)
                .unwrap()
                .with_runtime_credential(operation, "access", "synthetic-explicit-secret".into())
                .unwrap(),
            &operation.id,
            false,
        );
        assert_eq!(outcome.data.as_ref().unwrap()["seen"], false);
        evidence(&outcome);
    }
}

#[test]
fn reserved_ipc_credential_is_rejected() {
    use basecraft_plugin_host::contract;
    use basecraft_protocol::operation::OperationContract;
    crate::isolated_profile::isolate!();

    let temp = tempfile::tempdir().unwrap();
    let registry = fixture_registry(temp.path(), false, "Credential declaration");
    let operation = registry
        .resolve("org.example.credentials/inspect@1", None)
        .unwrap();
    let mut descriptor = OperationContract {
        schema: 1,
        operations: vec![operation.descriptor.clone()],
    };
    // The regular service token declaration and explicit pinned grant remain valid.
    assert!(contract::validate_contract(&operation.package.manifest.id, &descriptor).is_ok());
    assert!(
        SessionPolicy::new(temp.path(), true)
            .unwrap()
            .with_runtime_credential(operation, "access", "synthetic-service-secret".into())
            .is_ok()
    );

    descriptor.operations[0]
        .credentials
        .insert("access".into(), "BASECRAFT_TOKEN".into());
    assert!(
        contract::validate_contract(&operation.package.manifest.id, &descriptor).is_err(),
        "IPC authentication must not be a public credential declaration"
    );
    // Defend the grant boundary too, even if a native caller bypasses registry
    // declaration validation while constructing a resolved-operation value.
    let mut invalid_recipient = operation.clone();
    invalid_recipient.descriptor = descriptor.operations.remove(0);
    assert!(
        SessionPolicy::new(temp.path(), true)
            .unwrap()
            .with_runtime_credential(&invalid_recipient, "access", "synthetic-ipc-secret".into())
            .is_err(),
        "IPC authentication must not be grantable"
    );
}
