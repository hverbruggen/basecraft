//! Compatibility routes: CLI `tool` and `distance`, MCP `run_tool`.
//!
//! Each legacy name resolves to a registered operation. Registry resolution,
//! the enabled check and descriptor parameter validation/defaults are the ones
//! `operations run` uses. Execution then publishes one named result file through
//! `FileJob` with the accepted registry, the executor the GUI uses, instead of an
//! artifact collection. ADR 0022 records why and what full delegation needs.
use basecraft_ops::{DocId, Session};
use basecraft_plugin_host::{
    FileJob, contract, delimitation_pipeline, file_tool::AlignmentInput, registry::Registry,
};
use basecraft_protocol::{operation::InputView, plugin::InvokeResult};
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Arc;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Legacy tool names accepted by `basecraft tool` and MCP `run_tool`.
pub const NAMES: [&str; 6] = [
    "mafft",
    "trimal",
    "iqtree",
    "fasttree",
    "distance",
    "delimitation",
];

/// Document paths that only this route accepts. They are not scientific
/// parameters, so they bypass descriptor validation and are passed on as before.
const ROUTE_PATHS: [&str; 2] = ["analysis_input", "mapping"];

/// The registered operation behind a legacy tool name. Delimitation takes the
/// operation from `params.method`, which is removed from the parameters.
pub fn operation_for(tool: &str, params: &mut Map<String, Value>) -> Result<String> {
    Ok(match tool {
        "mafft" => "basecraft.mafft/align@1".into(),
        "trimal" => "basecraft.trimal/trim@1".into(),
        "iqtree" => "basecraft.iqtree/tree@1".into(),
        "fasttree" => "basecraft.fasttree/tree@1".into(),
        "distance" => "basecraft.distance/tree@1".into(),
        "delimitation" => {
            let method = params
                .remove("method")
                .and_then(|value| value.as_str().map(str::to_owned))
                .ok_or("Delimitation requires params.method naming a delimitation operation")?;
            match method.as_str() {
                // Alignment workflows are reserved host operations.
                "gmyc_alignment_single" | "gmyc_alignment_multiple" => {
                    format!("basecraft.host/{method}@1")
                }
                "asap" => "basecraft.host/asap_alignment@1".into(),
                _ => format!("basecraft.delimitation/{method}@1"),
            }
        }
        _ => return Err("Unknown bundled tool".into()),
    })
}

pub fn run(
    registry: &Registry,
    session: &mut Session,
    doc: DocId,
    tool: &str,
    output: PathBuf,
    mut params: Map<String, Value>,
) -> Result<Value> {
    let operation = operation_for(tool, &mut params)?;
    let result = run_operation(
        registry,
        session,
        doc,
        &operation,
        output,
        params,
        Map::new(),
    )?;
    Ok(serde_json::to_value(result)?)
}

