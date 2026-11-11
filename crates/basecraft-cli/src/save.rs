//! Headless presentation of the shared Session save boundary.
use std::path::Path;

use basecraft_ops::{
    DocId, Error, Session,
    access::{AccessError, Denial},
    input_evidence::SavePolicy,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Wire choices are explicit; the Session, never this snapshot, owns authority.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decision {
    pub token: String,
    pub known_policy: SavePolicy,
    pub acknowledge_incomplete: bool,
}

#[derive(Clone, Copy, Default, clap::Args)]
pub(crate) struct Options {
    /// For this invocation's saves only; independent of the known-analysis policy.
    #[arg(long, global = true)]
    pub acknowledge_incomplete_evidence: bool,
    /// Known affected analyses still require their own explicit policy.
    #[arg(long, global = true, value_parser = crate::save_policy, default_value = "cancel")]
    pub save_known_policy: SavePolicy,
}

impl Options {
    pub fn save(
        self,
        session: &mut Session,
        doc: DocId,
        path: Option<&Path>,
        no_overwrite: bool,
    ) -> basecraft_ops::Result<()> {
        if self.acknowledge_incomplete_evidence || self.save_known_policy != SavePolicy::Cancel {
            let check = session.check_save(doc, path)?;
            let decision = session.decide_save(
                doc,
                path,
                check.token(),
                self.save_known_policy,
                self.acknowledge_incomplete_evidence,
            )?;
            let outcome = if no_overwrite {
                session.save_new_with_decision(doc, path, decision)?
            } else {
                session.save_with_decision(doc, path, decision)?
            };
            // Existing commands retain stdout contracts. Never discard warned success.
            if !outcome.warnings.is_empty() {
                eprintln!("{}", json!({"state":"saved", "warnings":outcome.warnings}));
            }
            Ok(())
        } else if no_overwrite {
            session.save_new(doc, path)
        } else {
            session.save(doc, path)
        }
    }
}

/// Typed errors stay typed through CLI, MCP and Python, including the fresh check.
pub(crate) fn error_value(error: &Error) -> Value {
    let code = error.code();
    let mut value = json!({"state":"failed", "errors":[{"code":code,"message":error.to_string()}]});
    match error {
        Error::Access(AccessError::Denied(reason)) => {
            let reason = match reason {
                Denial::InvalidPath => "invalid_path",
                Denial::OutsideRoot => "outside_root",
                Denial::ReadOnly => "read_only",
                Denial::SymbolicLink => "symbolic_link",
                Denial::Hardlink => "hardlink",
                Denial::ResourceType => "resource_type",
                Denial::RootMutation => "root_mutation",
                Denial::ServiceUnavailable => "service_unavailable",
            };
            value["access"] = json!({"kind":"policy_denied", "reason":reason});
        }
        Error::Access(AccessError::DestinationExists) => {
            value["access"] = json!({"kind":"destination_exists"});
        }
        Error::Access(AccessError::Io(error)) => {
            value["access"] = json!({"kind":"io", "raw_os_error":error.raw_os_error()});
        }
        Error::PublishedWithFailure(failure) => {
            value["state"] = json!("published_with_failure");
            value["completion"] = json!(failure.completion);
            value["cause"] = error_value(&failure.cause);
        }
        Error::EvidenceConflict(conflict) => value["conflict"] = json!(conflict),
        Error::EvidenceCheckIncomplete(evidence) => value["evidence"] = json!(evidence),
        Error::SaveCheckRequired(check) | Error::SaveDecisionStale(check) => {
            value["check"] = json!(check);
        }
        _ => {}
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use basecraft_ops::{
        input_evidence::{EvidenceConflict, InspectionCode, InspectionIssue, InspectionRelevance},
        replay::{PublishedWithFailure, RecoveryCompletion},
        session::{EvidenceAction, SaveWarning},
    };

    #[test]
    fn published_failure_envelope_retains_completion_and_typed_cause() {
        let completion = RecoveryCompletion {
            destination: "recovered".into(),
            outputs: vec!["recovered/a.spart".into(), "recovered/nested/b.xml".into()],
            warnings: vec![SaveWarning::EvidenceNotChecked {
                issues: vec![InspectionIssue {
                    code: InspectionCode::StoreUnavailable,
                    location: "unavailable-store".into(),
                    relevance: InspectionRelevance::Unknown,
                    reason: "store is unavailable".into(),
                }],
            }],
            damage: Some("retained verified prefix before torn entry".into()),
            registered_stores: vec!["recovered/.basecraft/analyses-v1".into()],
            evidence_actions: vec![EvidenceAction {
                conflict: EvidenceConflict {
                    target: "recovered/a.spart".into(),
                    analyses: Vec::new(),
                    options: vec![SavePolicy::Cancel, SavePolicy::BreakEvidence],
                },
                policy: SavePolicy::BreakEvidence,
                completed: true,
            }],
        };
        let expected = serde_json::to_value(&completion).unwrap();
        let error = Error::PublishedWithFailure(Box::new(PublishedWithFailure {
            completion: completion.clone(),
            cause: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "registry denied")
                .into(),
        }));
        let value = error_value(&error);
        assert_eq!(value["state"], "published_with_failure");
        assert_eq!(value["errors"][0]["code"], "published_with_failure");
        assert_eq!(value["completion"], expected);
        assert_eq!(value["cause"]["errors"][0]["code"], "permission_denied");
        assert_eq!(value["cause"]["errors"][0]["message"], "registry denied");
        // The actual JSON transport must preserve the same partial outcome.
        let transported: Value =
            serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(transported, value);
        for cause in [
            AccessError::Denied(Denial::ServiceUnavailable),
            AccessError::Io(std::io::Error::from_raw_os_error(13)),
            AccessError::DestinationExists,
        ] {
            let cause: Error = cause.into();
            let expected_cause = error_value(&cause);
            let nested = error_value(&Error::PublishedWithFailure(Box::new(
                PublishedWithFailure {
                    completion: completion.clone(),
                    cause,
                },
            )));
            let roundtrip: Value =
                serde_json::from_slice(&serde_json::to_vec(&nested).unwrap()).unwrap();
            assert_eq!(roundtrip["completion"], expected);
            assert_eq!(roundtrip["cause"], expected_cause);
            assert_eq!(roundtrip["state"], "published_with_failure");
        }
    }

    #[test]
    fn access_denial_reasons_are_structured_and_not_waivable_checks() {
        for (reason, wire) in [
            (Denial::InvalidPath, "invalid_path"),
            (Denial::OutsideRoot, "outside_root"),
            (Denial::ReadOnly, "read_only"),
            (Denial::SymbolicLink, "symbolic_link"),
            (Denial::Hardlink, "hardlink"),
            (Denial::ResourceType, "resource_type"),
            (Denial::RootMutation, "root_mutation"),
            (Denial::ServiceUnavailable, "service_unavailable"),
        ] {
            let value = error_value(&AccessError::Denied(reason).into());
            let value: Value =
                serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert_eq!(value["errors"][0]["code"], "permission_denied");
            assert_eq!(
                value["access"],
                json!({"kind":"policy_denied", "reason":wire})
            );
            assert_eq!(value["state"], "failed");
            assert!(value.get("check").is_none());
            assert!(value.get("evidence").is_none());
        }
    }

    #[test]
    fn access_io_and_conflict_keep_categories_without_message_matching() {
        let permission = error_value(
            &AccessError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "ordinary OS denial",
            ))
            .into(),
        );
        assert_eq!(permission["errors"][0]["code"], "permission_denied");
        assert_eq!(
            permission["access"],
            json!({"kind":"io", "raw_os_error":null})
        );
        let raw = std::io::Error::from_raw_os_error(13);
        let expected_code = if raw.kind() == std::io::ErrorKind::PermissionDenied {
            "permission_denied"
        } else {
            "io_error"
        };
        let value = error_value(&AccessError::Io(raw).into());
        assert_eq!(value["errors"][0]["code"], expected_code);
        assert_eq!(value["access"], json!({"kind":"io", "raw_os_error":13}));
        let text = error_value(
            &AccessError::Io(std::io::Error::other(
                "permission_denied output_conflict evidence_check_incomplete",
            ))
            .into(),
        );
        assert_eq!(text["errors"][0]["code"], "io_error");
        assert_eq!(text["access"]["kind"], "io");
        assert!(text.get("check").is_none());
        let conflict = error_value(&AccessError::DestinationExists.into());
        assert_eq!(conflict["errors"][0]["code"], "output_conflict");
        assert_eq!(conflict["access"], json!({"kind":"destination_exists"}));
        for value in [permission, value, text, conflict] {
            let roundtrip: Value =
                serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert_eq!(roundtrip, value);
        }
    }

    #[test]
    fn published_failure_without_registrations_is_not_an_unpublished_error() {
        let error = Error::PublishedWithFailure(Box::new(PublishedWithFailure {
            completion: RecoveryCompletion {
                destination: "recovered".into(),
                outputs: vec!["recovered/a.fasta".into()],
                warnings: Vec::new(),
                damage: None,
                registered_stores: Vec::new(),
                evidence_actions: Vec::new(),
            },
            cause: std::io::Error::other("parent sync failed").into(),
        }));
        let value = error_value(&error);
        assert_eq!(value["state"], "published_with_failure");
        assert_eq!(value["completion"]["destination"], "recovered");
        assert_eq!(value["completion"]["outputs"], json!(["recovered/a.fasta"]));
        assert_eq!(value["completion"]["registered_stores"], json!([]));
        assert_eq!(value["completion"]["warnings"], json!([]));
        assert_eq!(value["completion"]["evidence_actions"], json!([]));
        assert_eq!(value["completion"]["damage"], Value::Null);
        assert_eq!(value["cause"]["errors"][0]["code"], "io_error");
        let ordinary = error_value(&std::io::Error::other("write failed").into());
        assert_eq!(ordinary["state"], "failed");
        assert!(ordinary.get("completion").is_none());
    }
}

