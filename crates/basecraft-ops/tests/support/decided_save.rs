//! The save route users take: check, choose a policy for known analyses, save.
use basecraft_ops::{DocId, Session, records::SavePolicy};
use std::path::Path;

pub fn save_with_policy(
    session: &mut Session,
    doc: DocId,
    path: Option<&Path>,
    policy: SavePolicy,
) -> basecraft_ops::Result<()> {
    let check = session.check_save(doc, path)?;
    session
        .save_with_decision(doc, path, check.decide(policy, false))
        .map(|_| ())
}
