use std::io::{self, Write};

/// Write `seq` wrapped at `width` columns.
pub fn write_wrapped(w: &mut impl Write, seq: &[u8], width: usize) -> io::Result<()> {
    for line in seq.chunks(width) {
        w.write_all(line)?;
        w.write_all(b"\n")?;
    }
    Ok(())
}
