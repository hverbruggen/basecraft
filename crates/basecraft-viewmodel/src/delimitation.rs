//! Species hypotheses as bar columns on the tree canvas, with stable IDs.
#![allow(clippy::cast_precision_loss)]
use crate::{
    Rgba,
    svg::escape,
    theme::Theme,
    tree::{Figure, TipBand, TipColumn},
};
use basecraft_protocol::delimitation::{
    DelimitationWorkspace, Group, Hypothesis, HypothesisOrigin,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

/// Default bar width in pixels; each column can be set wider or narrower.
pub const BAR_WIDTH: u32 = 9;
pub const MIN_BAR_WIDTH: u32 = 5;
pub const MAX_BAR_WIDTH: u32 = 200;
/// Unset gaps scale with the bar width: across to the next column, and
/// down between one species' bar and the next.
pub const GAP_PER_WIDTH: f32 = 1.5;
pub const SPACING_PER_WIDTH: f32 = 0.65;
/// The largest gap or spacing a column can be given, in pixels.
pub const MAX_GAP: u32 = 400;
/// Pixels between the tip names and the first column.
const FIRST_GAP: f32 = 10.0;

/// Theme colours in the order groups receive them.
#[must_use]
pub fn palette(theme: &Theme) -> Vec<String> {
    [
        theme.blue,
        theme.red,
        theme.green,
        theme.orange,
        theme.magenta,
        theme.cyan,
        theme.yellow,
        theme.brown,
    ]
    .map(hex)
    .to_vec()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn hex(c: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c[0]), byte(c[1]), byte(c[2]))
}

/// A user colour from the view, else the theme palette by group position.
fn color(
    work: &DelimitationWorkspace,
    h: &Hypothesis,
    group: &Group,
    index: usize,
    palette: &[String],
) -> String {
    work.view
        .columns
        .get(&h.id)
        .and_then(|style| style.colors.get(&group.id))
        .cloned()
        .or_else(|| palette.get(index % palette.len().max(1)).cloned())
        .unwrap_or_else(|| "#808080".into())
}

