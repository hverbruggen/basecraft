//! Standalone native preparation engine; plugins use this versioned request.
use basecraft_distance::DistanceMatrixExt;
use basecraft_protocol::{
    delimitation::{DelimitationRequest, DelimitationResult},
    records::{JobRecord, JobStatus},
};
use clap::Parser;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    request: Option<PathBuf>,
    #[arg(long)]
    hash: Option<PathBuf>,
    /// Write the companion of this SPART file for the workspace JSON on stdin.
    #[arg(long)]
    companion: Option<PathBuf>,
}

/// Write the companion beside `spart` for `workspace`; returns its path.
fn write_companion(
    spart: &Path,
    workspace: &basecraft_protocol::delimitation::DelimitationWorkspace,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use basecraft_protocol::companion_records::{Companion, DelimitationCompanion};
    let companion = Companion {
        delimitation: Some(DelimitationCompanion {
            path_base: None,
            workspace: workspace.clone(),
        }),
        ..Companion::new(basecraft_io::hash_file(spart)?)
    };
    let path = PathBuf::from(format!("{}.basecraft.json", spart.display()));
    basecraft_io::write_atomic(&path, |w| companion.write(w))?;
    Ok(path)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(path) = args.hash {
        println!("{}", basecraft_io::hash_file(&path)?);
        return Ok(());
    }
    if let Some(spart) = args.companion {
        let workspace = serde_json::from_reader(std::io::stdin())?;
        write_companion(&spart, &workspace)?;
        return Ok(());
    }
    let request: DelimitationRequest = serde_json::from_slice(&std::fs::read(
        args.request.ok_or("--request is required")?,
    )?)?;
    let started = Instant::now();
    let output = Path::new(&request.output);
    for suffix in [
        "",
        ".spart",
        ".spart.basecraft.json",
        ".unique.fasta",
        ".haplotypes.json",
        ".relative.nwk",
    ] {
        if same_file(
            Path::new(&format!("{}{suffix}", request.output)),
            Path::new(&request.input),
        ) {
            return Err(
                "output artifacts must differ from immutable input, including aliases".into(),
            );
        }
    }
    if request
        .progress
        .as_deref()
        .is_some_and(|path| same_file(Path::new(path), Path::new(&request.input)))
    {
        return Err("progress must differ from immutable input".into());
    }
    let mut result = DelimitationResult {
        version: 1,
        workspace: None,
        metrics: BTreeMap::new(),
        job: JobRecord {
            id: request.job_id.clone(),
            generation: request.generation,
            status: JobStatus::Completed,
            method: request.method.clone(),
            inputs: [(request.input.clone(), request.input_hash.clone())].into(),
            outputs: BTreeMap::new(),
            diagnostics: vec![],
            elapsed_seconds: 0.0,
        },
    };
    progress(&request, "preparation", 0, 1)?;
    let run = run(&request, &mut result).and_then(|()| prepare_workspace(&request, &mut result));
    if let Err(error) = run {
        result.job.status = if request
            .cancel_file
            .as_ref()
            .is_some_and(|p| Path::new(p).exists())
        {
            JobStatus::Cancelled
        } else {
            JobStatus::Failed
        };
        result.workspace = None;
        result.job.diagnostics.push(error.to_string());
    }
    if let Some(workspace) = &result.workspace {
        basecraft_docs::delimitation::validate(workspace)?;
        let spart = PathBuf::from(format!("{}.spart", request.output));
        basecraft_io::write_atomic(&spart, |w| {
            w.write_all(basecraft_io::spart::write_text(workspace).0.as_bytes())
        })?;
        let spart_hash = basecraft_io::hash_file(&spart)?;
        let sidecar = write_companion(&spart, workspace)?;
        result
            .job
            .outputs
            .insert(spart.display().to_string(), spart_hash);
        result.job.outputs.insert(
            sidecar.display().to_string(),
            basecraft_io::hash_file(&sidecar)?,
        );
    }
    progress(&request, "finished", 1, 1)?;
    result.job.elapsed_seconds = started.elapsed().as_secs_f64();
    basecraft_io::write_atomic(output, |w| {
        serde_json::to_writer_pretty(w, &result)?;
        Ok(())
    })?;
    println!("{}", serde_json::to_string(&result)?);
    if result.job.status != JobStatus::Completed {
        std::process::exit(1);
    }
    Ok(())
}

fn same_file(first: &Path, second: &Path) -> bool {
    if first == second {
        return true;
    }
    if let (Ok(left), Ok(right)) = (first.canonicalize(), second.canonicalize())
        && left == right
    {
        return true;
    }
    #[cfg(unix)]
    if let (Ok(left), Ok(right)) = (std::fs::metadata(first), std::fs::metadata(second)) {
        use std::os::unix::fs::MetadataExt;
        if left.dev() == right.dev() && left.ino() == right.ino() {
            return true;
        }
    }
    false
}

