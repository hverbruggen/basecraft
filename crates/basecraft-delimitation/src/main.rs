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
    if output == Path::new(&request.input) {
        return Err("output must differ from immutable input".into());
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
    let run = run(&request, &mut result);
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
    let input_bytes = std::fs::read(&request.input)?;
    let hash = basecraft_io::hash_bytes(&input_bytes);
    if hash != request.input_hash {
        return Err("immutable input hash differs from request".into());
    }
    if request.method == "haplotypes" {
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
        result.metrics.insert(
            "haplotypes".into(),
            serde_json::to_value(basecraft_delimitation::haplotypes::deduplicate(&rows?)?)?,
        );
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
            result
                .metrics
                .insert("conversion".into(), serde_json::to_value(converted)?);
        }
        "gmyc_single" | "gmyc_multiple" | "gmyc_exhaustive" => {
            let time =
                basecraft_delimitation::time_tree::validate(&tree, request.rooted, 1e-8, true)?;
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
            let fit = pool.install(|| match request.method.as_str() {
                "gmyc_single" => basecraft_delimitation::gmyc::single(&tree, &time, cancel),
                "gmyc_multiple" => {
                    basecraft_delimitation::gmyc::multiple(&tree, &time, budget, cancel)
                }
                _ => basecraft_delimitation::gmyc::exhaustive(&tree, &time, budget, cancel),
            })?;
            result
                .metrics
                .insert("gmyc".into(), serde_json::to_value(&fit)?);
            result.job.diagnostics.extend(time.diagnostics.clone());
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
