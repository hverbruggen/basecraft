//! The window's open documents: one row per tab, shown by QML as a list
//! model, with the open, close, quit and cross-document decisions made in
//! `basecraft_viewmodel::workspace`. Also the transport for opening a
//! sequence, alignment or tree on a worker into a new tab.
use crate::document_open::{Candidate, OpenError, Opens, Phase};
use basecraft_ops::files::Kind;
use basecraft_viewmodel::workspace::{self, Close, Delimit, Open, Quit, Save, Target, Workspace};
use cxx_qt::{CxxQtType, Threading, casting::Downcast};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QVariant,
};
use std::pin::Pin;

#[cxx_qt::bridge]
pub mod qobject {
    impl cxx_qt::Threading for DocumentOpens {}
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!(<QtCore/QObject>);
        #[cxx_name = "QObject"]
        type OpenObject = cxx_qt::QObject;
        include!(<QtCore/QAbstractListModel>);
        type QAbstractListModel;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count)]
        type DocumentOpens = super::OpenRust;

        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut DocumentOpens>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut DocumentOpens>);
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut DocumentOpens>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut DocumentOpens>);
        #[inherit]
        fn index(self: &DocumentOpens, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut DocumentOpens>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QList_i32,
        );
        #[cxx_override]
        fn data(self: &DocumentOpens, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &DocumentOpens, parent: &QModelIndex) -> i32;
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &DocumentOpens) -> QHash_i32_QByteArray;

        #[qinvokable]
        fn open(self: Pin<&mut DocumentOpens>, path: &QString, review: bool) -> QString;
        #[qinvokable]
        fn add_tab(self: Pin<&mut DocumentOpens>, path: &QString, kind: &QString) -> i32;
        #[qinvokable]
        fn save_step(self: &DocumentOpens, index: i32, choose_name: bool) -> QString;
        #[qinvokable]
        fn species_request_allowed(
            self: &DocumentOpens,
            kind: &QString,
            busy: bool,
            method: &QString,
            linked: bool,
        ) -> bool;
        #[qinvokable]
        fn new_sequence(self: Pin<&mut DocumentOpens>, name: &QString) -> i32;
        #[qinvokable]
        fn set_working(self: Pin<&mut DocumentOpens>, index: i32, working: bool);
        #[qinvokable]
        fn trace_review(self: &DocumentOpens, path: &QString, new_tab: bool) -> i32;
        #[qinvokable]
        fn remove_tab(self: Pin<&mut DocumentOpens>, index: i32);
        #[qinvokable]
        fn index_of(self: &DocumentOpens, token: &QString) -> i32;
        #[qinvokable]
        fn target_index(self: &DocumentOpens, token: &QString, path: &QString) -> i32;
        #[qinvokable]
        fn path_at(self: &DocumentOpens, index: i32) -> QString;
        #[qinvokable]
        fn token_at(self: &DocumentOpens, index: i32) -> QString;
        #[qinvokable]
        fn set_dirty(self: Pin<&mut DocumentOpens>, index: i32, dirty: bool);
        #[qinvokable]
        fn close(self: &DocumentOpens, index: i32) -> QString;
        #[qinvokable]
        fn quit(self: Pin<&mut DocumentOpens>, drafts: bool) -> QString;
        /// D13: copy the file `refused` names to `target` and keep its
        /// refused edit for when the copy opens. "" or why it failed.
        #[qinvokable]
        fn copy_for_editing(self: &DocumentOpens, refused: &QString, target: &QString) -> QString;
        #[qinvokable]
        fn switch_project(
            self: Pin<&mut DocumentOpens>,
            drafts: bool,
            folder: &QString,
            current: &QString,
        ) -> QString;
        #[qinvokable]
        fn discard_all(self: &DocumentOpens, consent: &QString) -> QString;
        #[qinvokable]
        fn save_all(self: Pin<&mut DocumentOpens>, consent: &QString) -> QString;
        #[qinvokable]
        fn quit_saved(self: Pin<&mut DocumentOpens>, success: bool) -> QString;
        #[qinvokable]
        fn begin_delimitation(self: Pin<&mut DocumentOpens>, index: i32);
        #[qinvokable]
        fn request_delimitation(
            self: Pin<&mut DocumentOpens>,
            source_path: &QString,
            busy: bool,
            analysis: &QString,
        ) -> QString;
        #[qinvokable]
        fn creation_refused(self: Pin<&mut DocumentOpens>);
        #[qinvokable]
        fn workspace_created(
            self: Pin<&mut DocumentOpens>,
            source_path: &QString,
            path: &QString,
        ) -> QString;
        #[qinvokable]
        unsafe fn link_taxa(
            self: &DocumentOpens,
            source: *mut OpenObject,
            target: *mut OpenObject,
            names: &QString,
        );
        #[qinvokable]
        fn request(self: Pin<&mut DocumentOpens>, path: &QString, review: bool) -> QString;
        #[qinvokable]
        fn start(self: Pin<&mut DocumentOpens>, token: &QString);
        #[qinvokable]
        unsafe fn install(
            self: Pin<&mut DocumentOpens>,
            token: &QString,
            controller: *mut OpenObject,
        ) -> bool;
        #[qinvokable]
        unsafe fn acknowledge(
            self: Pin<&mut DocumentOpens>,
            token: &QString,
            controller: *mut OpenObject,
        ) -> bool;
        #[qinvokable]
        fn activate(self: Pin<&mut DocumentOpens>, token: &QString);
        #[qinvokable]
        fn path_changed(
            self: Pin<&mut DocumentOpens>,
            token: &QString,
            path: &QString,
            installed_path: &QString,
        );
        #[qinvokable]
        fn retire(self: Pin<&mut DocumentOpens>, token: &QString);
        #[qinvokable]
        fn revoke_focus(self: Pin<&mut DocumentOpens>);
        #[qinvokable]
        fn shutdown(self: Pin<&mut DocumentOpens>);
        #[qsignal]
        fn acquired(self: Pin<&mut DocumentOpens>, token: QString, kind: QString);
        #[qsignal]
        fn failed(self: Pin<&mut DocumentOpens>, token: QString, message: QString);
        #[qsignal]
        fn activated(
            self: Pin<&mut DocumentOpens>,
            token: QString,
            review: bool,
            installed_path: QString,
        );
    }
}
#[derive(Default)]
pub struct OpenRust {
    opens: Opens,
    documents: Workspace,
    count: i32,
}