fn prepare_workspace(
    request: &DelimitationRequest,
    result: &mut DelimitationResult,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(workspace) = &mut result.workspace {
        for hypothesis in &mut workspace.hypotheses {
            hypothesis
                .provenance
                .diagnostics
                .extend(result.job.diagnostics.clone());
        }
        if let Some(path) = request
            .parameters
            .get("alignment_snapshot")
            .and_then(serde_json::Value::as_str)
        {
            let bytes = std::fs::read(path)?;
            let hash = basecraft_io::hash_bytes(&bytes);
            if request
                .parameters
                .get("alignment_input_hash")
                .and_then(serde_json::Value::as_str)
                != Some(hash.as_str())
            {
                return Err("Retained alignment changed; repeat haplotype preparation".into());
            }
            if let Some(mapping) = request.parameters.get("haplotypes") {
                let index = basecraft_io::fasta::FastaIndex::build(&bytes)?;
                let rows: Result<Vec<_>, Box<dyn std::error::Error>> = index
                    .records
                    .iter()
                    .map(|record| {
                        Ok((
                            record.name.clone(),
                            String::from_utf8(record.read(&bytes, 0, record.len)?)?,
                        ))
                    })
                    .collect();
                let expected = basecraft_delimitation::haplotypes::deduplicate(&rows?)?;
                if serde_json::to_value(expected.members)? != *mapping {
                    return Err("Haplotype mapping differs from retained alignment identity".into());
                }
            }
            for hypothesis in &mut workspace.hypotheses {
                hypothesis
                    .provenance
                    .inputs
                    .insert(path.into(), hash.clone());
            }
        }
        if let Some(mapping) = request.parameters.get("haplotypes") {
            let mapping: BTreeMap<String, std::collections::BTreeSet<String>> =
                serde_json::from_value(mapping.clone())?;
            let tips: std::collections::BTreeSet<_> =
                workspace.specimens.iter().map(|s| s.id.clone()).collect();
            if tips != mapping.keys().cloned().collect() {
                return Err(
                    "Haplotype mapping representatives differ from fitted tree tips".into(),
                );
            }
            for h in &mut workspace.hypotheses {
                let groups = basecraft_delimitation::haplotypes::expand(
                    &h.groups
                        .iter()
                        .map(|g| g.members.clone())
                        .chain(std::iter::once(h.unassigned.clone()))
                        .collect::<Vec<_>>(),
                    &mapping,
                )?;
                h.unassigned = groups.last().cloned().unwrap_or_default();
                for (group, members) in h.groups.iter_mut().zip(groups) {
                    group.members = members;
                    group.individual_scores = group
                        .individual_scores
                        .iter()
                        .flat_map(|(id, scores)| {
                            mapping
                                .get(id)
                                .into_iter()
                                .flatten()
                                .map(move |member| (member.clone(), scores.clone()))
                        })
                        .collect();
                }
                h.provenance.haplotypes = mapping.clone();
            }
            workspace.specimens = mapping
                .values()
                .flatten()
                .map(|id| basecraft_protocol::delimitation::Specimen {
                    id: id.clone(),
                    label: id.clone(),
                    sources: request
                        .parameters
                        .get("original_source")
                        .and_then(serde_json::Value::as_str)
                        .map_or_else(BTreeMap::new, |path| {
                            [(path.to_string(), id.clone())].into()
                        }),
                    attributes: BTreeMap::new(),
                })
                .collect();
        }
    }
    if let Some(workspace) = &result.workspace {
        basecraft_docs::delimitation::validate(workspace)?;
    }
    Ok(())
}

