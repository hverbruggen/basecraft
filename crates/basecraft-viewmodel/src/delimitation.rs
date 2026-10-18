//! Partition bars use the tree canvas's exact row geometry and stable IDs.
#![allow(clippy::cast_precision_loss)]
use crate::{Rgba, svg::escape, theme::Theme};
use basecraft_docs::TreeDoc;
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
        let width = work
            .view
            .columns
            .get(id)
            .and_then(|style| style.width)
            .unwrap_or(BAR_WIDTH)
            .clamp(MIN_BAR_WIDTH, MAX_BAR_WIDTH) as f32;
        out.push(Column {
            hypothesis: id.clone(),
            x,
            width,
        });
        let gap = work
            .view
            .bar_gap
            .map_or(width, |gap| gap.min(MAX_BAR_WIDTH) as f32);
        x += width + gap;
    }
    out
}

/// The key `DelimitationView::flipped` stores for the clade spanning `tips`
/// (for one tip, its parent): the first tip, by name, of two of its child
/// subtrees. Flipping anything leaves the key pointing at the same clade.
#[must_use]
pub fn flip_key(tree: &TreeDoc, tips: &[String]) -> Option<Vec<String>> {
    let chain = |mut node: usize| {
        let mut out = vec![node];
        while let Some(parent) = tree.node(node).parent {
            out.push(parent);
            node = parent;
        }
        out
    };
    let mut ancestors = chain(tree.find_tip(tips.first()?)?);
    for name in &tips[1..] {
        let other: BTreeSet<_> = chain(tree.find_tip(name)?).into_iter().collect();
        let keep = ancestors.iter().position(|n| other.contains(n))?;
        ancestors.drain(..keep);
    }
    let mut node = *ancestors.first()?;
    if tree.node(node).is_tip() {
        node = tree.node(node).parent?;
    }
    let first_tip = |start: usize| {
        let mut stack = vec![start];
        let mut best: Option<&str> = None;
        while let Some(n) = stack.pop() {
            let current = tree.node(n);
            if current.is_tip() {
                best = Some(best.map_or(current.name.as_str(), |b| b.min(current.name.as_str())));
            }
            stack.extend(&current.children);
        }
        best.map(str::to_string)
    };
    let mut firsts: Vec<String> = tree
        .node(node)
        .children
        .iter()
        .filter_map(|&c| first_tip(c))
        .collect();
    firsts.sort();
    firsts.truncate(2);
    (firsts.len() == 2).then_some(firsts)
}

/// Draw the stored clades flipped. Flips are independent of one another, so
/// their order does not matter; keys that no longer resolve are skipped.
pub fn apply_flips(tree: &mut TreeDoc, flipped: &BTreeSet<Vec<String>>) {
    for clade in flipped {
        let _ = tree.rotate(clade);
    }
}

