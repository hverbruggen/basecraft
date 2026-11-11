//! Shared presentation adapter; document controllers supply snapshots/geometry.
use basecraft_viewmodel::find::{Row, Session};

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

pub fn rows_key(rows: &[Row], identity: &str) -> String {
    let mut bytes = identity.as_bytes().to_vec();
    for row in rows {
        bytes.extend_from_slice(&row.id.to_le_bytes());
        bytes.extend_from_slice(&row.name.len().to_le_bytes());
        bytes.extend_from_slice(row.name.as_bytes());
        bytes.extend_from_slice(&row.bases.len().to_le_bytes());
        bytes.extend_from_slice(&row.bases);
    }
    basecraft_io::hash_bytes(&bytes)
}

pub fn status(find: &Session, y: f32) -> String {
    let hit = find.hit();
    serde_json::json!({
        "busy":find.busy(), "count":find.results.hits.len(),
        "current":hit.map_or(0, |_| find.current+1),
        "label":hit.map_or("",|h|h.label.as_ref()),
        "row":hit.map(|h|h.row), "start":hit.filter(|h|h.kind != basecraft_viewmodel::find::Kind::Name).map(|h|h.start+1),
        "reverse":hit.is_some_and(|h|h.reverse), "y":y,
        "nucleotide":find.results.nucleotide,
        "limited":find.results.limited, "explanation":find.results.explanation,
        // Text-only species rows use the same matching results as canvases.
        "rows":find.results.hits.iter().filter(|h|h.kind == basecraft_viewmodel::find::Kind::Name).map(|h|h.row).collect::<Vec<_>>()
    }).to_string()
}

/// Find for a document controller. A scan runs as a job; when it finishes the
/// controller reveals the first hit and sets `find_status`.
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
                self.refresh();
            }
            /// The current status after moving `direction` hits (0 stays),
            /// starting the query's scan if it has not started.
            pub fn find_step(
                mut self: std::pin::Pin<&mut Self>,
                direction: i32,
            ) -> cxx_qt_lib::QString {
                $(if let (Some(session), Some(doc)) = (&self.rust().session, self.rust().doc) {
                    crate::bridge::checked_document!(sync self, session.$getter(doc));
                })?
                self.as_mut().find_update(true);
                if direction != 0 {
                    self.as_mut().rust_mut().find.step(direction < 0);
                    self.as_mut().find_reveal();
                    self.as_mut().refresh();
                }
                cxx_qt_lib::QString::from(&crate::find_bridge::status(
                    &self.rust().find,
                    self.find_y(),
                ))
            }
            fn find_start(
                mut self: std::pin::Pin<&mut Self>,
                scan: Option<basecraft_viewmodel::find::Scan>,
            ) {
                let Some(scan) = scan else {
                    return;
                };
                let started = crate::jobs::start(
                    self.qt_thread(),
                    move |_| Ok(scan.run()),
                    |mut this, found| {
                        let completed = {
                            let mut r = this.as_mut().rust_mut();
                            match found {
                                Ok(found) => r.find.finish(found),
                                Err(error) => {
                                    r.find.failed(error);
                                    false
                                }
                            }
                        };
                        if completed {
                            this.as_mut().find_reveal();
                        }
                        this.as_mut().refresh();
                        let status = crate::find_bridge::status(&this.rust().find, this.find_y());
                        this.set_find_status(cxx_qt_lib::QString::from(&status));
                    },
                );
                if let Err(error) = started {
                    self.as_mut()
                        .rust_mut()
                        .find
                        .failed(format!("Cannot start Find: {error}"));
                }
                let status = crate::find_bridge::status(&self.rust().find, self.find_y());
                self.set_find_status(cxx_qt_lib::QString::from(&status));
            }
        }
    };
}
pub(crate) use controller_find;

/// Keep miniature payloads bounded while always including the current result.
pub fn overview_marks(
    find: &Session,
    mut project: impl FnMut(
        &basecraft_viewmodel::find::Hit,
    ) -> Option<basecraft_viewmodel::overview::MatchMark>,
) -> String {
    if !find.active() {
        return "[]".into();
    }
    let stride = find.results.hits.len().div_ceil(4096).max(1);
    let marks: Vec<_> = find
        .results
        .hits
        .iter()
        .step_by(stride)
        .chain(find.hit())
        .filter_map(&mut project)
        .collect();
    serde_json::to_string(&marks).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod overview_tests {
    use super::*;
    use basecraft_viewmodel::{
        find::{Hit, Kind},
        overview::MatchMark,
    };

    #[test]
    fn find_overview_bounds_payload_and_keeps_current_hit() {
        let mut find = Session::default();
        find.set_query("A".into(), false);
        find.results.hits = (0..100_000)
            .map(|i| Hit {
                row: i,
                start: 1,
                end: 2,
                kind: Kind::Sequence,
                reverse: false,
                label: "row".into(),
            })
            .collect();
        find.current = 99_999;
        let json = overview_marks(&find, |hit| {
            Some(MatchMark {
                x: 0.0,
                y: f64::from(u32::try_from(hit.row).unwrap()) / 100_000.0,
                width: 1.0,
                height: 0.00001,
            })
        });
        let marks: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert!(marks.len() <= 4097);
        assert_eq!(marks.last().unwrap()["y"], serde_json::json!(0.99999));
        find.set_query(String::new(), false);
        assert_eq!(
            overview_marks(&find, |_| panic!("closed search must not project hits")),
            "[]"
        );
    }
}
