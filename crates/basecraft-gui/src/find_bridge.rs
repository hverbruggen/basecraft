//! Shared presentation adapter; document controllers supply snapshots/geometry.

pub fn document_key(session: &basecraft_ops::Session, doc: basecraft_ops::DocId) -> String {
    format!(
        "{}:{}:{}",
        session.id(),
        doc.0,
        session
            .revision(doc)
            .map_or_else(|_| "closed".into(), |revision| revision.to_string())
    )
}


/// Find for a document controller. A scan runs as a job; when it finishes the
/// controller reveals the first hit and publishes the find properties.
/// `find_update(true)` starts the first scan for a query; `find_update(false)`,
/// called from `refresh`, only rescans after the document changed.
macro_rules! controller_find {
    ($controller:ty, $getter:ident, |$document:ident| $snapshot:expr) => {
        crate::find_bridge::controller_find!($controller, $getter);
        impl $controller {
            fn find_update(mut self: std::pin::Pin<&mut Self>, first: bool) {
                if !self.rust().find.active() {
                    return;
                }
                let access_thread = self.qt_thread();
                if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                }
                let scan = {
                    let mut r = self.as_mut().rust_mut();
                    let r = &mut *r;
                    let (Some(session), Some(doc)) = (&r.session, r.doc) else {
                        return;
                    };
                    let key = crate::find_bridge::document_key(session, doc);
                    if !first && !r.find.stale(&key) {
                        return;
                    }
                    let $document =
                        crate::bridge::checked_document!(access_thread, session.$getter(doc));
                    r.find.update(key, || $snapshot)
                };
                self.find_start(scan);
            }
        }
    };
    ($controller:ty $(, $getter:ident)?) => {
        impl $controller {
            pub fn find_query(
                mut self: std::pin::Pin<&mut Self>,
                query: &cxx_qt_lib::QString,
                reverse: bool,
            ) {
                $(if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                })?
                self.as_mut()
                    .rust_mut()
                    .find
                    .set_query(query.to_string(), reverse);
                self.as_mut().refresh();
                self.find_publish(false);
            }
            /// Move `direction` hits (0 stays), starting the query's scan if
            /// it has not started.
            pub fn find_step(mut self: std::pin::Pin<&mut Self>, direction: i32) {
                $(if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                })?
                self.as_mut().find_update(true);
                if direction != 0 {
                    self.as_mut().rust_mut().find.step(direction < 0);
                    self.as_mut().find_reveal();
                    self.as_mut().refresh();
                }
                self.find_publish(direction != 0);
            }
            /// Show the find state; `reveal` asks the view to scroll to the
            /// current hit.
            fn find_publish(mut self: std::pin::Pin<&mut Self>, reveal: bool) {
                let find = &self.rust().find;
                let text = basecraft_viewmodel::find::status_text(find);
                let rows = basecraft_viewmodel::find::name_rows(find);
                let count = i32::try_from(find.results.hits.len()).unwrap_or(i32::MAX);
                let row = find
                    .hit()
                    .map_or(-1, |hit| i32::try_from(hit.row).unwrap_or(i32::MAX));
                let nucleotide = find.results.nucleotide;
                let y = f64::from(self.find_y());
                self.as_mut().set_find_status(cxx_qt_lib::QString::from(&text));
                self.as_mut().set_find_count(count);
                self.as_mut().set_find_row(row);
                self.as_mut().set_find_hit_y(y);
                self.as_mut().set_find_nucleotide(nucleotide);
                self.as_mut().set_find_rows(cxx_qt_lib::QString::from(&rows));
                if reveal && count > 0 {
                    let next = self.find_reveals().wrapping_add(1);
                    self.set_find_reveals(next);
                }
            }
            fn find_start(
                mut self: std::pin::Pin<&mut Self>,
                scan: Option<basecraft_viewmodel::find::Scan>,
            ) {
                let Some(scan) = scan else {
                    return;
                };
                let token = scan.token();
                let started = crate::jobs::start(
                    self.qt_thread(),
                    move |_| Ok(scan.run()),
                    move |mut this, found| {
                        let completed = {
                            let mut r = this.as_mut().rust_mut();
                            match found {
                                Ok(found) => r.find.finish(found),
                                Err(error) => {
                                    r.find.failed(token, error);
                                    false
                                }
                            }
                        };
                        if completed {
                            this.as_mut().find_reveal();
                        }
                        this.as_mut().refresh();
                        this.find_publish(completed);
                    },
                );
                if let Err(error) = started {
                    self.as_mut()
                        .rust_mut()
                        .find
                        .failed(token, format!("Cannot start Find: {error}"));
                }
                self.find_publish(false);
            }
        }
    };
}
pub(crate) use controller_find;