fn run(
    request: &DelimitationRequest,
    result: &mut DelimitationResult,
) -> Result<(), Box<dyn std::error::Error>> {
    if request.version != 1 || request.workers == 0 {
        return Err("unsupported request version or zero workers".into());
    }
    if request
        .cancel_file
        .as_ref()
        .is_some_and(|p| Path::new(p).exists())
    {
        return Err("cancelled before preparation".into());
    }
    if request
        .parameters
        .get("deduplicate")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && request.method != "haplotypes"
        && (!request.parameters.contains_key("alignment_snapshot")
            || !request.parameters.contains_key("haplotypes"))
    {
        return Err(
            "Deduplication requires verified alignment preparation and a representative mapping"
                .into(),
        );
    }
    let input_bytes = std::fs::read(&request.input)?;
    let hash = basecraft_io::hash_bytes(&input_bytes);
    if hash != request.input_hash {
        return Err("immutable input hash differs from request".into());
    }
    if request.method == "haplotypes" || request.method == "workspace_alignment" {
        return alignment_method(request, result, &input_bytes);
    }
    if request.method == "asap" {
        return run_asap(request, result);
    }
    if request.method == "asap_import" {
        return import_asap(request, result, &input_bytes);
    }
    let trees = basecraft_io::newick::parse_trees(std::str::from_utf8(&input_bytes)?)?;
    let parsed = trees
        .get(request.tree_index)
        .ok_or("selected tree index outside file")?;
    let tree = basecraft_docs::TreeDoc::from_parsed(&parsed.name, parsed);
    // The request can state whether the tree is rooted; otherwise the
    // tree's own reading decides: its tag, or two clades at the base (D18).
    let rooting = tree.rooting();
    let rooted = request
        .rooted
        .unwrap_or(rooting.value == basecraft_protocol::facts::Rooting::Rooted);
    if request.rooted.is_none()
        && rooted
        && rooting.source == basecraft_protocol::facts::FactSource::Guessed
    {
        result.job.diagnostics.push(
            "the tree has two clades at its base and no rooting tag, so it was read as rooted (a guess; state it with tree.set_rooting)".into(),
        );
    }
    result
        .metrics
        .insert("tree_count".into(), trees.len().into());
    result
        .metrics
        .insert("selected_tree_index".into(), request.tree_index.into());
    match request.method.as_str() {
        "workspace_tree" => {
            result.workspace = Some(empty_workspace(
                request,
                tree.tips_in_order()
                    .iter()
                    .map(|&i| tree.node(i).name.clone())
                    .collect(),
                true,
            )?);
        }
        "inspect_tree" => {
            result.metrics.insert(
                "tips".into(),
                serde_json::to_value(basecraft_delimitation::time_tree::substitution_tips(
                    &tree, rooted,
                )?)?,
            );
        }
        "ptp" | "mptp" => {
            basecraft_delimitation::time_tree::substitution_tips(&tree, rooted)?;
            let groups: Vec<std::collections::BTreeSet<String>> = serde_json::from_value(
                request
                    .parameters
                    .get("groups")
                    .ok_or("import requires groups")?
                    .clone(),
            )?;
            let likelihood = request
                .parameters
                .get("log_likelihood")
                .and_then(serde_json::Value::as_f64)
                .ok_or("import requires finite log likelihood")?;
            let name = format!(
                "{} species hypothesis ({} species, lnL {likelihood:.2})",
                method_label(&request.method),
                groups.len()
            );
            result.workspace = Some(workspace(request, &tree, groups, likelihood, name));
            if let Some(diagnostics) = request.parameters.get("diagnostics") {
                result
                    .job
                    .diagnostics
                    .extend(serde_json::from_value::<Vec<String>>(diagnostics.clone())?);
            }
        }
        "validate_time_tree" | "manual_cut" => {
            let time = basecraft_delimitation::time_tree::validate(&tree, rooted, true)?;
            result.job.diagnostics.extend(time.diagnostics.clone());
            if request.method == "manual_cut" {
                let threshold = request
                    .parameters
                    .get("threshold")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or("manual cut requires threshold")?;
                let groups = basecraft_delimitation::time_tree::cut(&tree, &time, threshold)?;
                result
                    .metrics
                    .insert("groups".into(), serde_json::to_value(&groups)?);
                let name = format!("Manual threshold cut at {threshold}");
                let mut work = workspace(request, &tree, groups, 0.0, name);
                for hypothesis in &mut work.hypotheses {
                    hypothesis.origin =
                        basecraft_protocol::delimitation::HypothesisOrigin::ManualCut;
                    hypothesis.scores.clear();
                    hypothesis.provenance.seed = None;
                    hypothesis.provenance.workers = None;
                    hypothesis.provenance.parameters.insert(
                        "boundary".into(),
                        serde_json::json!("node_age_le_threshold"),
                    );
                }
                result.workspace = Some(work);
            }
            result
                .metrics
                .insert("time_tree".into(), serde_json::to_value(time)?);
        }
        "relative_time" => {
            let converted = basecraft_delimitation::time_tree::convert(&tree, rooted, 10_000)?;
            if !converted.converged {
                return Err("relative-time optimization did not converge".into());
            }
            let mut derived = parsed.clone();
            for (node, length) in derived.nodes.iter_mut().zip(&converted.derived_lengths) {
                node.length = *length;
            }
            let derived = basecraft_docs::TreeDoc::from_parsed(&parsed.name, &derived);
            artifact(
                request,
                result,
                ".relative.nwk",
                derived.to_newick().as_bytes(),
            )?;
            result
                .metrics
                .insert("conversion".into(), serde_json::to_value(converted)?);
        }
        "gmyc_single" | "gmyc_multiple" | "gmyc_exhaustive" => {
            gmyc(request, result, &tree, rooted)?;
        }
        other => return Err(format!("unknown native method {other}").into()),
    }
    Ok(())
}