/// Model roles; QML delegates read them as required properties.
const PATH: i32 = 0x0100;
const KIND: i32 = 0x0101;
const DIRTY: i32 = 0x0102;
const TOKEN: i32 = 0x0103;

fn json(value: &serde_json::Value) -> QString {
    QString::from(&value.to_string())
}
fn target_json(target: &Target) -> serde_json::Value {
    serde_json::json!({"token": target.token.to_string(), "path": target.path})
}
fn targets(consent: &QString) -> Vec<Target> {
    serde_json::from_str::<Vec<serde_json::Value>>(&consent.to_string())
        .unwrap_or_default()
        .iter()
        .filter_map(|t| {
            Some(Target {
                token: t["token"].as_str()?.parse().ok()?,
                path: t["path"].as_str()?.to_owned(),
            })
        })
        .collect()
}
/// A quit step for QML: `drafts`, `ask` (with `error`), `save`, `stay`,
/// `choose`, `refuse` or `leave`. `drafts` and `leave` carry `project` during a
/// project switch.
fn quit_json(step: &Quit) -> QString {
    json(&match step {
        Quit::Drafts { project } => serde_json::json!({"drafts": true, "project": project}),
        Quit::Ask { targets, error } => serde_json::json!({
            "ask": targets.iter().map(target_json).collect::<Vec<_>>(), "error": error
        }),
        Quit::Save(target) => serde_json::json!({"save": target_json(target)}),
        Quit::Stay => serde_json::json!({"stay": true}),
        Quit::Choose(project) => serde_json::json!({"choose": project}),
        Quit::Refuse(error) => serde_json::json!({"refuse": error}),
        Quit::Leave { drafts, project } => serde_json::json!({
            "leave": drafts.iter().map(u64::to_string).collect::<Vec<_>>(), "project": project
        }),
    })
}
fn row(index: i32) -> Option<usize> {
    usize::try_from(index).ok()
}
fn as_row(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX)
}

