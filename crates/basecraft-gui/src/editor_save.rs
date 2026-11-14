//! Shared Qt forwarding for live document save decisions.
use std::path::Path;

#[derive(Default)]
pub(crate) struct SaveState {
    pub decision: Option<basecraft_ops::session::SaveDecision>,
    pub unchecked: bool,
    pub succeeded: bool,
}

pub(crate) fn check_json(
    result: basecraft_ops::Result<basecraft_ops::session::SaveCheck>,
) -> cxx_qt_lib::QString {
    cxx_qt_lib::QString::from(
        match result {
            Ok(check) => serde_json::json!({"check": check}),
            Err(error) => serde_json::json!({"error": error.to_string(), "code": error.code()}),
        }
        .to_string(),
    )
}

/// `{"saved": bool}`: the answer to a save request that needed no question.
pub(crate) fn saved_json(saved: bool) -> cxx_qt_lib::QString {
    cxx_qt_lib::QString::from(&serde_json::json!({ "saved": saved }).to_string())
}

/// The check a refused save carries when the refusal is a question for the
/// user: unchecked analyses, or a check that went stale before the answer.
pub(crate) fn question(error: &basecraft_ops::Error) -> Option<basecraft_ops::session::SaveCheck> {
    match error {
        basecraft_ops::Error::SaveCheckRequired(check)
        | basecraft_ops::Error::SaveDecisionStale(check) => Some((**check).clone()),
        _ => None,
    }
}

/// Whether a check (as sent to the window) names analyses that used the
/// file, or parts it could not check: then the user answers before saving.
fn needs_answer(check: &serde_json::Value) -> bool {
    let listed =
        |value: &serde_json::Value| value.as_array().is_some_and(|items| !items.is_empty());
    listed(&check["evidence"]["issues"]) || listed(&check["evidence"]["conflict"]["analyses"])
}

/// Save As: check the destination first, then save unless the user must
/// answer. A new file's unchecked analyses would only warn while saving, and
/// the window asks about them instead.
pub(crate) fn request_save_as(
    check: cxx_qt_lib::QString,
    save: impl FnOnce(&cxx_qt_lib::QString) -> bool,
) -> cxx_qt_lib::QString {
    let answer: serde_json::Value = serde_json::from_str(&check.to_string()).unwrap_or_default();
    match answer.get("check") {
        Some(check) if !needs_answer(check) => saved_json(save(&cxx_qt_lib::QString::from(
            check["token"].as_str().unwrap_or_default(),
        ))),
        _ => check,
    }
}

pub(crate) fn save(
    session: &mut basecraft_ops::Session,
    doc: basecraft_ops::DocId,
    path: Option<&Path>,
    decision: Option<basecraft_ops::session::SaveDecision>,
    unchecked: &mut bool,
) -> basecraft_ops::Result<()> {
    *unchecked = false;
    if let Some(decision) = decision {
        *unchecked = !session
            .save_with_decision(doc, path, decision)?
            .warnings
            .is_empty();
        Ok(())
    } else {
        session.save(doc, path)
    }
}

