//! QML boundary for GenBank marker submissions (Data submission tab).
//!
//! A thin adapter over `basecraft_ops::submission`: every change is one typed
//! `submission.edit` in a recoverable session, saved at once, as the
//! specimen table does. table2asn runs through the registered operation
//! runner (`AppOperations` in QML submits the request this controller
//! builds); this controller then records the job's outcome.
use std::path::{Path, PathBuf};
use std::pin::Pin;

use basecraft_ops::submission::{self, Review};
use basecraft_ops::{DocId, Session};
use basecraft_protocol::submission::{
    Author, Decision, FrameChoice, Publication, RowDecision, SubmissionEdit,
};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{Value, json};

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for SubmissionController {}
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, project_dir)]
        #[qproperty(QString, path)]
        #[qproperty(QString, review_stage)]
        #[qproperty(bool, reviewing)]
        #[qproperty(bool, available)]
        #[qproperty(bool, busy)]
        #[qproperty(QString, document)]
        #[qproperty(QString, review)]
        #[qproperty(QString, package)]
        #[qproperty(QString, message)]
        #[qproperty(QString, message_kind)]
        #[qproperty(bool, can_undo)]
        #[qproperty(bool, can_redo)]
        type SubmissionController = super::SubmissionRust;
        #[qsignal]
        #[cxx_name = "saveQuestion"]
        fn save_question(self: Pin<&mut SubmissionController>);
        #[qinvokable]
        fn check_save(self: &SubmissionController, path: &QString) -> QString;
        #[qinvokable]
        fn save_confirmed(
            self: Pin<&mut SubmissionController>,
            path: &QString,
            token: &QString,
            policy: &QString,
            acknowledge: bool,
        ) -> bool;
        #[qinvokable]
        fn use_project(self: Pin<&mut SubmissionController>, path: &QString);
        #[qinvokable]
        fn create(
            self: Pin<&mut SubmissionController>,
            alignment: &QString,
            name: &QString,
        ) -> bool;
        #[qinvokable]
        fn open(self: Pin<&mut SubmissionController>, path: &QString) -> bool;
        #[qsignal]
        fn opened(self: Pin<&mut SubmissionController>, success: bool);
        #[qinvokable]
        fn edit(self: Pin<&mut SubmissionController>, edit: &QString) -> bool;
        #[qinvokable]
        fn set_publication(
            self: Pin<&mut SubmissionController>,
            title: &QString,
            authors: &QString,
        ) -> bool;
        #[qinvokable]
        fn set_details(self: Pin<&mut SubmissionController>, details: &QString) -> bool;
        #[qinvokable]
        fn preview_authors(self: &SubmissionController, authors: &QString) -> QString;
        #[qinvokable]
        fn preview_inclusion(self: Pin<&mut SubmissionController>, rule: &QString) -> QString;
        #[qinvokable]
        fn set_decision(
            self: Pin<&mut SubmissionController>,
            record: &QString,
            decision: &QString,
            accession: &QString,
        ) -> bool;
        #[qinvokable]
        fn set_frame(
            self: Pin<&mut SubmissionController>,
            record: &QString,
            codon_start: i32,
        ) -> bool;
        #[qinvokable]
        fn confirm_detected(self: Pin<&mut SubmissionController>) -> bool;
        #[qinvokable]
        fn undo(self: Pin<&mut SubmissionController>);
        #[qinvokable]
        fn redo(self: Pin<&mut SubmissionController>);
        #[qinvokable]
        fn refresh_review(self: Pin<&mut SubmissionController>);
        #[qinvokable]
        fn export_package(self: Pin<&mut SubmissionController>) -> bool;
        #[qinvokable]
        fn validation_request(self: Pin<&mut SubmissionController>) -> QString;
        #[qinvokable]
        fn record_validation(self: Pin<&mut SubmissionController>, outcome: &QString) -> bool;
        #[qinvokable]
        fn plan_accessions(self: Pin<&mut SubmissionController>, text: &QString) -> QString;
        #[qinvokable]
        fn record_accessions(self: Pin<&mut SubmissionController>, accessions: &QString) -> bool;
        #[qinvokable]
        fn write_accession_table(self: Pin<&mut SubmissionController>, output: &QString) -> bool;
        #[qinvokable]
        fn genetic_codes(self: &SubmissionController) -> QString;
    }
}