/// The alignment methods: deduplicate to haplotypes, or start an empty
/// species workspace naming the alignment's sequences.
fn alignment_method(
    request: &DelimitationRequest,
    result: &mut DelimitationResult,
    input_bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let index = basecraft_io::fasta::FastaIndex::build(input_bytes)?;
    let rows: Result<Vec<_>, Box<dyn std::error::Error>> = index
        .records
        .iter()
        .map(|record| {
            Ok((
                record.name.clone(),
                String::from_utf8(record.read(input_bytes, 0, record.len)?)?,
            ))
        })
        .collect();
    if request.method == "workspace_alignment" {
        result.workspace = Some(empty_workspace(
            request,
            rows?.iter().map(|(name, _)| name.clone()).collect(),
            false,
        )?);
        return Ok(());
    }
    let haplotypes = basecraft_delimitation::haplotypes::deduplicate(&rows?)?;
    let mut fasta = String::new();
    for (name, sequence) in &haplotypes.unique {
        fasta.push('>');
        fasta.push_str(name);
        fasta.push('\n');
        fasta.push_str(sequence);
        fasta.push('\n');
    }
    artifact(request, result, ".unique.fasta", fasta.as_bytes())?;
    artifact(
        request,
        result,
        ".haplotypes.json",
        &serde_json::to_vec_pretty(&haplotypes)?,
    )?;
    result
        .metrics
        .insert("haplotypes".into(), serde_json::to_value(haplotypes)?);
    Ok(())
}

/// Read an ASAP SPART result into a workspace, its partitions named as ASAP's.
fn import_asap(
    request: &DelimitationRequest,
    result: &mut DelimitationResult,
    input_bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    use basecraft_protocol::delimitation::{HypothesisOrigin, Provenance};
    let mut work = basecraft_io::spart::parse_xml(std::str::from_utf8(input_bytes)?)?;
    for (i, h) in work.hypotheses.iter_mut().enumerate() {
        h.id = format!("{}-{i}", request.job_id);
        h.origin = HypothesisOrigin::Inferred;
        h.provenance = Provenance {
            method: "ASAP".into(),
            version: request
                .parameters
                .get("upstream_version")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .into(),
            inputs: [(request.input.clone(), request.input_hash.clone())].into(),
            parameters: request.parameters.clone(),
            seed: Some(request.seed),
            workers: Some(1),
            status: Some(JobStatus::Completed),
            ..Provenance::default()
        };
        for score in &mut h.scores {
            score.method = "ASAP".into();
            score.scale = "ASAP rank score".into();
            score.interpretation =
                "Weighted probability and barcode-gap rank; smaller ranks first".into();
        }
    }
    work.view.visible = work.hypotheses.iter().map(|h| h.id.clone()).collect();
    result.workspace = Some(work);
    Ok(())
}

