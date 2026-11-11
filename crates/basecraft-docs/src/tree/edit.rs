//! Undo retains only changed fields, except topology-changing root operations.
use super::{EditError, SupportSet, TreeDoc};
use basecraft_protocol::{CladeAnnotation, Op, TreeStyle};
use std::collections::BTreeMap;

pub struct TreeUndo {
    version: u64,
    change: Change,
}

enum Change {
    Root(Box<TreeDoc>),
    Position {
        lengths: [(usize, Option<f64>); 2],
        rooted: Option<bool>,
    },
    Children(Vec<(usize, Vec<usize>)>),
    Name {
        node: usize,
        name: String,
        references: Vec<(u32, usize)>,
    },
    Annotation(u32, Option<CladeAnnotation>),
    AddedSupport(String),
    Style {
        style: Box<StyleUndo>,
        removed: Vec<(usize, SupportSet)>,
    },
}

fn map_before<T: Clone + PartialEq>(
    before: &BTreeMap<String, T>,
    after: &BTreeMap<String, T>,
) -> BTreeMap<String, Option<T>> {
    before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .map(|key| (key.clone(), before.get(key).cloned()))
        .collect()
}
fn restore_map<T: Clone>(map: &mut BTreeMap<String, T>, before: &BTreeMap<String, Option<T>>) {
    for (key, value) in before {
        if let Some(value) = value {
            map.insert(key.clone(), value.clone());
        } else {
            map.remove(key);
        }
    }
}

// Keep the field list typed: adding a style field must also define its undo.
macro_rules! style_undo {
    (maps { $($map:ident : $map_type:ty),* $(,)? } fields { $($field:ident : $field_type:ty),* $(,)? }) => {
        struct StyleUndo {
            $($map: BTreeMap<String, Option<$map_type>>,)*
            $($field: Option<$field_type>,)*
        }
        impl StyleUndo {
            fn between(before: &TreeStyle, after: &TreeStyle) -> Self {
                let TreeStyle { $($map: _,)* $($field: _,)* } = before;
                Self {
                    $($map: map_before(&before.$map, &after.$map),)*
                    $($field: (before.$field != after.$field).then(|| before.$field.clone()),)*
                }
            }
            fn restore(&self, style: &mut TreeStyle) {
                $(restore_map(&mut style.$map, &self.$map);)*
                $(if let Some(value) = &self.$field { style.$field = value.clone(); })*
            }
        }
    };
}
style_undo! {
    maps {
        tip_labels: String, support_minimums: u32, support_fraction: bool,
        support_decimals: u8, support_proportions: bool, extra: serde_json::Value,
    }
    fields {
        support_placement: basecraft_protocol::tree::SupportPlacement,
        support_shown: Vec<String>, support_min: u32, support_legend: bool,
        symbols: Option<basecraft_protocol::tree::SupportSymbols>,
        tip_font: u32, support_font: u32, branch_width: u32,
        vertical_bar_labels: bool, scale_bar: bool,
    }
}

impl TreeUndo {
    pub fn whole(before: TreeDoc) -> Self {
        Self {
            version: before.version,
            change: Change::Root(Box::new(before)),
        }
    }
}

impl TreeDoc {
    pub fn undo_edit(&mut self, undo: &TreeUndo) {
        match &undo.change {
            Change::Root(before) => *self = (**before).clone(),
            Change::Position { lengths, rooted } => {
                for &(i, length) in lengths {
                    self.nodes[i].length = length;
                }
                self.rooted = *rooted;
            }
            Change::Children(nodes) => {
                for (i, children) in nodes {
                    self.nodes[*i].children.clone_from(children);
                }
            }
            Change::Name {
                node,
                name,
                references,
            } => {
                self.nodes[*node].name.clone_from(name);
                for &(id, index) in references {
                    self.annotations.get_mut(&id).unwrap().clade[index].clone_from(name);
                }
            }
            Change::Annotation(id, annotation) => {
                if let Some(annotation) = annotation {
                    self.annotations.insert(*id, annotation.clone());
                } else {
                    self.annotations.remove(id);
                }
            }
            Change::AddedSupport(name) => {
                self.supports.retain(|s| s.name != *name);
            }
            Change::Style { style, removed } => {
                for (i, support) in removed {
                    self.supports.insert(*i, support.clone());
                }
                style.restore(&mut self.style);
            }
        }
        self.version = undo.version;
    }