/// Run a registered operation on one session document, writing `output`.
/// `host` holds trusted CLI settings outside the descriptor (the distance
/// `--cache` path); MCP never supplies it.
pub fn run_operation(
    registry: &Registry,
    session: &mut Session,
    doc: DocId,
    id: &str,
    output: PathBuf,
    mut params: Map<String, Value>,
    host: Map<String, Value>,
) -> Result<InvokeResult> {
    let operation = registry.resolve(id, None)?;
    let descriptor = &operation.descriptor;
    let view = match &descriptor.host_workflow {
        Some(_) => InputView::AlignedFasta,
        None => descriptor
            .legacy_adapter
            .as_ref()
            .and_then(|adapter| adapter.primary_input.as_ref())
            .and_then(|input| descriptor.inputs.get(input))
            .map(|binding| binding.view)
            .filter(|view| {
                matches!(
                    view,
                    InputView::AlignedFasta | InputView::UngappedFasta | InputView::Newick
                )
            })
            .ok_or_else(|| {
                format!("{id} has no single-document route; use `basecraft operations run`")
            })?,
    };
    let mut paths = Map::new();
    for name in ROUTE_PATHS {
        if let Some(value) = params.remove(name) {
            paths.insert(name.into(), value);
        }
    }
    let Value::Object(mut params) =
        contract::normalize_parameters(descriptor, &Value::Object(params))?
    else {
        return Err("normalized parameters are not an object".into());
    };
    // Tools run inside their run folder, so a relative output would resolve there.
    let output = std::path::absolute(output)?;
    let source = session.root().join(session.doc_key(doc));
    if output.exists() && output.canonicalize()? == source.canonicalize()? {
        return Err("Output must differ from the scientific input".into());
    }
    let accepted = Arc::new(registry.clone());
    let workspace = session.is_delimitation(doc);
    let analysis_input = paths
        .remove("analysis_input")
        .map(|value| {
            value
                .as_str()
                .map(PathBuf::from)
                .ok_or("analysis_input must be a path")
        })
        .transpose()?;
    if analysis_input.is_some() && !workspace {
        return Err("analysis_input applies only to species workspaces".into());
    }
    let job = |plugin: &std::path::Path, stage: &str, output: PathBuf| -> Result<FileJob> {
        let stage = (plugin, stage);
        let job = if workspace {
            let input = analysis_input
                .as_deref()
                .ok_or("Workspace analysis requires analysis_input")?;
            FileJob::from_file(session, doc, input, stage, output)?
        } else if view == InputView::Newick {
            if !session.is_tree(doc) {
                return Err("This operation requires a tree".into());
            }
            FileJob::from_tree(session, doc, stage, output)?
        } else {
            if !session.is_alignment(doc) {
                return Err("This operation requires an alignment".into());
            }
            let form = if view == InputView::UngappedFasta {
                AlignmentInput::Ungapped
            } else {
                AlignmentInput::Aligned
            };
            FileJob::from_alignment(session, doc, stage, form, output)?
        };
        Ok(job.with_registry(accepted.clone()))
    };
    let delimitation = descriptor.host_workflow.is_some()
        || operation.package.manifest.id == "basecraft.delimitation";
    if delimitation {
        params.insert("generation".into(), session.generation(doc).into());
    }
    params.extend(paths);
    params.extend(host);
    let mut result = if let Some(workflow) = &descriptor.host_workflow {
        let fit = delimitation_pipeline::workflow_fit(workflow).ok_or("unknown host workflow")?;
        let haplotypes = registry.resolve("basecraft.delimitation/haplotypes@1", None)?;
        let preparation_output = PathBuf::from(format!("{}.preparation.json", output.display()));
        let mut preparation = job(
            &haplotypes.package.manifest.dir,
            "haplotypes",
            preparation_output,
        )?;
        // The descriptor default always supplies unique_haplotypes.
        let unique_haplotypes = params
            .get("unique_haplotypes")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        preparation.params.extend(params);
        let outcome = delimitation_pipeline::AlignmentJob {
            preparation,
            unique_haplotypes,
            fit_method: fit.into(),
            output,
        }
        .run();
        let output = outcome.output.clone();
        let result = outcome.record(session, doc)?;
        if workspace {
            basecraft_ops::delimitation::add_analysis_result(session, doc, &output)?;
        }
        result
    } else {
        let mut job = job(
            &operation.package.manifest.dir,
            &descriptor.implementation,
            output,
        )?;
        job.params.extend(params);
        let started = std::time::Instant::now();
        let outcome = job.run();
        job.record_terminal(session, doc, &outcome, started.elapsed().as_secs_f64())?;
        let result = outcome?;
        job.record(session, doc, &result)?;
        if workspace {
            basecraft_ops::delimitation::add_analysis_result(session, doc, &job.output)?;
        }
        result
    };
    if workspace {
        session.save(doc, None)?;
        result.outputs.insert(
            0,
            session
                .root()
                .join(session.doc_key(doc))
                .display()
                .to_string(),
        );
    }
    Ok(result)
}