/// One-letter column code and the method it stands for.
#[must_use]
pub fn origin(h: &Hypothesis) -> (&'static str, &'static str) {
    let method = h.provenance.method.to_ascii_lowercase();
    match h.origin {
        HypothesisOrigin::Manual => ("M", "Manual"),
        HypothesisOrigin::ManualCut => ("T", "Manual threshold cut"),
        HypothesisOrigin::Derived => ("E", "Edited copy"),
        _ if method == "asap" => ("A", "ASAP"),
        _ if method.starts_with("gmyc") => ("G", "GMYC"),
        _ if method == "ptp" || method == "mptp" => ("P", "PTP"),
        _ => ("I", "Imported"),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub hypothesis: String,
    pub x: f32,
    pub width: f32,
    /// Pixels from the previous column; the first column has none.
    pub gap: f32,
    /// Pixels between successive species bars.
    pub spacing: f32,
}

/// Visible hypothesis columns, left to right, starting at `left`.
#[must_use]
pub fn columns(work: &DelimitationWorkspace, left: f32) -> Vec<Column> {
    let mut x = left;
    let mut out = Vec::new();
    for id in &work.view.visible {
        if !work.hypotheses.iter().any(|h| &h.id == id) {
            continue;
        }
        let style = work.view.columns.get(id);
        let width = style
            .and_then(|style| style.width)
            .unwrap_or(BAR_WIDTH)
            .clamp(MIN_BAR_WIDTH, MAX_BAR_WIDTH) as f32;
        let set = |value: Option<u32>, per_width: f32| {
            value.map_or(width * per_width, |v| v.min(MAX_GAP) as f32)
        };
        let gap = set(style.and_then(|s| s.gap), GAP_PER_WIDTH);
        let spacing = set(style.and_then(|s| s.spacing), SPACING_PER_WIDTH);
        if !out.is_empty() {
            x += gap;
        }
        out.push(Column {
            hypothesis: id.clone(),
            x,
            width,
            gap,
            spacing,
        });
        x += width;
    }
    out
}

#[derive(Default)]
pub struct Selection {
    pub specimens: BTreeSet<String>,
    pub origin: Option<String>,
}
impl Selection {
    /// Same-origin echoes do not replace the exact disjoint selection.
    pub fn receive(&mut self, ids: BTreeSet<String>, origin: &str) -> bool {
        if self.origin.as_deref() == Some(origin) && self.specimens == ids {
            return false;
        }
        self.specimens = ids;
        self.origin = Some(origin.into());
        true
    }
    pub fn group(&mut self, h: &Hypothesis, group: &str, toggle: bool) -> Result<(), String> {
        let group = h
            .groups
            .iter()
            .find(|g| g.id == group)
            .ok_or("unknown group")?;
        if toggle {
            for id in &group.members {
                if !self.specimens.remove(id) {
                    self.specimens.insert(id.clone());
                }
            }
        } else {
            self.specimens.clone_from(&group.members);
        }
        self.origin = Some("bars".into());
        Ok(())
    }
}

/// Each row can represent a single specimen or an explicitly saved haplotype map.
pub fn mapping(work: &DelimitationWorkspace, name: &str) -> BTreeSet<String> {
    if work.specimens.iter().any(|s| s.id == name) {
        return [name.into()].into();
    }
    work.hypotheses
        .iter()
        .find_map(|h| h.provenance.haplotypes.get(name))
        .cloned()
        .unwrap_or_default()
}

/// Full-specimen displays keep one ID per tip. Unique-tip displays expand only
/// when their complete tip set matches a saved representative map exactly.
/// Subset/name coincidences alone do not establish haplotype identity.
pub fn tree_mapping(
    work: &DelimitationWorkspace,
    names: &BTreeSet<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let roster: BTreeSet<_> = work.specimens.iter().map(|s| s.id.clone()).collect();
    if names != &roster
        && let Some(mapping) = work
            .hypotheses
            .iter()
            .map(|h| &h.provenance.haplotypes)
            .find(|m| !m.is_empty() && m.keys().cloned().collect::<BTreeSet<_>>() == *names)
    {
        return mapping.clone();
    }
    names
        .iter()
        .filter(|id| roster.contains(*id))
        .map(|id| (id.clone(), [id.clone()].into()))
        .collect()
}

/// The visible hypotheses as bar columns for the tree canvas: each tip gets
/// its species, split in proportion when the specimens of one unique tip
/// belong to several.
#[must_use]
pub fn tip_columns(
    work: &DelimitationWorkspace,
    tips: &BTreeSet<String>,
    palette: &[String],
) -> Vec<TipColumn> {
    let mapping = tree_mapping(work, tips);
    columns(work, 0.0)
        .into_iter()
        .enumerate()
        .filter_map(|(k, column)| {
            let h = work.hypotheses.iter().find(|h| h.id == column.hypothesis)?;
            let membership: BTreeMap<_, _> = h
                .groups
                .iter()
                .enumerate()
                .flat_map(|(i, g)| g.members.iter().map(move |m| (m, (g, i))))
                .collect();
            let mut bands = std::collections::HashMap::new();
            for (tip, ids) in &mapping {
                let mut counts = BTreeMap::<usize, (&Group, usize)>::new();
                for id in ids {
                    if let Some(&(group, index)) = membership.get(id) {
                        counts.entry(index).or_insert((group, 0)).1 += 1;
                    }
                }
                let row: Vec<TipBand> = counts
                    .into_iter()
                    .map(|(index, (group, count))| TipBand {
                        id: group.id.clone(),
                        share: count as f32,
                        width: column.width,
                        color: color(work, h, group, index, palette),
                        label: group.label.clone(),
                    })
                    .collect();
                if !row.is_empty() {
                    bands.insert(tip.clone(), row);
                }
            }
            Some(TipColumn {
                id: h.id.clone(),
                // The first column keeps clear of the tip names.
                gap: if k == 0 { FIRST_GAP } else { column.gap },
                spacing: Some(column.spacing),
                round: true,
                labels: work.view.columns.get(&h.id).is_some_and(|s| s.names),
                vertical_labels: false,
                tips: bands,
            })
        })
        .collect()
}

/// The comparison figure: the tree with its bar columns, a letter above each
/// column and a legend of the letters below.
#[must_use]
pub fn comparison_svg(work: &DelimitationWorkspace, figure: &Figure, font: &str) -> String {
    // Column letters sit above the bars; the legend explains them below.
    const TOP: f32 = 20.0;
    let shown: Vec<(&Hypothesis, f32)> = figure
        .columns
        .iter()
        .filter_map(|(id, x, w)| Some((work.hypotheses.iter().find(|h| &h.id == id)?, x + w / 2.0)))
        .collect();
    let mut methods: Vec<(&str, &str)> = shown.iter().map(|(h, _)| origin(h)).collect();
    methods.sort_unstable();
    methods.dedup();
    let longest = shown
        .iter()
        .map(|(h, _)| h.name.chars().count())
        .max()
        .unwrap_or(0);
    let width = figure.width.max(longest as f32 * 7.0 + 40.0);
    let total = TOP + figure.height + 30.0 + (shown.len() + 1) as f32 * 14.0;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{total}\" font-family=\"{}\">\n<g id=\"tree\" transform=\"translate(0,{TOP})\">{}</g>\n",
        escape(font),
        figure.svg
    );
    for (h, x) in &shown {
        writeln!(
            svg,
            "<text x=\"{x}\" y=\"{}\" font-size=\"11\" text-anchor=\"middle\"><title>{}</title>{}</text>",
            TOP - 6.0,
            escape(&h.name),
            origin(h).0
        )
        .unwrap();
    }
    let legend = methods
        .iter()
        .map(|(code, name)| format!("{code} {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut y = TOP + figure.height + 24.0;
    writeln!(
        svg,
        "<text x=\"8\" y=\"{y}\" font-size=\"11\">{}</text>",
        escape(&legend)
    )
    .unwrap();
    for (h, _) in &shown {
        y += 14.0;
        writeln!(
            svg,
            "<text x=\"8\" y=\"{y}\" font-size=\"11\">{}: {}</text>",
            origin(h).0,
            escape(&h.name)
        )
        .unwrap();
    }
    svg.push_str("</svg>\n");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{TreeView, figure_with_columns};
    use basecraft_docs::TreeDoc;

    fn workspace() -> DelimitationWorkspace {
        basecraft_io::spart::parse_text(include_str!(
            "../../../testdata/fixtures/delimitation/manual.spart"
        ))
        .unwrap()
    }

    fn tips(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    #[test]
    fn representatives_expand_only_on_the_unique_tip_display() {
        let mut work = workspace();
        work.hypotheses[0].provenance.haplotypes = [
            ("a".into(), ["a".into(), "b".into()].into()),
            ("c".into(), ["c".into(), "d".into()].into()),
        ]
        .into();
        let unique = tree_mapping(&work, &tips(&["a", "c"]));
        assert_eq!(unique["a"], ["a".into(), "b".into()].into());
        let full = tree_mapping(&work, &tips(&["a", "b", "c", "d"]));
        assert_eq!(full["a"], ["a".into()].into());
        work.view.visible = vec!["partition-1".into()];
        // A unique tip whose specimens are in two species shows both, in
        // equal shares.
        let columns = tip_columns(&work, &tips(&["a", "c"]), &palette(&Theme::default()));
        let row = &columns[0].tips["a"];
        assert_eq!(row.len(), 2);
        assert!(
            row.iter()
                .all(|band| (band.share - 1.0).abs() < f32::EPSILON)
        );
        assert_ne!(row[0].id, row[1].id);
    }

    #[test]
    fn columns_follow_saved_widths_and_colours() {
        use basecraft_protocol::delimitation::ColumnStyle;
        let mut work = workspace();
        work.view.visible = vec!["partition-1".into(), "partition-2".into()];
        let first = work.hypotheses[0].id.clone();
        let group = work.hypotheses[0].groups[0].id.clone();
        work.view.columns.insert(
            first.clone(),
            ColumnStyle {
                width: Some(2),
                colors: [(group.clone(), "#123456".into())].into(),
                ..ColumnStyle::default()
            },
        );
        let laid = columns(&work, 10.0);
        // Widths below the minimum are raised to it.
        assert_eq!(laid[0].width, MIN_BAR_WIDTH as f32);
        // A gap is the space before a column. Unset, it scales with the
        // column's own bar width; set, it is pixels.
        assert_eq!(
            laid[1].x,
            10.0 + MIN_BAR_WIDTH as f32 + 1.5 * BAR_WIDTH as f32
        );
        assert!((laid[1].spacing - 0.65 * BAR_WIDTH as f32).abs() < 1e-4);
        let mut spaced = work.clone();
        spaced.view.columns.get_mut(&first).unwrap().gap = Some(30);
        assert_eq!(
            columns(&spaced, 10.0)[0].x,
            10.0,
            "The first column has no gap"
        );
        spaced
            .view
            .columns
            .entry(work.view.visible[1].clone())
            .or_default()
            .gap = Some(3);
        assert_eq!(
            columns(&spaced, 10.0)[1].x,
            10.0 + MIN_BAR_WIDTH as f32 + 3.0
        );
        let colors = palette(&Theme::default());
        let columns = tip_columns(&work, &tips(&["a", "b", "c", "d"]), &colors);
        assert_eq!(columns[0].tips["a"][0].color, "#123456");
        assert!(
            columns[1]
                .tips
                .values()
                .flatten()
                .all(|band| colors.contains(&band.color))
        );
    }

    #[test]
    fn species_bars_and_names_follow_the_drawn_tree() {
        let mut work = workspace();
        work.view.visible = vec!["partition-1".into()];
        work.hypotheses[0].groups[0].label = "Species alpha".into();
        let colors = palette(&Theme::default());
        let all = tips(&["a", "b", "c", "d"]);
        // Drawn a, c, b, d: each species of partition 1 is one block.
        let tree = TreeDoc::from_newick("t", "((a:1,c:1):1,(b:1,d:1):1);").unwrap();
        let mut view = TreeView::new(600.0, 200.0);
        view.set_columns(tip_columns(&work, &all, &colors));
        view.sync(&tree);
        let bars = view.bars();
        assert_eq!(bars.len(), 2, "{bars:?}");
        // Touching rows of a species are one bar, less the spacing between bars.
        let spacing = 0.65 * BAR_WIDTH as f32;
        assert!((bars[1].y - bars[0].y - bars[0].h - spacing).abs() < 1e-3);
        let (x, y) = (bars[0].x + 1.0, bars[0].y + 1.0);
        let hit = view.bar_at(x, y).unwrap();
        assert_eq!(hit.column, "partition-1");
        assert_eq!(hit.id, work.hypotheses[0].groups[0].id);
        assert_eq!(view.column_positions()[0].0, "partition-1");

        // Group 1 is a and c: adjacent here, so one name; drawn a, b, c, d
        // it is split, with a name beside each part.
        let svg = |tree: &TreeDoc, work: &DelimitationWorkspace| {
            let figure = figure_with_columns(
                tree,
                &tip_columns(work, &all, &colors),
                600.0,
                20.0,
                &Theme::default(),
                "A&B",
            );
            comparison_svg(work, &figure, "A&B")
        };
        assert!(!svg(&tree, &work).contains(">Species alpha</text>"));
        work.view
            .columns
            .entry("partition-1".into())
            .or_default()
            .names = true;
        assert_eq!(
            svg(&tree, &work).matches(">Species alpha</text>").count(),
            1
        );
        let split = TreeDoc::from_newick("t", "((a:1,b:1):1,(c:1,d:1):1);").unwrap();
        let figure = svg(&split, &work);
        assert_eq!(figure.matches(">Species alpha</text>").count(), 2);
        assert!(figure.contains("A&amp;B"));
        // Rounded bars, and the column's letter above it.
        assert!(figure.contains(" rx=\""));
        assert!(figure.contains("<title>first</title>"));
    }

    #[test]
    fn noncontiguous_groups_select_all_their_members() {
        let work = workspace();
        let mut selection = Selection::default();
        let h = &work.hypotheses[0];
        selection.group(h, &h.groups[0].id, false).unwrap();
        assert_eq!(selection.specimens, ["a".into(), "c".into()].into());
    }
}
