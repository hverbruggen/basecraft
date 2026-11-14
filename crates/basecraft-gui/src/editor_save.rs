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
                let started = std::time::Instant::now();
                let result = {
                    let mut r = self.as_mut().rust_mut(); let r = &mut *r;
                    let (Some(session), Some(doc)) = (&mut r.session, r.doc) else { return; };
                    crate::editor_save::save(session, doc, None, r.save_state.decision.take(), &mut r.save_state.unchecked)
                };
                match result {
                    Ok(()) => { self.as_mut().rust_mut().save_state.succeeded = true; self.as_mut().set_dirty(false); self.done(&format!("Saved{}", took(started))); }
                    Err(e) => self.fail(&format!("Save failed: {e}")),
                }
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
                if matches!(error, basecraft_ops::Error::SaveCheckRequired(_) | basecraft_ops::Error::SaveDecisionStale(_) | basecraft_ops::Error::InputConflict(_)) {
                    self.save_question();
                }
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
