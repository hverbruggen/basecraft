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

    /// Write the alphabet `id` states to its side file now, through the one
    /// companion writer, or remove the side file when nothing is stated. A
    /// document that was clean stays clean. A new document with no file yet
    /// gets its side file with its first save, and so does a session that
    /// publishes to a recovery destination.
    pub(super) fn write_alphabet_now(&mut self, id: DocId, was_clean: bool) -> Result<()> {
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
        let hash = if let Some(companion) = files::facts_companion(stated.as_ref(), &main_hash) {
            if let Some(unknown) = std::fs::read(&path)
                .ok()
                .and_then(|bytes| basecraft_protocol::records::unknown_fields::<Companion>(&bytes))
                .filter(|_| !std::mem::take(&mut self.docs[id.0].drop_newer_fields))
            {
                return Err(Error::NewerFieldsDropped {
                    path,
                    writer: unknown.writer,
                    fields: unknown.fields,
                });
            }
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
        if was_clean {
            slot.saved_at = Some(slot.mark());
        }
        Ok(())
    }
}
