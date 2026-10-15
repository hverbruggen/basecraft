//! Conservative mapping of proposed calls to original AB1 call events.
//! A proposal maps only when an exact base pair belongs to every optimal
//! semi-global edit alignment. Repeats and changed calls remain unmapped.

/// One proposed, oriented base maps to one original vendor event, or None.
pub fn unique_event_map(proposed: &[u8], vendor: &[u8]) -> Vec<Option<usize>> {
    event_map(proposed, vendor, false)
}

/// Map two Tracy call representations to the same event, including unique
/// substitutions. Unlike vendor calls, both sequences come from this engine.
pub fn tracy_event_map(proposed: &[u8], called: &[u8]) -> Vec<Option<usize>> {
    event_map(proposed, called, true)
}

fn event_map(proposed: &[u8], vendor: &[u8], substitutions: bool) -> Vec<Option<usize>> {
    let n = proposed.len();
    let m = vendor.len();
    // Typical Sanger calls are under 1500. Never allocate unbounded quadratic
    // matrices for malformed traces or unexpectedly long imported sequences.
    if n == 0 || m == 0 || n > 5000 || m > 5000 {
        return vec![None; n];
    }
    let stride = m + 1;
    let mut forward = vec![0u16; (n + 1) * stride];
    let mut backward = vec![0u16; (n + 1) * stride];
    for i in 1..=n {
        forward[i * stride] = forward[(i - 1) * stride] + 1;
    }
    for i in 1..=n {
        for j in 1..=m {
            let diagonal =
                forward[(i - 1) * stride + j - 1] + u16::from(proposed[i - 1] != vendor[j - 1]);
            let remove_proposed = forward[(i - 1) * stride + j] + 1;
            let remove_vendor = forward[i * stride + j - 1] + 1;
            forward[i * stride + j] = diagonal.min(remove_proposed).min(remove_vendor);
        }
    }
    // Free vendor suffix, and no free proposed suffix.
    for i in (0..n).rev() {
        backward[i * stride + m] = backward[(i + 1) * stride + m] + 1;
    }
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let diagonal = backward[(i + 1) * stride + j + 1] + u16::from(proposed[i] != vendor[j]);
            let remove_proposed = backward[(i + 1) * stride + j] + 1;
            let remove_vendor = backward[i * stride + j + 1] + 1;
            backward[i * stride + j] = diagonal.min(remove_proposed).min(remove_vendor);
        }
    }
    let best = *forward[n * stride..(n + 1) * stride]
        .iter()
        .min()
        .expect("nonempty final row");
    let mut map = vec![None; n];
    for i in 0..n {
        for j in 0..m {
            if (substitutions || proposed[i] == vendor[j])
                && forward[i * stride + j]
                    + u16::from(proposed[i] != vendor[j])
                    + backward[(i + 1) * stride + j + 1]
                    == best
            {
                if map[i].is_some() {
                    map[i] = Some(m);
                    break;
                }
                map[i] = Some(j);
            }
        }
        // A sole exact candidate is still unsafe if another optimal path
        // deletes this proposed event or aligns it to a different base.
        let skipped =
            (0..=m).any(|j| forward[i * stride + j] + 1 + backward[(i + 1) * stride + j] == best);
        let mismatched = (0..m).any(|j| {
            proposed[i] != vendor[j]
                && forward[i * stride + j] + 1 + backward[(i + 1) * stride + j + 1] == best
        });
        if map[i] == Some(m) || skipped || (!substitutions && mismatched) {
            map[i] = None;
        }
    }
    map
}

/// Reverse-complement the original call sequence for a reverse-oriented row.
/// A mapped oriented event index converts back to original AB1 index via
/// `vendor.len() - 1 - index`; its sample coordinate remains the AB1 PLOC.
pub fn reverse_complement(calls: &[u8]) -> Vec<u8> {
    calls
        .iter()
        .rev()
        .map(|&base| match base {
            b'A' => b'T',
            b'C' => b'G',
            b'G' => b'C',
            b'T' => b'A',
            b'R' => b'Y',
            b'Y' => b'R',
            b'K' => b'M',
            b'M' => b'K',
            b'B' => b'V',
            b'V' => b'B',
            b'D' => b'H',
            b'H' => b'D',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_unique_calls_retain_original_indices() {
        assert_eq!(
            unique_event_map(b"CGTA", b"AACGTATT"),
            vec![Some(2), Some(3), Some(4), Some(5)]
        );
    }

    #[test]
    fn repeat_placement_and_changed_base_remain_unmapped() {
        assert_eq!(unique_event_map(b"AAA", b"AAAA"), vec![None, None, None]);
        assert_eq!(unique_event_map(b"AA", b"A"), vec![None, None]);
        assert_eq!(
            unique_event_map(b"ACGT", b"ACCT"),
            vec![Some(0), Some(1), None, Some(3)]
        );
    }

    #[test]
    fn reverse_orientation_retains_original_coordinate_semantics() {
        assert_eq!(reverse_complement(b"ACGTMR"), b"YKACGT");
        let oriented = reverse_complement(b"AACCGT");
        let map = unique_event_map(b"ACGG", &oriented);
        assert_eq!(map.len(), 4);
    }
}