// Each bool is a QML property.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct SubmissionRust {
    project_dir: QString,
    path: QString,
    available: bool,
    busy: bool,
    open_epoch: u64,
    review_stage: QString,
    reviewing: bool,
    review_active: bool,
    review_epoch: u64,
    review_cache: Option<submission::ReviewCache>,
    document: QString,
    review: QString,
    package: QString,
    message: QString,
    message_kind: QString,
    can_undo: bool,
    can_redo: bool,
    session: Option<Session>,
    doc: Option<DocId>,
}

/// Authors as one per line, `Last, First M.`.
pub(crate) fn parse_authors(text: &str) -> Result<Vec<Author>, String> {
    basecraft_docs::submission::parse_authors(text).map_err(|error| error.0)
}

pub(crate) fn authors_text(authors: &[Author]) -> String {
    authors
        .iter()
        .map(|a| {
            let given = [a.first.as_str(), a.middle.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            format!("{}, {given}", a.last)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The review as the dialog shows it: one row per alignment row with its
/// decision, specimen, organism, frame and issue counts; issues carry the
/// row they concern so selecting one selects the row.
pub(crate) fn review_view(review: &Review) -> Value {
    let p = &review.prepared;
    let mut positions = std::collections::BTreeMap::new();
    let mut counts = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    for (index, record) in p.records.iter().enumerate() {
        positions.entry(record.record.as_str()).or_insert(index);
    }
    for issue in &p.issues {
        if let Some(record) = issue.record.as_deref() {
            let count = counts.entry(record).or_default();
            match issue.severity {
                basecraft_docs::submission::Severity::Blocking => count.0 += 1,
                basecraft_docs::submission::Severity::Review => count.1 += 1,
                _ => {}
            }
        }
    }
    let rows: Vec<Value> = p
        .records
        .iter()
        .map(|r| {
            let count = counts.get(r.record.as_str()).copied().unwrap_or_default();
            let (decision, accession) = match &r.decision {
                Decision::New => ("new", r.accession.clone().unwrap_or_default()),
                Decision::Existing { accession } => ("existing", accession.clone()),
                Decision::Exclude => ("exclude", String::new()),
            };
            json!({"record": r.record, "row": r.row, "decision": decision, "accession": accession,
                "selection_reason": r.selection_reason, "accession_candidates": r.accession_candidates,
                "export_id": r.export_id, "specimen": r.specimen.clone().unwrap_or_default(),
                "organism": r.organism.clone().unwrap_or_default(),
                "research_name": r.research_name.clone().unwrap_or_default(),
                "length": r.residues.len(), "trimmed": [r.trimmed.0, r.trimmed.1],
                "feature": r.feature.as_ref().map(|f| f.kind.clone()).unwrap_or_default(),
                "frame": r.frame,
                "blocking": count.0,
                "review": count.1})
        })
        .collect();
    let issues: Vec<Value> = p
        .issues
        .iter()
        .map(|i| {
            let row = i
                .record
                .as_ref()
                .and_then(|record| positions.get(record.as_str()).copied());
            json!({"severity": i.severity.as_str(), "source": i.source, "code": i.code,
                "message": i.message, "record": i.record, "specimen": i.specimen, "row": row})
        })
        .collect();
    json!({"rows": rows, "issues": issues, "blocking": p.blocking(),
        "exported": p.exported().count(), "source_hash": review.source_hash,
        "selection_hash": review.selection_hash, "selection_confirmed": review.selection_confirmed})
}

fn qs(value: &Value) -> QString {
    QString::from(value.to_string().as_str())
}

/// The package folder of a submission document: beside it, named after it.
fn package_dir(document: &Path) -> PathBuf {
    let name = document
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let stem = name.strip_suffix(submission::SUFFIX).unwrap_or(name);
    document.with_file_name(stem)
}

impl SubmissionRust {
    fn project(&self) -> PathBuf {
        PathBuf::from(self.project_dir.to_string())
    }

    fn package_path(&self) -> Option<PathBuf> {
        let (session, doc) = (self.session.as_ref()?, self.doc?);
        Some(package_dir(
            &session.root().join(session.doc_key(doc).ok()?),
        ))
    }
}

impl qobject::SubmissionController {
    #[cfg(test)]
    #[allow(dead_code)] // Read by the real-shell bootstrap test.
    pub(crate) fn open_identity(&self) -> Option<(String, DocId, u64, u64)> {
        let session = self.rust().session.as_ref()?;
        let doc = self.rust().doc?;
        if !session.is_open(doc) {
            return None;
        }
        Some((
            session.id().into(),
            doc,
            session.generation(doc).unwrap(),
            session.revision(doc).unwrap(),
        ))
    }

    fn report(mut self: Pin<&mut Self>, kind: &str, text: &str) {
        self.as_mut().set_message_kind(QString::from(kind));
        self.set_message(QString::from(text));
    }

    fn refresh(mut self: Pin<&mut Self>) {
        if let Some(warning) =
            crate::document_open::recovery_notice(self.rust().session.as_ref(), self.rust().doc)
        {
            self.as_mut().set_message_kind(QString::from("error"));
            self.as_mut().set_message(warning);
        }
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.submission(doc));
        }
        let state = match (&self.rust().session, self.rust().doc) {
            (Some(session), Some(doc)) => Some((
                serde_json::to_value(crate::bridge::checked_document!(
                    access_thread,
                    session.submission(doc)
                ))
                .unwrap_or(Value::Null),
                authors_text(
                    &crate::bridge::checked_document!(access_thread, session.submission(doc))
                        .submission_citation()
                        .authors,
                ),
                authors_text(
                    &crate::bridge::checked_document!(access_thread, session.submission(doc))
                        .publication
                        .authors,
                ),
                crate::bridge::checked_document!(access_thread, session.doc_key(doc)).to_string(),
                session.can_undo(doc).unwrap_or(false),
                session.can_redo(doc).unwrap_or(false),
            )),
            _ => None,
        };
        if let Some((mut document, authors, publication_authors, key, undo, redo)) = state {
            document["authors_text"] = json!(authors);
            document["publication_authors_text"] = json!(publication_authors);
            self.as_mut().set_document(qs(&document));
            self.as_mut().set_path(QString::from(key.as_str()));
            self.as_mut().set_available(true);
            self.as_mut().set_can_undo(undo);
            self.as_mut().set_can_redo(redo);
        } else {
            self.as_mut().set_document(QString::from("null"));
            self.as_mut().set_path(QString::default());
            self.as_mut().set_available(false);
            self.as_mut().set_can_undo(false);
            self.as_mut().set_can_redo(false);
        }

        self.refresh_review();
    }

    /// Recompute the review: after edits, and when the specimen table or
    /// the submitter changes.
    pub fn refresh_review(mut self: Pin<&mut Self>) {
        let access_thread = self.qt_thread();
        if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
            crate::bridge::checked_document!(sync self, session.submission(doc));
        }
        self.as_mut().rust_mut().review_epoch += 1;
        if self.rust().review_active {
            return;
        }
        let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) else {
            self.as_mut()
                .set_review(qs(&json!({"rows": [], "issues": []})));
            self.as_mut().set_package(qs(&Value::Null));
            self.set_reviewing(false);
            return;
        };
        let (project, sub, package_path) = (
            session.root().to_path_buf(),
            crate::bridge::checked_document!(access_thread, session.submission(doc)).clone(),
            self.rust().package_path(),
        );
        let epoch = self.rust().review_epoch;
        let mut cache = self
            .as_mut()
            .rust_mut()
            .review_cache
            .take()
            .unwrap_or_default();
        self.as_mut().rust_mut().review_active = true;
        self.as_mut().set_reviewing(true);
        let thread = self.qt_thread();
        let progress_thread = thread.clone();
        // Reuse the shared panic/ownership-safe background continuation. The live
        // owner stays on Qt; this detached owner contains no mutable user state.
        let worker = Session::detached(&project);
        let spawned = crate::document_jobs::background_session(
            worker,
            move |_| {
                let started = std::time::Instant::now();
                let submitter = submission::submitter_preference().unwrap_or_default();
                let stage = |stage: &str| {
                    let stage = stage.to_string();
                    let _ = progress_thread.queue(move |mut this| {
                        if this.rust().review_epoch == epoch && this.rust().review_active {
                            this.as_mut()
                                .set_review_stage(QString::from(stage.as_str()));
                        }
                    });
                };
                let review = match submission::review_submission_cached_with_stage(
                    &project, &sub, &submitter, &mut cache, stage,
                ) {
                    Ok(review) => {
                        let preparation_seconds = started.elapsed().as_secs_f64();
                        stage("Formatting sequence and issue tables");
                        let conversion = std::time::Instant::now();
                        let view = review_view(&review);
                        if std::env::var_os("BASECRAFT_SUBMISSION_TIMINGS").is_some() {
                            eprintln!(
                                "submission-review prepare={preparation_seconds:.6}s conversion={:.6}s",
                                conversion.elapsed().as_secs_f64()
                            );
                        }
                        view
                    }
                    Err(error) => json!({"rows": [], "issues": [], "error": error.to_string()}),
                };
                let package = package_path.map_or(Value::Null, |dir| {
                let validation = submission::current_validation(&project, &dir).ok().flatten();
                let manifest: Value = std::fs::read(dir.join("package.json")).ok()
                    .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
                json!({"directory": dir, "exported": manifest.is_object(), "status": manifest["status"],
                    "input_hash": manifest["input_hash"], "validation": validation})
            });
                Ok((review, package, cache, started.elapsed().as_secs_f64()))
            },
            move |_, result| {
                let _ = thread.queue(move |mut this| {
                    this.as_mut().rust_mut().review_active = false;
                    let current = epoch == this.rust().review_epoch;
                    if std::env::var_os("BASECRAFT_SUBMISSION_TIMINGS").is_some() {
                        eprintln!("submission-review epoch={epoch} accepted={current}");
                    }
                    match result {
                        Ok((review, package, cache, _seconds)) => {
                            this.as_mut().rust_mut().review_cache = Some(cache);
                            if current {
                                this.as_mut().set_review(qs(&review));
                                this.as_mut().set_package(qs(&package));
                            }
                        }
                        Err(error) => {
                            if current {
                                this.as_mut().report("error", &error);
                            }
                        }
                    }
                    if current {
                        this.as_mut().set_reviewing(false);
                    } else {
                        this.as_mut().refresh_review();
                    }
                });
            },
        );
        if let Err((_, error)) = spawned {
            self.as_mut().rust_mut().review_active = false;
            self.as_mut().set_reviewing(false);
            self.report("error", &error);
        }
    }

    pub fn use_project(mut self: Pin<&mut Self>, path: &QString) {
        self.as_mut().rust_mut().open_epoch += 1;
        self.as_mut().set_busy(false);
        if *path == self.rust().project_dir {
            return;
        }
        self.as_mut().set_project_dir(path.clone());
        {
            let mut rust = self.as_mut().rust_mut();
            rust.session = None;
            rust.doc = None;
        }
        self.as_mut().report("", "");
        self.refresh();
    }

    fn session(mut self: Pin<&mut Self>) -> Result<(), String> {
        if self.rust().session.is_some() {
            return Ok(());
        }
        if self.rust().project_dir.is_empty() {
            return Err("Choose a project folder first".into());
        }
        let project = basecraft_ops::project::Project::new(self.rust().project())
            .map_err(|_| "The chosen project folder is missing".to_string())?;
        let session = Session::recoverable(project.root())
            .map_err(|e| e.to_string())?
            .in_project(Some(project));
        self.as_mut().rust_mut().session = Some(session);
        Ok(())
    }

    /// Start a submission for an alignment, saved as
    /// `submissions/<name>.submission.json` in the project.
    pub fn create(mut self: Pin<&mut Self>, alignment: &QString, name: &QString) -> bool {
        let name = name.to_string();
        let result = self.as_mut().session().and_then(|()| {
            if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
                return Err("Enter a name without slashes".into());
            }
            let mut rust = self.as_mut().rust_mut();
            let session = rust.session.as_mut().ok_or("no session")?;
            let path = Path::new("submissions").join(format!("{name}{}", submission::SUFFIX));
            submission::create(session, &path, Path::new(&alignment.to_string()))
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(doc) => {
                self.as_mut().rust_mut().doc = Some(doc);
                self.as_mut().report("ok", "Created the submission");
                self.refresh();
                true
            }
            Err(error) => {
                self.report("error", &error);
                false
            }
        }
    }

    pub fn open(mut self: Pin<&mut Self>, path: &QString) -> bool {
        if self.rust().busy {
            self.report(
                "error",
                "Wait for the running operation before replacing this document",
            );
            return false;
        }
        self.as_mut().set_busy(true);
        self.as_mut().rust_mut().open_epoch += 1;
        let epoch = self.rust().open_epoch;
        let thread = self.qt_thread();
        let Ok(project) = (!self.rust().project_dir.is_empty())
            .then(|| basecraft_ops::project::Project::new(self.rust().project()))
            .transpose()
        else {
            self.as_mut().set_busy(false);
            self.as_mut()
                .report("error", "The chosen project folder is missing");
            self.opened(false);
            return false;
        };
        let started = crate::document_open::start(path.to_string(), project, move |result| {
            let _ = thread.queue(move |mut this| {
                if this.rust().open_epoch != epoch {
                    return;
                }
                let result = result.and_then(|candidate| {
                    candidate.check(basecraft_ops::files::Kind::Submission)?;
                    let mut state = this.as_mut().rust_mut();
                    state.session = Some(candidate.session);
                    state.doc = Some(candidate.doc);
                    Ok(())
                });
                let ok = result.is_ok();
                match result {
                    Ok(()) => {
                        this.as_mut().report("", "");
                        this.as_mut().refresh();
                    }
                    Err(error) => this.as_mut().report("error", &error.to_string()),
                }
                this.as_mut().set_busy(false);
                this.opened(ok);
            });
        });
        if let Err(error) = started {
            self.as_mut().set_busy(false);
            self.as_mut().report("error", &error.to_string());
            self.opened(false);
            return false;
        }
        true
    }

    /// Apply edits as one undo step and save.
    fn apply(mut self: Pin<&mut Self>, edits: Vec<SubmissionEdit>, done: &str) -> bool {
        if self.rust().doc.is_none() || self.rust().session.is_none() {
            self.report("error", "Open or start a submission first");
            return false;
        }
        let started = std::time::Instant::now();
        let result = {
            let mut rust = self.as_mut().rust_mut();
            let Some(doc) = rust.doc else {
                return false;
            };
            let Some(session) = rust.session.as_mut() else {
                return false;
            };
            session.begin_group();
            let applied = edits
                .into_iter()
                .try_for_each(|edit| submission::edit(session, doc, edit));
            session.end_group();
            applied.map(|()| session.save(doc, None))
        };
        if std::env::var_os("BASECRAFT_SUBMISSION_TIMINGS").is_some() {
            eprintln!(
                "submission-edit persistence={:.6}s",
                started.elapsed().as_secs_f64()
            );
        }
        let ok = match result {
            Err(error) => {
                self.as_mut().report("error", &error.to_string());
                false
            }
            Ok(Err(error)) => {
                self.as_mut().save_error(&error);
                true
            }
            Ok(Ok(())) => {
                self.as_mut().report("ok", done);
                true
            }
        };
        self.refresh();
        ok
    }

    pub fn edit(self: Pin<&mut Self>, edit: &QString) -> bool {
        match serde_json::from_str::<SubmissionEdit>(&edit.to_string()) {
            Ok(edit) => {
                let done = submission::describe(&edit);
                self.apply(vec![edit], &done)
            }
            Err(error) => {
                self.report("error", &error.to_string());
                false
            }
        }
    }

    pub fn set_publication(self: Pin<&mut Self>, title: &QString, authors: &QString) -> bool {
        match parse_authors(&authors.to_string()) {
            Ok(authors) => self.apply(
                vec![SubmissionEdit::SetPublication {
                    publication: Publication {
                        title: title.to_string().trim().to_string(),
                        authors,
                    },
                }],
                "Saved the authors and title",
            ),
            Err(error) => {
                self.report("error", &error);
                false
            }
        }
    }

    pub fn preview_authors(&self, authors: &QString) -> QString {
        qs(&match parse_authors(&authors.to_string()) {
            Ok(authors) => {
                let export: Vec<_> = authors
                    .iter()
                    .map(|author| {
                        let person = basecraft_io::ncbi::Person::from(author);
                        json!({"first": person.first, "last": person.last,
                        "middle": person.middle_name(), "initials": person.initials()})
                    })
                    .collect();
                json!({"authors": authors, "export": export})
            }
            Err(error) => json!({"error": error}),
        })
    }

    pub fn set_details(self: Pin<&mut Self>, details: &QString) -> bool {
        let result = (|| -> Result<SubmissionEdit, String> {
            let value: Value =
                serde_json::from_str(&details.to_string()).map_err(|e| e.to_string())?;
            let citation = |key: &str| -> Result<Publication, String> {
                Ok(Publication {
                    title: value[key]["title"]
                        .as_str()
                        .unwrap_or_default()
                        .trim()
                        .into(),
                    authors: parse_authors(value[key]["authors"].as_str().unwrap_or_default())?,
                })
            };
            Ok(SubmissionEdit::SetDetails {
                submission: citation("submission")?,
                publication: citation("publication")?,
                publication_differs: value["publication_differs"].as_bool().unwrap_or(false),
            })
        })();
        match result {
            Ok(edit) => self.apply(vec![edit], "Saved submission details"),
            Err(error) => {
                self.report("error", &error);
                false
            }
        }
    }

    pub fn preview_inclusion(mut self: Pin<&mut Self>, rule: &QString) -> QString {
        let result = (|| -> Result<_, String> {
            let rule = serde_json::from_str(&rule.to_string()).map_err(|e| e.to_string())?;
            let session = self
                .rust()
                .session
                .as_ref()
                .ok_or("Open a submission first")?;
            let doc = self.rust().doc.ok_or("Open a submission first")?;
            submission::plan_inclusion(session, doc, &rule).map_err(|e| e.to_string())
        })();
        match result {
            Ok(plan) => qs(&json!(plan)),
            Err(error) => {
                self.as_mut().report("error", &error);
                qs(&json!({"error": error}))
            }
        }
    }

    pub fn set_decision(
        self: Pin<&mut Self>,
        record: &QString,
        decision: &QString,
        accession: &QString,
    ) -> bool {
        let decision = match decision.to_string().as_str() {
            "new" => Decision::New,
            "exclude" => Decision::Exclude,
            "existing" => Decision::Existing {
                accession: accession.to_string().trim().to_string(),
            },
            other => {
                self.report("error", &format!("{other} is not a row decision"));
                return false;
            }
        };
        self.apply(
            vec![SubmissionEdit::SetRows {
                rows: vec![RowDecision {
                    record: record.to_string(),
                    decision,
                }],
            }],
            "Saved the row decision",
        )
    }

    /// Confirm or choose a record's reading frame; 0 forgets the choice.
    pub fn set_frame(self: Pin<&mut Self>, record: &QString, codon_start: i32) -> bool {
        let record = record.to_string();
        let edit = match u8::try_from(codon_start) {
            Ok(0) => SubmissionEdit::SetFrames {
                frames: Vec::new(),
                clear: vec![record],
            },
            Ok(start) => SubmissionEdit::SetFrames {
                frames: vec![FrameChoice {
                    record,
                    codon_start: start,
                }],
                clear: Vec::new(),
            },
            Err(_) => {
                self.report("error", "codon_start must be 1, 2 or 3");
                return false;
            }
        };
        self.apply(vec![edit], "Saved the reading frame")
    }

    /// Confirm every uniquely detected frame not yet confirmed.
    pub fn confirm_detected(self: Pin<&mut Self>) -> bool {
        let review: Value =
            serde_json::from_str(&self.rust().review.to_string()).unwrap_or_default();
        let frames: Vec<FrameChoice> = review["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["frame"]["chosen"].is_null())
            .filter_map(|r| {
                Some(FrameChoice {
                    record: r["record"].as_str()?.to_string(),
                    codon_start: u8::try_from(r["frame"]["detected"].as_u64()?).ok()?,
                })
            })
            .collect();
        if frames.is_empty() {
            self.report("", "No detected frames are waiting for confirmation");
            return false;
        }
        let count = frames.len();
        self.apply(
            vec![SubmissionEdit::SetFrames {
                frames,
                clear: Vec::new(),
            }],
            &format!("Confirmed {count} detected reading frames"),
        )
    }

    pub fn undo(mut self: Pin<&mut Self>) {
        self.as_mut().history(true);
    }

    pub fn redo(mut self: Pin<&mut Self>) {
        self.as_mut().history(false);
    }

    fn history(mut self: Pin<&mut Self>, undo: bool) {
        let result = {
            let mut rust = self.as_mut().rust_mut();
            let (Some(doc), Some(session)) = (rust.doc, rust.session.as_mut()) else {
                return;
            };
            let done = if undo {
                session.undo(doc)
            } else {
                session.redo(doc)
            };
            done.and_then(|_| session.save(doc, None))
        };
        if let Err(error) = result {
            self.as_mut().save_error(&error);
        } else {
            self.as_mut()
                .report("ok", if undo { "Undone" } else { "Redone" });
        }
        self.refresh();
    }

    /// Write the package beside the submission document.
    pub fn export_package(mut self: Pin<&mut Self>) -> bool {
        let (Some(doc), Some(package)) = (self.rust().doc, self.rust().package_path()) else {
            self.report("error", "Open or start a submission first");
            return false;
        };
        let result = {
            let mut rust = self.as_mut().rust_mut();
            let Some(session) = rust.session.as_mut() else {
                return false;
            };
            submission::submitter_preference()
                .and_then(|submitter| submission::export(session, doc, &package, &submitter))
        };
        let ok = match result {
            Ok(summary) => {
                let text = match summary.status.as_str() {
                    "draft" => format!(
                        "Exported a draft to {}: {} blocking issues remain",
                        summary.directory, summary.blocking
                    ),
                    _ => format!(
                        "Exported {} records to {}",
                        summary.records, summary.directory
                    ),
                };
                self.as_mut().report("ok", &text);
                true
            }
            Err(error) => {
                self.as_mut().report("error", &error.to_string());
                false
            }
        };
        self.refresh_review();
        ok
    }

    /// The table2asn operation request for the exported package, as JSON,
    /// or empty with a message when there is none.
    pub fn validation_request(self: Pin<&mut Self>) -> QString {
        let result = match (&self.rust().session, self.rust().package_path()) {
            (Some(session), Some(package)) => {
                submission::validation_request(session.root(), &package).map_err(|e| e.to_string())
            }
            _ => Err("Open or start a submission first".into()),
        };
        match result {
            Ok(request) => {
                self.report("", "Validating with table2asn");
                qs(&request)
            }
            Err(error) => {
                self.report("error", &error);
                QString::default()
            }
        }
    }

    /// Record a finished table2asn job's outcome in the package.
    pub fn record_validation(mut self: Pin<&mut Self>, outcome: &QString) -> bool {
        let result = match (&self.rust().session, self.rust().package_path()) {
            (Some(session), Some(package)) => serde_json::from_str::<Value>(&outcome.to_string())
                .map_err(|e| e.to_string())
                .and_then(|outcome| {
                    submission::record_validation(session.root(), &package, &outcome)
                        .map_err(|e| e.to_string())
                }),
            _ => Err("Open or start a submission first".into()),
        };
        let ok = match result {
            Ok(summary) => {
                let (kind, text) = match summary.status.as_str() {
                    "passed" => (
                        "ok",
                        format!(
                            "table2asn {} reported no errors ({} warnings). Local validation only; GenBank reviews every submission",
                            summary.tool_version, summary.warnings
                        ),
                    ),
                    "errors" => (
                        "error",
                        format!(
                            "table2asn {} reported {} errors and {} warnings",
                            summary.tool_version, summary.errors, summary.warnings
                        ),
                    ),
                    _ => (
                        "error",
                        format!("Validation could not run: {}", summary.message),
                    ),
                };
                self.as_mut().report(kind, &text);
                true
            }
            Err(error) => {
                self.as_mut().report("error", &error);
                false
            }
        };
        self.refresh_review();
        ok
    }

    pub fn plan_accessions(self: Pin<&mut Self>, text: &QString) -> QString {
        let result = match (&self.rust().session, self.rust().doc) {
            (Some(session), Some(doc)) => submission::submitter_preference()
                .and_then(|submitter| {
                    submission::plan_accessions(session, doc, &text.to_string(), &submitter)
                })
                .map_err(|e| e.to_string()),
            _ => Err("Open or start a submission first".into()),
        };
        match result {
            Ok(plan) => qs(&serde_json::to_value(plan).unwrap_or_default()),
            Err(error) => {
                self.report("error", &error);
                QString::default()
            }
        }
    }

    pub fn record_accessions(self: Pin<&mut Self>, accessions: &QString) -> bool {
        match serde_json::from_str(&accessions.to_string()) {
            Ok(accessions) => self.apply(
                vec![SubmissionEdit::SetAccessions { accessions }],
                "Recorded the accessions",
            ),
            Err(error) => {
                self.report("error", &error.to_string());
                false
            }
        }
    }

    pub fn write_accession_table(self: Pin<&mut Self>, output: &QString) -> bool {
        let result = match (&self.rust().session, self.rust().doc) {
            (Some(session), Some(doc)) => submission::submitter_preference()
                .and_then(|submitter| submission::accession_table(session, doc, &submitter))
                .map_err(|e| e.to_string())
                .and_then(|table| {
                    basecraft_io::write_atomic(Path::new(&output.to_string()), |w| {
                        w.write_all(&table)
                    })
                    .map_err(|e| e.to_string())
                }),
            _ => Err("Open or start a submission first".into()),
        };
        match result {
            Ok(()) => {
                self.report("ok", "Saved the accession table");
                true
            }
            Err(error) => {
                self.report("error", &error);
                false
            }
        }
    }

    pub fn genetic_codes(&self) -> QString {
        qs(&json!(
            basecraft_docs::genetic_code::CODES
                .iter()
                .map(|c| json!({"id": c.id, "name": format!("{}: {}", c.id, c.name)}))
                .collect::<Vec<_>>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authors_round_trip_as_lines() {
        let authors = parse_authors("Doe, Jane M.\n\n  Verbruggen, Heroen\n").unwrap();
        assert_eq!(authors.len(), 2);
        assert_eq!(
            (
                authors[0].last.as_str(),
                authors[0].first.as_str(),
                authors[0].middle.as_str()
            ),
            ("Doe", "Jane", "M.")
        );
        assert_eq!(authors_text(&authors), "Doe, Jane M.\nVerbruggen, Heroen");
        assert!(parse_authors("Jane Doe").is_err());
    }

    #[test]
    fn package_folder_is_named_after_the_document() {
        assert_eq!(
            package_dir(Path::new("/p/submissions/rbcL.submission.json")),
            Path::new("/p/submissions/rbcL")
        );
    }
}

crate::editor_save::auto_save!(qobject::SubmissionController);
