//! A list that keeps its newest items in memory and moves older ones to an
//! anonymous temporary file, so a long session uses steady memory and still
//! has every item when it publishes. The file disappears with the process.
use crate::Result;
use serde::{Serialize, de::DeserializeOwned};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};

pub(crate) struct Retained<T> {
    recent: Vec<T>,
    /// Older items, one JSON line each, oldest first.
    spill: Option<File>,
    spilled: usize,
    bytes: u64,
    keep: usize,
}

impl<T: Serialize + DeserializeOwned + Clone> Retained<T> {
    /// Keep between `keep` and twice `keep` items in memory.
    pub(crate) fn new(keep: usize) -> Self {
        Self {
            recent: Vec::new(),
            spill: None,
            spilled: 0,
            bytes: 0,
            keep: keep.max(1),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.spilled + self.recent.len()
    }

    /// The newest item. Spilling always leaves `keep` items in memory.
    pub(crate) fn last(&self) -> Option<&T> {
        self.recent.last()
    }

    /// Add items. If the temporary file cannot be written, they all stay in memory.
    pub(crate) fn extend(&mut self, items: impl IntoIterator<Item = T>) {
        self.recent.extend(items);
        if self.recent.len() > 2 * self.keep {
            let older = self.recent.len() - self.keep;
            if self.spill_oldest(older).is_ok() {
                self.recent.drain(..older);
                self.spilled += older;
            }
        }
    }

    fn spill_oldest(&mut self, count: usize) -> Result<()> {
        let file = match &mut self.spill {
            Some(file) => file,
            None => self.spill.insert(tempfile::tempfile()?),
        };
        // Drop whatever a failed earlier write left behind.
        file.set_len(self.bytes)?;
        file.seek(SeekFrom::Start(self.bytes))?;
        let mut out = std::io::BufWriter::new(&mut *file);
        for item in &self.recent[..count] {
            serde_json::to_writer(&mut out, item)?;
            out.write_all(b"\n")?;
        }
        out.flush()?;
        drop(out);
        self.bytes = file.stream_position()?;
        Ok(())
    }

    /// Every item that `keep` accepts, oldest first.
    pub(crate) fn matching(&self, keep: impl Fn(&T) -> bool) -> Result<Vec<T>> {
        let mut found = Vec::new();
        if let Some(file) = &self.spill {
            let mut file = file.try_clone()?;
            file.seek(SeekFrom::Start(0))?;
            for line in BufReader::new(file).lines().take(self.spilled) {
                let item: T = serde_json::from_str(&line?)?;
                if keep(&item) {
                    found.push(item);
                }
            }
        }
        found.extend(self.recent.iter().filter(|item| keep(item)).cloned());
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::Retained;

    #[test]
    fn older_items_leave_memory_and_come_back_in_order() {
        let mut list = Retained::new(3);
        for i in 0..20u32 {
            list.extend([i]);
            assert!(list.recent.len() <= 6);
        }
        assert_eq!(list.len(), 20);
        assert_eq!(
            list.matching(|_| true).unwrap(),
            (0..20).collect::<Vec<_>>()
        );
        assert_eq!(list.matching(|i| i % 7 == 0).unwrap(), vec![0, 7, 14]);
    }
}
