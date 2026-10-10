//! Append-only JSON Lines log, one stream per machine, rotated monthly:
//! `<project>/.basecraft/log/<host>/<YYYY-MM>.jsonl`.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use basecraft_protocol::LogEntry;

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
    let mut entries = Vec::new();
    let dir = log_dir(root);
    if !dir.exists() {
        return Ok(entries);
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
                    if !line.trim().is_empty() {
                        entries.push(serde_json::from_str(&line)?);
                    }
                }
            }
        }
    }
    Ok(entries)
}
