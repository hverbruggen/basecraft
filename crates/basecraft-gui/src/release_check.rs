//! The opt-in release check: one GET on a job thread. Dropping the check, as
//! the window does when it closes, cancels a request still waiting and waits
//! for its thread, so quitting during a slow answer leaves no network thread
//! running into Qt's teardown.
use std::{pin::Pin, sync::mpsc, time::Duration};

use cxx_qt_lib::QString;

pub(crate) struct ReleaseCheck {
    finished: mpsc::Receiver<()>,
}

impl ReleaseCheck {
    /// GET `url`; `done` gets a 200 reply's body on the owner's thread.
    pub(crate) fn start<T>(
        owner: impl crate::jobs::Owner<T>,
        url: String,
        done: impl FnOnce(Pin<&mut T>, String) + Send + 'static,
    ) -> Option<Self> {
        let (finish, finished) = mpsc::channel();
        let fetch = move |_: &basecraft_ops::work::Work| {
            let body = crate::settings_bridge::qobject::fetch_latest_release(&QString::from(&url));
            let _ = finish.send(());
            Ok(body.to_string())
        };
        // A failed start means no notice, as when offline.
        crate::jobs::start(owner, fetch, |owner, body: Result<String, String>| {
            if let Ok(body) = body {
                done(owner, body);
            }
        })
        .ok()?;
        Some(Self { finished })
    }
}

impl Drop for ReleaseCheck {
    fn drop(&mut self) {
        crate::settings_bridge::qobject::cancel_release_check();
        let _ = self.finished.recv_timeout(Duration::from_secs(5));
    }
}
