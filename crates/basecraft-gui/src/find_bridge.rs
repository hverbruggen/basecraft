//! Shared presentation adapter; document controllers supply snapshots/geometry.
use basecraft_viewmodel::find::{Row, Session};

pub fn document_key(session: &basecraft_ops::Session, doc: basecraft_ops::DocId) -> String {
    format!("{}:{}:{}", session.id(), doc.0, session.revision(doc))
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

macro_rules! controller_find {
    ($controller:ty) => {
        impl $controller {
            pub fn find_query(
                mut self: std::pin::Pin<&mut Self>,
                query: &cxx_qt_lib::QString,
                reverse: bool,
            ) {
                self.as_mut()
                    .rust_mut()
                    .find
                    .set_query(query.to_string(), reverse);
                self.refresh();
            }
            pub fn find_poll(
                mut self: std::pin::Pin<&mut Self>,
                direction: i32,
            ) -> cxx_qt_lib::QString {
                self.as_mut().find_update();
                let reveal = {
                    let mut r = self.as_mut().rust_mut();
                    let completed = r.find.poll();
                    // A pending scan's first Enter still reveals its first hit.
                    if direction != 0 && !completed {
                        r.find.step(direction < 0);
                    }
                    completed || direction != 0
                };
                if reveal {
                    self.as_mut().find_reveal();
                    self.as_mut().refresh();
                }
                let revision = self.rust().find.revision();
                if direction == 0 && self.rust().find_status_revision == revision {
                    return cxx_qt_lib::QString::default();
                }
                self.as_mut().rust_mut().find_status_revision = revision;
                cxx_qt_lib::QString::from(&crate::find_bridge::status(
                    &self.rust().find,
                    self.find_y(),
                ))
            }
        }
    };
}
pub(crate) use controller_find;