    pub fn apply_edit(&mut self, op: &Op) -> Result<TreeUndo, EditError> {
        let version = self.version;
        let change = match op {
            Op::TreeReroot { .. } | Op::TreeMidpointRoot {} | Op::TreeMinimumVarianceRoot {} => {
                let before = Box::new(self.clone());
                let result = match op {
                    Op::TreeReroot { clade } => self.reroot(clade),
                    Op::TreeMidpointRoot {} => self.midpoint_root(),
                    _ => self.minimum_variance_root(),
                };
                if let Err(error) = result {
                    *self = *before;
                    return Err(error);
                }
                Change::Root(before)
            }
            Op::TreeRootPosition { clade, fraction } => {
                let (a, b, _) = self
                    .root_branch()
                    .ok_or_else(|| EditError("the root needs two branches with lengths".into()))?;
                let change = Change::Position {
                    lengths: [(a, self.nodes[a].length), (b, self.nodes[b].length)],
                    rooted: self.rooted,
                };
                self.set_root_position(clade, *fraction)?;
                change
            }
            Op::TreeRotate { clade } => {
                let node = self.resolve(clade)?;
                let children = self.nodes[node].children.clone();
                self.rotate(clade)?;
                Change::Children(vec![(node, children)])
            }
            Op::TreeLadderize { small_first } => {
                let counts = self.tip_counts();
                let mut changed = Vec::new();
                for (i, node) in self.nodes.iter_mut().enumerate() {
                    let mut after = node.children.clone();
                    after.sort_by_key(|&c| {
                        if *small_first {
                            counts[c]
                        } else {
                            usize::MAX - counts[c]
                        }
                    });
                    if after != node.children {
                        changed.push((i, std::mem::replace(&mut node.children, after)));
                    }
                }
                self.edited();
                Change::Children(changed)
            }
            Op::TreeRenameTip { from, to } => {
                let node = self
                    .find_tip(from)
                    .ok_or_else(|| EditError(format!("no tip named {from:?}")))?;
                let references = self
                    .annotations
                    .iter()
                    .flat_map(|(&id, a)| {
                        a.clade
                            .iter()
                            .enumerate()
                            .filter(|(_, tip)| *tip == from)
                            .map(move |(i, _)| (id, i))
                    })
                    .collect();
                let name = self.nodes[node].name.clone();
                self.rename_tip(from, to)?;
                Change::Name {
                    node,
                    name,
                    references,
                }
            }
            Op::TreeAnnotate { id, annotation } => {
                let before = self.annotations.get(id).cloned();
                self.annotate(*id, annotation.clone())?;
                Change::Annotation(*id, before)
            }
            Op::TreeAddSupport {
                name,
                source,
                values,
                kind,
            } => {
                self.add_support(name, source, values)?;
                self.set_support_kind(name.trim(), *kind);
                Change::AddedSupport(name.trim().into())
            }
            Op::TreeRemoveSupport { name } => self.edit_style(None, std::slice::from_ref(name))?,
            Op::TreeStyle {
                style,
                remove_supports,
            } => self.edit_style(Some(style.clone()), remove_supports)?,
            _ => return Err(EditError(format!("{} is not a tree edit", op.name()))),
        };
        Ok(TreeUndo { version, change })
    }

