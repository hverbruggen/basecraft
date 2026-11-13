//! The Sanger review's decisions as commands (D1): start a review from a
//! read's instrument calls, decide calls on draft columns, and (through
//! `Session::export_sanger_consensus`) export the consensus. The window, the
//! CLI and MCP all call these, so a decision means the same on every route.
use std::path::Path;

use basecraft_protocol::Op;
use basecraft_protocol::sanger::{ReviewChoice, ReviewOp};

use crate::alignment_add::AdditionError;
use crate::{DocId, Error, Result, Session};

/// A new review of `read` from its instrument calls, in `session`.
pub fn review_read(session: &mut Session, read: &Path) -> Result<DocId> {
    let review = crate::sanger_review::instrument_review(read).map_err(Error::Invalid)?;
    session.create_sanger_review(&review)
}

/// Apply one review decision to the review's current call set.
pub fn decide(session: &mut Session, review: DocId, edit: ReviewOp) -> Result<()> {
    let call_set = session.sanger_review(review)?.draft_sha256.clone();
    session.apply(
        review,
        Op::SangerReview {
            call_set,
            edit: Box::new(edit),
        },
    )?;
    Ok(())
}

/// A decision or column as a person wrote it that basecraft cannot read.
#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("{0} is not a base; give one of ACGT, an IUPAC code or -")]
    NotABase(char),
    #[error("{0} is not a decision; give accept, reject, pending or a base")]
    NotADecision(String),
    #[error("{0} is not a column number (from 1)")]
    NotAColumn(String),
    #[error("{0}: the range runs backwards")]
    Backwards(String),
}

/// A decision as a person states it: `accept`, `reject`, `pending`, or the
/// base to put there (IUPAC, or `-` for a gap).
pub fn choice(text: &str) -> Result<ReviewChoice> {
    match text.to_ascii_lowercase().as_str() {
        "accept" => Ok(ReviewChoice::Accept),
        "reject" => Ok(ReviewChoice::Reject),
        "pending" => Ok(ReviewChoice::Pending),
        base if base.chars().count() == 1 => {
            let base = base.chars().next().unwrap_or('N').to_ascii_uppercase();
            if base == '-' || "ACGTURYSWKMBDHVN".contains(base) {
                Ok(ReviewChoice::Edit(base))
            } else {
                Err(InputError::NotABase(base).into())
            }
        }
        other => Err(InputError::NotADecision(other.into()).into()),
    }
}

/// Draft columns as a person numbers them, from 1: `12`, `12,15` or `12-14`.
pub fn columns(text: &str) -> Result<Vec<usize>> {
    let number = |part: &str| {
        part.trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| Error::from(InputError::NotAColumn(part.trim().into())))
    };
    let mut columns = Vec::new();
    for part in text.split(',') {
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (number(from)?, number(to)?);
                if from > to {
                    return Err(InputError::Backwards(part.into()).into());
                }
                columns.extend(from - 1..to);
            }
            None => columns.push(number(part)? - 1),
        }
    }
    columns.sort_unstable();
    columns.dedup();
    Ok(columns)
}

/// The rows that add a reviewed consensus (`text`, named `name`) to
/// `alignment`, checked against it and against the accepted review
/// (`evidence`, from `SangerReview::addition_evidence`). Nothing is recorded:
/// the caller adds the row, as it is or placed by MAFFT, then calls
/// `record_consensus`.
pub fn consensus_rows(
    session: &Session,
    alignment: DocId,
    evidence: &str,
    text: &str,
    name: &str,
) -> Result<Vec<(String, String)>> {
    let rows = crate::alignment_add::addition_rows(session.alignment(alignment)?, text, name)?;
    if rows.len() != 1 {
        return Err(AdditionError::NotOneSpecimen.into());
    }
    crate::sanger_review::check_addition(evidence, &rows[0].1)?;
    Ok(rows)
}

/// Record the accepted review as the input of the consensus row just added
/// to `alignment` (`sequence` is the row as reviewed, without placement
/// gaps). If this fails, the caller undoes the row, so the two stay together.
pub fn record_consensus(
    session: &mut Session,
    alignment: DocId,
    evidence: &str,
    sequence: &str,
) -> Result<()> {
    crate::sanger_review::record_addition(session, alignment, evidence, sequence)
}

