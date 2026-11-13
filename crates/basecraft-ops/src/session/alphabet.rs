//! The answer to "what is this sequence?" goes to the side file at once
//! (D22): no save, and the user's sequence file is not touched.

use basecraft_protocol::companion_records::Companion;

use super::{DocId, Session};
use crate::{Error, Result, files};

impl Session {
    /// Whether `id` has edits its files on disk don't hold. An answered
    /// alphabet question leaves a saved document saved.
    pub fn is_unsaved(&self, id: DocId) -> bool {
        self.docs.get(id.0).is_some_and(|slot| !slot.is_clean())
    }

    /// [`Session::write_alphabet_now`], or, when the side file can't take
    /// the answer now, the note saying so: also added to the document's
    /// notice, so an undo or redo that has no notes of its own reports it.
    /// `changed_on_disk`: the document's files changed on disk before this
    /// step, so none is replaced now (a save would refuse too).
    pub(super) fn answer_written(
        &mut self,
        id: DocId,
        keep_saved: bool,
        changed_on_disk: bool,
    ) -> Option<String> {
        let error = if changed_on_disk {
            "its files changed on disk since they were read".to_owned()
        } else {
            self.write_alphabet_now(id, keep_saved).err()?.to_string()
        };
        let note = format!("The alphabet answer is kept until the next save: {error}");
        let notice = &mut self.docs[id.0].recovery_notice;
        *notice = Some(
            notice
                .take()
                .map_or(note.clone(), |n| format!("{n}\n{note}")),
        );
        Some(note)
    }

    /// Write the alphabet `id` states to its side file now, through the one
    /// companion writer, or remove the side file when nothing is stated. With
    /// `keep_saved`, a document that was saved stays saved. A new document
    /// with no file yet gets its side file with its first save, and so does
    /// a session that publishes to a recovery destination. A side file that
    /// changed on disk since the session last read or wrote it is left alone,
    /// as a save would refuse to replace it.
    fn write_alphabet_now(&mut self, id: DocId, keep_saved: bool) -> Result<()> {
        if !matches!(
            self.write_publication,
            crate::documents::WritePublication::Immediate
        ) {
            return Ok(());
        }
        let slot = &self.docs[id.0];
        let Some((main, main_hash)) = slot.accepted_hashes.first().cloned() else {
            return Ok(());
        };
        let stated = match (slot.content.alignment(), slot.content.sequence()) {
            (Some(opened), _) => opened.doc.stated_alphabet().cloned(),
            (_, Some(opened)) => opened.doc.stated_alphabet().cloned(),
            _ => return Ok(()),
        };
        let path = files::tree_sidecar(&self.resolve(&main));
        let key = self.relative(&path);
        let known = slot.source_stamps.iter().find(|(p, _)| p == &path);
        let now = crate::records::input_evidence::stamp(&path).ok();
        if known.map_or(now.is_some(), |(_, known)| *known != now) {
            return Err(Error::ChangedOnDisk { path });
        }
        let companion = files::facts_companion(stated.as_ref(), &main_hash);
        // D14: a side file a later basecraft wrote is not overwritten or
        // removed with fields this build doesn't know, unless allowed.
        let drop_newer_fields = std::mem::take(&mut self.docs[id.0].drop_newer_fields);
        let unknown = std::fs::read(&path)
            .ok()
            .and_then(|bytes| match &companion {
                Some(_) => basecraft_protocol::records::unknown_fields::<Companion>(&bytes),
                None => basecraft_protocol::records::removed_fields(&bytes),
            });
        if let Some(unknown) = unknown.filter(|_| !drop_newer_fields) {
            return Err(Error::NewerFieldsDropped(vec![(path, unknown)]));
        }
        let hash = if let Some(companion) = companion {
            let bytes = basecraft_protocol::records::encode(&companion)?;
            basecraft_io::write_atomic(&path, |w| w.write_all(&bytes))?;
            Some(basecraft_io::hash_bytes(&bytes))
        } else {
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
            None
        };
        // Keep what the session knows of the files on disk, as a save does.
        let stamp = crate::records::input_evidence::stamp(&path).ok();
        let slot = &mut self.docs[id.0];
        slot.absent_sidecars.retain(|absent| absent != &key);
        if let Some(hash) = hash {
            if let Some(entry) = slot.accepted_hashes.iter_mut().find(|(k, _)| k == &key) {
                entry.1 = hash;
            }
        } else {
            slot.accepted_hashes.retain(|(k, _)| k != &key);
            slot.absent_sidecars.push(key);
        }
        match slot.source_stamps.iter_mut().find(|(p, _)| p == &path) {
            Some(entry) => entry.1 = stamp,
            None => slot.source_stamps.push((path, stamp)),
        }
        if keep_saved {
            slot.saved_at = Some(slot.mark());
        }
        Ok(())
    }
}