    pub fn check_style(
        &self,
        style: Option<&TreeStyle>,
        remove: &[String],
    ) -> Result<Vec<usize>, EditError> {
        if style.is_some_and(|s| s.support_decimals.values().any(|&n| n > 10)) {
            return Err(EditError(
                "support decimal places must be between 0 and 10".into(),
            ));
        }
        let mut indices = Vec::new();
        for name in remove {
            let i = self
                .supports
                .iter()
                .position(|s| s.name == *name)
                .ok_or_else(|| EditError(format!("no support set named {name:?}")))?;
            if indices.contains(&i) {
                return Err(EditError(format!("repeated support set {name:?}")));
            }
            indices.push(i);
        }
        indices.sort_unstable();
        Ok(indices)
    }

    fn edit_style(
        &mut self,
        style: Option<TreeStyle>,
        remove: &[String],
    ) -> Result<Change, EditError> {
        let indices = self.check_style(style.as_ref(), remove)?;
        let before = if let Some(style) = style {
            self.edited();
            std::mem::replace(&mut self.style, style)
        } else {
            self.style.clone()
        };
        let mut removed = Vec::new();
        for &i in indices.iter().rev() {
            let name = self.supports[i].name.clone();
            // Retain just the removed support. remove_support also adjusts presentation.
            removed.push((i, self.supports[i].clone()));
            self.remove_support(&name)?;
        }
        removed.reverse();
        let style = Box::new(StyleUndo::between(&before, &self.style));
        Ok(Change::Style { style, removed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equal(a: &TreeDoc, b: &TreeDoc) {
        assert_eq!(a.to_newick_with_rooting(), b.to_newick_with_rooting());
        assert_eq!(a.nodes, b.nodes);
        assert_eq!(a.root, b.root);
        assert_eq!(a.supports, b.supports);
        assert_eq!(a.annotations, b.annotations);
        assert_eq!(a.style, b.style);
        assert_eq!(a.version, b.version);
    }

    #[test]
    fn every_edit_undo_restores_exact_payload_and_revision() {
        let mut tree = TreeDoc::from_newick("test", "((a:1,b:2)85:3,c:4);").unwrap();
        tree.annotate(
            7,
            Some(CladeAnnotation::new(vec!["a".into(), "b".into()], "group")),
        )
        .unwrap();
        tree.style.tip_labels.insert("a".into(), "display A".into());
        tree.style.support_shown = vec!["Support".into()];
        tree.style.support_decimals.insert("Support".into(), 2);
        let original = tree.clone();
        let mut style = tree.style.clone();
        style.tip_labels.insert("a".into(), "new label".into());
        style.branch_width = 4;
        for op in [
            Op::TreeRenameTip {
                from: "a".into(),
                to: "renamed".into(),
            },
            Op::TreeAnnotate {
                id: 7,
                annotation: None,
            },
            Op::TreeAnnotate {
                id: 9,
                annotation: Some(CladeAnnotation::new(vec!["c".into()], "C")),
            },
            Op::TreeRotate {
                clade: vec!["a".into(), "b".into()],
            },
            Op::TreeLadderize { small_first: true },
            Op::TreeRemoveSupport {
                name: "Support".into(),
            },
            Op::TreeAddSupport {
                name: "other".into(),
                source: "fixture".into(),
                values: vec![None, Some("0.9".into())],
                kind: basecraft_protocol::SupportKind::Probability,
            },
            Op::TreeStyle {
                style,
                remove_supports: vec!["Support".into()],
            },
            Op::TreeRootPosition {
                clade: vec!["c".into()],
                fraction: 0.123,
            },
            Op::TreeReroot {
                clade: vec!["a".into()],
            },
            Op::TreeMidpointRoot {},
            Op::TreeMinimumVarianceRoot {},
        ] {
            let undo = tree.apply_edit(&op).unwrap();
            tree.undo_edit(&undo);
            equal(&tree, &original);
            let undo = tree.apply_edit(&op).unwrap();
            tree.undo_edit(&undo);
            equal(&tree, &original);
        }
        for names in [
            vec!["Support".into(), "missing".into()],
            vec!["Support".into(), "Support".into()],
        ] {
            assert!(
                tree.apply_edit(&Op::TreeStyle {
                    style: TreeStyle::default(),
                    remove_supports: names
                })
                .is_err()
            );
            equal(&tree, &original);
        }
    }
}