/// Fit GMYC (single, multiple or exhaustive) on the time tree read from `tree`.
fn gmyc(
    request: &DelimitationRequest,
    result: &mut DelimitationResult,
    tree: &basecraft_docs::TreeDoc,
    rooted: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut time = basecraft_delimitation::time_tree::validate(tree, rooted, true)?;
    let clamp = request
        .parameters
        .get("reference_age_clamp")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    // Tips at zero distance from each other are fitted as one tip and
    // restored afterwards; the published workspace keeps every tip.
    let full = tree;
    let mut merged = BTreeMap::new();
    let tree = match basecraft_delimitation::gmyc::collapse_zero_age(full, &time)? {
        Some((collapsed, groups)) if !clamp => {
            // Keep the record of the user's tree; the collapsed one
            // is checked again for the fit.
            let full_diagnostics = std::mem::take(&mut time.diagnostics);
            time = basecraft_delimitation::time_tree::validate(&collapsed, rooted, true)?;
            time.diagnostics.splice(0..0, full_diagnostics);
            let tips: usize = groups.values().map(std::collections::BTreeSet::len).sum();
            time.diagnostics.push(format!(
                "{tips} tips in {} zero-age tree clades (zero distance within each clade); each clade was fitted as one tip and its members restored; no exact sequence identity is inferred",
                groups.len()
            ));
            result.metrics.insert(
                "zero_distance_groups".into(),
                serde_json::to_value(&groups)?,
            );
            merged = groups;
            collapsed
        }
        _ => full.clone(),
    };
    if clamp {
        let mut changed = 0;
        for &node in &time.events {
            if time.ages[node] < 1e-6 {
                time.ages[node] = 1e-6;
                changed += 1;
            }
        }
        time.root_height = time.ages[tree.root()];
        time.diagnostics.push(format!("Explicit splits compatibility preprocessing: clamped {changed} internal node ages to 1e-6 input time units; original tree retained"));
        result.metrics.insert(
            "reference_age_clamp".into(),
            serde_json::json!({"minimum_input_age":1e-6,"changed_nodes":changed}),
        );
    }
    let cancel = request.cancel_file.as_deref().map(Path::new);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(request.workers)
        .build()?;
    let budget = request
        .parameters
        .get("candidate_budget")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(10_000);
    let progress_state = std::sync::Mutex::new((0usize, Vec::new()));
    let report_progress = |completed: usize, total: usize| {
        let mut state = progress_state.lock().unwrap();
        if completed >= state.0 {
            state.0 = completed;
            let phase = if request.method == "gmyc_single" {
                "GMYC threshold candidates"
            } else {
                "GMYC candidates / budget"
            };
            if let Err(error) = progress(request, phase, completed, total) {
                state.1.push(error.to_string());
            }
        }
    };
    let mut fit = pool.install(|| match request.method.as_str() {
        "gmyc_single" => basecraft_delimitation::gmyc::single_with_progress(
            &tree,
            &time,
            cancel,
            &report_progress,
        ),
        "gmyc_multiple" => basecraft_delimitation::gmyc::multiple_with_progress(
            &tree,
            &time,
            budget,
            cancel,
            &report_progress,
        ),
        _ => basecraft_delimitation::gmyc::exhaustive(&tree, &time, budget, cancel),
    })?;
    match request
        .parameters
        .get("reference_version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("1.0-20")
    {
        "1.0-20" => {}
        "1.1-dc187fd" => basecraft_delimitation::gmyc::use_newer_summary(&mut fit),
        _ => return Err("unsupported GMYC reference version".into()),
    }
    if request
        .parameters
        .get("reference_age_clamp")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        fit.reference
            .push_str("; explicit recent-age compatibility clamp enabled");
        fit.warnings.retain(|w| !w.starts_with("Recent-age clamp"));
    }
    result
        .metrics
        .insert("gmyc".into(), serde_json::to_value(&fit)?);
    result.job.diagnostics.extend(time.diagnostics.clone());
    result
        .job
        .diagnostics
        .extend(progress_state.into_inner().unwrap().1);
    result.job.diagnostics.extend(fit.warnings.clone());
    if fit
        .stopping_condition
        .starts_with("candidate budget reached")
    {
        return Err(
            "multi-threshold search budget reached; no completed hypothesis published".into(),
        );
    }
    let best = &fit.profile[fit.best];
    result.job.diagnostics.extend(best.diagnostics.clone());
    // Publish every distinct partition in the support set (best first),
    // so the alternatives GMYC cannot rule out are visible.
    let mut ranked = fit.support_candidates.clone();
    ranked.retain(|&i| i != fit.best);
    ranked.sort_by(|&a, &b| {
        fit.profile[b]
            .log_likelihood
            .total_cmp(&fit.profile[a].log_likelihood)
            .then(a.cmp(&b))
    });
    ranked.insert(0, fit.best);
    let mut seen = std::collections::BTreeSet::new();
    for index in ranked {
        if seen.len() == 10 {
            break;
        }
        let candidate = &fit.profile[index];
        let groups: Vec<std::collections::BTreeSet<String>> =
            basecraft_delimitation::gmyc::groups(&tree, &time, candidate)
                .into_iter()
                .map(|group| {
                    group
                        .into_iter()
                        .flat_map(|tip| merged.get(&tip).cloned().unwrap_or_else(|| [tip].into()))
                        .collect()
                })
                .collect();
        if !seen.insert(
            groups
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
        ) {
            continue;
        }
        let rank = seen.len();
        let name = format!(
            "GMYC species hypothesis {rank} ({} species, lnL {:.2})",
            groups.len(),
            candidate.log_likelihood
        );
        let mut work = workspace(request, full, groups, candidate.log_likelihood, name);
        let hypothesis = &mut work.hypotheses[0];
        hypothesis
            .attributes
            .insert("gmyc_rank".into(), rank.to_string());
        hypothesis.attributes.insert(
            "gmyc_thresholds".into(),
            candidate
                .thresholds
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        match &mut result.workspace {
            None => result.workspace = Some(work),
            Some(published) => {
                let mut hypothesis = work.hypotheses.remove(0);
                hypothesis.id = format!("{}-{rank}", request.job_id);
                published.view.visible.push(hypothesis.id.clone());
                published.hypotheses.push(hypothesis);
            }
        }
    }
    if let Some(work) = &mut result.workspace {
        for hypothesis in &mut work.hypotheses {
            hypothesis
                .provenance
                .parameters
                .entry("reference_version".into())
                .or_insert_with(|| "1.0-20".into());
            hypothesis
                .provenance
                .parameters
                .entry("reference_age_clamp".into())
                .or_insert_with(|| false.into());
            hypothesis
                .provenance
                .parameters
                .insert("root_height".into(), time.root_height.into());
            hypothesis.provenance.parameters.insert(
                "sampling_policy".into(),
                "each retained tree tip once; multiplicity restored after fitting".into(),
            );
        }
    }
    Ok(())
}

/// Native ASAP on Basecraft distances from the immutable aligned FASTA.
fn run_asap(
    request: &DelimitationRequest,
    result: &mut DelimitationResult,
) -> Result<(), Box<dyn std::error::Error>> {
    use basecraft_asap as asap;
    use basecraft_protocol::delimitation::{
        Group, Hypothesis, HypothesisOrigin, Provenance, Score,
    };
    let parameter = |name: &str| request.parameters.get(name);
    let model = match parameter("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("p")
    {
        "p" => basecraft_distance::Model::P,
        "jc69" => basecraft_distance::Model::Jc69,
        "k2p" => basecraft_distance::Model::K2p,
        _ => return Err("ASAP distance model must be p, jc69 or k2p".into()),
    };
    let min_overlap = parameter("min_overlap")
        .map_or(Some(1), serde_json::Value::as_u64)
        .filter(|&n| n >= 1)
        .ok_or("ASAP minimum shared sites must be a positive integer")?;
    let max_simulations = parameter("replicates")
        .map_or(Some(100_000), serde_json::Value::as_u64)
        .filter(|n| (100..=10_000_000).contains(n))
        .ok_or("ASAP simulations per node must be between 100 and 10000000")?;
    let reported = parameter("partitions")
        .map_or(Some(10), serde_json::Value::as_u64)
        .filter(|n| (1..=1000).contains(n))
        .ok_or("ASAP reported partitions must be between 1 and 1000")?;
    let seed = u32::try_from(request.seed)
        .ok()
        .filter(|s| (1..=2_147_483_647).contains(s))
        .ok_or("ASAP seed must be between 1 and 2147483647")?;
    // Parallel matches the reference's seed-to-seed spread (crate README);
    // "reference" reproduces the reference program exactly, on one thread.
    let stream = match parameter("stream")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("parallel")
    {
        "parallel" => asap::Stream::Parallel,
        "reference" => asap::Stream::Reference,
        _ => return Err("ASAP stream must be parallel or reference".into()),
    };
    let alignment = basecraft_distance::Alignment::read(Path::new(&request.input))
        .map_err(|e| format!("ASAP requires an aligned DNA FASTA input: {e}"))?;
    if alignment.names.len() < 2 {
        return Err("ASAP requires at least two sequences".into());
    }
    let columns = alignment.sequences[0].len();
    let started = Instant::now();
    let matrix = basecraft_distance::DistanceMatrix::calculate(
        &alignment,
        model,
        min_overlap,
        request.workers,
    )
    .map_err(|e| e.to_string())?;
    let distance_seconds = started.elapsed().as_secs_f64();
    let mut matrix = matrix;
    // JC69 of identical sequences is -0.0; ASAP compares it equal to 0.
    for d in &mut matrix.distances {
        *d += 0.0;
    }
    let missing = matrix.distances.iter().filter(|d| !d.is_finite()).count();
    if missing > 0 {
        let at = matrix
            .distances
            .iter()
            .position(|d| !d.is_finite())
            .unwrap_or(0);
        let mut i = 1;
        while i * (i + 1) / 2 <= at {
            i += 1;
        }
        let j = at - i * (i - 1) / 2;
        return Err(format!(
            "ASAP needs a distance for every pair; {missing} pairs have fewer than {min_overlap} shared sites or a saturated {model:?} distance (first: {} and {})",
            alignment.names[i], alignment.names[j]
        )
        .into());
    }
    let settings = asap::Settings {
        sequence_length: u32::try_from(columns).map_err(|_| "alignment too long for ASAP")?,
        seed,
        max_simulations: usize::try_from(max_simulations)?,
        reported: usize::try_from(reported)?,
        stream,
        ..asap::Settings::default()
    };
    let distances = asap::Distances {
        n: alignment.names.len(),
        values: &matrix.distances,
    };
    let cancel = request.cancel_file.as_deref().map(Path::new);
    let mut last_report = Instant::now();
    let mut progress_error = None;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(request.workers)
        .build()?;
    let started = Instant::now();
    let analysis = pool.install(|| {
        asap::analyse(&distances, &settings, |done, total| {
            if last_report.elapsed().as_secs_f64() < 0.25 {
                return true;
            }
            last_report = Instant::now();
            if let Err(error) = progress(request, "ASAP partitions", done, total) {
                progress_error.get_or_insert(error.to_string());
            }
            !cancel.is_some_and(Path::exists)
        })
    })?;
    let analysis_seconds = started.elapsed().as_secs_f64();
    artifact(
        request,
        result,
        ".asap.tsv",
        analysis.exact_table().as_bytes(),
    )?;
    let mut work = empty_workspace(request, alignment.names.clone(), false)?;
    // Deduplicated runs fit representatives; link the full retained alignment.
    if let (Some(path), Some(hash)) = (
        parameter("alignment_snapshot").and_then(serde_json::Value::as_str),
        parameter("alignment_input_hash").and_then(serde_json::Value::as_str),
    ) {
        work.exchange_fields = [
            ("basecraft_alignment_snapshot".into(), path.into()),
            ("basecraft_alignment_hash".into(), hash.into()),
        ]
        .into();
    }
    let mut parameters = request.parameters.clone();
    for (key, value) in [
        ("model", serde_json::to_value(model)?),
        ("min_overlap", min_overlap.into()),
        ("replicates", max_simulations.into()),
        ("partitions", reported.into()),
        ("stream", serde_json::to_value(stream)?),
        ("sequence_length", columns.into()),
        ("p_threshold", f64::from(settings.p_threshold).into()),
        ("slope_weight", f64::from(settings.slope_weight).into()),
        ("score_weight", f64::from(settings.score_weight).into()),
        (
            "distance_engine",
            format!("basecraft-distance {}", basecraft_protocol::VERSION).into(),
        ),
        ("distance_gap_policy", matrix.gap_policy.clone().into()),
    ] {
        parameters.insert(key.into(), value);
    }
    let colors = ["#377eb8", "#e41a1c", "#4daf4a", "#984ea3", "#ff7f00"];
    for (rank, partition) in analysis.partitions.iter().enumerate() {
        let Some(subsets) = &partition.subsets_detail else {
            break;
        };
        let groups = subsets
            .iter()
            .enumerate()
            .map(|(j, subset)| Group {
                id: format!("group-{}", j + 1),
                label: (j + 1).to_string(),
                color: colors[j % colors.len()].into(),
                members: subset
                    .members
                    .iter()
                    .map(|&m| alignment.names[m].clone())
                    .collect(),
                scores: (0.0..1.0)
                    .contains(&subset.p_value)
                    .then(|| Score {
                        value: subset.p_value,
                        method: "ASAP".into(),
                        scale: "p-value".into(),
                        interpretation: "Coalescent-simulation p-value of this subset's node; small values support splitting it".into(),
                    })
                    .into_iter()
                    .collect(),
                individual_scores: BTreeMap::new(),
                attributes: BTreeMap::new(),
            })
            .collect();
        let attributes = [
            ("asap_rank", (rank + 1).to_string()),
            ("threshold_distance", partition.threshold.to_string()),
            ("merge_distance", partition.distance.to_string()),
            ("p_value", partition.probability.to_string()),
            ("p_value_rank", partition.probability_rank.to_string()),
            ("barcode_gap_slope", partition.slope.to_string()),
            ("slope_rank", partition.slope_rank.to_string()),
            ("subsets", partition.subsets.to_string()),
        ]
        .map(|(k, v)| (k.to_string(), v))
        .into();
        work.hypotheses.push(Hypothesis {
            id: format!("{}-{rank}", request.job_id),
            name: format!(
                "ASAP species hypothesis {} ({} species, score {})",
                rank + 1,
                partition.subsets,
                partition.score
            ),
            origin: HypothesisOrigin::Inferred,
            parent: None,
            groups,
            unassigned: std::collections::BTreeSet::new(),
            scores: vec![Score {
                value: partition.score,
                method: "ASAP".into(),
                scale: "ASAP rank score".into(),
                interpretation: "Weighted probability and barcode-gap rank; smaller ranks first"
                    .into(),
            }],
            provenance: Provenance {
                method: "ASAP".into(),
                version: format!(
                    "basecraft-asap {} (native port of reference ASAP)",
                    basecraft_protocol::VERSION
                ),
                inputs: [(request.input.clone(), request.input_hash.clone())].into(),
                parameters: parameters.clone(),
                seed: Some(request.seed),
                workers: Some(request.workers),
                status: Some(JobStatus::Completed),
                ..Provenance::default()
            },
            attributes,
        });
    }
    work.view.visible = work.hypotheses.iter().map(|h| h.id.clone()).collect();
    result.metrics.insert(
        "asap".into(),
        serde_json::json!({
            "partitions": analysis.partitions.len(),
            "reported": work.hypotheses.len(),
            "distance_range": analysis.distance_range,
            "settings": analysis.settings,
            "distance_seconds": distance_seconds,
            "analysis_seconds": analysis_seconds,
        }),
    );
    result.job.diagnostics.extend(analysis.diagnostics);
    result.job.diagnostics.extend(progress_error);
    result.workspace = Some(work);
    Ok(())
}

/// Method name shown to the user, shared by every species hypothesis.
fn method_label(method: &str) -> &str {
    match method {
        "gmyc_single" | "gmyc_multiple" | "gmyc_exhaustive" => "GMYC",
        "ptp" => "PTP",
        "mptp" => "mPTP",
        "manual_cut" => "Manual threshold cut",
        other => other,
    }
}

fn workspace(
    request: &DelimitationRequest,
    tree: &basecraft_docs::TreeDoc,
    groups: Vec<std::collections::BTreeSet<String>>,
    likelihood: f64,
    name: String,
) -> basecraft_protocol::delimitation::DelimitationWorkspace {
    use basecraft_protocol::delimitation::{
        DelimitationView, DelimitationWorkspace, Group, Hypothesis, HypothesisOrigin, Provenance,
        Score, Specimen,
    };
    let id = request.job_id.clone();
    DelimitationWorkspace { version:1,name:tree.name.clone(),specimens:tree.tips_in_order().iter().map(|&i| Specimen { id:tree.node(i).name.clone(),label:tree.node(i).name.clone(),sources: [(request.input.clone(),tree.node(i).name.clone())].into(),attributes:BTreeMap::new() }).collect(),hypotheses:vec![Hypothesis { id:id.clone(),name,origin:HypothesisOrigin::Inferred,parent:None,groups:groups.into_iter().enumerate().map(|(i,members)| Group { id:format!("group-{}",i+1),label:(i+1).to_string(),color:["#377eb8","#e41a1c","#4daf4a","#984ea3","#ff7f00"][i%5].into(),members,scores:vec![],individual_scores:BTreeMap::new(),attributes:BTreeMap::new() }).collect(),unassigned:std::collections::BTreeSet::new(),scores:vec![Score {value:likelihood,method:request.method.clone(),scale:"logLikelihood".into(),interpretation:"Maximized mixed waiting-time log likelihood; convergence is not evidence of biological support".into()}],provenance:Provenance {method:request.method.clone(),version:basecraft_protocol::VERSION.into(),inputs:[(request.input.clone(),request.input_hash.clone())].into(),analysis_tree:Some(request.input_hash.clone()),parameters:request.parameters.clone(),seed:Some(request.seed),workers:Some(request.workers),status:Some(JobStatus::Completed),..Provenance::default()},attributes:BTreeMap::new() }],view:DelimitationView {display_tree:Some(request.input.clone()),visible:vec![id],..DelimitationView::default()},exchange_fields:BTreeMap::new() }
}

fn progress(
    request: &DelimitationRequest,
    phase: &str,
    completed: usize,
    total: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = &request.progress {
        basecraft_io::write_atomic(Path::new(path), |w| {
            serde_json::to_writer(
                w,
                &basecraft_protocol::producer_records::DelimitationProgress {
                    job_id: request
                        .parameters
                        .get("progress_job_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(&request.job_id)
                        .into(),
                    stage_job_id: request.job_id.clone(),
                    generation: request.generation,
                    phase: phase.into(),
                    completed,
                    total,
                },
            )?;
            Ok(())
        })?;
    }
    Ok(())
}

fn empty_workspace(
    request: &DelimitationRequest,
    names: Vec<String>,
    tree: bool,
) -> Result<basecraft_protocol::delimitation::DelimitationWorkspace, Box<dyn std::error::Error>> {
    use basecraft_protocol::delimitation::{DelimitationView, DelimitationWorkspace, Specimen};
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    if unique.len() != names.len() || names.iter().any(String::is_empty) {
        return Err("Source has empty or duplicate specimen identifiers".into());
    }
    Ok(DelimitationWorkspace {
        version: 1,
        name: "Species hypotheses".into(),
        specimens: names
            .into_iter()
            .map(|id| Specimen {
                label: id.clone(),
                sources: [(
                    request
                        .parameters
                        .get("original_source")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(&request.input)
                        .into(),
                    id.clone(),
                )]
                .into(),
                id,
                attributes: BTreeMap::new(),
            })
            .collect(),
        hypotheses: vec![],
        view: DelimitationView {
            display_tree: tree.then(|| request.input.clone()),
            ..DelimitationView::default()
        },
        exchange_fields: if tree {
            BTreeMap::new()
        } else {
            [
                ("basecraft_alignment_snapshot".into(), request.input.clone()),
                (
                    "basecraft_alignment_hash".into(),
                    request.input_hash.clone(),
                ),
            ]
            .into()
        },
    })
}

fn artifact(
    request: &DelimitationRequest,
    result: &mut DelimitationResult,
    suffix: &str,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(format!("{}{suffix}", request.output));
    basecraft_io::write_atomic(&path, |w| w.write_all(bytes))?;
    result
        .job
        .outputs
        .insert(path.display().to_string(), basecraft_io::hash_bytes(bytes));
    Ok(())
}
