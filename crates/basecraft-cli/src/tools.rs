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
                | "ptp"
                | "mptp"
                | "asap" => {}
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
    let mut job = if session.is_tree(doc) {
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
    let result = outcome?;
    job.record(session, doc, &result)?;
    Ok(serde_json::to_value(result)?)
}