/// Add the decided consensus of `review` to `alignment` as a new last row
/// named `label`, unplaced (the window can place it with MAFFT instead).
pub fn add_consensus(
    session: &mut Session,
    review: DocId,
    alignment: DocId,
    label: &str,
) -> Result<()> {
    let reviewed = session.sanger_review(review)?;
    let evidence = reviewed
        .addition_evidence(session.project())
        .map_err(AdditionError::Evidence)?;
    let sequence = reviewed.resolved_sequence().unwrap_or_default();
    let rows = consensus_rows(session, alignment, &evidence, &sequence, label)?;
    let sequence = rows[0].1.clone();
    crate::renames::apply_op(
        session,
        alignment,
        Op::AlnAddSequences {
            rows,
            reference: None,
        },
    )?;
    if let Err(error) = record_consensus(session, alignment, &evidence, &sequence) {
        session.undo(alignment)?;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_and_columns_read_as_a_person_writes_them() {
        assert_eq!(choice("Accept").unwrap(), ReviewChoice::Accept);
        assert_eq!(choice("g").unwrap(), ReviewChoice::Edit('G'));
        assert_eq!(choice("-").unwrap(), ReviewChoice::Edit('-'));
        assert!(choice("Z").unwrap_err().to_string().contains("not a base"));
        assert!(
            choice("maybe")
                .unwrap_err()
                .to_string()
                .contains("not a decision")
        );
        assert_eq!(columns("3,1-2,3").unwrap(), vec![0, 1, 2]);
        assert!(columns("0").is_err());
        assert!(
            columns("5-4")
                .unwrap_err()
                .to_string()
                .contains("backwards")
        );
    }

    #[test]
    fn a_read_is_reviewed_decided_and_exported() {
        let folder = tempfile::tempdir().unwrap();
        let read = folder.path().join("forward.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &read,
        )
        .unwrap();
        let mut session = Session::new(folder.path());
        let review = review_read(&mut session, &read).unwrap();
        let before = session.sanger_review(review).unwrap().draft_sha256.clone();
        let every = (0..session.sanger_review(review).unwrap().draft.columns.len()).collect();
        decide(
            &mut session,
            review,
            ReviewOp::Set {
                columns: every,
                choice: ReviewChoice::Accept,
            },
        )
        .unwrap();
        decide(
            &mut session,
            review,
            ReviewOp::Set {
                columns: vec![0],
                choice: ReviewChoice::Edit('A'),
            },
        )
        .unwrap();
        let decided = session.sanger_review(review).unwrap();
        assert_eq!(decided.draft_sha256, before, "decisions keep the call set");
        assert_eq!(decided.decision(0), Some(&ReviewChoice::Edit('A')));
        assert_eq!(decided.unresolved(), 0);
        let consensus = folder.path().join("consensus.fasta");
        session
            .export_sanger_consensus(review, &consensus, "HV1")
            .unwrap();
        assert!(
            std::fs::read_to_string(&consensus)
                .unwrap()
                .starts_with(">HV1")
        );
    }

    #[test]
    fn a_decided_consensus_joins_an_alignment_once() {
        let folder = tempfile::tempdir().unwrap();
        let read = folder.path().join("forward.ab1");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../testdata/fixtures/compact/sanger-review/forward.ab1"),
            &read,
        )
        .unwrap();
        let alignment = folder.path().join("alignment.fasta");
        std::fs::write(&alignment, ">a\nACGT\n>b\nACGA\n").unwrap();
        let mut session = Session::recoverable(folder.path()).unwrap();
        let review = review_read(&mut session, &read).unwrap();
        session
            .save_new(
                review,
                Some(&folder.path().join("forward.sanger-review.json")),
            )
            .unwrap();
        let aln = session.open(&alignment).unwrap();
        let undecided = add_consensus(&mut session, review, aln, "HV1").unwrap_err();
        assert!(
            undecided
                .to_string()
                .contains("Resolve every review column")
        );
        let every = (0..session.sanger_review(review).unwrap().draft.columns.len()).collect();
        decide(
            &mut session,
            review,
            ReviewOp::Set {
                columns: every,
                choice: ReviewChoice::Accept,
            },
        )
        .unwrap();
        // Checking a consensus, as before a MAFFT placement that may be
        // cancelled, records nothing.
        let runs = |session: &Session| {
            session
                .document_record(aln)
                .unwrap()
                .events
                .iter()
                .filter(|entry| matches!(entry.op, Op::ToolRun { .. }))
                .count()
        };
        let evidence = session
            .sanger_review(review)
            .unwrap()
            .addition_evidence(None)
            .unwrap();
        let sequence = session
            .sanger_review(review)
            .unwrap()
            .resolved_sequence()
            .unwrap();
        consensus_rows(&session, aln, &evidence, &sequence, "HV1").unwrap();
        assert_eq!(runs(&session), 0, "a checked consensus records no run");
        add_consensus(&mut session, review, aln, "HV1").unwrap();
        assert_eq!(runs(&session), 1, "the added row and its run go together");
        let rows = session.alignment(aln).unwrap();
        assert_eq!(rows.row_count(), 3);
        assert_eq!(rows.row(2).name, "HV1");
        let again = add_consensus(&mut session, review, aln, "HV1").unwrap_err();
        assert_eq!(runs(&session), 1, "a refused addition records no run");
        assert!(
            again.to_string().contains("already in the alignment"),
            "{again}"
        );
    }
}
