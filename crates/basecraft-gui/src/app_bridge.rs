//! Qt transport for transient, new-tab canvas opens.
use crate::document_open::{Candidate, OpenError, Opens, Phase};
use basecraft_ops::files::Kind;
use cxx_qt::{CxxQtType, Threading, casting::Downcast};
use cxx_qt_lib::QString;
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
    }
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        type DocumentOpens = super::OpenRust;
        #[qinvokable]
        unsafe fn link_taxa(
            self: &DocumentOpens,
            source: *mut OpenObject,
            target: *mut OpenObject,
            names: &QString,
        );
        #[qinvokable]
        fn eligible(self: &DocumentOpens, path: &QString) -> bool;
        #[qinvokable]
        fn existing(self: &DocumentOpens, path: &QString) -> QString;
        #[qinvokable]
        fn track(self: Pin<&mut DocumentOpens>, path: &QString) -> QString;
        #[qinvokable]
        fn request(
            self: Pin<&mut DocumentOpens>,
            path: &QString,
            context: &QString,
            review: bool,
        ) -> QString;
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
        if let Some(id) = token(value)
            && let Some(request) = self.as_mut().rust_mut().opens.requests.get_mut(&id)
            && request.phase == Phase::Ready
        {
            request.path = path.to_string();
            request.installed_path = installed_path.to_string();
        }
    }

    fn track(mut self: Pin<&mut Self>, path: &QString) -> QString {
        match self.as_mut().rust_mut().opens.track(path.to_string()) {
            Ok(id) => QString::from(id.to_string()),
            Err(_) => QString::default(),
        }
    }

    fn eligible(&self, path: &QString) -> bool {
        crate::document_open::eligible(&path.to_string())
    }
    fn existing(&self, path: &QString) -> QString {
        self.rust()
            .opens
            .requests
            .iter()
            .find(|(_, r)| {
                r.path == path.to_string()
                    && !r.cancelled.load(std::sync::atomic::Ordering::Acquire)
            })
            .map_or_else(QString::default, |(id, _)| QString::from(id.to_string()))
    }
    fn request(
        mut self: Pin<&mut Self>,
        path: &QString,
        context: &QString,
        review: bool,
    ) -> QString {
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
            .begin(path.to_string(), context.to_string(), review)
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
        if self.rust().opens.requests.contains_key(&id) {
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
