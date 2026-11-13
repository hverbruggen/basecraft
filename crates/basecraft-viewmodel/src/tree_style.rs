//! The support-value rows of the tree style dialog: built from the tree's
//! support sets and style, and folded back into the style on save.
use basecraft_protocol::TreeStyle;
use serde::{Deserialize, Serialize};

/// A support set as the tree reports it.
pub struct SupportSet<'a> {
    pub name: &'a str,
    pub source: &'a str,
    /// Decimals the set shows now.
    pub decimals: u8,
    /// Decimals the set shows when left automatic.
    pub automatic_decimals: u8,
    /// Whether the set shows proportions unless chosen otherwise.
    pub proportions: bool,
}

/// One row of the dialog.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportRow {
    pub name: String,
    pub source: String,
    pub shown: bool,
    pub decimals: u8,
    pub auto_default: u8,
    /// 1 shows the values as proportions, 0 as they are.
    pub scale: u8,
    pub minimum: u32,
    pub auto_decimals: bool,
}

/// Shown sets first, in their order, then the hidden ones.
pub fn rows(sets: &[SupportSet], style: &TreeStyle) -> Vec<SupportRow> {
    let shown: Vec<&str> = if style.support_shown.is_empty() {
        sets.iter().map(|set| set.name).collect()
    } else {
        style.support_shown.iter().map(String::as_str).collect()
    };
    let row = |set: &SupportSet, shown: bool| SupportRow {
        name: set.name.to_owned(),
        source: set.source.to_owned(),
        shown,
        decimals: set.decimals,
        auto_default: set.automatic_decimals,
        scale: u8::from(
            *style
                .support_fraction
                .get(set.name)
                .unwrap_or(&set.proportions),
        ),
        minimum: *style
            .support_minimums
            .get(set.name)
            .unwrap_or(&style.support_min),
        auto_decimals: !style.support_decimals.contains_key(set.name),
    };
    let mut out: Vec<SupportRow> = shown
        .iter()
        .filter_map(|name| sets.iter().find(|set| set.name == *name))
        .map(|set| row(set, true))
        .collect();
    out.extend(
        sets.iter()
            .filter(|set| !shown.contains(&set.name))
            .map(|set| row(set, false)),
    );
    out
}

/// Fold the dialog's rows into `style`, forgetting the sets in `removed`.
pub fn apply(style: &mut TreeStyle, rows: &[SupportRow], removed: &[String]) {
    style.support_shown = rows
        .iter()
        .filter(|row| row.shown)
        .map(|row| row.name.clone())
        .collect();
    for row in rows {
        if row.auto_decimals {
            style.support_decimals.remove(&row.name);
        } else {
            style
                .support_decimals
                .insert(row.name.clone(), row.decimals);
        }
        style
            .support_fraction
            .insert(row.name.clone(), row.scale == 1);
        style.support_minimums.insert(row.name.clone(), row.minimum);
    }
    for name in removed {
        style.support_decimals.remove(name);
        style.support_proportions.remove(name);
        style.support_fraction.remove(name);
        style.support_minimums.remove(name);
    }
    if style
        .symbols
        .as_ref()
        .is_some_and(|symbols| removed.contains(&symbols.set))
    {
        style.symbols = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sets() -> [SupportSet<'static>; 2] {
        [
            SupportSet {
                name: "Bootstrap",
                source: "",
                decimals: 0,
                automatic_decimals: 0,
                proportions: false,
            },
            SupportSet {
                name: "BPP",
                source: "",
                decimals: 2,
                automatic_decimals: 2,
                proportions: true,
            },
        ]
    }

    #[test]
    fn rows_list_shown_sets_in_order_then_hidden_ones() {
        let mut style = TreeStyle::default();
        let all = rows(&sets(), &style);
        assert_eq!(
            all.iter()
                .map(|r| (r.name.as_str(), r.shown))
                .collect::<Vec<_>>(),
            [("Bootstrap", true), ("BPP", true)]
        );
        assert_eq!((all[0].scale, all[1].scale), (0, 1));
        assert!(all.iter().all(|r| r.auto_decimals && r.minimum == 0));
        style.support_shown = vec!["BPP".into()];
        style.support_minimums.insert("BPP".into(), 90);
        style.support_decimals.insert("Bootstrap".into(), 1);
        let ordered = rows(&sets(), &style);
        assert_eq!(
            ordered
                .iter()
                .map(|r| (r.name.as_str(), r.shown))
                .collect::<Vec<_>>(),
            [("BPP", true), ("Bootstrap", false)]
        );
        assert_eq!(ordered[0].minimum, 90);
        assert!(!ordered[1].auto_decimals);
    }

    #[test]
    fn saving_keeps_automatic_defaults_and_forgets_removed_sets() {
        let mut style = TreeStyle::default();
        let mut edited = rows(&sets(), &style);
        apply(&mut style, &edited, &[]);
        assert!(style.support_decimals.is_empty() && style.support_proportions.is_empty());
        assert!(!style.support_fraction["Bootstrap"]);
        assert!(style.support_fraction["BPP"]);
        assert_eq!(style.support_minimums["Bootstrap"], 0);
        edited[0].auto_decimals = false;
        edited[0].decimals = 1;
        edited[1].scale = 0;
        edited[1].minimum = 90;
        apply(&mut style, &edited, &[]);
        assert_eq!(style.support_decimals["Bootstrap"], 1);
        assert!(!style.support_decimals.contains_key("BPP"));
        assert_eq!(
            (style.support_fraction["BPP"], style.support_minimums["BPP"]),
            (false, 90)
        );
        style.symbols = Some(basecraft_protocol::SupportSymbols {
            set: "BPP".into(),
            high: 90,
            low: 70,
        });
        apply(&mut style, &edited[..1], &["BPP".into()]);
        assert!(!style.support_fraction.contains_key("BPP") && style.symbols.is_none());
        assert_eq!(style.support_shown, ["Bootstrap"]);
    }
}
