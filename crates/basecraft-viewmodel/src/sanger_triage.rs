//! Explicit signal and score thresholds for review triage. Ratios and Tracy
//! scores are evidence filters, never calibrated probabilities.
use basecraft_docs::alignment::dna_mask;
use basecraft_docs::sanger::ReviewSettings;

#[derive(Clone, Debug)]
pub struct CallEvidence {
    pub called: u8,
    pub score: i32,
    pub peaks: [u32; 4],
}
impl CallEvidence {
    pub fn primary(&self) -> usize {
        (0..4)
            .max_by_key(|&i| (self.peaks[i], std::cmp::Reverse(i)))
            .unwrap()
    }
    pub fn secondary_percent(&self) -> f64 {
        let i = self.primary();
        let second = (0..4)
            .filter(|&j| j != i)
            .map(|j| self.peaks[j])
            .max()
            .unwrap();
        100.0 * f64::from(second) / f64::from(self.peaks[i].max(1))
    }
    fn clean(&self, settings: &ReviewSettings) -> bool {
        let primary = self.primary();
        self.peaks[primary] > 0
            && dna_mask(self.called) == 1 << primary
            && self.secondary_percent() < f64::from(settings.ambiguity_percent)
    }
}
#[derive(Clone, Debug)]
pub struct Assessment {
    pub call: Option<char>,
    pub suggested: Option<char>,
    pub reason: String,
}
impl Assessment {
    pub fn issue(reason: impl Into<String>) -> Self {
        Self {
            call: None,
            suggested: None,
            reason: reason.into(),
        }
    }
}

