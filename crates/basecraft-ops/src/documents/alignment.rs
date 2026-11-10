use std::path::{Path, PathBuf};

use basecraft_docs::FeatureId;
use basecraft_docs::alignment::Undo as AlnUndo;
use basecraft_protocol::{DocState, Op};

use super::Document;
use crate::Result;
use crate::error::invalid;
use crate::files::{self, Format, HashingWriter, OpenedAlignment};
use crate::session::{Inverse, normalise};

impl Document for OpenedAlignment {
    fn revision(&self, _generation: u64) -> u64 {
        self.doc.version()
    }

    fn fingerprint(&self) -> DocState {
        DocState::from_counts(basecraft_protocol::ops::DocumentCounts::Alignment {
            columns: self.doc.width(),
            rows: self.doc.row_count() as u64,
        })
    }

    fn apply(&mut self, op: &Op) -> Result<(Inverse, Option<FeatureId>)> {
        execute_alignment(self, op).map(|steps| (Inverse::Alignment(steps), None))
    }

    fn revert(&mut self, inverse: &Inverse) -> Result<()> {
        let Inverse::Alignment(steps) = inverse else {
            return Err(invalid("undo of the wrong kind of edit"));
        };
        self.doc.undo(steps);
        Ok(())
    }

    fn write(
        &self,
        target: &Path,
        format: Format,
        _recovery_root: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<(PathBuf, String)>> {
        if format != self.format {
            return Err(invalid("saving to a different format isn't supported yet"));
        }
        let mut hash = String::new();
        basecraft_io::write_atomic(target, |w| {
            let mut hw = HashingWriter::new(w);
            files::write_alignment(&self.doc, &mut hw)?;
            hash = hw.hash();
            Ok(())
        })?;
        Ok(vec![(target.to_path_buf(), hash)])
    }

    fn saved_hashes(
        &self,
        _target: &Path,
        _publication: &super::WritePublication,
    ) -> Result<Vec<String>> {
        let mut hw = HashingWriter::new(std::io::sink());
        files::write_alignment(&self.doc, &mut hw)?;
        Ok(vec![hw.hash()])
    }

    fn merge(&self, previous: &Op, next: &Op) -> Option<Op> {
        merge_alignment(previous, next)
    }
}

fn execute_alignment(a: &mut OpenedAlignment, op: &Op) -> Result<Vec<AlnUndo>> {
    let doc = &mut a.doc;
    let edit = match op {
        Op::AlnReplace {
            row,
            start,
            end,
            residues,
        } => {
            let residues = normalise(residues)?;
            doc.replace(*row as usize, *start, *end, residues.as_bytes())
        }
        Op::AlnInsert {
            rows,
            col,
            residues,
        } => {
            let residues = normalise(residues)?;
            doc.insert(rows, *col, residues.as_bytes())
        }
        Op::AlnFill {
            rows,
            start,
            end,
            residue,
        } => {
            let residue = normalise(residue)?;
            let &[b] = residue.as_bytes() else {
                return Err(invalid(format!(
                    "aln.fill takes one residue, not {residue:?}"
                )));
            };
            doc.fill(rows, *start, *end, b)
        }
        Op::AlnInsertGaps { rows, col, count } => doc.insert_gaps(rows, *col, *count),
        Op::AlnDelete { rows, start, end } => doc.delete(rows, *start, *end),
        Op::AlnShift {
            rows,
            start,
            end,
            by,
        } => doc.shift(rows, *start, *end, *by),
        Op::AlnRemoveGapColumns {} => Ok(doc.remove_gap_columns().0),
        Op::AlnMoveRows { rows, to } => doc.move_rows(rows, *to),
        Op::AlnRenameRow { row, name } => doc.rename(*row as usize, name),
        Op::AlnRemoveRows { rows } => doc.remove_rows(rows),
        Op::AlnAddSequences { rows, reference } => {
            let rows = rows
                .iter()
                .map(|(name, seq)| Ok((name.clone(), normalise(seq)?)))
                .collect::<Result<Vec<_>>>()?;
            doc.add_sequences(&rows, reference.as_deref())
        }
        _ => return Err(invalid(format!("{} needs a sequence", op.name()))),
    };
    edit.map_err(|e| invalid(e.0))
}

/// One alignment edit that does what `prev` then `next` do, if there is one:
/// typing over residues along a row, gaps inserted after the previous ones,
/// or a block slid on from where it was left.
fn merge_alignment(prev: &Op, next: &Op) -> Option<Op> {
    match (prev, next) {
        (
            Op::AlnReplace {
                row,
                start,
                end,
                residues,
            },
            Op::AlnReplace {
                row: row2,
                start: start2,
                end: end2,
                residues: residues2,
            },
        ) if row == row2
            && end - start == residues.len() as u64
            && end2 - start2 == residues2.len() as u64
            && start2 == end =>
        {
            Some(Op::AlnReplace {
                row: *row,
                start: *start,
                end: *end2,
                residues: format!("{residues}{residues2}"),
            })
        }
        (
            Op::AlnInsert {
                rows,
                col,
                residues,
            },
            Op::AlnInsert {
                rows: rows2,
                col: col2,
                residues: residues2,
            },
        ) if rows == rows2 && *col2 == col + residues.len() as u64 => Some(Op::AlnInsert {
            rows: rows.clone(),
            col: *col,
            residues: format!("{residues}{residues2}"),
        }),
        (
            Op::AlnInsertGaps { rows, col, count },
            Op::AlnInsertGaps {
                rows: rows2,
                col: col2,
                count: count2,
            },
        ) if rows == rows2 && *col2 == col + count => Some(Op::AlnInsertGaps {
            rows: rows.clone(),
            col: *col,
            count: count + count2,
        }),
        (
            Op::AlnShift {
                rows,
                start,
                end,
                by,
            },
            Op::AlnShift {
                rows: rows2,
                start: start2,
                end: end2,
                by: by2,
            },
        ) if rows == rows2
            && start.checked_add_signed(*by) == Some(*start2)
            && end.checked_add_signed(*by) == Some(*end2) =>
        {
            Some(Op::AlnShift {
                rows: rows.clone(),
                start: *start,
                end: *end,
                by: by + by2,
            })
        }
        _ => None,
    }
}
