//! A tool run asked for on files (D1): input files by role, results by output
//! role (or `role/member` inside a collection), settings by name, all named
//! from one root folder. The CLI's `run` and the window's runs on project
//! files build their requests here, so both ask for a run the same way.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use basecraft_protocol::operation::{InputReference, OperationRequest};

use crate::{Error, Result};

/// `path` as a request names it: relative to `root` when inside it, else the
/// absolute path, which the run's policy then accepts or refuses. A result
/// file need not exist yet; its folder must.
pub fn request_path(root: &Path, path: &Path) -> Result<String> {
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    let folder = folder
        .canonicalize()
        .map_err(|error| Error::FolderUnavailable {
            folder: folder.to_path_buf(),
            reason: error.to_string(),
        })?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::NoFileName(path.to_path_buf()))?;
    let path = folder.join(name);
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    Ok(path
        .strip_prefix(&root)
        .map_or(path.as_path(), |relative| relative)
        .display()
        .to_string())
}

/// A request to run `operation` on `inputs` (files by input role) with
/// `parameters`, writing `outputs` (files by output role or `role/member`).
pub fn file_request(
    root: &Path,
    operation: &str,
    inputs: &BTreeMap<String, Vec<PathBuf>>,
    parameters: serde_json::Value,
    outputs: &BTreeMap<String, PathBuf>,
) -> Result<OperationRequest> {
    let inputs = inputs
        .iter()
        .map(|(role, files)| {
            let files = files
                .iter()
                .map(|file| {
                    Ok(InputReference::File {
                        path: request_path(root, file)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((role.clone(), files))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let destinations = outputs
        .iter()
        .map(|(member, file)| Ok((member.clone(), request_path(root, file)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(OperationRequest {
        operation: operation.into(),
        inputs,
        parameters,
        destinations,
        pins: None,
        request_key: None,
        target: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_inside_the_root_are_named_relative_to_it_and_others_absolutely() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("loci")).unwrap();
        let inputs = BTreeMap::from([(
            "sources".to_string(),
            vec![
                root.path().join("loci/a.fasta"),
                elsewhere.path().join("b.fasta"),
            ],
        )]);
        let outputs = BTreeMap::from([("report".to_string(), root.path().join("out.json"))]);
        let request = file_request(
            root.path(),
            "org.example/count@1",
            &inputs,
            serde_json::json!({"mode": "normal"}),
            &outputs,
        )
        .unwrap();
        let paths: Vec<_> = request.inputs["sources"]
            .iter()
            .map(|input| match input {
                InputReference::File { path } => path.clone(),
                other @ InputReference::Document { .. } => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(paths[0], "loci/a.fasta");
        assert!(Path::new(&paths[1]).is_absolute());
        assert_eq!(request.destinations["report"], "out.json");
        assert_eq!(request.parameters["mode"], "normal");
        // A result in a folder that does not exist is refused, naming it.
        let missing = BTreeMap::from([("report".to_string(), root.path().join("no/out.json"))]);
        let refused = file_request(root.path(), "x", &inputs, serde_json::json!({}), &missing);
        assert!(refused.unwrap_err().to_string().contains("no"));
    }
}