pub fn assess(
    settings: &ReviewSettings,
    reads: &[CallEvidence],
    reference: Option<[u32; 4]>,
) -> Assessment {
    if reads.is_empty() {
        return Assessment::issue("No mapped read evidence");
    }
    let mut candidates = Vec::new();
    for base in 0..4 {
        let clean: Vec<_> = reads
            .iter()
            .filter(|r| r.primary() == base && r.clean(settings))
            .collect();
        if clean.iter().any(|r| r.score >= settings.strong_score)
            || clean
                .iter()
                .filter(|r| r.score >= settings.paired_score)
                .count()
                >= 2
        {
            candidates.push(base);
        }
    }
    if candidates.len() != 1 {
        return Assessment::issue(if candidates.len() > 1 {
            "Confident reads disagree"
        } else {
            "Insufficient clear evidence: need one clean read at the strong score, or two agreeing clean reads at the paired score"
        });
    }
    let base = candidates[0];
    let letter = b"ACGT"[base] as char;
    for read in reads {
        if read.clean(settings) && read.primary() == base {
            continue;
        }
        let compatible = settings.allow_weak_disagreement
            && read.peaks[read.primary()] > 0
            && u64::from(read.peaks[base]) * 100
                >= u64::from(read.peaks[read.primary()]) * u64::from(settings.weak_support_percent)
            && !(read.clean(settings) && read.score >= settings.paired_score);
        if !compatible {
            return Assessment {
                call: None,
                suggested: Some(letter),
                reason: format!(
                    "{} is best supported, but an opposing or ambiguous read fails the {}% candidate/primary peak requirement",
                    letter, settings.weak_support_percent
                ),
            };
        }
    }
    if settings.check_reference
        && let Some(counts) = reference
    {
        let total: u64 = counts.iter().map(|&n| u64::from(n)).sum();
        if total < u64::from(settings.reference_min_rows)
            || counts[base] == 0
            || u64::from(counts[base]) * 100 < total * u64::from(settings.reference_percent)
        {
            return Assessment {
                call: None,
                suggested: Some(letter),
                reason: format!(
                    "{} is supported by traces, but reference support is {}/{} A/C/G/T rows (require ≥{} rows and ≥{}%; at least one occurrence)",
                    letter,
                    counts[base],
                    total,
                    settings.reference_min_rows,
                    settings.reference_percent
                ),
            };
        }
    }
    Assessment {
        call: Some(letter),
        suggested: Some(letter),
        reason: format!(
            "Automatic {}: sufficient clean evidence; other covering reads are compatible{}",
            letter,
            if settings.check_reference && reference.is_some() {
                "; whole-alignment frequency passes"
            } else {
                ""
            }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read(base: u8, score: i32, peaks: [u32; 4]) -> CallEvidence {
        CallEvidence {
            called: base,
            score,
            peaks,
        }
    }
    #[test]
    fn near_tie_does_not_overrule_confident_call_but_real_conflicts_do() {
        let settings = ReviewSettings::default();
        let a = read(b'A', 35, [100, 5, 5, 5]);
        let weak_g = read(b'G', 10, [95, 5, 100, 5]);
        assert_eq!(
            assess(&settings, &[a.clone(), weak_g.clone()], None).call,
            Some('A')
        );
        let strict = ReviewSettings {
            allow_weak_disagreement: false,
            ..settings.clone()
        };
        assert_eq!(assess(&strict, &[a.clone(), weak_g], None).call, None);
        assert_eq!(
            assess(&settings, &[a, read(b'G', 35, [5, 5, 100, 5])], None).call,
            None
        );
    }
    #[test]
    fn score_ambiguity_and_compatibility_thresholds_are_independent() {
        let defaults = ReviewSettings::default();
        let a = read(b'A', 25, [100, 5, 30, 5]);
        assert!(
            assess(&defaults, std::slice::from_ref(&a), None)
                .call
                .is_none()
        );
        let lower = ReviewSettings {
            strong_score: 25,
            ..defaults.clone()
        };
        assert_eq!(
            assess(&lower, std::slice::from_ref(&a), None).call,
            Some('A')
        );
        let sensitive = ReviewSettings {
            ambiguity_percent: 25,
            ..lower.clone()
        };
        assert!(
            assess(&sensitive, std::slice::from_ref(&a), None)
                .call
                .is_none()
        );
        assert_eq!(
            assess(&defaults, &[a.clone(), a.clone()], None).call,
            Some('A')
        );
        let weak = read(b'G', 10, [79, 0, 100, 0]);
        assert!(
            assess(&lower, &[a.clone(), weak.clone()], None)
                .call
                .is_none()
        );
        let lenient = ReviewSettings {
            weak_support_percent: 75,
            ..lower
        };
        assert_eq!(assess(&lenient, &[a, weak], None).call, Some('A'));
    }
    #[test]
    fn reference_uses_minority_counts_not_majority_consensus() {
        let settings = ReviewSettings::default();
        let a = read(b'A', 35, [100, 1, 1, 1]);
        assert_eq!(
            assess(&settings, std::slice::from_ref(&a), Some([1, 0, 99, 0])).call,
            Some('A')
        );
        let strict = ReviewSettings {
            reference_percent: 2,
            ..settings.clone()
        };
        assert!(
            assess(&strict, std::slice::from_ref(&a), Some([1, 0, 99, 0]))
                .call
                .is_none()
        );
        assert!(
            assess(&settings, std::slice::from_ref(&a), Some([0, 0, 99, 0]))
                .call
                .is_none()
        );
        assert!(
            assess(&settings, std::slice::from_ref(&a), Some([0; 4]))
                .call
                .is_none()
        );
        let off = ReviewSettings {
            check_reference: false,
            ..settings
        };
        assert_eq!(assess(&off, &[a], Some([0, 0, 99, 0])).call, Some('A'));
    }
    #[test]
    fn ambiguous_or_absent_signals_cannot_supply_a_confident_anchor() {
        assert!(
            assess(
                &ReviewSettings::default(),
                &[read(b'A', 40, [100, 0, 99, 0])],
                None
            )
            .call
            .is_none()
        );
        assert!(
            assess(&ReviewSettings::default(), &[read(b'A', 40, [0; 4])], None)
                .call
                .is_none()
        );
    }
}
