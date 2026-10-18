//! Shared file-tool entry point for the CLI and MCP client.
use basecraft_ops::{DocId, Session};
use basecraft_plugin_host::{FileJob, file_tool::AlignmentInput};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub fn run(
    session: &mut Session,
    doc: DocId,
    name: &str,
    output: PathBuf,
    mut params: Map<String, Value>,
) -> Result<Value, Box<dyn std::error::Error>> {
    // Tools run inside their run folder, so a relative output would resolve there.
    let output = std::path::absolute(&output)?;
    if name != "delimitation" && !session.is_alignment(doc) {
        return Err("This tool requires an alignment".into());
    }
    let (operation, input) = match name {
        "mafft" => ("align", AlignmentInput::Ungapped),
        "trimal" => {
            params.entry("method").or_insert(json!("automated1"));
            ("trim", AlignmentInput::Aligned)
        }
        "iqtree" => {
            params.entry("model").or_insert(json!("MFP"));
            params.entry("threads").or_insert(json!(4));
            params.entry("seed").or_insert(json!(1));
            ("tree", AlignmentInput::Aligned)
        }
        "delimitation" => {
            let method = params
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("gmyc_single")
                .to_string();
            match method.as_str() {
                "workspace_tree"
                | "workspace_alignment"
                | "haplotypes"
                | "validate_time_tree"
                | "relative_time"
                | "manual_cut"
                | "gmyc_single"
                | "gmyc_multiple"
                | "gmyc_exhaustive"
                | "gmyc_alignment_single"
                | "gmyc_alignment_multiple"
                | "ptp"
                | "mptp"
                | "asap"
                | "group_statistics" => {}
                _ => return Err("Unknown delimitation operation".into()),
            }
            let source = session.root().join(session.doc_key(doc));
            if output.exists() && output.canonicalize()? == source.canonicalize()? {
                return Err("Output must differ from scientific input".into());
            }
            return run_delimitation(session, doc, output, params, &method);
        }
        "fasttree" | "distance" => ("tree", AlignmentInput::Aligned),
        _ => return Err("Unknown bundled alignment tool".into()),
    };
    let source = session.root().join(session.doc_key(doc));
    if output.exists() && output.canonicalize()? == source.canonicalize()? {
        return Err("Output must differ from the source alignment".into());
    }
    let plugins = std::env::var_os("BASECRAFT_PLUGINS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
        PathBuf::from,
    );
    let mut job = FileJob::from_alignment(
        session,
        doc,
        (&plugins.join(name), operation),
        input,
        output,
    )?;
    job.params.extend(params);
    let started = std::time::Instant::now();
    let outcome = job.run();
    job.record_terminal(session, doc, &outcome, started.elapsed().as_secs_f64())?;
    let result = outcome?;
    job.record(session, doc, &result)?;
    Ok(serde_json::to_value(result)?)
}

fn run_delimitation(
    session: &mut Session,
    doc: DocId,
    output: PathBuf,
    mut params: Map<String, Value>,
    method: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let plugins = std::env::var_os("BASECRAFT_PLUGINS").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
        PathBuf::from,
    );
    if matches!(
        method,
        "gmyc_alignment_single" | "gmyc_alignment_multiple" | "asap"
    ) {
        let preparation_output = PathBuf::from(format!("{}.preparation.json", output.display()));
        let mut preparation = if session.is_delimitation(doc) {
            let input = params
                .remove("analysis_input")
                .and_then(|value| value.as_str().map(PathBuf::from))
                .ok_or("Workspace analysis requires analysis_input")?;
            FileJob::from_file(
                session,
                doc,
                &input,
                (&plugins.join("delimitation"), "haplotypes"),
                preparation_output,
            )?
        } else if session.is_alignment(doc) {
            FileJob::from_alignment(
                session,
                doc,
                (&plugins.join("delimitation"), "haplotypes"),
                AlignmentInput::Aligned,
                preparation_output,
            )?
        } else {
            return Err(
                "This workflow requires an aligned DNA document or workspace with analysis_input"
                    .into(),
            );
        };
        params.insert("generation".into(), session.generation(doc).into());
        let unique_haplotypes = params
            .get("unique_haplotypes")
            .and_then(Value::as_bool)
            .unwrap_or(method != "asap");
        preparation.params.extend(params);
        let job = basecraft_plugin_host::delimitation_pipeline::AlignmentJob {
            preparation,
            unique_haplotypes,
            fit_method: method.replace("_alignment", ""),
            output,
        };
        let outcome = job.run();
        let output = outcome.output.clone();
        let mut result = outcome.record(session, doc)?;
        if session.is_delimitation(doc) {
            basecraft_ops::delimitation::add_analysis_result(session, doc, &output)?;
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
        return Ok(serde_json::to_value(result)?);
    }
    let mut job = if session.is_delimitation(doc) {
        let input = params
            .remove("analysis_input")
            .and_then(|v| v.as_str().map(PathBuf::from))
            .ok_or("Workspace analysis requires analysis_input")?;
        FileJob::from_file(
            session,
            doc,
            &input,
            (&plugins.join("delimitation"), method),
            output,
        )?
    } else if session.is_tree(doc) {
        FileJob::from_tree(
            session,
            doc,
            (&plugins.join("delimitation"), method),
            output,
        )?
    } else if session.is_alignment(doc) {
        FileJob::from_alignment(
            session,
            doc,
            (&plugins.join("delimitation"), method),
            AlignmentInput::Aligned,
            output,
        )?
    } else {
        return Err("Delimitation requires an alignment or tree".into());
    };
    params.insert("generation".into(), session.generation(doc).into());
    job.params.extend(params);
    let started = std::time::Instant::now();
    let outcome = job.run();
    job.record_terminal(session, doc, &outcome, started.elapsed().as_secs_f64())?;
    let mut result = outcome?;
    job.record(session, doc, &result)?;
    if session.is_delimitation(doc) {
        basecraft_ops::delimitation::add_analysis_result(session, doc, &job.output)?;
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
    Ok(serde_json::to_value(result)?)
}
