//! Whole-tree, branch-length rooting. See crates/basecraft-docs/ROOTING.md.
use super::{EditError, TreeDoc, TreeNode, err};

#[derive(Clone)]
struct Branch {
    ends: [usize; 2],
    length: f64,
    support: Vec<Option<String>>,
}

struct Graph {
    adj: Vec<Vec<(usize, usize)>>,
    edges: Vec<Branch>,
    // Original incoming branch -> undirected branch (also for merged roots).
    incoming: Vec<Option<usize>>,
    anchor: usize,
    scale: f64,
}

/// A stable, mergeable population moment; no subtraction of squared means.
#[derive(Clone, Copy, Default)]
struct Moments {
    count: usize,
    mean: f64,
    m2: f64,
}

impl Moments {
    fn shifted(self, by: f64) -> Self {
        Self {
            mean: self.mean + by,
            ..self
        }
    }

    // A resident node arena cannot approach 2^53 tips; these counts are exact.
    #[allow(clippy::cast_precision_loss)]
    fn plus(self, other: Self) -> Self {
        if self.count == 0 {
            return other;
        }
        if other.count == 0 {
            return self;
        }
        let count = self.count + other.count;
        let weight = other.count as f64 / count as f64;
        let delta = other.mean - self.mean;
        Self {
            count,
            mean: self.mean + delta * weight,
            m2: self.m2 + other.m2 + delta * delta * self.count as f64 * weight,
        }
    }
}

impl TreeDoc {
    /// Midpoint of a longest tip-to-tip path. No selection is used.
    pub fn midpoint_root(&mut self) -> Result<(), EditError> {
        let graph = self.rooting_graph()?;
        let (edge, position) = graph.midpoint(self);
        self.install_root(&graph, edge, position)
    }

    /// Global minimum of unweighted root-to-all-tip distance variance,
    /// including every edge interior and endpoint (MRTTV).
    pub fn minimum_variance_root(&mut self) -> Result<(), EditError> {
        let graph = self.rooting_graph()?;
        let (edge, position) = graph.minimum_variance(self);
        self.install_root(&graph, edge, position)
    }

