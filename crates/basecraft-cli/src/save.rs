//! Headless presentation of the shared Session save boundary.
use std::path::Path;

use basecraft_ops::{DocId, Error, Session, input_evidence::SavePolicy};
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
    let code = match error {
        Error::Io(e) | Error::Format(basecraft_io::Error::Io(e))
            if e.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            "permission_denied"
        }
        Error::Io(_) | Error::Format(basecraft_io::Error::Io(_)) => "io_error",
        Error::Format(_) => "format_error",
        Error::Json(_) | Error::Invalid(_) => "invalid_arguments",
        Error::UnknownFormat(_) => "unknown_format",
        Error::Empty(_) => "empty_document",
        Error::EvidenceConflict(_) => "analysis_input_conflict",
        Error::EvidenceCheckIncomplete(_) | Error::SaveCheckRequired(_) => {
            "evidence_check_incomplete"
        }
        Error::SaveDecisionStale(_) => "save_decision_stale",
        Error::UnknownDocument { .. } => "unknown_document",
        Error::WrongKind { .. } => "wrong_document_kind",
        Error::NothingTo(_) => "nothing_to_do",
        Error::PublishedWithFailure(_) => "published_with_failure",
    };
    let mut value = json!({"state":"failed", "errors":[{"code":code,"message":error.to_string()}]});
    match error {
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
            completion,
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
