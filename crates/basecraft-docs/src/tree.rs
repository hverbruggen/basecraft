//! A phylogenetic tree with support values, clade annotations and a
//! drawing style.
//!
//! Nodes live in an arena; edits that restructure the tree rebuild it.
//! Support values belong to branches and are stored on the node below the
//! branch, so rerooting moves them along the reversed path. Clades are
//! named by tips (see [`CladeRef`]), which survive restructuring.

use std::collections::BTreeMap;

use basecraft_io::newick::{self, ParsedTree};
use basecraft_protocol::{CladeAnnotation, CladeRef, TreeStyle};

pub use crate::alignment::EditError;

fn err<T>(msg: impl Into<String>) -> Result<T, EditError> {
    Err(EditError(msg.into()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// A tip's name, or a name given to an internal node.
    pub name: String,
    /// Length of the branch above.
    pub length: Option<f64>,
}

impl TreeNode {
    pub fn is_tip(&self) -> bool {
        self.children.is_empty()
    }
}

/// Support values, one per node or per internal node, as text.
pub type SupportValues = Vec<Option<String>>;

/// One kind of support, such as bootstraps or posterior probabilities:
/// one value per node, for the branch above it, as text.
#[derive(Clone, Debug, PartialEq)]
pub struct SupportSet {
    pub name: String,
    pub source: String,
    pub values: Vec<Option<String>>,
    /// Every value is at most 1: proportions rather than percentages.
    pub proportions: bool,
}

impl SupportSet {
    fn new(name: String, source: String, values: Vec<Option<String>>) -> Self {
        let proportions = values
            .iter()
            .flatten()
            .all(|v| v.parse::<f64>().is_ok_and(|x| x <= 1.0));
        Self {
            name,
            source,
            values,
            proportions,
        }
    }

    /// A node's value as a percentage, for thresholds.
    pub fn percent(&self, node: usize) -> Option<f64> {
        let v: f64 = self.values.get(node)?.as_ref()?.parse().ok()?;
        Some(if self.proportions { v * 100.0 } else { v })
    }
}

#[derive(Clone, Debug)]
pub struct TreeDoc {
    pub name: String,
    nodes: Vec<TreeNode>,
    root: usize,
    supports: Vec<SupportSet>,
    annotations: BTreeMap<u32, CladeAnnotation>,
    style: TreeStyle,
    version: u64,
}

impl TreeDoc {
    /// A tree as read from a file. Numeric internal labels become support
    /// sets: one per `/`-separated part, as IQ-TREE writes SH-aLRT and
    /// bootstrap together. `prob=` comments (`MrBayes`) become a set of
    /// posterior probabilities. Other internal labels stay node names.
    pub fn from_parsed(name: impl Into<String>, parsed: &ParsedTree) -> Self {
        let n = parsed.nodes.len();
        let mut nodes: Vec<TreeNode> = parsed
            .nodes
            .iter()
            .map(|p| TreeNode {
                parent: p.parent,
                children: p.children.clone(),
                name: p.name.clone(),
                length: p.length,
            })
            .collect();
        let support_label =
            |s: &str| !s.is_empty() && s.split('/').all(|part| part.trim().parse::<f64>().is_ok());
        let parts = parsed
            .nodes
            .iter()
            .filter(|p| !p.children.is_empty() && support_label(&p.name))
            .map(|p| p.name.split('/').count())
            .max()
            .unwrap_or(0);
        let mut supports = Vec::new();
        for k in 0..parts {
            let values = (0..n)
                .map(|i| {
                    let p = &parsed.nodes[i];
                    if p.children.is_empty() || !support_label(&p.name) {
                        return None;
                    }
                    p.name.split('/').nth(k).map(|v| v.trim().to_string())
                })
                .collect();
            let name = if parts == 1 {
                "Support".to_string()
            } else {
                format!("Support {}", k + 1)
            };
            supports.push(SupportSet::new(name, String::new(), values));
        }
        for (i, p) in parsed.nodes.iter().enumerate() {
            if !p.children.is_empty() && support_label(&p.name) {
                nodes[i].name.clear();
            }
        }
        let posterior: Vec<Option<String>> = parsed
            .nodes
            .iter()
            .map(|p| {
                if p.children.is_empty() {
                    return None;
                }
                comment_value(&p.comment, "prob").map(|v| tidy_number(&v))
            })
            .collect();
        if posterior.iter().any(Option::is_some) {
            supports.push(SupportSet::new(
                "Posterior".into(),
                String::new(),
                posterior,
            ));
        }
        let mut doc = Self {
            name: name.into(),
            nodes,
            root: 0,
            supports,
            annotations: BTreeMap::new(),
            style: TreeStyle::default(),
            version: 0,
        };
        // The root has no branch above it to support.
        for s in &mut doc.supports {
            s.values[0] = None;
        }
        doc
    }

    pub fn from_newick(name: impl Into<String>, text: &str) -> Result<Self, EditError> {
        let parsed = newick::parse_newick(text).map_err(|e| EditError(e.to_string()))?;
        Ok(Self::from_parsed(name, &parsed))
    }

    // Reading.

    pub fn nodes(&self) -> &[TreeNode] {
        &self.nodes
    }

    pub fn node(&self, i: usize) -> &TreeNode {
        &self.nodes[i]
    }

    pub fn root(&self) -> usize {
        self.root
    }

    pub fn supports(&self) -> &[SupportSet] {
        &self.supports
    }

    pub fn support(&self, name: &str) -> Option<&SupportSet> {
        self.supports.iter().find(|s| s.name == name)
    }

    pub fn annotations(&self) -> &BTreeMap<u32, CladeAnnotation> {
        &self.annotations
    }

    pub fn style(&self) -> &TreeStyle {
        &self.style
    }

    /// Bumped by every edit.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn tip_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_tip()).count()
    }

    /// Whether any branch has a length; without, the tree draws as a
    /// cladogram.
    pub fn has_lengths(&self) -> bool {
        self.nodes
            .iter()
            .enumerate()
            .any(|(i, n)| i != self.root && n.length.is_some())
    }

    /// Nodes in preorder, children in order.
    pub fn preorder(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![self.root];
        while let Some(i) = stack.pop() {
            out.push(i);
            stack.extend(self.nodes[i].children.iter().rev());
        }
        out
    }

    /// Internal nodes in preorder: the order `tree.add_support` values
    /// follow.
    pub fn internal_preorder(&self) -> Vec<usize> {
        self.preorder()
            .into_iter()
            .filter(|&i| !self.nodes[i].is_tip())
            .collect()
    }

    /// Tips top to bottom.
    pub fn tips_in_order(&self) -> Vec<usize> {
        self.preorder()
            .into_iter()
            .filter(|&i| self.nodes[i].is_tip())
            .collect()
    }

    /// The first and last tip under `node`, in drawing order.
    pub fn tip_range(&self, node: usize) -> (usize, usize) {
        let mut first = node;
        while let Some(&c) = self.nodes[first].children.first() {
            first = c;
        }
        let mut last = node;
        while let Some(&c) = self.nodes[last].children.last() {
            last = c;
        }
        (first, last)
    }

    pub fn find_tip(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.is_tip() && n.name == name)
    }

    fn ancestors(&self, mut i: usize) -> Vec<usize> {
        let mut out = vec![i];
        while let Some(p) = self.nodes[i].parent {
            out.push(p);
            i = p;
        }
        out
    }

    /// The node a clade reference names: one tip, or the most recent common
    /// ancestor of two.
    pub fn resolve(&self, clade: &[String]) -> Result<usize, EditError> {
        let find = |name: &String| {
            self.find_tip(name)
                .ok_or_else(|| EditError(format!("no tip named {name:?}")))
        };
        match clade {
            [a] => find(a),
            [a, b] => {
                let up = self.ancestors(find(a)?);
                let other = self.ancestors(find(b)?);
                up.into_iter()
                    .find(|n| other.contains(n))
                    .ok_or_else(|| EditError("tips without a common ancestor".into()))
            }
            _ => err("a clade is named by one or two tips"),
        }
    }

    /// How to name `node` in an operation.
    pub fn clade_ref(&self, node: usize) -> CladeRef {
        if self.nodes[node].is_tip() {
            return vec![self.nodes[node].name.clone()];
        }
        let (a, b) = self.tip_range(node);
        vec![self.nodes[a].name.clone(), self.nodes[b].name.clone()]
    }

    /// Number of tips under each node.
    pub fn tip_counts(&self) -> Vec<usize> {
        let mut counts = vec![0; self.nodes.len()];
        for &i in self.preorder().iter().rev() {
            counts[i] = if self.nodes[i].is_tip() {
                1
            } else {
                self.nodes[i].children.iter().map(|&c| counts[c]).sum()
            };
        }
        counts
    }

    // Editing.

    fn edited(&mut self) {
        self.version += 1;
    }

    /// Reverse the order of a clade's children.
    pub fn rotate(&mut self, clade: &[String]) -> Result<(), EditError> {
        let n = self.resolve(clade)?;
        if self.nodes[n].is_tip() {
            return err("a tip has nothing to rotate");
        }
        self.nodes[n].children.reverse();
        self.edited();
        Ok(())
    }

    /// The root's branch, if the root has two children: the two children
    /// and the branch's whole length.
    pub fn root_branch(&self) -> Option<(usize, usize, f64)> {
        let [a, b] = self.nodes[self.root].children[..] else {
            return None;
        };
        let length = self.nodes[a].length? + self.nodes[b].length?;
        Some((a, b, length))
    }

    /// Move the root along its branch: `clade`, one of its two children,
    /// gets `fraction` of the branch's length.
    pub fn set_root_position(&mut self, clade: &[String], fraction: f64) -> Result<(), EditError> {
        if !(0.0..=1.0).contains(&fraction) {
            return err(format!("{fraction} isn't between 0 and 1"));
        }
        let node = self.resolve(clade)?;
        let Some((a, b, length)) = self.root_branch() else {
            return err("the root needs two branches with lengths");
        };
        let other = if node == a {
            b
        } else if node == b {
            a
        } else {
            return err("that clade isn't beside the root");
        };
        self.nodes[node].length = Some(length * fraction);
        self.nodes[other].length = Some(length * (1.0 - fraction));
        self.edited();
        Ok(())
    }

    /// Order every node's children by size.
    pub fn ladderize(&mut self, small_first: bool) {
        let counts = self.tip_counts();
        for node in &mut self.nodes {
            // Stable, so equal clades keep their order.
            if small_first {
                node.children.sort_by_key(|&c| counts[c]);
            } else {
                node.children.sort_by_key(|&c| std::cmp::Reverse(counts[c]));
            }
        }
        self.edited();
    }

    pub fn rename_tip(&mut self, from: &str, to: &str) -> Result<(), EditError> {
        let to = to.trim();
        if to.is_empty() {
            return err("a tip needs a name");
        }
        if from != to && self.find_tip(to).is_some() {
            return err(format!("there's already a tip named {to:?}"));
        }
        let i = self
            .find_tip(from)
            .ok_or_else(|| EditError(format!("no tip named {from:?}")))?;
        self.nodes[i].name = to.to_string();
        for a in self.annotations.values_mut() {
            for t in &mut a.clade {
                if t == from {
                    *t = to.to_string();
                }
            }
        }
        self.edited();
        Ok(())
    }

    pub fn annotate(
        &mut self,
        id: u32,
        annotation: Option<CladeAnnotation>,
    ) -> Result<(), EditError> {
        match annotation {
            Some(a) => {
                self.resolve(&a.clade)?;
                if !a.colour.starts_with('#') || !matches!(a.colour.len(), 7 | 9) {
                    return err(format!("{:?} isn't a #rrggbb colour", a.colour));
                }
                self.annotations.insert(id, a);
            }
            None => {
                if self.annotations.remove(&id).is_none() {
                    return err(format!("no annotation {id}"));
                }
            }
        }
        self.edited();
        Ok(())
    }

    /// The next free annotation id.
    pub fn next_annotation_id(&self) -> u32 {
        self.annotations.keys().next_back().map_or(1, |k| k + 1)
    }

    /// Add a support set: `values` holds one per internal node, in
    /// preorder.
    pub fn add_support(
        &mut self,
        name: &str,
        source: &str,
        values: &[Option<String>],
    ) -> Result<(), EditError> {
        let name = name.trim();
        if name.is_empty() {
            return err("a support set needs a name");
        }
        if self.support(name).is_some() {
            return err(format!("there's already a support set named {name:?}"));
        }
        let internal = self.internal_preorder();
        if values.len() != internal.len() {
            return err(format!(
                "{} support values for {} internal nodes",
                values.len(),
                internal.len()
            ));
        }
        let mut all = vec![None; self.nodes.len()];
        for (&node, v) in internal.iter().zip(values) {
            if node != self.root {
                all[node].clone_from(v);
            }
        }
        self.supports
            .push(SupportSet::new(name.into(), source.into(), all));
        self.edited();
        Ok(())
    }

    pub fn remove_support(&mut self, name: &str) -> Result<(), EditError> {
        let i = self
            .supports
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| EditError(format!("no support set named {name:?}")))?;
        self.supports.remove(i);
        self.style.support_shown.retain(|s| s != name);
        if self.style.symbols.as_ref().is_some_and(|s| s.set == name) {
            self.style.symbols = None;
        }
        self.edited();
        Ok(())
    }

    pub fn rename_support(&mut self, from: &str, to: &str) {
        if let Some(s) = self.supports.iter_mut().find(|s| s.name == from) {
            s.name = to.to_string();
        }
    }

    pub fn set_style(&mut self, style: TreeStyle) {
        self.style = style;
        self.edited();
    }

    /// Root halfway along the branch above `clade`.
    #[allow(clippy::many_single_char_names)]
    pub fn reroot(&mut self, clade: &[String]) -> Result<(), EditError> {
        let target = self.resolve(clade)?;
        if target == self.root {
            return err("that's already the root");
        }
        let sets = self.supports.len();
        // The tree as undirected edges, each carrying its length and
        // support values. A root of two children is one edge, not a node.
        let n = self.nodes.len();
        let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
        let mut edges: Vec<Edge> = Vec::new();
        let old_root = self.root;
        let root_children = self.nodes[old_root].children.clone();
        let merge_root = root_children.len() == 2;
        for (i, node) in self.nodes.iter().enumerate() {
            let Some(p) = node.parent else { continue };
            if merge_root && p == old_root {
                continue;
            }
            let e = edges.len();
            edges.push(Edge {
                length: node.length,
                support: (0..sets)
                    .map(|s| self.supports[s].values[i].clone())
                    .collect(),
            });
            adj[i].push((p, e));
            adj[p].push((i, e));
        }
        if merge_root {
            let (a, b) = (root_children[0], root_children[1]);
            let length = match (self.nodes[a].length, self.nodes[b].length) {
                (None, None) => None,
                (x, y) => Some(x.unwrap_or(0.0) + y.unwrap_or(0.0)),
            };
            let support = (0..sets)
                .map(|s| {
                    let v = &self.supports[s].values;
                    v[a].clone().or_else(|| v[b].clone())
                })
                .collect();
            let e = edges.len();
            edges.push(Edge { length, support });
            adj[a].push((b, e));
            adj[b].push((a, e));
        }
        // The branch to root on: from the target to its parent, or across a
        // merged root.
        let parent = self.nodes[target].parent.unwrap_or(old_root);
        let other = if merge_root && parent == old_root {
            root_children
                .iter()
                .copied()
                .find(|&c| c != target)
                .unwrap_or(parent)
        } else {
            parent
        };
        let &(_, split) = adj[target]
            .iter()
            .find(|(m, _)| *m == other)
            .ok_or_else(|| EditError("can't find the branch to root on".into()))?;
        // Order neighbours as they were drawn: children in order, then the
        // old parent.
        let order = |i: usize, from: usize, adj: &[Vec<(usize, usize)>]| -> Vec<(usize, usize)> {
            let mut out: Vec<(usize, usize)> = Vec::new();
            for &c in &self.nodes[i].children {
                if let Some(&x) = adj[i].iter().find(|(m, _)| *m == c) {
                    out.push(x);
                }
            }
            for &x in &adj[i] {
                if !out.contains(&x) {
                    out.push(x);
                }
            }
            out.retain(|(m, _)| *m != from);
            out
        };
        let mut nodes: Vec<TreeNode> = Vec::with_capacity(n + 1);
        let mut values: Vec<Vec<Option<String>>> = vec![Vec::with_capacity(n + 1); sets];
        nodes.push(TreeNode {
            parent: None,
            children: Vec::new(),
            name: String::new(),
            length: None,
        });
        for v in &mut values {
            v.push(None);
        }
        let half = edges[split].length.map(|l| l / 2.0);
        // (old node, came from, new parent, length, support) to place.
        let mut stack: Vec<(usize, usize, usize, Option<f64>, SupportValues)> = vec![
            (other, target, 0, half, vec![None; sets]),
            (target, other, 0, half, edges[split].support.clone()),
        ];
        while let Some((old, from, parent, length, support)) = stack.pop() {
            let id = nodes.len();
            nodes.push(TreeNode {
                parent: Some(parent),
                children: Vec::new(),
                name: self.nodes[old].name.clone(),
                length,
            });
            for (s, v) in values.iter_mut().enumerate() {
                v.push(support[s].clone());
            }
            nodes[parent].children.push(id);
            for &(next, e) in order(old, from, &adj).iter().rev() {
                stack.push((next, old, id, edges[e].length, edges[e].support.clone()));
            }
        }
        self.nodes = nodes;
        self.root = 0;
        for (s, v) in values.into_iter().enumerate() {
            self.supports[s].values = v;
        }
        self.edited();
        Ok(())
    }

    // Combining support from another tree.

    /// For each support set of `other`, its values on this tree's branches:
    /// a branch gets the value of the branch in `other` that splits the
    /// tips the same way, or none. Returned per set as one value per
    /// internal node in preorder, ready for `tree.add_support`. Tips are
    /// matched by name; tips missing from either tree are an error.
    pub fn support_from(&self, other: &TreeDoc) -> Result<Vec<(String, SupportValues)>, EditError> {
        let mut names: Vec<&str> = self
            .nodes
            .iter()
            .filter(|n| n.is_tip())
            .map(|n| n.name.as_str())
            .collect();
        names.sort_unstable();
        let mut theirs: Vec<&str> = other
            .nodes
            .iter()
            .filter(|n| n.is_tip())
            .map(|n| n.name.as_str())
            .collect();
        theirs.sort_unstable();
        if names != theirs {
            let missing = names.iter().find(|n| theirs.binary_search(n).is_err());
            let extra = theirs.iter().find(|n| names.binary_search(n).is_err());
            return err(match (missing, extra) {
                (Some(m), _) => format!("{m} isn't in the other tree"),
                (_, Some(x)) => format!("the other tree has {x}, which this one doesn't"),
                _ => "the trees have different tips".into(),
            });
        }
        let index = |name: &str| names.binary_search(&name).unwrap_or(0);
        let ours = self.splits(&index);
        // A rooted tree's two root branches are one split; either may hold
        // the value.
        let mut by_split: std::collections::HashMap<Vec<u64>, Vec<usize>> =
            std::collections::HashMap::new();
        for (node, split) in other.splits(&index).into_iter().enumerate() {
            if let Some(s) = split {
                by_split.entry(s).or_default().push(node);
            }
        }
        let internal = self.internal_preorder();
        Ok(other
            .supports
            .iter()
            .map(|set| {
                let values = internal
                    .iter()
                    .map(|&i| {
                        let s = ours[i].as_ref()?;
                        by_split.get(s)?.iter().find_map(|&n| set.values[n].clone())
                    })
                    .collect();
                (set.name.clone(), values)
            })
            .collect())
    }

    /// Each non-root node's split, as a bitset over tip indices, flipped so
    /// tip 0 is never in it: the same branch gives the same split however
    /// the tree is rooted.
    fn splits(&self, index: &dyn Fn(&str) -> usize) -> Vec<Option<Vec<u64>>> {
        let tips = self.tip_count();
        let words = tips.div_ceil(64);
        let mut sets: Vec<Vec<u64>> = vec![vec![0; words]; self.nodes.len()];
        for &i in self.preorder().iter().rev() {
            if self.nodes[i].is_tip() {
                let t = index(&self.nodes[i].name);
                sets[i][t / 64] |= 1 << (t % 64);
            } else {
                for &c in &self.nodes[i].children.clone() {
                    let child = sets[c].clone();
                    for (w, b) in sets[i].iter_mut().zip(child) {
                        *w |= b;
                    }
                }
            }
        }
        let last = if tips % 64 == 0 {
            u64::MAX
        } else {
            (1u64 << (tips % 64)) - 1
        };
        sets.into_iter()
            .enumerate()
            .map(|(i, mut s)| {
                if i == self.root {
                    return None;
                }
                if s[0] & 1 == 1 {
                    for w in &mut s {
                        *w = !*w;
                    }
                    if let Some(w) = s.last_mut() {
                        *w &= last;
                    }
                }
                Some(s)
            })
            .collect()
    }

    // Writing.

    /// The tree as Newick. Internal nodes carry their support values joined
    /// with `/`, in set order, or their name.
    pub fn to_newick(&self) -> String {
        let mut out = String::new();
        self.write_node(self.root, &mut out);
        out.push(';');
        out
    }

    fn write_node(&self, i: usize, out: &mut String) {
        let node = &self.nodes[i];
        if !node.is_tip() {
            out.push('(');
            for (k, &c) in node.children.iter().enumerate() {
                if k > 0 {
                    out.push(',');
                }
                self.write_node(c, out);
            }
            out.push(')');
        }
        let label = if node.is_tip() || self.supports.is_empty() || self.support_text(i).is_empty()
        {
            if node.name.is_empty() {
                String::new()
            } else {
                newick::quote_name(&node.name)
            }
        } else {
            self.support_text(i)
        };
        out.push_str(&label);
        if i != self.root
            && let Some(l) = node.length
        {
            out.push(':');
            out.push_str(&newick::format_length(l));
        }
    }

    /// Every set's value at `node`, joined with `/`; empty if none has one.
    fn support_text(&self, node: usize) -> String {
        if self.supports.iter().all(|s| s.values[node].is_none()) {
            return String::new();
        }
        self.supports
            .iter()
            .map(|s| s.values[node].clone().unwrap_or_else(|| "-".into()))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Set names and sources, annotations and style, restored from a
    /// sidecar file after reading the Newick.
    pub fn restore(
        &mut self,
        support_names: &[(String, String)],
        annotations: BTreeMap<u32, CladeAnnotation>,
        style: TreeStyle,
    ) {
        if support_names.len() == self.supports.len() {
            for (s, (name, source)) in self.supports.iter_mut().zip(support_names) {
                s.name.clone_from(name);
                s.source.clone_from(source);
            }
        }
        // Values written as "-" mean none.
        for s in &mut self.supports {
            for v in &mut s.values {
                if v.as_deref() == Some("-") {
                    *v = None;
                }
            }
        }
        self.annotations = annotations
            .into_iter()
            .filter(|(_, a)| self.resolve(&a.clade).is_ok())
            .collect();
        self.style = style;
    }
}