    fn rooting_graph(&self) -> Result<Graph, EditError> {
        let mut tips: Vec<_> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_tip())
            .collect();
        if tips.len() < 2 {
            return err("automatic rooting needs at least two tips");
        }
        tips.sort_unstable_by(|a, b| a.1.name.cmp(&b.1.name));
        if tips.windows(2).any(|w| w[0].1.name == w[1].1.name) {
            return err("automatic rooting needs unique tip names; rename repeated tips first");
        }
        if self.nodes[self.root].children.len() == 1 {
            return err("automatic rooting does not support a unary root; remove its stem first");
        }
        let mut scale = 0.0_f64;
        let mut smallest = f64::MAX;
        for (i, node) in self.nodes.iter().enumerate() {
            if i == self.root {
                continue;
            }
            let Some(length) = node.length else {
                return err("automatic rooting needs a length on every branch");
            };
            if !length.is_finite() || length < 0.0 {
                return err("automatic rooting needs finite, nonnegative branch lengths");
            }
            scale = scale.max(length);
            if length > 0.0 {
                smallest = smallest.min(length);
            }
        }
        if scale == 0.0 {
            return err("automatic rooting is undefined when all branch lengths are zero");
        }
        if smallest / scale < 1e-100 {
            return err("branch-length range is too large for reliable automatic rooting");
        }
        let root_annotations = self
            .annotations
            .values()
            .map(|a| self.resolve(&a.clade))
            .collect::<Result<Vec<_>, _>>()?;
        let children = &self.nodes[self.root].children;
        let merge = children.len() == 2
            && self.nodes[self.root].name.is_empty()
            && !root_annotations.contains(&self.root)
            && self.supports.iter().all(|s| {
                let (a, b) = (&s.values[children[0]], &s.values[children[1]]);
                a.is_none() || b.is_none() || a == b
            });
        let mut graph = Graph {
            adj: vec![Vec::new(); self.nodes.len()],
            edges: Vec::new(),
            incoming: vec![None; self.nodes.len()],
            anchor: tips[0].0,
            scale,
        };
        for (i, node) in self.nodes.iter().enumerate() {
            let Some(parent) = node.parent else {
                continue;
            };
            if merge && parent == self.root {
                continue;
            }
            let edge = graph.add_branch(
                parent,
                i,
                node.length.unwrap_or(0.0),
                self.supports.iter().map(|s| s.values[i].clone()).collect(),
            );
            graph.incoming[i] = Some(edge);
        }
        if merge {
            let (a, b) = (children[0], children[1]);
            let length = self.nodes[a].length.unwrap_or(0.0) + self.nodes[b].length.unwrap_or(0.0);
            if !length.is_finite() {
                return err("merged root branch length exceeds the supported numeric range");
            }
            let edge = graph.add_branch(
                a,
                b,
                length,
                self.supports
                    .iter()
                    .map(|s| s.values[a].clone().or_else(|| s.values[b].clone()))
                    .collect(),
            );
            graph.incoming[a] = Some(edge);
            graph.incoming[b] = Some(edge);
        }
        // Canonical traversal from the alphabetically first tip, sorting
        // components by their smallest tip. Independent of stored root/order.
        let (order, parent, _) = graph.walk(graph.anchor);
        let mut first = vec![None::<usize>; self.nodes.len()];
        for &node in order.iter().rev() {
            if self.nodes[node].is_tip() {
                first[node] = Some(node);
            }
            for &(child, _) in &graph.adj[node] {
                if parent[child] == node
                    && let Some(tip) = first[child]
                    && first[node].is_none_or(|old| self.nodes[tip].name < self.nodes[old].name)
                {
                    first[node] = Some(tip);
                }
            }
        }
        for neighbours in &mut graph.adj {
            neighbours.sort_by(|(a, _), (b, _)| {
                let key = |i: usize| first[i].map_or("", |t| self.nodes[t].name.as_str());
                key(*a).cmp(key(*b))
            });
        }
        Ok(graph)
    }

    // Only exact endpoints may remove the split; a near endpoint still has
    // two nonzero pieces whose lengths must be preserved.
    #[allow(clippy::float_cmp)]
    fn install_root(&mut self, graph: &Graph, edge: usize, position: f64) -> Result<(), EditError> {
        let branch = &graph.edges[edge];
        let [a, b] = branch.ends;
        if !position.is_finite() || !(0.0..=branch.length).contains(&position) {
            return err("automatic root position is outside the supported numeric range");
        }
        let endpoint = if position == 0.0 && !self.nodes[a].is_tip() {
            Some(a)
        } else if position == branch.length && !self.nodes[b].is_tip() {
            Some(b)
        } else {
            None
        };
        let mut result = self.clone();
        result.nodes.clear();
        for set in &mut result.supports {
            set.values.clear();
        }
        let mut mapping = vec![None; self.nodes.len()];
        let mut edge_node = vec![None; graph.edges.len()];
        let mut stack = Vec::new();
        if let Some(node) = endpoint {
            stack.push((node, usize::MAX, None, None, None));
        } else {
            result.nodes.push(TreeNode {
                parent: None,
                children: Vec::new(),
                name: String::new(),
                length: None,
            });
            for set in &mut result.supports {
                set.values.push(None);
            }
            // Store split-edge metadata once, preferentially on an internal
            // side (Newick cannot store support on a labelled leaf).
            let carrier = if self.nodes[a].is_tip() { b } else { a };
            stack.push((
                b,
                a,
                Some(0),
                Some(branch.length - position),
                (carrier == b).then_some(edge),
            ));
            stack.push((
                a,
                b,
                Some(0),
                Some(position),
                (carrier == a).then_some(edge),
            ));
        }
        while let Some((old, from, parent, length, incoming)) = stack.pop() {
            let id = result.nodes.len();
            mapping[old] = Some(id);
            result.nodes.push(TreeNode {
                parent,
                children: Vec::new(),
                name: self.nodes[old].name.clone(),
                length,
            });
            if let Some(p) = parent {
                result.nodes[p].children.push(id);
            }
            if let Some(e) = incoming {
                edge_node[e] = Some(id);
            }
            for (s, set) in result.supports.iter_mut().enumerate() {
                set.values
                    .push(incoming.and_then(|e| graph.edges[e].support[s].clone()));
            }
            for &(next, e) in graph.adj[old].iter().rev() {
                if next != from {
                    stack.push((next, old, Some(id), Some(graph.edges[e].length), Some(e)));
                }
            }
        }
        result.root = 0;
        // Branch labels follow the undirected split. Bars/fields retain their
        // original taxon reference. Mixed annotations cannot safely do both.
        for (&id, annotation) in &self.annotations {
            if !annotation.on_branch {
                continue;
            }
            let old = self.resolve(&annotation.clade)?;
            let new = if old == self.root {
                mapping[old]
            } else {
                graph.incoming[old].and_then(|e| edge_node[e])
            }
            .ok_or_else(|| EditError("cannot preserve a root annotation".into()))?;
            let clade = result.clade_ref(new);
            if result.resolve(&clade)? != new {
                return err("cannot preserve a branch annotation on a unary node after rooting");
            }
            if (annotation.bar || annotation.field) && result.resolve(&annotation.clade)? != new {
                return err(
                    "a combined branch and clade annotation would change meaning; separate its branch label before rooting",
                );
            }
            if let Some(edited) = result.annotations.get_mut(&id) {
                edited.clade = clade;
            }
        }
        if result.nodes.iter().enumerate().any(|(i, n)| {
            !n.name.is_empty()
                && !n.is_tip()
                && result.supports.iter().any(|s| s.values[i].is_some())
        }) {
            return err(
                "rooting would place support on a named internal node that Newick cannot preserve",
            );
        }
        result.edited();
        *self = result;
        Ok(())
    }
}