impl qobject::DocumentOpens {
    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(d) = row(index.row()).and_then(|i| self.rust().documents.documents().get(i))
        else {
            return QVariant::default();
        };
        match role {
            PATH => QVariant::from(&QString::from(&d.path)),
            KIND => QVariant::from(&QString::from(d.kind.name())),
            DIRTY => QVariant::from(&d.dirty),
            TOKEN => QVariant::from(&QString::from(&d.token.to_string())),
            _ => QVariant::default(),
        }
    }
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        as_row(self.rust().documents.documents().len())
    }
    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (role, name) in [
            (PATH, "path"),
            (KIND, "kind"),
            (DIRTY, "dirty"),
            (TOKEN, "openToken"),
        ] {
            roles.insert(role, QByteArray::from(name));
        }
        roles
    }
    /// Append a tab row for `token`; returns its index.
    fn insert_row(
        mut self: Pin<&mut Self>,
        token: u64,
        path: String,
        kind: workspace::Kind,
    ) -> i32 {
        let at = as_row(self.rust().documents.documents().len());
        // SAFETY: rows are appended between the matching begin and end calls.
        unsafe {
            self.as_mut()
                .begin_insert_rows(&QModelIndex::default(), at, at);
        };
        self.as_mut().rust_mut().documents.add(token, path, kind);
        unsafe { self.as_mut().end_insert_rows() };
        self.set_count(at + 1);
        at
    }
    fn changed(mut self: Pin<&mut Self>, index: usize, role: i32) {
        let at = self.index(as_row(index), 0, &QModelIndex::default());
        let mut roles = QList::<i32>::default();
        roles.append(role);
        self.as_mut().data_changed(&at, &at, &roles);
    }

    /// Open `path`: switch to its tab, open it on a worker in a new tab, add a
    /// tab for the views that open themselves, or name the project dialog.
    fn open(mut self: Pin<&mut Self>, path: &QString, review: bool) -> QString {
        let text = path.to_string();
        let worker = crate::document_open::eligible(&text);
        let kind = if worker {
            "canvas"
        } else {
            crate::bridge::file_kind(std::path::Path::new(&text))
        };
        let decision = self.rust().documents.open(&text, kind);
        if !worker {
            return match decision {
                Open::Project => json(&serde_json::json!({"project": kind})),
                Open::Switch(index) => {
                    json(&serde_json::json!({"switch": index, "review": review}))
                }
                Open::New => {
                    let index = self.as_mut().add_tab(path, &QString::from(kind));
                    json(&serde_json::json!({"new": index, "review": review}))
                }
            };
        }
        // The worker's request carries the review flag and focus either way.
        let token = self.as_mut().request(path, review);
        let Some(id) = self::token(&token) else {
            return json(&serde_json::json!({"error": "Could not allocate an open request"}));
        };
        match decision {
            Open::Switch(index) => json(
                &serde_json::json!({"switch": index, "token": token.to_string(), "activate": true}),
            ),
            Open::New | Open::Project => {
                let index = self.as_mut().insert_row(id, text, workspace::Kind::Canvas);
                self.as_mut().start(&token);
                json(&serde_json::json!({"new": index}))
            }
        }
    }
    /// "destination" (ask for a file first), "write" or "trace" (save the
    /// review through its own dialog) for saving tab `index`.
    fn save_step(&self, index: i32, choose_name: bool) -> QString {
        QString::from(
            match row(index).and_then(|i| self.rust().documents.save(i, choose_name)) {
                Some(Save::Destination) => "destination",
                Some(Save::Write) => "write",
                Some(Save::TraceReview) => "trace",
                None => "",
            },
        )
    }
    fn species_request_allowed(
        &self,
        kind: &QString,
        busy: bool,
        method: &QString,
        linked: bool,
    ) -> bool {
        workspace::species_request_allowed(&kind.to_string(), busy, &method.to_string(), linked)
    }
    fn set_working(mut self: Pin<&mut Self>, index: i32, working: bool) {
        if let Some(i) = row(index) {
            self.as_mut().rust_mut().documents.set_working(i, working);
        }
    }
    /// The tab to switch to (its index), -1 to start in a new tab, or -2 to
    /// open the read the usual way.
    fn trace_review(&self, path: &QString, new_tab: bool) -> i32 {
        match self
            .rust()
            .documents
            .trace_review(&path.to_string(), new_tab)
        {
            workspace::TraceReview::Switch(index) => as_row(index),
            workspace::TraceReview::NewTab => -1,
            workspace::TraceReview::Open => -2,
        }
    }
    /// A tab for a new, never-saved sequence called `name`; returns its index.
    fn new_sequence(mut self: Pin<&mut Self>, name: &QString) -> i32 {
        let path = self
            .as_mut()
            .rust_mut()
            .documents
            .untitled_path(&name.to_string());
        self.add_tab(&QString::from(&path), &QString::from("sequence"))
    }
    /// A tab whose view opens its own file; returns its index, or -1.
    fn add_tab(mut self: Pin<&mut Self>, path: &QString, kind: &QString) -> i32 {
        match self.as_mut().rust_mut().opens.track(path.to_string()) {
            Ok(id) => self.insert_row(
                id,
                path.to_string(),
                workspace::Kind::parse(&kind.to_string()),
            ),
            Err(_) => -1,
        }
    }
    fn remove_tab(mut self: Pin<&mut Self>, index: i32) {
        let Some(token) = row(index)
            .and_then(|i| self.rust().documents.documents().get(i))
            .map(|d| d.token)
        else {
            return;
        };
        // SAFETY: the row is removed between the matching begin and end calls.
        unsafe {
            self.as_mut()
                .begin_remove_rows(&QModelIndex::default(), index, index);
        };
        self.as_mut().rust_mut().documents.remove(token);
        unsafe { self.as_mut().end_remove_rows() };
        self.as_mut().rust_mut().opens.retire(token);
        let count = as_row(self.rust().documents.documents().len());
        self.set_count(count);
    }
    fn index_of(&self, token: &QString) -> i32 {
        self::token(token)
            .and_then(|id| self.rust().documents.index(id))
            .map_or(-1, as_row)
    }
    fn target_index(&self, token: &QString, path: &QString) -> i32 {
        let Some(token) = self::token(token) else {
            return -1;
        };
        self.rust()
            .documents
            .target_index(&Target {
                token,
                path: path.to_string(),
            })
            .map_or(-1, as_row)
    }
    fn path_at(&self, index: i32) -> QString {
        row(index)
            .and_then(|i| self.rust().documents.documents().get(i))
            .map_or_else(QString::default, |d| QString::from(&d.path))
    }
    fn token_at(&self, index: i32) -> QString {
        row(index)
            .and_then(|i| self.rust().documents.documents().get(i))
            .map_or_else(QString::default, |d| QString::from(&d.token.to_string()))
    }
    fn set_dirty(mut self: Pin<&mut Self>, index: i32, dirty: bool) {
        if let Some(i) = row(index)
            && self.as_mut().rust_mut().documents.set_dirty(i, dirty)
        {
            self.changed(i, DIRTY);
        }
    }
    /// The tab now shows `path` (Save As, or a review saved to a file).
    fn renamed(mut self: Pin<&mut Self>, id: u64, path: &str) {
        if let Some(i) = self
            .as_mut()
            .rust_mut()
            .documents
            .set_path(id, path.to_owned())
        {
            self.changed(i, PATH);
        }
    }
    /// `{"ask": target}` when the tab has unsaved changes, else
    /// `{"draft": bool}`: close it, deleting an empty species workspace.
    fn close(&self, index: i32) -> QString {
        match row(index).and_then(|i| self.rust().documents.close(i)) {
            Some(Close::Ask(target)) => json(&serde_json::json!({"ask": target_json(&target)})),
            Some(Close::Remove { draft }) => json(&serde_json::json!({"draft": draft})),
            None => json(&serde_json::json!({})),
        }
    }
    fn quit(mut self: Pin<&mut Self>, drafts: bool) -> QString {
        quit_json(&self.as_mut().rust_mut().documents.quit(drafts))
    }
    fn copy_for_editing(&self, refused: &QString, target: &QString) -> QString {
        let target = std::path::PathBuf::from(target.to_string());
        crate::document_open::copy_for_editing(&refused.to_string(), &target)
            .err()
            .map_or_else(QString::default, |error| QString::from(&error))
    }
    /// A running listed job, from a tab or not, refuses the switch first.
    fn switch_project(
        mut self: Pin<&mut Self>,
        drafts: bool,
        folder: &QString,
        current: &QString,
    ) -> QString {
        quit_json(&self.as_mut().rust_mut().documents.switch_project(
            drafts,
            crate::jobs::working(),
            folder.to_string(),
            &current.to_string(),
        ))
    }
    fn discard_all(&self, consent: &QString) -> QString {
        quit_json(&self.rust().documents.discard_all(&targets(consent)))
    }
    fn save_all(mut self: Pin<&mut Self>, consent: &QString) -> QString {
        quit_json(
            &self
                .as_mut()
                .rust_mut()
                .documents
                .save_all(targets(consent)),
        )
    }
    fn quit_saved(mut self: Pin<&mut Self>, success: bool) -> QString {
        quit_json(&self.as_mut().rust_mut().documents.quit_saved(success))
    }
    fn begin_delimitation(mut self: Pin<&mut Self>, index: i32) {
        if let Some(i) = row(index) {
            self.as_mut().rust_mut().documents.begin_delimitation(i);
        }
    }
    /// "create" (ask the source to create a workspace), the token of the
    /// workspace tab to run the analysis in, or "" to do nothing.
    fn request_delimitation(
        mut self: Pin<&mut Self>,
        source_path: &QString,
        busy: bool,
        analysis: &QString,
    ) -> QString {
        let Ok(analysis) = serde_json::from_str(&analysis.to_string()) else {
            return QString::default();
        };
        match self.as_mut().rust_mut().documents.request_delimitation(
            &source_path.to_string(),
            busy,
            analysis,
        ) {
            Delimit::Create => QString::from("create"),
            Delimit::InWorkspace(token) => QString::from(&token.to_string()),
            Delimit::Refuse => QString::default(),
        }
    }
    fn creation_refused(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().documents.creation_refused();
    }
    /// Open the new workspace in a tab and return the analysis to start in
    /// it, with the tab's token; "" when nothing should open.
    fn workspace_created(
        mut self: Pin<&mut Self>,
        source_path: &QString,
        path: &QString,
    ) -> QString {
        let Some(analysis) = self
            .as_mut()
            .rust_mut()
            .documents
            .workspace_created(&source_path.to_string(), &path.to_string())
        else {
            return QString::default();
        };
        let index = self.as_mut().add_tab(path, &QString::from("delimitation"));
        let Some(token) = row(index)
            .and_then(|i| self.rust().documents.documents().get(i))
            .map(|d| d.token)
        else {
            return QString::default();
        };
        self.as_mut().rust_mut().documents.mark_draft(token);
        let mut reply = serde_json::to_value(&analysis).unwrap_or_default();
        reply["token"] = serde_json::json!(token.to_string());
        json(&reply)
    }
}
fn token(value: &QString) -> Option<u64> {
    value.to_string().parse().ok()
}
// Owned, transient descriptions: no QObject borrow survives receiver dispatch.
struct TaxaEndpoint {
    kind: Kind,
    path: String,
    linked_alignment: String,
}
impl TaxaEndpoint {
    fn describe(object: &cxx_qt::QObject) -> Option<Self> {
        if let Some(ctl) =
            object.downcast::<crate::alignment_bridge::qobject::AlignmentController>()
        {
            Some(Self {
                kind: Kind::Alignment,
                path: ctl.file_path().to_string(),
                linked_alignment: String::new(),
            })
        } else if let Some(ctl) = object.downcast::<crate::tree_bridge::qobject::TreeController>() {
            Some(Self {
                kind: Kind::Tree,
                path: ctl.file_path().to_string(),
                linked_alignment: ctl.linked_alignment().to_string(),
            })
        } else {
            object
                .downcast::<crate::delimitation_bridge::qobject::DelimitationController>()
                .map(|ctl| Self {
                    kind: Kind::Delimitation,
                    path: ctl.file_path().to_string(),
                    linked_alignment: ctl.linked_alignment().to_string(),
                })
        }
    }
}
fn linked_taxa_target(source: &TaxaEndpoint, target: &TaxaEndpoint) -> bool {
    match source.kind {
        Kind::Alignment => {
            !source.path.is_empty()
                && matches!(target.kind, Kind::Tree | Kind::Delimitation)
                && target.linked_alignment == source.path
        }
        Kind::Tree | Kind::Delimitation => {
            !source.linked_alignment.is_empty()
                && target.kind == Kind::Alignment
                && target.path == source.linked_alignment
        }
        _ => false,
    }
}
impl qobject::DocumentOpens {
    unsafe fn link_taxa(
        &self,
        source: *mut cxx_qt::QObject,
        target: *mut cxx_qt::QObject,
        names: &QString,
    ) {
        if source.is_null() || target.is_null() || source == target {
            return;
        }
        // SAFETY: QML supplies live objects for this synchronous call. Copy only
        // endpoint values, then release every object borrow before highlight_taxa
        // can refresh and reenter QML. No pointer is retained by this adapter.
        let endpoints =
            unsafe { TaxaEndpoint::describe(&*source).zip(TaxaEndpoint::describe(&*target)) };
        let Some((source, destination)) = endpoints else {
            return;
        };
        if !linked_taxa_target(&source, &destination) {
            return;
        }
        // SAFETY: self exclusion above and owned descriptors ensure no source or
        // target reference aliases this pinned receiver. Qt owns its lifetime.
        let mut object = unsafe { Pin::new_unchecked(&mut *target) };
        if let Some(ctl) = object
            .as_mut()
            .downcast_pin::<crate::alignment_bridge::qobject::AlignmentController>()
        {
            ctl.highlight_taxa(names);
        } else if let Some(ctl) = object
            .as_mut()
            .downcast_pin::<crate::tree_bridge::qobject::TreeController>()
        {
            ctl.highlight_taxa(names);
        } else if let Some(ctl) = object
            .as_mut()
            .downcast_pin::<crate::delimitation_bridge::qobject::DelimitationController>(
        ) {
            ctl.highlight_taxa(names);
        }
    }

