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
        Error::NothingTo(_) => "nothing_to_do",
    };
    let mut value = json!({"state":"failed", "errors":[{"code":code,"message":error.to_string()}]});
    match error {
        Error::EvidenceConflict(conflict) => value["conflict"] = json!(conflict),
        Error::EvidenceCheckIncomplete(evidence) => value["evidence"] = json!(evidence),
        Error::SaveCheckRequired(check) | Error::SaveDecisionStale(check) => {
            value["check"] = json!(check);
        }
        _ => {}
    }
    value
}