impl Graph {
    fn add_branch(
        &mut self,
        a: usize,
        b: usize,
        length: f64,
        support: Vec<Option<String>>,
    ) -> usize {
        let edge = self.edges.len();
        self.edges.push(Branch {
            ends: [a, b],
            length,
            support,
        });
        self.adj[a].push((b, edge));
        self.adj[b].push((a, edge));
        edge
    }

    fn walk(&self, start: usize) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
        let mut parent = vec![usize::MAX; self.adj.len()];
        let mut incoming = vec![usize::MAX; self.adj.len()];
        let mut order = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            order.push(node);
            for &(next, edge) in self.adj[node].iter().rev() {
                if next != parent[node] {
                    parent[next] = node;
                    incoming[next] = edge;
                    stack.push(next);
                }
            }
        }
        (order, parent, incoming)
    }

    #[allow(clippy::float_cmp)] // Exact ties, never shorten the diameter.
    fn farthest(&self, doc: &TreeDoc, start: usize) -> (usize, Vec<f64>, Vec<usize>, Vec<usize>) {
        let (order, parent, incoming) = self.walk(start);
        let mut distances = vec![0.0; self.adj.len()];
        let mut farthest = start;
        for &node in &order {
            if node != start {
                distances[node] =
                    distances[parent[node]] + self.edges[incoming[node]].length / self.scale;
            }
            if doc.nodes[node].is_tip()
                && (distances[node] > distances[farthest]
                    || (distances[node] == distances[farthest]
                        && doc.nodes[node].name < doc.nodes[farthest].name))
            {
                farthest = node;
            }
        }
        (farthest, distances, parent, incoming)
    }

    fn midpoint(&self, doc: &TreeDoc) -> (usize, f64) {
        let (a, _, _, _) = self.farthest(doc, self.anchor);
        let (mut b, distances, parent, incoming) = self.farthest(doc, a);
        let half = distances[b] / 2.0;
        while distances[parent[b]] > half {
            b = parent[b];
        }
        let edge = incoming[b];
        let branch = &self.edges[edge];
        let from_parent = ((half - distances[parent[b]]) * self.scale).clamp(0.0, branch.length);
        (
            edge,
            if branch.ends[0] == parent[b] {
                from_parent
            } else {
                branch.length - from_parent
            },
        )
    }

    #[allow(clippy::cast_precision_loss)] // Resident arenas have fewer than 2^53 tips.
    fn minimum_variance(&self, doc: &TreeDoc) -> (usize, f64) {
        let (order, parent, incoming) = self.walk(self.anchor);
        let mut below = vec![Moments::default(); self.adj.len()];
        let mut above = below.clone();
        for &node in order.iter().rev() {
            let mut moment = Moments::default();
            if doc.nodes[node].is_tip() {
                moment.count = 1;
            }
            for &(child, edge) in &self.adj[node] {
                if parent[child] == node {
                    moment =
                        moment.plus(below[child].shifted(self.edges[edge].length / self.scale));
                }
            }
            below[node] = moment;
        }
        let mut best = (f64::INFINITY, 0, 0.0);
        for &node in &order {
            let children: Vec<_> = self.adj[node]
                .iter()
                .copied()
                .filter(|(c, _)| parent[*c] == node)
                .collect();
            let mut prefix = Vec::with_capacity(children.len() + 1);
            let own = Moments {
                count: usize::from(doc.nodes[node].is_tip()),
                ..Moments::default()
            };
            prefix.push(above[node].plus(own));
            for (k, &(child, edge)) in children.iter().enumerate() {
                prefix.push(
                    prefix[k].plus(below[child].shifted(self.edges[edge].length / self.scale)),
                );
            }
            let mut suffix = Moments::default();
            for (k, &(child, edge)) in children.iter().enumerate().rev() {
                let outside = prefix[k].plus(suffix);
                above[child] = outside.shifted(self.edges[edge].length / self.scale);
                suffix = suffix.plus(below[child].shifted(self.edges[edge].length / self.scale));
            }
            if node == self.anchor {
                continue;
            }
            let edge = incoming[node];
            let branch = &self.edges[edge];
            let length = branch.length / self.scale;
            // At x from node towards its parent: below distances increase
            // by x, above distances decrease by x. Equalize group means;
            // their within-group M2 is invariant. This is the edge quadratic.
            let x = ((above[node].mean - below[node].mean) / 2.0).clamp(0.0, length);
            let moment = below[node].shifted(x).plus(above[node].shifted(-x));
            let variance = moment.m2 / moment.count as f64;
            // Canonical traversal wins numerical ties (relative tolerance).
            let tolerance = 64.0 * f64::EPSILON * variance.abs().max(best.0.abs());
            if !best.0.is_finite() || variance < best.0 - tolerance {
                let x = (x * self.scale).clamp(0.0, branch.length);
                best = (
                    variance,
                    edge,
                    if branch.ends[0] == node {
                        x
                    } else {
                        branch.length - x
                    },
                );
            }
        }
        (best.1, best.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fmt::Write;

    fn tree(text: &str) -> TreeDoc {
        TreeDoc::from_newick("rooting", text).unwrap()
    }
    fn close(a: f64, b: f64) {
        assert!(
            (a - b).abs() <= 2e-10 * a.abs().max(b.abs()).max(1e-200),
            "{a} != {b}"
        );
    }
    fn distances(doc: &TreeDoc, start: usize) -> Vec<f64> {
        let mut distances = vec![0.0; doc.nodes.len()];
        let mut stack = vec![(start, usize::MAX)];
        while let Some((node, from)) = stack.pop() {
            let mut neighbours: Vec<_> = doc.nodes[node]
                .children
                .iter()
                .map(|&c| (c, doc.nodes[c].length.unwrap()))
                .collect();
            if let Some(p) = doc.nodes[node].parent {
                neighbours.push((p, doc.nodes[node].length.unwrap()));
            }
            for (next, length) in neighbours {
                if next != from {
                    distances[next] = distances[node] + length;
                    stack.push((next, node));
                }
            }
        }
        distances
    }
    fn tip_distances(doc: &TreeDoc) -> BTreeMap<String, f64> {
        let d = distances(doc, doc.root);
        doc.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_tip())
            .map(|(i, n)| (n.name.clone(), d[i]))
            .collect()
    }
    #[allow(clippy::cast_precision_loss)] // Small independent oracle fixtures.
    fn variance(values: &[f64]) -> f64 {
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        values.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / values.len() as f64
    }
    #[allow(clippy::items_after_statements)] // Local split oracle supports only this assertion.
    fn invariants(before: &TreeDoc, after: &TreeDoc) {
        assert_eq!(before.tip_count(), after.tip_count());
        for start in before.tips_in_order() {
            let old = distances(before, start);
            let new = distances(after, after.find_tip(&before.nodes[start].name).unwrap());
            for end in before.tips_in_order() {
                close(
                    old[end],
                    new[after.find_tip(&before.nodes[end].name).unwrap()],
                );
            }
        }
        // Splits and aggregated lengths include zero branches and degree-two
        // subdivisions. This catches topology changes even at zero length.
        fn splits(doc: &TreeDoc) -> BTreeMap<Vec<String>, (f64, Vec<String>)> {
            let mut all: Vec<_> = doc
                .nodes
                .iter()
                .filter(|n| n.is_tip())
                .map(|n| n.name.clone())
                .collect();
            all.sort();
            let mut result: BTreeMap<Vec<String>, (f64, Vec<String>)> = BTreeMap::new();
            for (i, node) in doc.nodes.iter().enumerate() {
                if node.parent.is_none() {
                    continue;
                }
                let mut names = Vec::new();
                let mut stack = vec![i];
                while let Some(n) = stack.pop() {
                    if doc.nodes[n].is_tip() {
                        names.push(doc.nodes[n].name.clone());
                    }
                    stack.extend(&doc.nodes[n].children);
                }
                names.sort();
                if names.contains(&all[0]) {
                    names = all.iter().filter(|s| !names.contains(s)).cloned().collect();
                }
                let entry = result.entry(names).or_default();
                entry.0 += node.length.unwrap();
                for set in &doc.supports {
                    if let Some(v) = &set.values[i] {
                        entry.1.push(format!("{}:{v}", set.name));
                    }
                }
            }
            for (_, values) in result.values_mut() {
                values.sort();
                values.dedup();
            }
            result
        }
        let old = splits(before);
        let new = splits(after);
        assert_eq!(
            old.keys().collect::<Vec<_>>(),
            new.keys().collect::<Vec<_>>()
        );
        for (split, (length, support)) in old {
            close(length, new[&split].0);
            assert_eq!(support, new[&split].1);
        }
    }

    #[test]
    fn automatic_root_hand_solved_methods_differ() {
        let original = tree("(A:1,B:3,C:6);");
        let mut midpoint = original.clone();
        midpoint.midpoint_root().unwrap();
        let mut variance = original.clone();
        variance.minimum_variance_root().unwrap();
        let mp = tip_distances(&midpoint);
        let mv = tip_distances(&variance);
        close(mp["A"], 2.5);
        close(mp["B"], 4.5);
        close(mp["C"], 4.5);
        close(mv["A"], 3.0);
        close(mv["B"], 5.0);
        close(mv["C"], 4.0);
        invariants(&original, &midpoint);
        invariants(&original, &variance);
    }

    /// Independent brute-force oracle: enumerate every original edge, measure
    /// distances to each tip, and interpolate the variance quadratic from
    /// evaluations at 0, 1/2 and 1. Does not use production traversal moments.
    fn oracle(doc: &TreeDoc) -> f64 {
        let tips = doc.tips_in_order();
        let mut best = f64::INFINITY;
        for (node, n) in doc.nodes.iter().enumerate() {
            let Some(parent) = n.parent else {
                continue;
            };
            let left = distances(doc, node);
            let right = distances(doc, parent);
            let length = n.length.unwrap();
            let value = |fraction: f64| {
                variance(
                    &tips
                        .iter()
                        .map(|&tip| {
                            (left[tip] + fraction * length)
                                .min(right[tip] + (1.0 - fraction) * length)
                        })
                        .collect::<Vec<_>>(),
                )
            };
            let (v0, vm, v1) = (value(0.0), value(0.5), value(1.0));
            let quadratic = 2.0 * (v1 + v0 - 2.0 * vm);
            let linear = v1 - v0 - quadratic;
            let x = if quadratic > 0.0 {
                (-linear / (2.0 * quadratic)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            best = best.min(value(x)).min(v0).min(v1);
        }
        best
    }

    #[test]
    fn automatic_root_matches_independent_edge_oracle_and_prior_roots() {
        for seed in 1_u64..41 {
            let mut state = seed;
            let mut length = || {
                state = (state * 48_271) % 2_147_483_647;
                f64::from(u32::try_from(state % 19).unwrap()) / 4.0
            };
            let text = format!(
                "((A:{},B:{}):{},C:{},(D:{},E:{},F:{}):{});",
                length(),
                length(),
                length(),
                length(),
                length(),
                length(),
                length(),
                length()
            );
            let original = tree(&text);
            let expected = oracle(&original);
            let mut result = original.clone();
            result.minimum_variance_root().unwrap();
            let got = variance(&tip_distances(&result).values().copied().collect::<Vec<_>>());
            assert!(
                (got - expected).abs() < 1e-10,
                "{text}: {got} != {expected}"
            );
            invariants(&original, &result);
            for method in [TreeDoc::midpoint_root, TreeDoc::minimum_variance_root] {
                let mut baseline = original.clone();
                method(&mut baseline).unwrap();
                for name in ["A", "C", "E"] {
                    let mut moved = original.clone();
                    moved.reroot(&[name.into()]).unwrap();
                    method(&mut moved).unwrap();
                    let a = tip_distances(&baseline);
                    let b = tip_distances(&moved);
                    for tip in a.keys() {
                        close(a[tip], b[tip]);
                    }
                }
            }
        }
    }

    #[test]
    fn automatic_root_equal_variance_minima_choose_canonical_tip() {
        // Two distinct global minima: three units toward either long tip.
        // Canonical traversal from A chooses the A edge, irrespective of root.
        for text in ["((A:10,B:1):0,C:10,D:1);", "(D:1,C:10,(B:1,A:10):0);"] {
            for previous in ["A", "B", "C", "D"] {
                let mut result = tree(text);
                result.reroot(&[previous.into()]).unwrap();
                result.minimum_variance_root().unwrap();
                let depths = tip_distances(&result);
                close(depths["A"], 7.0);
                close(depths["B"], 4.0);
                close(depths["C"], 13.0);
                close(depths["D"], 4.0);
                close(
                    variance(&depths.values().copied().collect::<Vec<_>>()),
                    13.5,
                );
            }
        }
    }

    #[test]
    fn automatic_root_endpoints_zeros_polytomies_two_tips_and_scaling() {
        for text in [
            "(A:2,B:2,C:2);",
            "(A:0,B:0,C:2,D:0);",
            "(A:1,B:5);",
            "((A:1,B:2):0,C:3,D:0);",
            "(A:1e-200,B:3e-200,C:6e-200);",
            "(A:1e200,B:3e200,C:6e200);",
        ] {
            let original = tree(text);
            for method in [TreeDoc::midpoint_root, TreeDoc::minimum_variance_root] {
                let mut rooted = original.clone();
                method(&mut rooted).unwrap();
                invariants(&original, &rooted);
                let roundtrip = tree(&rooted.to_newick());
                invariants(&rooted, &roundtrip);
                let first = tip_distances(&rooted);
                method(&mut rooted).unwrap();
                for (name, distance) in first {
                    close(distance, tip_distances(&rooted)[&name]);
                }
            }
        }
        let mut symmetric = tree("(A:2,B:2,C:2);");
        symmetric.minimum_variance_root().unwrap();
        assert_eq!(symmetric.nodes[symmetric.root].children.len(), 3);
    }

    #[test]
    fn automatic_root_metadata_and_artificial_root_policy() {
        for text in [
            "((A:1,B:3)90:2,(C:6,D:1)90:3);",
            "((A:1,B:3)90:2,(C:6,D:1)80:3);",
            "((A:1,B:3):2,(C:6,D:1):3)named;",
        ] {
            let original = tree(text);
            for method in [TreeDoc::midpoint_root, TreeDoc::minimum_variance_root] {
                let mut rooted = original.clone();
                method(&mut rooted).unwrap();
                invariants(&original, &rooted);
                let roundtrip = tree(&rooted.to_newick());
                invariants(&rooted, &roundtrip);
                if text.contains("named") {
                    assert!(rooted.nodes.iter().any(|n| n.name == "named"));
                }
                method(&mut rooted).unwrap();
                invariants(&original, &rooted);
            }
        }
        let mut annotated = tree("(A:1,B:2,(C:3,D:9)90:4);");
        let mut annotation =
            basecraft_protocol::CladeAnnotation::new(vec!["C".into(), "D".into()], "branch label");
        annotation.on_branch = true;
        annotation.bar = false;
        annotated.annotate(1, Some(annotation)).unwrap();
        annotated.minimum_variance_root().unwrap();
        let node = annotated.resolve(&annotated.annotations[&1].clade).unwrap();
        assert_eq!(annotated.supports[0].values[node].as_deref(), Some("90"));
    }

    #[test]
    fn automatic_root_multiple_support_sets_and_annotation_refusals() {
        let original = tree("(A:1,B:2,(C:3,D:9)90/-:4,(E:1,F:2)-/0.8:2);");
        assert_eq!(original.supports.len(), 2);
        for method in [TreeDoc::midpoint_root, TreeDoc::minimum_variance_root] {
            let mut result = original.clone();
            method(&mut result).unwrap();
            invariants(&original, &result);
            let reloaded = tree(&result.to_newick());
            invariants(&result, &reloaded);
            assert_eq!(reloaded.supports, result.supports);
        }
        let mut annotated = tree("(A:1,B:2,(C:3,D:9)90:4);");
        let mut annotation =
            basecraft_protocol::CladeAnnotation::new(vec!["C".into(), "D".into()], "mixed");
        annotation.on_branch = true;
        annotated.annotate(1, Some(annotation)).unwrap();
        let before = annotated.to_newick();
        let version = annotated.version;
        assert!(annotated.minimum_variance_root().is_err());
        assert_eq!(annotated.to_newick(), before);
        assert_eq!(annotated.version, version);
        // A named degree-two root remains a vertex; an artificial one does not.
        let mut artificial = tree("((A:1,B:2):3,(C:4,D:5):6);");
        let count = artificial.nodes.len();
        artificial.minimum_variance_root().unwrap();
        assert!(artificial.nodes.len() <= count);
    }

    #[test]
    fn automatic_root_rejections_are_nonmutating() {
        for text in [
            "A;",
            "(A,B:1);",
            "(A:-1,B:2);",
            "(A:1e999,B:2);",
            "(A:0,B:0);",
            "(A:1,A:2);",
            "((A:1,B:2):1);",
            "(A:1e-200,B:1e200);",
        ] {
            let original = tree(text);
            for method in [TreeDoc::midpoint_root, TreeDoc::minimum_variance_root] {
                let mut result = original.clone();
                assert!(method(&mut result).is_err(), "{text}");
                assert_eq!(result.to_newick(), original.to_newick());
                assert_eq!(result.version, original.version);
            }
        }
    }

    #[test]
    fn automatic_root_nonfinite_in_memory_is_rejected() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut result = tree("(A:1,B:2);");
            result.nodes[1].length = Some(value);
            let before = result.to_newick();
            assert!(result.midpoint_root().is_err());
            assert!(result.minimum_variance_root().is_err());
            assert_eq!(result.to_newick(), before);
            assert_eq!(result.version, 0);
        }
    }

    #[test]
    fn automatic_root_large_deep_tree_is_iterative() {
        let count = 12_000;
        let mut text = String::new();
        for i in 0..count {
            write!(text, "(T{i}:1,").unwrap();
        }
        text.push_str("Last:1");
        for _ in 1..count {
            text.push_str("):1");
        }
        text.push_str(");");
        let original = tree(&text);
        for method in [TreeDoc::midpoint_root, TreeDoc::minimum_variance_root] {
            let mut result = original.clone();
            method(&mut result).unwrap();
            assert_eq!(result.tip_count(), count + 1);
            let reloaded = tree(&result.to_newick());
            assert_eq!(reloaded.tip_count(), count + 1);
        }
    }
}
