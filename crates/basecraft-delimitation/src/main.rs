//! Standalone native preparation engine; plugins use this versioned request.
use basecraft_protocol::delimitation::{
    DelimitationRequest, DelimitationResult, JobRecord, JobStatus,
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
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(path) = args.hash {
        println!("{}", basecraft_io::hash_file(&path)?);
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
        let sidecar = PathBuf::from(format!("{}.basecraft.json", spart.display()));
        basecraft_io::write_atomic(&spart, |w| {
            w.write_all(basecraft_io::spart::write_text(workspace).0.as_bytes())
        })?;
        let spart_hash = basecraft_io::hash_file(&spart)?;
        basecraft_io::write_atomic(&sidecar, |w| {
            serde_json::to_writer_pretty(
                w,
                &serde_json::json!({"version":1,"exchange_hash":spart_hash,"workspace":workspace}),
            )?;
            Ok(())
        })?;
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
    if let (Ok(left), Ok(right)) = (first.canonicalize(), second.canonicalize()) {
        if left == right {
            return true;
        }
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
        let index = basecraft_io::fasta::FastaIndex::build(&input_bytes)?;
        let rows: Result<Vec<_>, Box<dyn std::error::Error>> = index
            .records
            .iter()
            .map(|record| {
                Ok((
                    record.name.clone(),
                    String::from_utf8(record.read(&input_bytes, 0, record.len)?)?,
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
        return Ok(());
    }
    if request.method == "asap_import" {
        use basecraft_protocol::delimitation::{HypothesisOrigin, Provenance};
        let mut work = basecraft_io::spart::parse_xml(std::str::from_utf8(&input_bytes)?)?;
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
        return Ok(());
    }
    let trees = basecraft_io::newick::parse_trees(std::str::from_utf8(&input_bytes)?)?;
    let parsed = trees
        .get(request.tree_index)
        .ok_or("selected tree index outside file")?;
    let tree = basecraft_docs::TreeDoc::from_parsed(&parsed.name, parsed);
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
                    &tree,
                    request.rooted,
                )?)?,
            );
        }
        "ptp" | "mptp" => {
            basecraft_delimitation::time_tree::substitution_tips(&tree, request.rooted)?;
            let groups = serde_json::from_value(
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
            result.workspace = Some(workspace(request, &tree, groups, likelihood));
            if let Some(diagnostics) = request.parameters.get("diagnostics") {
                result
                    .job
                    .diagnostics
                    .extend(serde_json::from_value::<Vec<String>>(diagnostics.clone())?);
            }
        }
        "validate_time_tree" | "manual_cut" => {
            let time =
                basecraft_delimitation::time_tree::validate(&tree, request.rooted, 1e-8, true)?;
            result.job.diagnostics.clone_from(&time.diagnostics);
            if request.method == "manual_cut" {
                let threshold = request
                    .parameters
                    .get("threshold")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or("manual cut requires threshold")?;
                result.metrics.insert(
                    "groups".into(),
                    serde_json::to_value(basecraft_delimitation::time_tree::cut(
                        &tree, &time, threshold,
                    )?)?,
                );
            }
            result
                .metrics
                .insert("time_tree".into(), serde_json::to_value(time)?);
        }
        "relative_time" => {
            let converted =
                basecraft_delimitation::time_tree::convert(&tree, request.rooted, 10_000)?;
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
            let mut time =
                basecraft_delimitation::time_tree::validate(&tree, request.rooted, 1e-8, true)?;
            if request
                .parameters
                .get("reference_age_clamp")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
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
                    "multi-threshold search budget reached; no completed hypothesis published"
                        .into(),
                );
            }
            let best = &fit.profile[fit.best];
            let groups = basecraft_delimitation::gmyc::groups(&tree, &time, best);
            result.workspace = Some(workspace(request, &tree, groups, best.log_likelihood));
            result.job.diagnostics.extend(best.diagnostics.clone());
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
        }
        other => return Err(format!("unknown native method {other}").into()),
    }
    Ok(())
}

fn workspace(
    request: &DelimitationRequest,
    tree: &basecraft_docs::TreeDoc,
    groups: Vec<std::collections::BTreeSet<String>>,
    likelihood: f64,
) -> basecraft_protocol::delimitation::DelimitationWorkspace {
    use basecraft_protocol::delimitation::{
        DelimitationView, DelimitationWorkspace, Group, Hypothesis, HypothesisOrigin, Provenance,
        Score, Specimen,
    };
    let id = request.job_id.clone();
    DelimitationWorkspace { version:1,name:tree.name.clone(),specimens:tree.tips_in_order().iter().map(|&i| Specimen { id:tree.node(i).name.clone(),label:tree.node(i).name.clone(),sources: [(request.input.clone(),tree.node(i).name.clone())].into(),attributes:BTreeMap::new() }).collect(),hypotheses:vec![Hypothesis { id:id.clone(),name:request.method.clone(),origin:HypothesisOrigin::Inferred,parent:None,groups:groups.into_iter().enumerate().map(|(i,members)| Group { id:format!("group-{}",i+1),label:(i+1).to_string(),color:["#377eb8","#e41a1c","#4daf4a","#984ea3","#ff7f00"][i%5].into(),members,scores:vec![],individual_scores:BTreeMap::new(),attributes:BTreeMap::new() }).collect(),unassigned:std::collections::BTreeSet::new(),scores:vec![Score {value:likelihood,method:request.method.clone(),scale:"logLikelihood".into(),interpretation:"Maximized mixed waiting-time log likelihood; convergence is not evidence of biological support".into()}],provenance:Provenance {method:request.method.clone(),version:env!("CARGO_PKG_VERSION").into(),inputs:[(request.input.clone(),request.input_hash.clone())].into(),analysis_tree:Some(request.input_hash.clone()),parameters:request.parameters.clone(),seed:Some(request.seed),workers:Some(request.workers),status:Some(JobStatus::Completed),..Provenance::default()},attributes:BTreeMap::new() }],view:DelimitationView {display_tree:Some(request.input.clone()),visible:vec![id],selected:std::collections::BTreeSet::new()},exchange_fields:BTreeMap::new() }
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
                &serde_json::json!({"job_id":request.job_id,"generation":request.generation,"phase":phase,"completed":completed,"total":total}),
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
