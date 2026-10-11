//! Append-only JSON Lines log, one stream per machine, rotated monthly:
//! `<project>/.basecraft/log/<host>/<YYYY-MM>.jsonl`.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use basecraft_protocol::{Actor, LogEntry, Op};

use crate::Result;

pub fn log_dir(root: &Path) -> PathBuf {
    root.join(".basecraft").join("log")
}

pub struct LogWriter {
    dir: PathBuf,
    open: Option<(String, File)>,
}

impl LogWriter {
    pub fn new(root: &Path, host: &str) -> Self {
        Self {
            dir: log_dir(root).join(host),
            open: None,
        }
    }

    pub fn append(&mut self, entry: &LogEntry) -> Result<()> {
        let month = entry.t.get(..7).unwrap_or("unknown").to_string();
        if self.open.as_ref().is_none_or(|(m, _)| *m != month) {
            fs::create_dir_all(&self.dir)?;
            let path = self.dir.join(format!("{month}.jsonl"));
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            self.open = Some((month, file));
        }
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        if let Some((_, f)) = &mut self.open {
            // One write per line: with O_APPEND, a crash can't interleave
            // half lines.
            f.write_all(&line)?;
        }
        Ok(())
    }
}

/// Every entry under a project's log directory, from all hosts. Within a
/// host's stream, file order is kept: it is the order of events.
pub fn read_all(root: &Path) -> Result<Vec<LogEntry>> {
    Ok(read_entries(root, false)?.0)
}

/// Like `read_all`, but lines that can't be read are skipped and counted
/// instead of failing the whole read.
pub fn read_readable(root: &Path) -> Result<(Vec<LogEntry>, usize)> {
    read_entries(root, true)
}

