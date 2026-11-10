use std::{error::Error, io::Write, path::Path};
const ROWS: usize = 32;
const WIDTH: usize = 4096;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub fn fixture(path: &Path, bytes: Option<u64>) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    if let Some(bytes) = bytes {
        // Exact decimal file bytes, one single-record sequence with wrapped lines.
        out.write_all(b">large\n")?;
        let mut remaining = bytes - 7;
        let line = [b"ACGT".repeat(20), vec![b'\n']].concat();
        while remaining > 0 {
            let count = usize::try_from(remaining.min(line.len() as u64))?;
            out.write_all(&line[..count])?;
            remaining -= count as u64;
        }
    } else {
        for row in 0..ROWS {
            writeln!(out, ">row{row:03}")?;
            for col in 0..WIDTH {
                out.write_all(&[b"ACGT"[(row + col) % 4]])?;
            }
            writeln!(out)?;
        }
    }
    out.flush()?;
    Ok(())
}
