//! What the five editor bridges share: their state and the methods that are
//! the same in each. The Qt declarations stay in each bridge, because
//! cxx-qt reads them from the source; these macros supply the Rust side.

/// The Rust state of one editor bridge, with its `Default`. Every editor gets
/// the find, status, message and file fields. `drawn` adds the frame, theme
/// colours and overview of an editor that paints its document; `documents`
/// adds the ops session and the open document. The bridge lists its own
/// fields in the braces as `name: Type = default,`, each ending in a comma.
/// They come before the shared ones, so they are dropped first: a running
/// job is cancelled before the session goes.
macro_rules! editor_state {
    ($name:ident [drawn $($flag:ident)*] { $($rest:tt)* }) => {
        crate::editor::editor_state!($name [$($flag)*] {
            $($rest)*
            frame: cxx_qt_lib::QByteArray = cxx_qt_lib::QByteArray::default(),
            ok_color: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            error_color: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            background: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            foreground: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            accent: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            mono_font: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            zoom_text: cxx_qt_lib::QString = cxx_qt_lib::QString::default(),
            overview_content: cxx_qt_lib::QString = cxx_qt_lib::QString::from("{\"shapes\":[]}"),
            overview_matches: cxx_qt_lib::QString = cxx_qt_lib::QString::from("[]"),
            overview_cache: basecraft_viewmodel::overview::Cache = Default::default(),
            overview_match_cache: basecraft_viewmodel::overview::Cache = Default::default(),
            theme: basecraft_viewmodel::Theme = basecraft_viewmodel::Theme::default(),
            scratch: basecraft_viewmodel::Frame = basecraft_viewmodel::Frame::new(),
        });
    };
    ($name:ident [documents $($flag:ident)*] { $($rest:tt)* }) => {
        crate::editor::editor_state!($name [$($flag)*] {
            $($rest)*
            session: Option<basecraft_ops::Session> = None,
            doc: Option<basecraft_ops::DocId> = None,
        });
    };
    ($name:ident [] { $($(#[$meta:meta])* $field:ident: $ty:ty = $default:expr),* $(,)? }) => {
        #[allow(clippy::struct_excessive_bools)]
        pub struct $name {
            find: basecraft_viewmodel::find::Session,
            find_status: cxx_qt_lib::QString,
            find_count: i32,
            find_row: i32,
            find_hit_y: f64,
            find_nucleotide: bool,
            find_rows: cxx_qt_lib::QString,
            find_reveals: i32,
            history_revision: cxx_qt_lib::QString,
            status: cxx_qt_lib::QString,
            message: cxx_qt_lib::QString,
            /// "ok", "error", or "" for a neutral hint.
            message_kind: cxx_qt_lib::QString,
            file_path: cxx_qt_lib::QString,
            dirty: bool,
            busy: bool,
            /// The copy offer for an edit a read-only folder refused (D13).
            refused_edit: cxx_qt_lib::QString,
            $($(#[$meta])* $field: $ty,)*
        }
        impl Default for $name {
            fn default() -> Self {
                Self {
                    find: basecraft_viewmodel::find::Session::default(),
                    find_status: cxx_qt_lib::QString::default(),
                    find_count: 0,
                    find_row: -1,
                    find_hit_y: 0.0,
                    find_nucleotide: false,
                    find_rows: cxx_qt_lib::QString::from("[]"),
                    find_reveals: 0,
                    history_revision: cxx_qt_lib::QString::default(),
                    status: cxx_qt_lib::QString::default(),
                    message: cxx_qt_lib::QString::default(),
                    message_kind: cxx_qt_lib::QString::default(),
                    file_path: cxx_qt_lib::QString::default(),
                    dirty: false,
                    busy: false,
                    refused_edit: cxx_qt_lib::QString::default(),
                    $($field: $default,)*
                }
            }
        }
    };
}
pub(crate) use editor_state;