    fn path_changed(
        mut self: Pin<&mut Self>,
        value: &QString,
        path: &QString,
        installed_path: &QString,
    ) {
        let Some(id) = token(value) else { return };
        if let Some(request) = self.as_mut().rust_mut().opens.requests.get_mut(&id)
            && request.phase == Phase::Ready
        {
            request.path = path.to_string();
            request.installed_path = installed_path.to_string();
        }
        self.renamed(id, &path.to_string());
    }

    fn request(mut self: Pin<&mut Self>, path: &QString, review: bool) -> QString {
        let undelivered: Vec<_> = self
            .rust()
            .opens
            .requests
            .iter()
            .filter(|(_, request)| request.cancelled.load(std::sync::atomic::Ordering::Acquire))
            .map(|(&id, _)| id)
            .collect();
        for id in undelivered {
            self.as_mut().reject(id, &OpenError::Queue);
        }
        match self
            .as_mut()
            .rust_mut()
            .opens
            .begin(path.to_string(), review)
        {
            Ok((id, _)) => QString::from(id.to_string()),
            Err(_) => QString::default(),
        }
    }
    fn start(mut self: Pin<&mut Self>, value: &QString) {
        let Some(id) = token(value) else { return };
        let Some(request) = self.rust().opens.requests.get(&id) else {
            return;
        };
        if request.phase != Phase::Queued {
            return;
        }
        let path = request.path.clone();
        let cancelled = request.cancelled.clone();
        #[cfg(test)]
        let gate = barriers::take(&path);
        #[cfg(test)]
        let fault = faults::take(&path);
        #[cfg(test)]
        if fault == faults::Fault::Spawn {
            self.reject(
                id,
                &OpenError::Spawn(std::io::Error::other("injected spawn failure")),
            );
            return;
        }
        // Mark the dispatch before spawning, preventing a pending duplicate
        // from starting a second acquisition.
        if let Some(request) = self.as_mut().rust_mut().opens.requests.get_mut(&id) {
            request.phase = Phase::Loading;
        }
        let thread = self.qt_thread();
        let delivery_cancelled = cancelled.clone();
        #[cfg(test)]
        let completion_path = path.clone();
        let spawned = crate::jobs::spawn("document-open", move || {
            let acquired = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                #[cfg(test)]
                if let Some(gate) = gate {
                    gate.wait();
                }
                #[cfg(test)]
                assert_ne!(fault, faults::Fault::Worker, "injected worker panic");
                Candidate::acquire(&path, &cancelled)
            }))
            .unwrap_or(Err(OpenError::Interrupted));
            {
                let result = acquired;
                let delivery = move |mut this: Pin<&mut Self>| {
                    let Some(request) = this.rust().opens.requests.get(&id) else {
                        return;
                    };
                    if request.phase != Phase::Loading {
                        return;
                    }
                    match result {
                        Ok(candidate) => {
                            let kind = match candidate.kind {
                                basecraft_ops::files::Kind::Sequence => "sequence",
                                basecraft_ops::files::Kind::Alignment => "alignment",
                                basecraft_ops::files::Kind::Tree => "tree",
                                _ => {
                                    this.reject(id, &OpenError::RouteMismatch);
                                    return;
                                }
                            };
                            if let Some(request) =
                                this.as_mut().rust_mut().opens.requests.get_mut(&id)
                            {
                                request.candidate = Some(candidate);
                                request.phase = Phase::Acquired;
                            }
                            this.acquired(QString::from(id.to_string()), QString::from(kind));
                        }
                        Err(error) => this.reject(id, &error),
                    }
                };
                #[cfg(test)]
                let queued = if fault == faults::Fault::Queue || fault == faults::Fault::DoubleQueue
                {
                    drop(delivery);
                    Err(cxx_qt::ThreadingQueueError::InvokeMethodFailed)
                } else {
                    thread.queue(delivery)
                };
                #[cfg(not(test))]
                let queued = thread.queue(delivery);
                if queued.is_err() {
                    delivery_cancelled.store(true, std::sync::atomic::Ordering::Release);
                    // A failed invoke on a surviving endpoint gets one error
                    // delivery attempt. Destruction drops the candidate and
                    // the endpoint's request lifetimes without joining workers.
                    #[cfg(test)]
                    let retry = fault != faults::Fault::DoubleQueue;
                    #[cfg(not(test))]
                    let retry = true;
                    if retry {
                        let _ = thread.queue(move |this| this.reject(id, &OpenError::Queue));
                    }
                }
                #[cfg(test)]
                barriers::finished(&completion_path);
            }
        });
        if let Err(error) = spawned {
            self.reject(id, &OpenError::Spawn(error));
        }
    }
    fn reject(mut self: Pin<&mut Self>, id: u64, error: &OpenError) {
        if let Some(request) = self.rust().opens.requests.get(&id) {
            // An edit kept for a copy that did not open is not redone later.
            let _ = crate::document_open::take_refused(std::path::Path::new(&request.path));
            self.as_mut().rust_mut().opens.retire(id);
            self.failed(
                QString::from(id.to_string()),
                QString::from(error.to_string()),
            );
        }
    }
    unsafe fn install(
        mut self: Pin<&mut Self>,
        value: &QString,
        controller: *mut cxx_qt::QObject,
    ) -> bool {
        let Some(id) = token(value) else { return false };
        let Some(request) = self.rust().opens.requests.get(&id) else {
            return false;
        };
        if request.phase != Phase::Acquired || controller.is_null() {
            return false;
        }
        let candidate = self
            .as_mut()
            .rust_mut()
            .opens
            .requests
            .get_mut(&id)
            .and_then(|r| r.candidate.take());
        let Some(candidate) = candidate else {
            return false;
        };
        if let Err(error) = candidate.check(candidate.kind) {
            self.reject(id, &error);
            return false;
        }
        let installed_path = candidate.path.to_string_lossy().into_owned();
        #[cfg(test)]
        let identity = (
            candidate.session.id().to_owned(),
            candidate.doc,
            candidate.session.generation(candidate.doc).unwrap(),
            candidate.session.revision(candidate.doc).unwrap(),
        );
        #[cfg(all(test, feature = "test-counters"))]
        testing::admitted(candidate.path.clone(), candidate.reads.clone());

        // SAFETY: QML supplies a live QObject for this synchronous invocation;
        // no reference or raw pointer is dereferenced after returning to QML.
        let mut object = unsafe { Pin::new_unchecked(&mut *controller) };
        let result = if let Some(mut ctl) = object
            .as_mut()
            .downcast_pin::<crate::bridge::qobject::GenomeController>()
        {
            {
                let result = ctl.as_mut().install_fresh(candidate);
                #[cfg(test)]
                if result.is_ok() {
                    assert_eq!(ctl.open_identity(), Some(identity));
                }
                result
            }
        } else if let Some(mut ctl) = object
            .as_mut()
            .downcast_pin::<crate::alignment_bridge::qobject::AlignmentController>(
        ) {
            {
                let result = ctl.as_mut().install_fresh(candidate);
                #[cfg(test)]
                if result.is_ok() {
                    assert_eq!(ctl.open_identity(), Some(identity));
                }
                result
            }
        } else if let Some(mut ctl) = object
            .as_mut()
            .downcast_pin::<crate::tree_bridge::qobject::TreeController>()
        {
            {
                let result = ctl.as_mut().install_fresh(candidate);
                #[cfg(test)]
                if result.is_ok() {
                    assert_eq!(ctl.open_identity(), Some(identity));
                }
                result
            }
        } else {
            Err(OpenError::RouteMismatch)
        };
        match result {
            Ok(()) => {
                if let Some(request) = self.as_mut().rust_mut().opens.requests.get_mut(&id) {
                    request.phase = Phase::Constructing;
                    request.controller = controller as usize;
                    request.installed_path = installed_path;
                    return true;
                }
                false
            }
            Err(error) => {
                self.reject(id, &error);
                false
            }
        }
    }
    unsafe fn acknowledge(
        mut self: Pin<&mut Self>,
        value: &QString,
        controller: *mut cxx_qt::QObject,
    ) -> bool {
        let Some(id) = token(value) else { return false };
        let mut state = self.as_mut().rust_mut();
        let Some(request) = state.opens.requests.get_mut(&id) else {
            return false;
        };
        if request.phase != Phase::Constructing || request.controller != controller as usize {
            return false;
        }
        request.phase = Phase::Ready;
        #[cfg(test)]
        barriers::ready(&request.path);
        true
    }
    fn activate(mut self: Pin<&mut Self>, value: &QString) {
        let Some(id) = token(value) else { return };
        if !self.rust().opens.wants_focus(id) {
            return;
        }
        let (review, installed_path) = {
            let mut state = self.as_mut().rust_mut();
            let Some(request) = state.opens.requests.get_mut(&id) else {
                return;
            };
            if request.phase != Phase::Ready {
                return;
            }
            (
                std::mem::take(&mut request.review),
                request.installed_path.clone(),
            )
        };
        self.activated(value.clone(), review, QString::from(installed_path));
    }

    fn retire(mut self: Pin<&mut Self>, value: &QString) {
        if let Some(id) = token(value) {
            #[cfg(test)]
            if let Some(request) = self.rust().opens.requests.get(&id) {
                barriers::retired(&request.path);
            }
            self.as_mut().rust_mut().opens.retire(id);
        }
    }
    fn revoke_focus(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().opens.revoke_focus();
    }
    fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().opens.stop();
    }
}

