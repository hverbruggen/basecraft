//! Partition bars use the tree canvas's exact row geometry and stable IDs.
#![allow(clippy::cast_precision_loss)]
use crate::svg::escape;
use basecraft_protocol::delimitation::{DelimitationWorkspace, Group, Hypothesis};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

const GROUP_COLORS: [&str; 6] = [
    "#377eb8", "#e41a1c", "#4daf4a", "#984ea3", "#ff7f00", "#a65628",
];
fn color(group: &Group, index: usize) -> &str {
    if group.color.is_empty() {
        GROUP_COLORS[index % GROUP_COLORS.len()]
    } else {
        &group.color
    }
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
    column_width: f32,
    viewport: Option<(f32, f32)>,
) -> (Vec<BarSegment>, BTreeSet<String>) {
    let mapping = tree_mapping(
        work,
        &rows.iter().map(|(name, _, _)| name.clone()).collect(),
    );
    let mut segments = Vec::new();
    let mut unmatched = BTreeSet::new();
    for (column, id) in work.view.visible.iter().enumerate() {
        let Some(h) = work.hypotheses.iter().find(|h| &h.id == id) else {
            continue;
        };
        let membership: BTreeMap<_, _> = h
            .groups
            .iter()
            .enumerate()
            .flat_map(|(i, g)| g.members.iter().map(move |m| (m, (g, i))))
            .collect();
        for (name, y, height) in rows {
            let Some(ids) = mapping.get(name) else {
                unmatched.insert(name.clone());
                continue;
            };
            let Some((group, index)) = ids.first().and_then(|id| membership.get(id)).copied()
            else {
                continue;
            };
            if ids.iter().any(|id| !group.members.contains(id)) {
                continue;
            }
            let y = y - height / 2.0;
            let mut top = y;
            let mut bottom = y + height;
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
                x: left + column as f32 * column_width,
                y: top,
                width: column_width - 6.0,
                height: bottom - top,
                color: color(group, index).into(),
                label: group.label.clone(),
            });
        }
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
) -> String {
    let width = tree_width + work.view.visible.len() as f32 * 90.0 + 24.0;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{}\" font-family=\"{}\">\n<g id=\"tree\">{tree_svg}</g>\n",
        height
            + 50.0
            + work
                .hypotheses
                .iter()
                .filter(|h| work.view.visible.contains(&h.id))
                .map(|h| h.groups.len())
                .max()
                .unwrap_or(0) as f32
                * 14.0,
        escape(font)
    );
    let (segments, _) = bars(work, rows, tree_width + 8.0, 90.0, None);
    svg.push_str("<g id=\"partitions\">\n");
    for s in segments {
        writeln!(svg,"<rect data-hypothesis=\"{}\" data-group=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"><title>{}</title></rect>",escape(&s.hypothesis),escape(&s.group),s.x,s.y,s.width,s.height,escape(&s.color),escape(&s.label)).unwrap();
    }
    for (i, id) in work.view.visible.iter().enumerate() {
        if let Some(h) = work.hypotheses.iter().find(|h| &h.id == id) {
            let x = tree_width + 8.0 + i as f32 * 90.0;
            writeln!(
                svg,
                "<text x=\"{x}\" y=\"{}\" font-size=\"12\">{}</text>",
                height + 20.0,
                escape(&h.name)
            )
            .unwrap();
            for (j, g) in h.groups.iter().enumerate() {
                writeln!(svg,"<text data-group=\"{}\" x=\"{x}\" y=\"{}\" font-size=\"11\" fill=\"{}\">{}</text>",escape(&g.id),height+36.0+j as f32*14.0,escape(color(g,j)),escape(&g.label)).unwrap();
            }
        }
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
        let (segments, missing) = bars(&work, &rows, 500.0, 90.0, Some((0.0, 100.0)));
        assert_eq!(missing, BTreeSet::new());
        assert_eq!(segments.len(), 10);
        let (hypothesis, group) = hit(&segments, 510.0, 20.0).unwrap();
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
        );
        assert!(svg.contains("A&amp;B"));
        assert_eq!(svg.matches("data-hypothesis").count(), 10);
        assert!(!svg.contains("<image"));
    }
}