#[cfg(test)]
mod typed_error_tests {
    use super::*;

    #[test]
    fn core_error_codes_survive_structured_adapters() {
        let cases = [
            (
                Error::ChangedOnDisk {
                    path: "input.fa".into(),
                },
                "changed_on_disk",
            ),
            (
                Error::WrongKind {
                    id: basecraft_ops::DocId(1),
                    expected: basecraft_ops::files::Kind::Tree,
                },
                "wrong_document_kind",
            ),
            (
                Error::Limit {
                    what: "journal".into(),
                    limit: 1024,
                },
                "limit",
            ),
            (
                Error::NotFound {
                    path: "input.fa".into(),
                },
                "not_found",
            ),
            (
                Error::Damaged {
                    path: "input.fa".into(),
                    detail: "invalid contents".into(),
                },
                "damaged",
            ),
            (Error::Unsupported("future format".into()), "unsupported"),
            (Error::Busy("writer active".into()), "busy"),
            (std::io::Error::other("disk full").into(), "io_error"),
            (
                std::io::Error::from(std::io::ErrorKind::NotFound).into(),
                "io_error",
            ),
            (
                std::io::Error::from(std::io::ErrorKind::PermissionDenied).into(),
                "permission_denied",
            ),
            (
                Error::Json(serde_json::from_str::<Value>("{").unwrap_err()),
                "invalid_arguments",
            ),
            (Error::Invalid("bad argument".into()), "invalid_arguments"),
            (Error::UnknownFormat("unknown.ext".into()), "unknown_format"),
            (Error::Empty("empty.fa".into()), "empty_document"),
            (Error::NothingTo("undo"), "nothing_to_do"),
            (
                Error::EvidenceConflict(Box::new(
                    basecraft_ops::input_evidence::EvidenceConflict {
                        target: "input.fa".into(),
                        analyses: vec![],
                        options: vec![],
                    },
                )),
                "analysis_input_conflict",
            ),
        ];
        for (error, code) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error_value(&error)["errors"][0]["code"], code);
            let outcome = basecraft_plugin_host::jobs::error_value(
                "test",
                basecraft_plugin_host::jobs::ops_error_code(&error),
                error.to_string(),
            );
            assert_eq!(outcome["errors"][0]["code"], code);
        }
    }
}
