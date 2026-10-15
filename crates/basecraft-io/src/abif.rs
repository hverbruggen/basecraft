//! Applied Biosystems ABIF trace import. The source file is never modified.
//! Only the processed four-channel signal and vendor call sets are interpreted;
//! all coordinates remain zero-based sample indices in the original trace.

use std::collections::HashMap;
use std::path::Path;

use crate::{Result, error::parse_error};

const ENTRY_LEN: usize = 28;
const MAX_FILE: u64 = 128 * 1024 * 1024;
const MAX_ITEMS: usize = 10_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSet {
    /// PBAS/PLOC/PCON suffix, usually 1 (edited) or 2 (basecaller).
    pub number: u32,
    pub bases: Vec<u8>,
    pub positions: Vec<u32>,
    /// PCON may be absent. Never infer quality from signal height.
    pub qualities: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct Trace {
    /// Processed DATA9..12 in A, C, G, T order; raw negative baseline is retained.
    pub channels: [Vec<i32>; 4],
    /// Both vendor call sets are retained when present.
    pub call_sets: Vec<CallSet>,
    pub channel_order: [u8; 4],
}

impl Trace {
    /// Prefer the user-edited set when present, then the original basecaller set.
    pub fn display_calls(&self) -> &CallSet {
        self.call_sets
            .iter()
            .find(|calls| calls.number == 1)
            .unwrap_or(&self.call_sets[0])
    }

    pub fn samples(&self) -> usize {
        self.channels[0].len()
    }
}

#[derive(Clone, Copy)]
struct Entry {
    kind: u16,
    width: usize,
    count: usize,
    size: usize,
    offset: usize,
    inline: [u8; 4],
}

fn bad(message: impl Into<String>) -> crate::Error {
    parse_error("ABIF", 0, message)
}

fn u16be(data: &[u8], at: usize) -> Result<u16> {
    let bytes = data
        .get(at..at + 2)
        .ok_or_else(|| bad("truncated directory"))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn u32be(data: &[u8], at: usize) -> Result<u32> {
    let bytes = data
        .get(at..at + 4)
        .ok_or_else(|| bad("truncated directory"))?;
    Ok(u32::from_be_bytes(
        bytes.try_into().map_err(|_| bad("invalid integer"))?,
    ))
}

fn directory_entry(data: &[u8], at: usize) -> Result<([u8; 4], u32, Entry)> {
    let bytes = data
        .get(at..at + ENTRY_LEN)
        .ok_or_else(|| bad("truncated entry"))?;
    let tag: [u8; 4] = bytes[..4].try_into().map_err(|_| bad("invalid tag"))?;
    let count = usize::try_from(u32be(bytes, 12)?).map_err(|_| bad("entry count too large"))?;
    let size = usize::try_from(u32be(bytes, 16)?).map_err(|_| bad("entry size too large"))?;
    let offset = usize::try_from(u32be(bytes, 20)?).map_err(|_| bad("entry offset too large"))?;
    Ok((
        tag,
        u32be(bytes, 4)?,
        Entry {
            kind: u16be(bytes, 8)?,
            width: usize::from(u16be(bytes, 10)?),
            count,
            size,
            offset,
            inline: bytes[20..24]
                .try_into()
                .map_err(|_| bad("invalid inline data"))?,
        },
    ))
}

fn payload<'a>(data: &'a [u8], e: &'a Entry) -> Result<&'a [u8]> {
    if e.count > MAX_ITEMS || e.width == 0 || e.count.checked_mul(e.width) != Some(e.size) {
        return Err(bad("invalid element count or width"));
    }
    if e.size <= 4 {
        Ok(&e.inline[..e.size])
    } else {
        data.get(e.offset..e.offset.saturating_add(e.size))
            .ok_or_else(|| bad("entry payload outside file"))
    }
}

fn integers(data: &[u8], e: &Entry) -> Result<Vec<i32>> {
    let p = payload(data, e)?;
    let (width, signed) = match e.kind {
        1 => (1, false),
        2 => (1, true),
        3 => (2, false),
        4 => (2, true),
        5 => (4, true),
        _ => return Err(bad(format!("unsupported integer type {}", e.kind))),
    };
    if e.width != width {
        return Err(bad("integer width does not match type"));
    }
    Ok(p.chunks_exact(width)
        .map(|chunk| match (width, signed) {
            (1, false) => i32::from(chunk[0]),
            (1, true) => i32::from(i8::from_be_bytes([chunk[0]])),
            (2, false) => i32::from(u16::from_be_bytes([chunk[0], chunk[1]])),
            (2, true) => i32::from(i16::from_be_bytes([chunk[0], chunk[1]])),
            _ => i32::from_be_bytes(chunk.try_into().expect("four-byte chunk")),
        })
        .collect())
}

pub fn parse(data: &[u8]) -> Result<Trace> {
    if data.len() < 34 || &data[..4] != b"ABIF" {
        return Err(bad("missing ABIF header"));
    }
    if data.len() as u64 > MAX_FILE {
        return Err(bad("trace exceeds 128 MiB limit"));
    }
    let (_, _, root) = directory_entry(data, 6)?;
    if root.count == 0 || root.count > 100_000 || root.offset < 34 {
        return Err(bad("invalid root directory"));
    }
    let bytes = root
        .count
        .checked_mul(ENTRY_LEN)
        .ok_or_else(|| bad("directory too large"))?;
    data.get(root.offset..root.offset.saturating_add(bytes))
        .ok_or_else(|| bad("root directory outside file"))?;
    let mut entries = HashMap::new();
    for index in 0..root.count {
        let at = root.offset + index * ENTRY_LEN;
        let (tag, number, entry) = directory_entry(data, at)?;
        if entries.insert((tag, number), entry).is_some() {
            return Err(bad("duplicate directory tag"));
        }
    }
    let wheel = entries
        .get(&(*b"FWO_", 1))
        .ok_or_else(|| bad("missing FWO_1"))?;
    let wheel_bytes = payload(data, wheel)?;
    let channel_order: [u8; 4] = wheel_bytes
        .try_into()
        .map_err(|_| bad("FWO_1 must hold four bases"))?;
    let mut channels: [Vec<i32>; 4] = std::array::from_fn(|_| Vec::new());
    for (i, &base) in channel_order.iter().enumerate() {
        let slot = match base.to_ascii_uppercase() {
            b'A' => 0,
            b'C' => 1,
            b'G' => 2,
            b'T' => 3,
            _ => return Err(bad("invalid channel order")),
        };
        if !channels[slot].is_empty() {
            return Err(bad("duplicate channel in FWO_1"));
        }
        let data_tag = entries
            .get(&(
                *b"DATA",
                9 + u32::try_from(i).map_err(|_| bad("channel index exceeds u32"))?,
            ))
            .ok_or_else(|| bad("missing processed DATA9..12"))?;
        channels[slot] = integers(data, data_tag)?;
    }
    let samples = channels[0].len();
    if samples == 0 || channels.iter().any(|channel| channel.len() != samples) {
        return Err(bad("processed channels have unequal or empty lengths"));
    }
    let mut call_sets = Vec::new();
    for number in [1, 2] {
        let (Some(base_entry), Some(position_entry)) = (
            entries.get(&(*b"PBAS", number)),
            entries.get(&(*b"PLOC", number)),
        ) else {
            continue;
        };
        let bases = payload(data, base_entry)?.to_vec();
        let positions = integers(data, position_entry)?;
        if bases.is_empty()
            || bases.len() != positions.len()
            || bases.iter().any(|base| !base.is_ascii_alphabetic())
            || positions.iter().any(|&position| {
                usize::try_from(position).map_or(true, |position| position >= samples)
            })
            || positions.windows(2).any(|window| window[0] >= window[1])
        {
            return Err(bad(format!("invalid PBAS/PLOC{number} call list")));
        }
        let qualities = entries
            .get(&(*b"PCON", number))
            .map(|entry| integers(data, entry))
            .transpose()?
            .map(|values| {
                values
                    .into_iter()
                    .map(|value| u8::try_from(value).map_err(|_| bad("quality outside 0..255")))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        if qualities.as_ref().is_some_and(|q| q.len() != bases.len()) {
            return Err(bad(format!("PCON{number} length differs from calls")));
        }
        call_sets.push(CallSet {
            number,
            bases: bases
                .into_iter()
                .map(|base| base.to_ascii_uppercase())
                .collect(),
            positions: positions
                .into_iter()
                .map(|position| u32::try_from(position).map_err(|_| bad("negative call position")))
                .collect::<Result<Vec<_>>>()?,
            qualities,
        });
    }
    if call_sets.is_empty() {
        return Err(bad("missing PBAS/PLOC calls"));
    }
    Ok(Trace {
        channels,
        call_sets,
        channel_order,
    })
}

pub fn open(path: &Path) -> Result<Trace> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_FILE {
        return Err(bad("trace exceeds 128 MiB limit"));
    }
    parse(&std::fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_from_three_abi_instruments() {
        let dir = Path::new(
            "/private/local-path",
        );
        for (file, calls, samples) in [
            ("310.ab1", 868, 9826),
            ("3100.ab1", 795, 10303),
            ("3730.ab1", 1165, 16302),
        ] {
            let path = dir.join(file);
            if !path.exists() {
                continue;
            }
            let trace = open(&path).unwrap();
            assert_eq!(trace.display_calls().bases.len(), calls);
            assert_eq!(trace.samples(), samples);
            assert!(trace.display_calls().qualities.is_some());
        }
    }

    #[test]
    fn malformed_inputs_are_rejected() {
        assert!(parse(b"not ABIF").is_err());
        let mut bytes = vec![0; 34];
        bytes[..4].copy_from_slice(b"ABIF");
        assert!(parse(&bytes).is_err());
    }
}