// Shared Qt forwarding only: Session owns checking, consumption and saving.
macro_rules! confirmed_save {
    ($controller:ty) => {
        impl $controller {
            pub fn check_save(&self, path: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
                let path = path.to_string();
                let result = self
                    .rust()
                    .session
                    .as_ref()
                    .zip(self.rust().doc)
                    .ok_or(basecraft_ops::Error::UnknownDocument {
                        id: basecraft_ops::DocId(usize::MAX),
                    })
                    .and_then(|(s, d)| {
                        s.check_save(d, (!path.is_empty()).then(|| std::path::Path::new(&path)))
                    });
                crate::editor_save::check_json(result)
            }
            /// Save to the document's own file, or to `path`, unless the user
            /// must answer first. `{"saved": bool}`, `{"check": ...}` with the
            /// question to show, or `{"error": ...}`.
            pub fn request_save(
                mut self: std::pin::Pin<&mut Self>,
                path: &cxx_qt_lib::QString,
            ) -> cxx_qt_lib::QString {
                if !path.is_empty() {
                    let check = self.check_save(path);
                    return crate::editor_save::request_save_as(check, |token| {
                        self.as_mut().save_confirmed(path, token, &cxx_qt_lib::QString::from("cancel"), false)
                    });
                }
                // The save's own check decides. A refusal that is a question
                // carries its check, except a conflict found by a complete
                // check, which is checked once more to give the question.
                match self.as_mut().write_own() {
                    Ok(saved) => crate::editor_save::saved_json(saved),
                    Err(error) => match crate::editor_save::question(&error) {
                        Some(check) => crate::editor_save::check_json(Ok(check)),
                        None if matches!(error, basecraft_ops::Error::EvidenceConflict(_)) => self.check_save(path),
                        None => {
                            let message = format!("Save failed: {error}");
                            self.fail(&message);
                            cxx_qt_lib::QString::from(&serde_json::json!({ "error": message }).to_string())
                        }
                    },
                }
            }
            pub fn save_confirmed(
                mut self: std::pin::Pin<&mut Self>,
                path: &cxx_qt_lib::QString,
                token: &cxx_qt_lib::QString,
                policy: &cxx_qt_lib::QString,
                acknowledge: bool,
            ) -> bool {
                let target = path.to_string();
                let result = (|| -> basecraft_ops::Result<_> {
                    let policy =
                        serde_json::from_value(serde_json::Value::String(policy.to_string()))?;
                    let (session, doc) = self.rust().session.as_ref().zip(self.rust().doc).ok_or(
                        basecraft_ops::Error::UnknownDocument {
                            id: basecraft_ops::DocId(usize::MAX),
                        },
                    )?;
                    session.decide_save(
                        doc,
                        (!target.is_empty()).then(|| std::path::Path::new(&target)),
                        &token.to_string(),
                        policy,
                        acknowledge,
                    )
                })();
                let decision = match result {
                    Ok(d) => d,
                    Err(e) => {
                        self.fail(&e.to_string());
                        return false;
                    }
                };
                self.as_mut().rust_mut().save_state.decision = Some(decision);
                self.as_mut().rust_mut().save_state.succeeded = false;
                let saved = if target.is_empty() {
                    self.as_mut().save();
                    self.rust().save_state.succeeded
                } else {
                    self.as_mut().save_as(path)
                };
                self.as_mut().rust_mut().save_state.decision = None;
                if saved && self.rust().save_state.unchecked {
                    self.as_mut().set_message(cxx_qt_lib::QString::from(
                        "Saved. Some analysis evidence was not checked.",
                    ));
                }
                saved
            }
        }
    };
}
pub(crate) use confirmed_save;

macro_rules! ordinary_save {
    ($controller:ty $(, $busy:ident)?) => {
        impl $controller {
            pub fn save(mut self: std::pin::Pin<&mut Self>) {
                if let Err(e) = self.as_mut().write_own() { self.fail(&format!("Save failed: {e}")); }
            }
            /// Save to the document's own file; the error is the caller's to
            /// report. `Ok(false)` when no document is open.
            fn write_own(mut self: std::pin::Pin<&mut Self>) -> basecraft_ops::Result<bool> {
                let started = std::time::Instant::now();
                {
                    let mut r = self.as_mut().rust_mut(); let r = &mut *r;
                    let (Some(session), Some(doc)) = (&mut r.session, r.doc) else { return Ok(false); };
                    crate::editor_save::save(session, doc, None, r.save_state.decision.take(), &mut r.save_state.unchecked)?;
                }
                self.as_mut().rust_mut().save_state.succeeded = true;
                self.as_mut().set_dirty(false);
                self.done(&format!("Saved{}", took(started)));
                Ok(true)
            }
            pub fn save_as(mut self: std::pin::Pin<&mut Self>, path: &cxx_qt_lib::QString) -> bool {
                $(if self.rust().$busy {
                    self.fail("Wait for clade extraction before saving under a new name"); return false;
                })?
                let started = std::time::Instant::now();
                let target = std::path::PathBuf::from(path.to_string());
                let result = {
                    let mut r = self.as_mut().rust_mut(); let r = &mut *r;
                    let (Some(session), Some(doc)) = (&mut r.session, r.doc) else { return false; };
                    crate::editor_save::save(session, doc, Some(&target), r.save_state.decision.take(), &mut r.save_state.unchecked)
                };
                if let Err(e) = result { self.fail(&format!("Save failed: {e}")); return false; }
                if !self.as_mut().open_file(path) { return false; }
                self.done(&format!("Saved as {}{}", file_name(&target), took(started)));
                true
            }
        }
    };
}
pub(crate) use ordinary_save;

