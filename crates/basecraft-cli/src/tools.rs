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
    if !session.is_alignment(doc) {
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
    job.params = params;
    let result = job.run()?;
    job.record(session, doc, &result)?;
    Ok(serde_json::to_value(result)?)
}