#[cfg(all(test, feature = "test-counters"))]
pub(crate) mod testing {
    use basecraft_io::counters::Counters;
    use std::{cell::RefCell, path::PathBuf};
    thread_local! {
        static ADMISSIONS: RefCell<Vec<(PathBuf, Counters)>> = const { RefCell::new(Vec::new()) };
    }
    pub fn admitted(path: PathBuf, counts: Counters) {
        ADMISSIONS.with(|all| all.borrow_mut().push((path, counts)));
    }
    #[allow(dead_code)] // harness=false bootstrap consumes the audit.
    pub fn take() -> Vec<(PathBuf, Counters)> {
        ADMISSIONS.with(RefCell::take)
    }
}

#[cfg(test)]
pub(crate) mod barriers {
    use std::sync::{Arc, Condvar, Mutex, OnceLock};
    pub struct Gate {
        released: Mutex<bool>,
        changed: Condvar,
        finished: Mutex<bool>,
        finish_changed: Condvar,
    }
    impl Gate {
        pub fn wait(&self) {
            let released = self.released.lock().unwrap();
            let (released, timeout) = self
                .changed
                .wait_timeout_while(released, std::time::Duration::from_secs(10), |ready| {
                    !*ready
                })
                .unwrap();
            assert!(
                *released && !timeout.timed_out(),
                "open test barrier timed out"
            );
        }
        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.changed.notify_all();
        }
    }
    struct Held {
        path: String,
        release_path: String,
        handed_out: bool,
        gate: Arc<Gate>,
    }
    fn held() -> &'static Mutex<Option<Held>> {
        static HELD: OnceLock<Mutex<Option<Held>>> = OnceLock::new();
        HELD.get_or_init(|| Mutex::new(None))
    }
    #[allow(dead_code)] // driven by the harness=false bootstrap executable.
    pub fn configure(path: String, release_path: String) {
        *held().lock().unwrap() = Some(Held {
            path,
            release_path,
            handed_out: false,
            gate: Arc::new(Gate {
                released: Mutex::new(false),
                changed: Condvar::new(),
                finished: Mutex::new(false),
                finish_changed: Condvar::new(),
            }),
        });
    }
    pub fn take(path: &str) -> Option<Arc<Gate>> {
        let mut held = held().lock().unwrap();
        let state = held.as_mut()?;
        if state.handed_out || state.path != path {
            return None;
        }
        state.handed_out = true;
        Some(state.gate.clone())
    }
    pub fn finished(path: &str) {
        if let Some(state) = held().lock().unwrap().as_ref()
            && state.path == path
        {
            *state.gate.finished.lock().unwrap() = true;
            state.gate.finish_changed.notify_all();
        }
    }
    #[allow(dead_code)] // used by the bootstrap lifetime test.
    pub fn release_and_wait() {
        let gate = held().lock().unwrap().as_ref().unwrap().gate.clone();
        gate.release();
        let finished = gate.finished.lock().unwrap();
        let (finished, timeout) = gate
            .finish_changed
            .wait_timeout_while(finished, std::time::Duration::from_secs(10), |done| !*done)
            .unwrap();
        assert!(
            *finished && !timeout.timed_out(),
            "worker did not return after shell destruction"
        );
    }
    pub fn ready(path: &str) {
        if let Some(state) = held().lock().unwrap().as_ref()
            && state.release_path == path
        {
            state.gate.release();
        }
    }
    pub fn retired(path: &str) {
        if let Some(state) = held().lock().unwrap().as_ref()
            && state.path == path
        {
            state.gate.release();
        }
    }
}