// Auto-saving metadata/submission editors retain their pending edit on failure.
macro_rules! auto_save {
    ($controller:ty $(, $clean:ident)?) => {
        impl $controller {
            fn save_error(mut self: std::pin::Pin<&mut Self>, error: &basecraft_ops::Error) {
                self.as_mut().report("error", &format!("Changed but not saved: {error}"));
                // The question carries its check, so the window shows it without checking again.
                let check = match crate::editor_save::question(error) {
                    Some(check) => Some(crate::editor_save::check_json(Ok(check))),
                    None if matches!(error, basecraft_ops::Error::EvidenceConflict(_)) => Some(self.check_save(&cxx_qt_lib::QString::default())),
                    None => None,
                };
                if let Some(check) = check { self.save_question(check); }
            }
            pub fn check_save(&self, path: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
                let target = path.to_string();
                let result = self.rust().session.as_ref().zip(self.rust().doc)
                    .ok_or(basecraft_ops::Error::UnknownDocument { id: basecraft_ops::DocId(usize::MAX) })
                    .and_then(|(session, doc)| session.check_save(doc, (!target.is_empty()).then(|| std::path::Path::new(&target))));
                crate::editor_save::check_json(result)
            }
            pub fn save_confirmed(mut self: std::pin::Pin<&mut Self>, path: &cxx_qt_lib::QString,
                token: &cxx_qt_lib::QString, policy: &cxx_qt_lib::QString, acknowledge: bool) -> bool {
                let target = path.to_string();
                let mut copy_target = String::new();
                let result = (|| -> basecraft_ops::Result<_> {
                    let mut rust = self.as_mut().rust_mut();
                    let doc = rust.doc.ok_or(basecraft_ops::Error::UnknownDocument { id: basecraft_ops::DocId(usize::MAX) })?;
                    let session = rust.session.as_mut().unwrap();
                    let path = (!target.is_empty()).then(|| std::path::Path::new(&target));
                    let policy = serde_json::from_value(serde_json::Value::String(policy.to_string()))?;
                    let decision = session.decide_save(doc, path, &token.to_string(), policy, acknowledge)?;
                    let outcome = session.save_with_decision(doc, path, decision)?;
                    if session.document_record(doc)?.unsaved { copy_target.clone_from(&target); }
                    Ok(outcome)
                })();
                self.finish_save(result.map(|outcome| !outcome.warnings.is_empty()), &copy_target)
            }
            fn finish_save(mut self: std::pin::Pin<&mut Self>, result: basecraft_ops::Result<bool>, target: &str) -> bool {
                match result {
                    Ok(unchecked) => {
                        $(if target.is_empty() { self.as_mut().$clean(false); })?
                        self.as_mut().refresh();
                        let mut message = if target.is_empty() { "Saved.".to_owned() }
                            else { format!("Saved a copy to {target}. Original has unsaved changes.") };
                        if unchecked { message.push_str(" Some analysis evidence was not checked."); }
                        self.report("ok", &message);
                        true
                    }
                    Err(error) => { self.report("error", &error.to_string()); false }
                }
            }
        }
    };
}
pub(crate) use auto_save;