struct Edge {
    length: Option<f64>,
    support: Vec<Option<String>>,
}

/// The value of `key=` in a comment such as `prob=1.0,prob_stddev=0`.
fn comment_value(comment: &str, key: &str) -> Option<String> {
    comment.split(',').find_map(|part| {
        let (k, v) = part.split_once('=')?;
        (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().to_string())
    })
}

/// "1.00000000e+00" as "1", "9.87e-01" as "0.99".
fn tidy_number(s: &str) -> String {
    match s.parse::<f64>() {
        Ok(v) if v.fract() == 0.0 => format!("{v:.0}"),
        Ok(v) => {
            let t = format!("{v:.2}");
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        }
        Err(_) => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(text: &str) -> TreeDoc {
        TreeDoc::from_newick("t", text).unwrap()
    }

    fn tip_names(t: &TreeDoc) -> Vec<String> {
        t.tips_in_order()
            .into_iter()
            .map(|i| t.node(i).name.clone())
            .collect()
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn reads_supports_from_labels_and_comments() {
        let t = tree("((A:1,B:1)95/1:1,(C:1,D:1)80/0.9[&prob=0.987]:1)root;");
        assert_eq!(t.supports().len(), 3);
        assert_eq!(t.supports()[0].name, "Support 1");
        let ab = t.resolve(&names(&["A", "B"])).unwrap();
        assert_eq!(t.supports()[0].values[ab].as_deref(), Some("95"));
        assert_eq!(t.supports()[1].values[ab].as_deref(), Some("1"));
        assert!(t.supports()[1].proportions && !t.supports()[0].proportions);
        let cd = t.resolve(&names(&["C", "D"])).unwrap();
        assert_eq!(t.supports()[2].values[cd].as_deref(), Some("0.99"));
        assert_eq!(t.node(t.root()).name, "root");
        assert_eq!(
            t.to_newick(),
            "((A:1,B:1)95/1/-:1,(C:1,D:1)80/0.9/0.99:1)root;"
        );
    }

    #[test]
    fn rotate_ladderize_rename() {
        let mut t = tree("((A,(B,C)),D);");
        t.rotate(&names(&["B", "C"])).unwrap();
        assert_eq!(tip_names(&t), ["A", "C", "B", "D"]);
        t.ladderize(true);
        assert_eq!(tip_names(&t), ["D", "A", "C", "B"]);
        t.ladderize(false);
        assert_eq!(tip_names(&t), ["C", "B", "A", "D"]);
        t.annotate(1, Some(CladeAnnotation::new(names(&["B", "C"]), "x")))
            .unwrap();
        t.rename_tip("B", "Bee").unwrap();
        assert_eq!(t.annotations()[&1].clade, names(&["Bee", "C"]));
        assert!(t.rename_tip("C", "Bee").is_err());
    }

    #[test]
    fn reroot_moves_lengths_and_support() {
        // Unrooted, as IQ-TREE writes it: three children at the root.
        let mut t = tree("(A:1,B:2,(C:3,(D:4,E:5)90:6)80:7);");
        t.reroot(&names(&["D", "E"])).unwrap();
        assert_eq!(t.node(t.root()).children.len(), 2);
        let de = t.resolve(&names(&["D", "E"])).unwrap();
        assert_eq!(t.node(de).length, Some(3.0));
        assert_eq!(t.supports()[0].values[de].as_deref(), Some("90"));
        // The other side of the root: C, A and B, with the old root's
        // branch to C's parent now below it.
        assert_eq!(t.tip_count(), 5);
        let total: f64 = t.nodes().iter().filter_map(|n| n.length).sum();
        assert!((total - 28.0).abs() < 1e-9, "{total}");
        // Rerooting back and forth keeps branches and their support.
        t.reroot(&names(&["A"])).unwrap();
        t.reroot(&names(&["D", "E"])).unwrap();
        let de = t.resolve(&names(&["D", "E"])).unwrap();
        assert_eq!(t.supports()[0].values[de].as_deref(), Some("90"));
        let total: f64 = t.nodes().iter().filter_map(|n| n.length).sum();
        assert!((total - 28.0).abs() < 1e-9, "{total}");
        // The branch that grouped C, D and E now groups A and B.
        let ab = t.resolve(&names(&["A", "B"])).unwrap();
        assert_eq!(t.supports()[0].values[ab].as_deref(), Some("80"));
        // A bifurcating root merges into one branch.
        let mut r = tree("((A:1,B:1)70:2,(C:1,D:1)70:3);");
        r.reroot(&names(&["A"])).unwrap();
        let total: f64 = r.nodes().iter().filter_map(|n| n.length).sum();
        assert!((total - 9.0).abs() < 1e-9, "{total}");
        assert_eq!(r.tip_count(), 4);
        let cd = r.resolve(&names(&["C", "D"])).unwrap();
        assert_eq!(r.node(cd).length, Some(5.0));
        assert_eq!(r.supports()[0].values[cd].as_deref(), Some("70"));
    }

    #[test]
    fn root_position_splits_the_root_branch() {
        let mut t = tree("((A:1,B:1):2,(C:1,D:1):6);");
        let (_, _, l) = t.root_branch().unwrap();
        assert!((l - 8.0).abs() < 1e-12);
        t.set_root_position(&names(&["C", "D"]), 0.25).unwrap();
        let cd = t.resolve(&names(&["C", "D"])).unwrap();
        let ab = t.resolve(&names(&["A", "B"])).unwrap();
        assert_eq!(t.node(cd).length, Some(2.0));
        assert_eq!(t.node(ab).length, Some(6.0));
        assert!(t.set_root_position(&names(&["A"]), 0.5).is_err());
        assert!(t.set_root_position(&names(&["A", "B"]), 1.5).is_err());
    }

    #[test]
    fn support_from_another_tree_matches_branches() {
        let t = tree("((A,B)90,(C,(D,E)85)60,F);");
        // Rooted elsewhere, (A,B) missing, posterior on the rest.
        let other = tree(
            "(((D:1,E:1)[&prob=1]:1,C:1)[&prob=0.95]:1,(A:1,(B:1,F:1)[&prob=0.5]:1)[&prob=0.7]:1);",
        );
        let got = t.support_from(&other).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "Posterior");
        let internal = t.internal_preorder();
        let at = |clade: &[&str]| {
            let n = t.resolve(&names(clade)).unwrap();
            got[0].1[internal.iter().position(|&i| i == n).unwrap()].clone()
        };
        assert_eq!(at(&["D", "E"]).as_deref(), Some("1"));
        assert_eq!(at(&["C", "E"]).as_deref(), Some("0.95"));
        assert_eq!(at(&["A", "B"]), None);
        let mut t = t;
        t.add_support("PP", "other.tre", &got[0].1).unwrap();
        assert_eq!(t.supports().len(), 2);
        assert!(t.add_support("PP", "", &got[0].1).is_err());
        let wrong = tree("((A,B),(C,D));");
        assert!(t.support_from(&wrong).is_err());
    }
}
