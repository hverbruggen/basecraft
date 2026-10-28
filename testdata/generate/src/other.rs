//! Large FASTA, alignments and trees.

use std::io::{self, Write};

use crate::fasta::write_wrapped;
use crate::rng::Rng;

/// Multi-record FASTA of about `target_bytes`. Record lengths vary from 1 kb
/// to 5 Mb; most records wrap at 60, every seventh at 80.
pub fn write_big_fasta(w: &mut impl Write, target_bytes: u64, rng: &mut Rng) -> io::Result<()> {
    let mut written = 0u64;
    let mut i = 0u64;
    let mut buf = Vec::new();
    while written < target_bytes {
        i += 1;
        // Log-uniform-ish: pick a magnitude, then a length within it.
        let len = match rng.below(4) {
            0 => rng.range(1_000, 10_000),
            1 => rng.range(10_000, 100_000),
            2 => rng.range(100_000, 1_000_000),
            _ => rng.range(1_000_000, 5_000_000),
        };
        let width = if i.is_multiple_of(7) { 80 } else { 60 };
        buf.clear();
        buf.extend((0..len).map(|_| rng.base(0.45)));
        let header = format!(">seq{i:06} len={len}\n");
        w.write_all(header.as_bytes())?;
        write_wrapped(w, &buf, width)?;
        written += header.len() as u64 + len + len.div_ceil(width as u64);
    }
    Ok(())
}

/// Small FASTA where line widths vary within records. Not valid for
/// samtools faidx; tests that Basecraft's index copes.
pub fn write_ragged_fasta(w: &mut impl Write, rng: &mut Rng) -> io::Result<()> {
    for i in 1..=5 {
        writeln!(w, ">ragged{i}")?;
        let mut left = rng.range(500, 3_000);
        while left > 0 {
            let width = rng.range(40, 90).min(left);
            let line: Vec<u8> = (0..width).map(|_| rng.base(0.5)).collect();
            w.write_all(&line)?;
            w.write_all(b"\n")?;
            left -= width;
        }
    }
    Ok(())
}

/// `rows` × `cols` nucleotide alignment: rows mutate from one ancestor, with
/// gap runs and ragged ends.
pub fn write_alignment(w: &mut impl Write, rows: u64, cols: u64, rng: &mut Rng) -> io::Result<()> {
    #[allow(clippy::cast_possible_truncation)]
    let ancestor: Vec<u8> = (0..cols).map(|_| rng.base(0.4)).collect();
    let mut row = vec![0u8; ancestor.len()];
    for r in 1..=rows {
        for (c, &a) in row.iter_mut().zip(&ancestor) {
            *c = if rng.chance(0.05) { rng.base(0.4) } else { a };
        }
        let gap_runs = rng.below(cols / 1_000 + 2);
        for _ in 0..gap_runs {
            let start = rng.below(cols);
            let len = rng.range(1, 30).min(cols - start);
            #[allow(clippy::cast_possible_truncation)]
            row[start as usize..(start + len) as usize].fill(b'-');
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            let lead = rng.below(cols / 50 + 1) as usize;
            let trail = rng.below(cols / 50 + 1) as usize;
            row[..lead].fill(b'-');
            let n = row.len();
            row[n - trail..].fill(b'-');
        }
        writeln!(w, ">taxon{r:05}")?;
        write_wrapped(w, &row, 60)?;
    }
    Ok(())
}

/// Random binary tree with `tips` leaves in Newick, built by joining random
/// pairs. Written without recursion so deep trees can't overflow the stack.
pub fn write_tree(w: &mut impl Write, tips: u64, rng: &mut Rng) -> io::Result<()> {
    enum Node {
        Tip(u64),
        Inner(usize, usize),
    }
    enum Step {
        Visit(usize),
        Comma,
        Close(usize),
    }
    let mut nodes: Vec<(Node, f64)> = Vec::new();
    let mut pool: Vec<usize> = Vec::new();
    for t in 1..=tips {
        pool.push(nodes.len());
        nodes.push((Node::Tip(t), branch(rng)));
    }
    while pool.len() > 1 {
        #[allow(clippy::cast_possible_truncation)]
        let a = pool.swap_remove(rng.below(pool.len() as u64) as usize);
        #[allow(clippy::cast_possible_truncation)]
        let b = pool.swap_remove(rng.below(pool.len() as u64) as usize);
        pool.push(nodes.len());
        nodes.push((Node::Inner(a, b), branch(rng)));
    }

    let root = pool[0];
    let mut stack = vec![Step::Visit(root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Visit(i) => match nodes[i].0 {
                Node::Tip(t) => write!(w, "taxon{t:05}:{:.5}", nodes[i].1)?,
                Node::Inner(a, b) => {
                    w.write_all(b"(")?;
                    stack.push(Step::Close(i));
                    stack.push(Step::Visit(b));
                    stack.push(Step::Comma);
                    stack.push(Step::Visit(a));
                }
            },
            Step::Comma => w.write_all(b",")?,
            Step::Close(i) if i == root => w.write_all(b")")?,
            Step::Close(i) => write!(w, "):{:.5}", nodes[i].1)?,
        }
    }
    writeln!(w, ";")
}

fn branch(rng: &mut Rng) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let u = ((rng.next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
    -u.ln() * 0.05
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_has_all_tips_and_balanced_parens() {
        let mut out = Vec::new();
        write_tree(&mut out, 500, &mut Rng::new(3)).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.matches("taxon").count(), 500);
        assert_eq!(s.matches('(').count(), s.matches(')').count());
        assert_eq!(s.matches('(').count(), 499);
    }

    #[test]
    fn alignment_rows_have_equal_length() {
        let mut out = Vec::new();
        write_alignment(&mut out, 20, 1_000, &mut Rng::new(3)).unwrap();
        let s = String::from_utf8(out).unwrap();
        for rec in s.split('>').skip(1) {
            let residues: usize = rec.lines().skip(1).map(str::len).sum();
            assert_eq!(residues, 1_000);
        }
    }
}
