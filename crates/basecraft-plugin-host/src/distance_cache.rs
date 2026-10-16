//! Host-owned distance cache input and explicit secondary-output publication.
use std::path::{Path, PathBuf};

use basecraft_ops::{analysis_store::ArtifactRun, input_evidence};
use basecraft_protocol::provenance::ArtifactRevision;
use serde_json::{Map, Value, json};

use crate::{Error, Result};

const DECLARATION: &str = "cache-publication.json";

pub(crate) struct DistanceCache {
    destination: Option<PathBuf>,
    name: String,
}

fn error(message: &str) -> Error {
    Error::Protocol(message.into())
}

fn path_entry(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Reject symlink parents before canonicalizing, including lexical .. routes.
    for ancestor in parent.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        if std::fs::symlink_metadata(ancestor)?
            .file_type()
            .is_symlink()
        {
            return Err(error("distance cache parent is a symlink"));
        }
    }
    Ok(parent.canonicalize()?.join(
        path.file_name()
            .ok_or_else(|| error("cache filename missing"))?,
    ))
}

fn aliases(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let (Ok(a), Ok(b)) = (input_evidence::stamp(a), input_evidence::stamp(b)) else {
        return false;
    };
    a.device.is_some() && a.inode.is_some() && a.device == b.device && a.inode == b.inode
}

impl DistanceCache {
    pub(crate) fn prepare(
        run: &ArtifactRun,
        params: &mut Map<String, Value>,
        tool_output: &Path,
        protected: &[PathBuf],
    ) -> Result<Option<Self>> {
        let Some(requested) = params.get("cache") else {
            return Ok(None);
        };
        let requested = requested
            .as_str()
            .ok_or_else(|| error("distance cache path must be a string"))?;
        let path = path_entry(Path::new(requested))?;
        if path.starts_with(run.directory()) {
            return Err(error(
                "explicit cache conflicts with its allocated analysis run",
            ));
        }
        for reserved in protected {
            let parent = reserved
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let reserved = parent.canonicalize()?.join(
                reserved
                    .file_name()
                    .ok_or_else(|| error("reserved filename missing"))?,
            );
            if aliases(&path, &reserved) {
                return Err(error("distance cache aliases an input or another output"));
            }
        }
        let existing = match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() => true,
            Ok(_) => return Err(error("distance cache must be an ordinary non-symlink file")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        let name = format!(
            "{}.distances.json",
            tool_output
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| error("invalid output filename"))?
        );
        let staged = run.staging(&name)?;
        if existing {
            let before = input_evidence::stamp(&path)?;
            let input_name = "cache-input.json";
            let pending = run.staging(input_name)?;
            let mut target = std::fs::File::create_new(&pending)?;
            let mut hashing = basecraft_ops::files::HashingWriter::new(&mut target);
            let size = std::io::copy(&mut std::fs::File::open(&path)?, &mut hashing)?;
            let hash = hashing.hash();
            if before != input_evidence::stamp(&path)?
                || size != before.size
                || hash != basecraft_io::hash_file(&path).map_err(basecraft_ops::Error::from)?
            {
                return Err(error("distance cache changed during submission"));
            }
            target.sync_all()?;
            run.publish(
                input_name,
                basecraft_ops::analysis_store::ArtifactDestination::Metadata,
            )?;
            std::fs::copy(run.artifact(input_name)?, &staged)?;
            let mut input = input_evidence::read_input(run.directory())?;
            let artifact = ArtifactRevision {
                id: format!("{}:cache-input", run.id()),
                logical_name: "distance-cache-input".into(),
                hash: hash.clone(),
                size,
                retained: Some(input_name.into()),
                reference: None,
            };
            input
                .extra
                .insert("retained_inputs".into(), json!([artifact]));
            input_evidence::write_json(&run.artifact("input.json")?, &input)?;
            input_evidence::write_json(
                &run.artifact("cache-input-record.json")?,
                &json!({
                    "schema_version": 1, "source": path, "stamp": before, "hash": hash,
                    "size": size, "retained": input_name, "transformation": "identity-v1",
                }),
            )?;
        }
        input_evidence::write_json(
            &run.artifact(DECLARATION)?,
            &json!({
                "schema_version": 1, "status": "prepared", "cache": name,
                "requested": path, "existing_input": existing,
                "destination": if existing { None } else { Some(&path) },
            }),
        )?;
        params.insert("cache".into(), json!(staged));
        Ok(Some(Self {
            destination: (!existing).then_some(path),
            name,
        }))
    }

    pub(crate) fn finish(&self, run: &ArtifactRun) -> Result<Vec<String>> {
        let declaration = run.artifact(DECLARATION)?;
        let mut record: Value = serde_json::from_slice(&std::fs::read(&declaration)?)?;
        record["destination"] = json!(self.destination);
        record["cache"] = json!(self.name);
        record["hash"] = json!(
            basecraft_io::hash_file(&run.artifact(&self.name)?)
                .map_err(basecraft_ops::Error::from)?
        );
        record["status"] = json!("retained");
        input_evidence::write_json(&declaration, &record)?;
        let mut outputs = vec![declaration.display().to_string()];
        if let Some(destination) = &self.destination {
            // Recheck parent routing; persist_noclobber handles a late collision.
            let result = (|| -> Result<()> {
                if path_entry(destination)? != *destination {
                    return Err(error("secondary cache destination moved"));
                }
                run.publish_secondary(&self.name, destination)?;
                Ok(())
            })();
            if let Err(failure) = result {
                record["status"] = json!("failed");
                record["error"] = json!(failure.to_string());
                input_evidence::write_json(&declaration, &record)?;
                return Err(failure);
            }
            record["status"] = json!("published");
            input_evidence::write_json(&declaration, &record)?;
            outputs.push(destination.display().to_string());
        } else if run.artifact("cache-input-record.json")?.is_file() {
            outputs.push(
                run.artifact("cache-input-record.json")?
                    .display()
                    .to_string(),
            );
        }
        Ok(outputs)
    }

    pub(crate) fn is_secondary(&self, path: &Path) -> bool {
        self.destination.as_deref() == Some(path)
    }
}

/// The public result/log includes the intentional external export. Portable
/// publication uses its retained evidence plus the checked destination record,
/// so exports outside the primary folder never introduce unsafe relative paths.
pub(crate) fn publication_outputs(run: &ArtifactRun, outputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let declaration = run.artifact(DECLARATION)?;
    if !declaration.is_file() {
        return Ok(outputs.to_vec());
    }
    let record: Value = serde_json::from_slice(&std::fs::read(declaration)?)?;
    let destination = record["destination"].as_str().map(PathBuf::from);
    Ok(outputs
        .iter()
        .filter(|p| record["status"] != "published" || destination.as_ref() != Some(p))
        .cloned()
        .collect())
}