#[cfg(test)]
pub(crate) mod faults {
    use std::{cell::RefCell, collections::BTreeMap};
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum Fault {
        #[default]
        None,
        Spawn,
        Worker,
        Queue,
        DoubleQueue,
    }
    thread_local! { static NEXT: RefCell<BTreeMap<String, Fault>> = const { RefCell::new(BTreeMap::new()) }; }
    #[allow(dead_code)] // harness=false bootstrap sets fault cases.
    pub fn set(path: String, fault: Fault) {
        NEXT.with(|next| next.borrow_mut().insert(path, fault));
    }
    pub fn take(path: &str) -> Fault {
        NEXT.with(|next| next.borrow_mut().remove(path).unwrap_or_default())
    }
}

#[cfg(test)]
mod linked_taxa_tests {
    #[test]
    fn exact_relationships_in_both_directions() {
        use super::{TaxaEndpoint, linked_taxa_target};
        use basecraft_ops::files::Kind;
        let endpoint = |kind, path: &str, link: &str| TaxaEndpoint {
            kind,
            path: path.into(),
            linked_alignment: link.into(),
        };
        let alignment = endpoint(Kind::Alignment, "data/a.afa", "");
        for kind in [Kind::Tree, Kind::Delimitation] {
            let linked = endpoint(kind, "data/other", "data/a.afa");
            assert!(linked_taxa_target(&alignment, &linked));
            assert!(linked_taxa_target(&linked, &alignment));
            assert!(!linked_taxa_target(&linked, &linked));
            for path in ["", "data/b.afa", "data/./a.afa", "DATA/a.afa"] {
                let other = endpoint(Kind::Alignment, path, "");
                assert!(!linked_taxa_target(&other, &linked));
                assert!(!linked_taxa_target(&linked, &other));
            }
            let unlinked = endpoint(kind, "data/a.afa", "");
            assert!(!linked_taxa_target(&alignment, &unlinked));
            assert!(!linked_taxa_target(&unlinked, &alignment));
        }
        assert!(!linked_taxa_target(&alignment, &alignment));
        for kind in [Kind::Sequence, Kind::Metadata, Kind::Submission] {
            let other = endpoint(kind, "data/a.afa", "data/a.afa");
            assert!(!linked_taxa_target(&alignment, &other));
            assert!(!linked_taxa_target(&other, &alignment));
        }
    }
}
