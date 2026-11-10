//! Deterministic bounded-depth trees through the ordinary recoverable Session.
use super::Result;
use basecraft_docs::tree::TreeDoc;
use basecraft_ops::{Session, log, replay};
use basecraft_platform::paths::AppPaths;
use basecraft_protocol::Op;
use serde_json::json;
use std::{fmt::Write, path::Path, time::Instant};

const TIPS: usize = 50_000;

// Balanced binary splits, ceil(log2(TIPS)) depth, no random generator/state.
// Integer branch units are exactly representable; all input edges are positive.
fn subtree(out: &mut String, start: usize, count: usize, root: bool) -> Result<()> {
    if count == 1 {
        write!(out, "tip{start:05}")?;
    } else {
        out.push('(');
        let left = count / 2;
        subtree(out, start, left, false)?;
        out.push(',');
        subtree(out, start + left, count - left, false)?;
        out.push(')');
    }
    if !root {
        write!(out, ":{}", 1 + (start * 17 + count * 13) % 97)?;
    }
    Ok(())
}

// One bounded traversal: validate reachability, unique parents, reciprocal edges,
// all exact tip identities, and finite nonnegative branch data. Rooting may split
// an edge at an endpoint, so output zero lengths are valid.
fn verify(doc: &TreeDoc) -> Result<f64> {
    let nodes = doc.nodes();
    if nodes.len() != 2 * TIPS - 1 || doc.root() >= nodes.len() {
        return Err("tree node count/root changed".into());
    }
    let mut seen = vec![false; nodes.len()];
    let mut tips = vec![false; TIPS];
    let mut stack = vec![doc.root()];
    let mut total = 0.0;
    while let Some(i) = stack.pop() {
        if i >= nodes.len() || seen[i] {
            return Err("tree cycle or repeated child".into());
        }
        seen[i] = true;
        let node = &nodes[i];
        if i == doc.root() {
            if node.parent.is_some() || node.length.is_some() {
                return Err("invalid root branch".into());
            }
        } else {
            let length = node.length.ok_or("missing branch length")?;
            if !length.is_finite() || length < 0.0 {
                return Err("invalid branch length".into());
            }
            total += length;
        }
        if node.is_tip() {
            let number: usize = node
                .name
                .strip_prefix("tip")
                .ok_or("invalid tip name")?
                .parse()?;
            if number >= TIPS || node.name != format!("tip{number:05}") || tips[number] {
                return Err("duplicate or changed tip identity".into());
            }
            tips[number] = true;
        } else if node.children.len() != 2 {
            return Err("nonbinary tree fixture/output".into());
        }
        for &child in &node.children {
            if child >= nodes.len() || nodes[child].parent != Some(i) {
                return Err("tree parent/child mismatch".into());
            }
            stack.push(child);
        }
    }
    if seen.contains(&false) || tips.contains(&false) || !total.is_finite() {
        return Err("tree reachability/tip/length verification failed".into());
    }
    Ok(total)
}

pub fn run(scenario: &str, root: &Path) -> Result<()> {
    if !matches!(scenario, "tree-open-50000" | "tree-midpoint-50000") {
        return Err("unknown tree scenario".into());
    }
    let paths = AppPaths::discover()?;
    if !root.is_absolute() || paths.config != root.join("profile/config") {
        return Err("runner must supply an isolated absolute profile".into());
    }
    let mut input = String::from("[&U] ");
    subtree(&mut input, 0, TIPS, true)?;
    input.push_str(";\n");
    let file = root.join("tree.nwk");
    std::fs::write(&file, input.as_bytes())?;
    let fixture_bytes = file.metadata()?.len();
    let source_hash = basecraft_io::hash_bytes(input.as_bytes());
    let mut warm = Session::recoverable_with_paths(root, &paths)?;
    warm.open(&file)?;
    drop(warm);
    let mut session = Session::recoverable_with_paths(root, &paths)?;
    let start = Instant::now();
    let opened = session.open(&file);
    let mut duration_ns = start.elapsed().as_nanos();
    let id = opened?;
    if !session.is_tree(id) {
        return Err("fixture did not open as a tree".into());
    }
    let before_total = verify(session.tree(id))?;
    let before = session.tree(id).to_newick_with_rooting();
    let hashes = session.save_hashes(id)?;
    if scenario == "tree-midpoint-50000" {
        let op = Op::TreeMidpointRoot {};
        let start = Instant::now();
        let applied = session.apply(id, op);
        duration_ns = start.elapsed().as_nanos();
        applied?;
        let after_total = verify(session.tree(id))?;
        if (after_total - before_total).abs() > before_total * 1e-12
            || session.tree(id).rooted != Some(true)
        {
            return Err("reroot changed branch total or omitted rooting declaration".into());
        }
        let after = session.tree(id).to_newick_with_rooting();
        if after == before {
            return Err("midpoint operation did not change fixture".into());
        }
        // Undo, redo and durable-log replay are verification, not measured work.
        session.undo(id)?;
        verify(session.tree(id))?;
        if session.tree(id).to_newick_with_rooting() != before || session.save_hashes(id)? != hashes
        {
            return Err("undo did not restore original tree semantics/hashes".into());
        }
        session.redo(id)?;
        verify(session.tree(id))?;
        if session.tree(id).to_newick_with_rooting() != after {
            return Err("redo did not reproduce midpoint result".into());
        }
        session.flush()?;
        let entries = log::read_all(session.recovery_root())?;
        if !entries
            .iter()
            .any(|e| matches!(e.op, Op::TreeMidpointRoot {}))
        {
            return Err("midpoint operation missing from recovery journal".into());
        }
        let replayed = replay::replay(session.recovery_root())?;
        if !replayed.ok() || replayed.entries != entries.len() || replayed.entries < 4 {
            return Err(format!("tree recovery replay failed: {replayed:?}").into());
        }
    }
    if basecraft_io::hash_file(&file)? != source_hash || std::fs::read(&file)? != input.as_bytes() {
        return Err("tree source was modified".into());
    }
    println!(
        "{}",
        json!({
            "schema_version": 2, "scenario": scenario, "duration_ns": duration_ns,
            "operations": 1, "verified": true, "fixture_bytes": fixture_bytes,
            "output_bytes": file.metadata()?.len(), "rows": TIPS, "columns": null,
            "edits": if scenario == "tree-open-50000" { 0 } else { 1 },
            "work": {"protocol": "session-tree-v1", "tips": TIPS,
                "shape": "balanced-binary-floor-half-v1", "tip_names": "tipNNNNN-zero-based",
                "branches": "integer-1+(start*17+count*13)%97-v1", "source_rooting": "unrooted",
                "method": if scenario == "tree-open-50000" { "open" } else { "midpoint" },
                "recovery": "default-durable-first-edit", "operation_count": 1,
                "input_hash": source_hash},
            "source_commit": option_env!("BASECRAFT_BENCH_REVISION").unwrap_or("unknown"),
            "debug_assertions": cfg!(debug_assertions)
        })
    );
    Ok(())
}