/// One species name per run of touching bar segments of a column, centred on
/// the run: a species split by other tips gets a name beside each part.
#[must_use]
pub fn name_positions(segments: &[BarSegment], hypothesis: &str) -> Vec<(String, f32)> {
    let mut column: Vec<_> = segments
        .iter()
        .filter(|s| s.hypothesis == hypothesis)
        .collect();
    column.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut runs: Vec<(&BarSegment, f32, f32)> = Vec::new();
    for s in column {
        match runs.last_mut() {
            Some((first, _, bottom)) if first.group == s.group && s.y <= *bottom + 1.0 => {
                *bottom = s.y + s.height;
            }
            _ => runs.push((s, s.y, s.y + s.height)),
        }
    }
    runs.into_iter()
        .map(|(s, top, bottom)| (s.label.clone(), f32::midpoint(top, bottom)))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct BarSegment {
    pub hypothesis: String,
    pub group: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: String,
    pub label: String,
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
    if names != &roster {
        if let Some(mapping) = work
            .hypotheses
            .iter()
            .map(|h| &h.provenance.haplotypes)
            .find(|m| !m.is_empty() && m.keys().cloned().collect::<BTreeSet<_>>() == *names)
        {
            return mapping.clone();
        }
    }
    names
        .iter()
        .filter(|id| roster.contains(*id))
        .map(|id| (id.clone(), [id.clone()].into()))
        .collect()
}

pub fn bars(
    work: &DelimitationWorkspace,
    rows: &[(String, f32, f32)],
    left: f32,
    palette: &[String],
    viewport: Option<(f32, f32)>,
) -> (Vec<BarSegment>, BTreeSet<String>) {
    let mapping = tree_mapping(
        work,
        &rows.iter().map(|(name, _, _)| name.clone()).collect(),
    );
    let mut segments = Vec::new();
    let mut unmatched = BTreeSet::new();
    for column in columns(work, left) {
        let Some(h) = work.hypotheses.iter().find(|h| h.id == column.hypothesis) else {
            continue;
        };
        let membership: BTreeMap<_, _> = h
            .groups
            .iter()
            .enumerate()
            .flat_map(|(i, g)| g.members.iter().map(move |m| (m, (g, i))))
            .collect();
        let start = segments.len();
        for (name, y, height) in rows {
            let Some(ids) = mapping.get(name) else {
                unmatched.insert(name.clone());
                continue;
            };
            let mut row_groups = BTreeMap::<usize, (&Group, usize)>::new();
            for id in ids {
                if let Some(&(group, index)) = membership.get(id) {
                    row_groups.entry(index).or_insert((group, 0)).1 += 1;
                }
            }
            let mut cursor = y - height / 2.0;
            for (index, (group, count)) in row_groups {
                // If manually separated specimens share a unique tip, show
                // distinct proportional stripes instead of hiding their groups.
                let stripe_height = height * count as f32 / ids.len() as f32;
                let mut top = cursor;
                let mut bottom = cursor + stripe_height;
                cursor = bottom;
                if let Some((start, end)) = viewport {
                    if bottom <= start || top >= end {
                        continue;
                    }
                    top = top.max(start);
                    bottom = bottom.min(end);
                }
                segments.push(BarSegment {
                    hypothesis: h.id.clone(),
                    group: group.id.clone(),
                    x: column.x,
                    y: top,
                    width: column.width,
                    height: bottom - top,
                    color: color(work, h, group, index, palette),
                    label: group.label.clone(),
                });
            }
        }
        // One bar per run of touching rows in the same species.
        let mut merged: Vec<BarSegment> = Vec::new();
        for s in segments.drain(start..) {
            match merged.last_mut() {
                Some(last) if last.group == s.group && s.y <= last.y + last.height + 0.5 => {
                    last.height = s.y + s.height - last.y;
                }
                _ => merged.push(s),
            }
        }
        segments.append(&mut merged);
    }
    (segments, unmatched)
}

pub fn hit(segments: &[BarSegment], x: f32, y: f32) -> Option<(&str, &str)> {
    segments
        .iter()
        .find(|s| x >= s.x && x < s.x + s.width && y >= s.y && y < s.y + s.height)
        .map(|s| (s.hypothesis.as_str(), s.group.as_str()))
}

/// Build editable overlays from the same segments as the screen. A nested tree
/// SVG preserves its own typography, clipping and branch geometry.
pub fn comparison_svg(
    tree_svg: &str,
    work: &DelimitationWorkspace,
    rows: &[(String, f32, f32)],
    tree_width: f32,
    height: f32,
    font: &str,
    palette: &[String],
) -> String {
    // Column letters sit above the bars; the legend explains them below.
    const TOP: f32 = 20.0;
    let left = tree_width + 8.0;
    let columns = columns(work, left);
    let shown: Vec<_> = columns
        .iter()
        .filter_map(|c| work.hypotheses.iter().find(|h| h.id == c.hypothesis))
        .collect();
    let mut methods: Vec<(&str, &str)> = shown.iter().map(|h| origin(h)).collect();
    methods.dedup();
    methods.sort_unstable();
    methods.dedup();
    let bars_right = columns.last().map_or(left, |c| c.x + c.width);
    // Species names, for the columns that show them, in name columns after the bars.
    let mut names = Vec::new();
    let mut right = bars_right;
    for h in &shown {
        if !work
            .view
            .columns
            .get(&h.id)
            .is_some_and(|style| style.names)
        {
            continue;
        }
        let longest = h
            .groups
            .iter()
            .map(|g| g.label.chars().count())
            .max()
            .unwrap_or(0);
        names.push((h.id.clone(), right + 6.0));
        right += 6.0 + longest as f32 * 6.6;
    }
    let longest = shown
        .iter()
        .map(|h| h.name.chars().count())
        .max()
        .unwrap_or(0);
    let width = (right + 24.0).max(longest as f32 * 7.0 + 40.0);
    let total = TOP + height + 30.0 + (shown.len() + 1) as f32 * 14.0;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{total}\" font-family=\"{}\">\n<g id=\"tree\" transform=\"translate(0,{TOP})\">{tree_svg}</g>\n",
        escape(font)
    );
    let (segments, _) = bars(work, rows, left, palette, None);
    svg.push_str("<g id=\"partitions\">\n");
    for s in &segments {
        // Rounded ends, as on screen.
        let radius = s.width.min(s.height) / 2.0;
        writeln!(svg,"<rect data-hypothesis=\"{}\" data-group=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{radius}\" fill=\"{}\"><title>{}</title></rect>",escape(&s.hypothesis),escape(&s.group),s.x,s.y+TOP,s.width,s.height,escape(&s.color),escape(&s.label)).unwrap();
    }
    for (hypothesis, x) in &names {
        for (label, y) in name_positions(&segments, hypothesis) {
            writeln!(
                svg,
                "<text x=\"{x}\" y=\"{}\" font-size=\"11\" dominant-baseline=\"middle\">{}</text>",
                y + TOP,
                escape(&label)
            )
            .unwrap();
        }
    }
    for (column, h) in columns.iter().zip(&shown) {
        writeln!(
            svg,
            "<text x=\"{}\" y=\"{}\" font-size=\"11\" text-anchor=\"middle\"><title>{}</title>{}</text>",
            column.x + column.width / 2.0,
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
    let mut y = TOP + height + 24.0;
    writeln!(
        svg,
        "<text x=\"8\" y=\"{y}\" font-size=\"11\">{}</text>",
        escape(&legend)
    )
    .unwrap();
    for h in &shown {
        y += 14.0;
        writeln!(
            svg,
            "<text x=\"8\" y=\"{y}\" font-size=\"11\">{}: {}</text>",
            origin(h).0,
            escape(&h.name)
        )
        .unwrap();
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn representatives_expand_only_on_the_unique_tip_display() {
        let mut work = basecraft_io::spart::parse_text(include_str!(
            "../../../testdata/fixtures/delimitation/manual.spart"
        ))
        .unwrap();
        work.hypotheses[0].provenance.haplotypes = [
            ("a".into(), ["a".into(), "b".into()].into()),
            ("c".into(), ["c".into(), "d".into()].into()),
        ]
        .into();
        let unique = tree_mapping(&work, &["a".into(), "c".into()].into());
        assert_eq!(unique["a"], ["a".into(), "b".into()].into());
        let full = tree_mapping(
            &work,
            &["a".into(), "b".into(), "c".into(), "d".into()].into(),
        );
        assert_eq!(full["a"], ["a".into()].into());
        work.view.visible = vec!["partition-1".into()];
        let (segments, _) = bars(
            &work,
            &[("a".into(), 20.0, 20.0), ("c".into(), 40.0, 20.0)],
            0.0,
            &palette(&Theme::default()),
            None,
        );
        let row: Vec<_> = segments.iter().filter(|s| s.y < 30.0).collect();
        assert_eq!(row.len(), 2);
        assert!(
            row.iter()
                .all(|segment| (segment.height - 10.0).abs() < f32::EPSILON)
        );
        assert_ne!(row[0].group, row[1].group);
    }
    #[test]
    fn noncontiguous_groups_share_identity_and_select_all_segments() {
        let mut work = basecraft_io::spart::parse_text(include_str!(
            "../../../testdata/fixtures/delimitation/manual.spart"
        ))
        .unwrap();
        work.view.visible = vec![
            "partition-1".into(),
            "partition-2".into(),
            "partition-3".into(),
        ];
        let rows: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .enumerate()
            .map(|(i, &id)| (id.into(), 20.0 + i as f32 * 20.0, 20.0))
            .collect();
        let colors = palette(&Theme::default());
        let (segments, missing) = bars(&work, &rows, 500.0, &colors, Some((0.0, 100.0)));
        assert_eq!(missing, BTreeSet::new());
        // Partition 2 has a/b and c/d as touching pairs: one bar each.
        assert_eq!(segments.len(), 8);
        let (hypothesis, group) = hit(&segments, 504.0, 20.0).unwrap();
        let mut selection = Selection::default();
        selection
            .group(
                work.hypotheses.iter().find(|h| h.id == hypothesis).unwrap(),
                group,
                false,
            )
            .unwrap();
        assert_eq!(selection.specimens, ["a".into(), "c".into()].into());
        let svg = comparison_svg(
            "<svg><text>a &amp; b</text></svg>",
            &work,
            &rows,
            500.0,
            100.0,
            "A&B",
            &colors,
        );
        assert!(svg.contains("A&amp;B"));
        assert_eq!(svg.matches("data-hypothesis").count(), 8);
        assert!(!svg.contains("<image"));
    }
    #[test]
    fn columns_follow_saved_widths_and_colours() {
        use basecraft_protocol::delimitation::ColumnStyle;
        let mut work = basecraft_io::spart::parse_text(include_str!(
            "../../../testdata/fixtures/delimitation/manual.spart"
        ))
        .unwrap();
        work.view.visible = vec!["partition-1".into(), "partition-2".into()];
        let first = work.hypotheses[0].id.clone();
        let group = work.hypotheses[0].groups[0].id.clone();
        work.view.columns.insert(
            first.clone(),
            ColumnStyle {
                width: Some(2),
                colors: [(group.clone(), "#123456".into())].into(),
                names: false,
            },
        );
        let laid = columns(&work, 10.0);
        // Widths below the minimum are raised to it.
        assert_eq!(laid[0].width, MIN_BAR_WIDTH as f32);
        // Each column is followed by a gap as wide as its bars, unless set.
        assert_eq!(laid[1].x, 10.0 + 2.0 * MIN_BAR_WIDTH as f32);
        let mut spaced = work.clone();
        spaced.view.bar_gap = Some(3);
        assert_eq!(
            columns(&spaced, 10.0)[1].x,
            10.0 + MIN_BAR_WIDTH as f32 + 3.0
        );
        assert_eq!(laid[1].width, BAR_WIDTH as f32);
        let colors = palette(&Theme::default());
        let rows = [("a".to_string(), 20.0, 20.0)];
        let (segments, _) = bars(&work, &rows, 10.0, &colors, None);
        let custom = segments
            .iter()
            .find(|s| s.hypothesis == first && s.group == group);
        assert_eq!(custom.unwrap().color, "#123456");
        assert!(
            segments
                .iter()
                .filter(|s| s.hypothesis != first)
                .all(|s| colors.contains(&s.color))
        );
    }

    #[test]
    fn flips_keep_their_clade_and_names_follow_the_bars() {
        let original = TreeDoc::from_newick("t", "((a:1,b:1):1,(c:1,d:1):1);").unwrap();
        let order = |tree: &TreeDoc| {
            tree.tips_in_order()
                .iter()
                .map(|&i| tree.node(i).name.clone())
                .collect::<Vec<_>>()
        };
        let pair = flip_key(&original, &["c".into(), "d".into()]).unwrap();
        assert_eq!(pair, ["c", "d"]);
        // A tip names its parent's clade; the whole tree is keyed by both halves.
        assert_eq!(flip_key(&original, &["d".into()]).unwrap(), pair);
        let root = flip_key(&original, &["a".into(), "d".into()]).unwrap();
        assert_eq!(root, ["a", "c"]);
        let mut tree = original.clone();
        apply_flips(&mut tree, &[pair.clone(), root.clone()].into());
        assert_eq!(order(&tree), ["d", "c", "a", "b"]);
        // The keys still name the same clades once flipped.
        assert_eq!(flip_key(&tree, &["d".into()]).unwrap(), pair);
        assert_eq!(flip_key(&tree, &["b".into(), "c".into()]).unwrap(), root);

        let mut work = basecraft_io::spart::parse_text(include_str!(
            "../../../testdata/fixtures/delimitation/manual.spart"
        ))
        .unwrap();
        work.view.visible = vec!["partition-1".into()];
        work.hypotheses[0].groups[0].label = "Species alpha".into();
        let rows_in = |order: [&str; 4]| -> Vec<_> {
            order
                .iter()
                .enumerate()
                .map(|(i, n)| ((*n).to_string(), i as f32 * 20.0 + 10.0, 20.0))
                .collect()
        };
        let rows = rows_in(["a", "b", "c", "d"]);
        let colors = palette(&Theme::default());
        let plain = comparison_svg("", &work, &rows, 100.0, 80.0, "mono", &colors);
        assert!(!plain.contains(">Species alpha</text>"));
        work.view
            .columns
            .entry("partition-1".into())
            .or_default()
            .names = true;
        let named = comparison_svg("", &work, &rows, 100.0, 80.0, "mono", &colors);
        // Group 1 is a and c, which are not adjacent rows: one name per part.
        assert_eq!(named.matches(">Species alpha</text>").count(), 2);
        // Drawn a, c, b, d, each species is one block with one name.
        let (segments, _) = bars(&work, &rows_in(["a", "c", "b", "d"]), 0.0, &colors, None);
        // Touching rows of a species are drawn as one bar.
        assert_eq!(segments.len(), 2);
        assert!(
            segments.iter().all(|s| (s.height - 40.0).abs() < 1e-4),
            "{segments:?}"
        );
        let positions = name_positions(&segments, "partition-1");
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0].0, "Species alpha");
    }
}