fn read_entries(root: &Path, lenient: bool) -> Result<(Vec<LogEntry>, usize)> {
    let mut entries = Vec::new();
    let mut skipped = 0;
    let dir = log_dir(root);
    if !dir.exists() {
        return Ok((entries, skipped));
    }
    let mut hosts: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    hosts.sort();
    for host in hosts.into_iter().filter(|h| h.is_dir()) {
        // Monthly files sort by name.
        let mut files: Vec<PathBuf> = fs::read_dir(&host)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        files.sort();
        for file in files {
            if file.extension().is_some_and(|e| e == "jsonl") {
                for line in BufReader::new(File::open(&file)?).lines() {
                    let line = line?;
                    if line.trim().is_empty() {
                        continue;
                    }
                    match parse_entry(&line) {
                        Ok(e) => entries.push(e),
                        Err(_) if lenient => skipped += 1,
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
    }
    Ok((entries, skipped))
}

/// One log line, with entries written by earlier versions brought up to
/// date.
pub fn parse_entry(line: &str) -> serde_json::Result<LogEntry> {
    let mut v: serde_json::Value = serde_json::from_str(line)?;
    // aln.insert named one row before it took row ranges.
    if v["op"] == "aln.insert"
        && let Some(params) = v["params"].as_object_mut()
        && let Some(row) = params.remove("row").and_then(|r| r.as_u64())
    {
        params.insert("rows".into(), serde_json::json!([[row, row + 1]]));
    }
    serde_json::from_value(v)
}

/// A readable account of what happened in the projects under `roots` since
/// `since` (RFC 3339 UTC, as in log entries), oldest first. Times are shown
/// shifted by `utc_offset_min` minutes, to the viewer's local time.
pub fn activity_text(roots: &[PathBuf], since: &str, utc_offset_min: i32) -> Result<String> {
    let mut entries = Vec::new();
    let mut skipped = 0;
    for root in roots {
        let (all, n) = read_readable(root)?;
        skipped += n;
        entries.extend(all.into_iter().filter(|e| e.t.as_str() >= since));
    }
    entries.sort_by(|a, b| a.t.cmp(&b.t));
    let mut out = format!(
        "Basecraft activity since {}\n",
        local_time(since, utc_offset_min)
    );
    for root in roots {
        let _ = writeln!(out, "Full log: {}", log_dir(root).display());
    }
    if skipped > 0 {
        let _ = writeln!(
            out,
            "{skipped} log lines couldn't be read and are left out."
        );
    }
    out.push('\n');
    if entries.is_empty() {
        out.push_str("Nothing yet.\n");
    }
    for e in &entries {
        let _ = writeln!(
            out,
            "{}  {}  {}{}",
            local_time(&e.t, utc_offset_min),
            e.doc,
            describe(&e.op),
            actor_note(&e.actor)
        );
    }
    Ok(out)
}

/// "HH:MM:SS" of an RFC 3339 UTC time, shifted by `offset_min` minutes.
fn local_time(t: &str, offset_min: i32) -> String {
    let field =
        |r: std::ops::Range<usize>| t.get(r).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    let secs = field(11..13) * 3600 + field(14..16) * 60 + field(17..19) + offset_min * 60;
    let secs = secs.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn range(start: u64, end: u64) -> String {
    format!("{}–{end}", start + 1)
}

fn feature_name(f: &basecraft_protocol::Feature) -> String {
    let span = f
        .location
        .bounds()
        .map(|(a, b)| format!(" {}", range(a, b)))
        .unwrap_or_default();
    match f.label() {
        Some(l) if !l.is_empty() => format!("{} “{l}”{span}", f.kind),
        _ => format!("{}{span}", f.kind),
    }
}

/// One line for an operation.
fn describe(op: &Op) -> String {
    match op {
        Op::Open { .. } => "Opened".into(),
        Op::Save { .. } => "Saved".into(),
        Op::Extract {
            start, end, path, ..
        } => format!("Extracted {} to {path}", range(*start, *end)),
        Op::SeqReplace {
            start,
            end,
            residues,
        } => match (end - start, residues.len()) {
            (0, n) => format!("Inserted {n} bp at {}", start + 1),
            (d, 0) => format!("Deleted {d} bp at {}", range(*start, *end)),
            (_, n) => format!("Replaced {} with {n} bp", range(*start, *end)),
        },
        Op::FeatureAdd { feature } => format!("Added {}", feature_name(feature)),
        Op::FeatureUpdate { feature, .. } => format!("Edited {}", feature_name(feature)),
        Op::FeatureRemove { id } => format!("Deleted annotation #{id}"),
        Op::AlnReplace {
            row,
            start,
            end,
            residues,
        } => format!(
            "Replaced columns {} of row {} with {residues}",
            range(*start, *end),
            row + 1
        ),
        Op::AlnInsert {
            rows: r,
            col,
            residues,
        } => format!("Inserted {residues} at column {} in {}", col + 1, rows(r)),
        Op::AlnFill {
            rows: r,
            start,
            end,
            residue,
        } => format!(
            "Filled columns {} of {} with {residue}",
            range(*start, *end),
            rows(r)
        ),
        Op::ToolRun {
            plugin,
            operation,
            outputs,
            ..
        } => format!("Ran {plugin} {operation}, wrote {}", outputs.join(", ")),
        Op::AlnInsertGaps {
            rows: r,
            col,
            count,
        } => {
            format!("Inserted {count} gaps at column {} in {}", col + 1, rows(r))
        }
        Op::AlnDelete {
            rows: r,
            start,
            end,
        } => format!("Deleted columns {} in {}", range(*start, *end), rows(r)),
        Op::AlnShift {
            rows: r,
            start,
            end,
            by,
        } => format!(
            "Slid columns {} of {} {} by {}",
            range(*start, *end),
            rows(r),
            if *by < 0 { "left" } else { "right" },
            by.unsigned_abs()
        ),
        Op::AlnRemoveGapColumns {} => "Removed gap-only columns".into(),
        Op::AlnMoveRows { rows: r, to } => format!("Moved {} to row {}", rows(r), to + 1),
        Op::AlnRenameRow { row, name } => format!("Renamed row {} to {name}", row + 1),
        Op::AlnRemoveRows { rows: r } => format!("Deleted {}", rows(r)),
        Op::Export { path, format } => format!("Exported {format} to {path}"),
        Op::TreeReroot { clade } => format!("Rerooted on {}", clade_name(clade)),
        Op::TreeRotate { clade } => format!("Rotated {}", clade_name(clade)),
        Op::TreeLadderize { small_first } => format!(
            "Ladderized, {} clades on top",
            if *small_first { "small" } else { "large" }
        ),
        Op::TreeRenameTip { from, to } => format!("Renamed {from} to {to}"),
        Op::TreeAnnotate {
            annotation: Some(a),
            ..
        } => format!("Annotated {} as {:?}", clade_name(&a.clade), a.label),
        Op::TreeAnnotate {
            id,
            annotation: None,
        } => format!("Removed clade annotation {id}"),
        Op::TreeAddSupport { name, source, .. } if source.is_empty() => {
            format!("Added support values {name:?}")
        }
        Op::TreeAddSupport { name, source, .. } => {
            format!("Added support values {name:?} from {source}")
        }
        Op::TreeRemoveSupport { name } => format!("Removed support values {name:?}"),
        Op::TreeStyle { .. } => "Changed the tree's style".into(),
        Op::Undo { .. } => "Undo".into(),
        Op::Redo { .. } => "Redo".into(),
    }
}

/// A clade as people read it: "Codium fragile" or "the clade of A and B".
fn clade_name(clade: &[String]) -> String {
    match clade {
        [a] => a.clone(),
        [a, b] => format!("the clade of {a} and {b}"),
        _ => "a clade".into(),
    }
}

/// Alignment rows as people count them: "row 3", "rows 3–5".
fn rows(ranges: &[(u32, u32)]) -> String {
    let parts: Vec<String> = ranges
        .iter()
        .map(|&(a, b)| {
            if b == a + 1 {
                (a + 1).to_string()
            } else {
                format!("{}–{b}", a + 1)
            }
        })
        .collect();
    let one = ranges.len() == 1 && ranges[0].1 == ranges[0].0 + 1;
    format!("{} {}", if one { "row" } else { "rows" }, parts.join(", "))
}

/// Who acted, when it wasn't the user.
fn actor_note(actor: &Actor) -> String {
    match actor {
        Actor::Human { .. } => String::new(),
        Actor::Plugin { id, .. } => format!("  [plugin {id}]"),
        Actor::Tool { tool, .. } => format!("  [{tool}]"),
        Actor::Agent { model } => format!("  [agent {model}]"),
        Actor::External => "  [changed on disk]".into(),
    }
}

#[cfg(test)]
mod activity_tests {
    use super::*;

    #[test]
    fn local_time_shifts_and_wraps() {
        assert_eq!(local_time("2026-10-10T13:04:05.025Z", 600), "23:04:05");
        assert_eq!(local_time("2026-10-10T23:30:00Z", 60), "00:30:00");
        assert_eq!(local_time("2026-10-10T01:00:00Z", -120), "23:00:00");
    }

    #[test]
    fn describes_edits() {
        let op = Op::SeqReplace {
            start: 9,
            end: 9,
            residues: "ACG".into(),
        };
        assert_eq!(describe(&op), "Inserted 3 bp at 10");
        let op = Op::SeqReplace {
            start: 9,
            end: 12,
            residues: String::new(),
        };
        assert_eq!(describe(&op), "Deleted 3 bp at 10–12");
        let op = Op::AlnShift {
            rows: vec![(2, 5)],
            start: 9,
            end: 12,
            by: -2,
        };
        assert_eq!(describe(&op), "Slid columns 10–12 of rows 3–5 left by 2");
        let op = Op::AlnRemoveRows { rows: vec![(0, 1)] };
        assert_eq!(describe(&op), "Deleted row 1");
    }

    #[test]
    fn old_insert_entries_still_read() {
        let line = r#"{"v":1,"id":"01","t":"2026-10-11T07:37:59.475Z","host":"h","session":"s","actor":{"kind":"human","user":"u"},"doc":"a.fas","op":"aln.insert","params":{"row":3,"col":6,"residues":"A"},"state":{"len":10,"features":0,"rows":5}}"#;
        let e = parse_entry(line).unwrap();
        assert_eq!(
            e.op,
            Op::AlnInsert {
                rows: vec![(3, 4)],
                col: 6,
                residues: "A".into()
            }
        );
    }
}
