//! Publish validated artifacts and retain an outcome for repair.
use super::{
    ArtifactDestination, ArtifactRevision, BTreeMap, CollectionExportIntent, CollectionIntent,
    Error, InvokeResult, OperationOutcome, Path, PathBuf, PreparedOperation, PublishedArtifact,
    Result, export_collection, export_member, file_tool, records, verify_collection,
    verify_member_receipt,
};

type PublishedFiles = Vec<(String, PathBuf, String)>;

impl PreparedOperation {
    pub fn finalize(
        &self,
        result: &InvokeResult,
        validated: &[(String, PathBuf, String)],
        outcome: &mut OperationOutcome,
        repair: bool,
    ) -> Result<()> {
        if self.lifecycle.external() && !self.run.artifact("execution.json")?.is_file() {
            return Err(Error::Protocol("execution evidence unavailable; retain RID and receipt; publication-only repair cannot invent missing execution evidence".into()));
        }
        let (collections, mut exports) = self.publish_collections(validated, outcome, repair)?;
        let mut publication_outputs = self.publish_artifacts(validated, &collections, outcome)?;
        self.publish_workspace_export(outcome, &mut exports)?;
        self.record_exports(&exports, &mut publication_outputs)?;
        self.record_outcome(result, publication_outputs, outcome)
    }

    fn publish_collections(
        &self,
        validated: &[(String, PathBuf, String)],
        outcome: &mut OperationOutcome,
        repair: bool,
    ) -> Result<(BTreeMap<String, PathBuf>, PublishedFiles)> {
        let mut collections = BTreeMap::new();
        // Visible copies are publication outputs too, so verifying one checks
        // its bytes; they come first, as the run's primary results.
        let mut exports = Vec::new();
        for (role, binding) in &self.operation.descriptor.outputs {
            if !binding.collection {
                continue;
            }
            let members: Vec<_> = validated
                .iter()
                .filter(|(member, _, _)| member == role)
                .collect();
            if members.is_empty() {
                continue;
            }
            let staging = PathBuf::from(&self.context.outputs[role]);
            let retained = if self.operation.descriptor.host_workflow.is_some() {
                staging.clone()
            } else {
                self.run.artifact(role)?
            };
            let intent = self
                .run
                .artifact(&format!("collection-{role}-intent.json"))?;
            let mut expected = BTreeMap::new();
            for (_, path, hash) in &members {
                let relative = path.strip_prefix(&staging).map_err(|_| {
                    Error::Protocol("collection member lies outside allocation".into())
                })?;
                expected.insert(relative.to_path_buf(), (*hash).clone());
            }
            if retained == staging || retained.exists() {
                if retained != staging && (!repair || !intent.is_file()) {
                    return Err(Error::Protocol(
                        "output_conflict: retained collection already exists".into(),
                    ));
                }
                verify_collection(&retained, &expected)?;
            } else {
                verify_collection(&staging, &expected)?;
                records::write_json(
                    &intent,
                    &serde_json::to_value(CollectionIntent {
                        schema: 1,
                        members: expected.clone(),
                    })?,
                )?;
                for path in expected.keys() {
                    basecraft_platform::files::sync_file(&staging.join(path))?;
                }
                self.run.publish_collection(role)?;
            }
            record_collection(outcome, role, &retained, &expected);
            self.persist(outcome)?;
            if let Some(destination) = self.request.destinations.get(role) {
                let destination_path = self.policy.root.join(destination);
                let intent = self
                    .run
                    .artifact(&format!("collection-{role}-export.json"))?;
                if destination_path.exists() {
                    if !repair || !intent.is_file() {
                        return Err(Error::Protocol(
                            "output_conflict: collection destination exists".into(),
                        ));
                    }
                    verify_collection(&destination_path, &expected)?;
                } else {
                    let destination = self.policy.path(destination, true)?;
                    records::write_json(
                        &intent,
                        &serde_json::to_value(CollectionExportIntent {
                            schema: 1,
                            destination: destination.clone(),
                            members: expected.clone(),
                        })?,
                    )?;
                    export_collection(&retained, &destination, &expected)?;
                }
                record_collection(outcome, role, &destination_path, &expected);
                self.persist(outcome)?;
            }
            for (key, destination) in &self.request.destinations {
                let Some((member_role, Some(member))) =
                    crate::policy::destination_role(&self.operation, key)
                else {
                    continue;
                };
                if member_role != role {
                    continue;
                }
                let hash = expected.get(member).ok_or_else(|| {
                    Error::Protocol(format!(
                        "invalid_arguments: {role} has no member {}",
                        member.display()
                    ))
                })?;
                self.publish_member(role, &retained.join(member), hash, destination, outcome)?;
                exports.push((destination.clone(), retained.join(member), hash.clone()));
                // Component trees of a forest travel with their primary tree.
                if member == Path::new("tree.nwk") {
                    let stem = Path::new(destination)
                        .file_stem()
                        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
                    for index in 1.. {
                        let component = PathBuf::from(format!("tree.component-{index}.nwk"));
                        let Some(hash) = expected.get(&component) else {
                            break;
                        };
                        let target = Path::new(destination)
                            .with_file_name(format!("{stem}.component-{index}.nwk"))
                            .display()
                            .to_string();
                        self.publish_member(
                            role,
                            &retained.join(&component),
                            hash,
                            &target,
                            outcome,
                        )?;
                        exports.push((target, retained.join(&component), hash.clone()));
                    }
                }
            }
            if role == "artifacts"
                && self
                    .operation
                    .descriptor
                    .host
                    .as_ref()
                    .is_some_and(|host| host.bootstrap_exports)
                && self.parameters["bootstrap_destination"] == "working"
            {
                for (member, hash) in &expected {
                    let name = member.to_string_lossy();
                    if name.starts_with(&format!("bootstrap-{}", self.run.id()))
                        && name.ends_with(".nex")
                    {
                        let destination = self.bootstrap_destination(member);
                        self.publish_member(
                            role,
                            &retained.join(member),
                            hash,
                            &destination,
                            outcome,
                        )?;
                        exports.push((destination, retained.join(member), hash.clone()));
                    }
                }
            }
            collections.insert(role.clone(), retained);
        }
        Ok((collections, exports))
    }

    fn publish_artifacts(
        &self,
        validated: &[(String, PathBuf, String)],
        collections: &BTreeMap<String, PathBuf>,
        outcome: &mut OperationOutcome,
    ) -> Result<Vec<ArtifactRevision>> {
        let mut publication_outputs = Vec::new();
        for (index, (role, path, digest)) in validated.iter().enumerate() {
            let name = role.clone();
            if let Some(retained) = collections.get(role) {
                let relative = path
                    .strip_prefix(Path::new(&self.context.outputs[role]))
                    .map_err(|_| Error::Protocol("collection member escapes allocation".into()))?;
                let retained = retained.join(relative);
                publication_outputs.push(ArtifactRevision {
                    id: format!("{}:output-{index}", self.run.id()),
                    logical_name: records::relative(&self.policy.root, &retained)?,
                    hash: digest.clone(),
                    size: std::fs::metadata(&retained)?.len(),
                    retained: Some(records::relative(self.run.directory(), &retained)?),
                    reference: None,
                });
                continue;
            }
            let retained = self.run.artifact(&name)?;
            if outcome
                .artifacts
                .iter()
                .any(|a| a.path == retained.display().to_string() && &a.digest == digest)
            {
                if basecraft_io::hash_file(&retained)? != *digest {
                    return Err(Error::Protocol(
                        "retained artifact changed; repair refused".into(),
                    ));
                }
            } else {
                if basecraft_io::hash_file(path)? != *digest {
                    return Err(Error::Protocol("validated staging artifact changed".into()));
                }
                let staging = self.run.staging(&name)?;
                if &staging != path {
                    std::fs::rename(path, &staging)?;
                }
                file_tool::publish_artifact(&self.run, &name, ArtifactDestination::Metadata)?;
                outcome.artifacts.push(PublishedArtifact {
                    role: role.clone(),
                    path: retained.display().to_string(),
                    digest: digest.clone(),
                });
                self.persist(outcome)?;
            }
            if let Some(destination) = self.request.destinations.get(role) {
                let expected = self.policy.root.join(destination);
                if outcome
                    .artifacts
                    .iter()
                    .any(|a| a.path == expected.display().to_string() && &a.digest == digest)
                {
                    if basecraft_io::hash_file(&expected)? != *digest {
                        return Err(Error::Protocol(
                            "published destination changed; repair refused".into(),
                        ));
                    }
                } else {
                    let destination = self.policy.path(destination, true)?;
                    self.run.publish_secondary(&name, &destination)?;
                    outcome.artifacts.push(PublishedArtifact {
                        role: role.clone(),
                        path: destination.display().to_string(),
                        digest: digest.clone(),
                    });
                    self.persist(outcome)?;
                }
            }
            publication_outputs.push(ArtifactRevision {
                id: format!("{}:output-{index}", self.run.id()),
                logical_name: records::relative(
                    self.run
                        .directory()
                        .ancestors()
                        .nth(4)
                        .ok_or_else(|| Error::Protocol("invalid run location".into()))?,
                    &retained,
                )?,
                hash: digest.clone(),
                size: std::fs::metadata(&retained)?.len(),
                retained: Some(records::relative(self.run.directory(), &retained)?),
                reference: None,
            });
        }
        Ok(publication_outputs)
    }

    fn publish_workspace_export(
        &self,
        outcome: &mut OperationOutcome,
        exports: &mut Vec<(String, PathBuf, String)>,
    ) -> Result<()> {
        if let Some(destination) = self
            .request
            .destinations
            .get(crate::policy::WORKSPACE_DESTINATION)
            .filter(|_| {
                crate::policy::is_workspace_destination(
                    &self.operation,
                    crate::policy::WORKSPACE_DESTINATION,
                )
            })
        {
            let path = self.policy.root.join(destination);
            if let Some(artifact) = outcome
                .artifacts
                .iter()
                .find(|artifact| artifact.path == path.display().to_string())
            {
                if basecraft_io::hash_file(&path)? != artifact.digest {
                    return Err(Error::Protocol(
                        "published workspace changed; repair refused".into(),
                    ));
                }
            } else {
                let (published, roster) = self.publish_workspace(destination)?;
                outcome.warnings.extend(roster);
                outcome.artifacts.push(PublishedArtifact {
                    role: crate::policy::WORKSPACE_DESTINATION.into(),
                    path: published.display().to_string(),
                    digest: basecraft_io::hash_file(&published)?,
                });
                self.persist(outcome)?;
            }
            let artifact = outcome
                .artifacts
                .iter()
                .find(|artifact| artifact.path == path.display().to_string())
                .ok_or_else(|| Error::Protocol("missing workspace publication receipt".into()))?;
            let name = path
                .file_name()
                .ok_or_else(|| Error::Protocol("workspace has no name".into()))?;
            exports.push((
                destination.clone(),
                self.run.artifact(&name.to_string_lossy())?,
                artifact.digest.clone(),
            ));
        }
        Ok(())
    }

    fn record_exports(
        &self,
        exports: &[(String, PathBuf, String)],
        publication_outputs: &mut Vec<ArtifactRevision>,
    ) -> Result<()> {
        let mut exported = Vec::new();
        for (destination, retained, hash) in exports {
            let path = self.policy.root.join(destination);
            exported.push(ArtifactRevision {
                id: format!("{}:export-{}", self.run.id(), exported.len()),
                logical_name: records::relative(&self.policy.root, &path)?,
                hash: hash.clone(),
                size: std::fs::metadata(&path)?.len(),
                retained: Some(records::relative(self.run.directory(), retained)?),
                reference: None,
            });
            // An exported tree's support semantics and alignment link travel
            // with it, so a complete export keeps them.
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nwk"))
            {
                for companion in [
                    basecraft_ops::files::tree_sidecar(&path),
                    basecraft_ops::records::sidecar(&path),
                ] {
                    if companion.is_file() && companion.starts_with(self.run.directory()) {
                        exported.push(ArtifactRevision {
                            id: format!("{}:export-{}", self.run.id(), exported.len()),
                            logical_name: records::relative(&self.policy.root, &companion)?,
                            hash: basecraft_io::hash_file(&companion)?,
                            size: std::fs::metadata(&companion)?.len(),
                            retained: Some(records::relative(self.run.directory(), &companion)?),
                            reference: None,
                        });
                    }
                }
            }
        }
        publication_outputs.splice(0..0, exported);
        Ok(())
    }

    fn record_outcome(
        &self,
        result: &InvokeResult,
        publication_outputs: Vec<ArtifactRevision>,
        outcome: &mut OperationOutcome,
    ) -> Result<()> {
        outcome.data = result.data.as_deref().cloned();
        let publication = basecraft_ops::records::publish_run(
            self.run.directory(),
            self.run.id(),
            format!(
                "{} executed by {}; initiated by {}",
                self.operation.id, self.operation.package.manifest.id, self.initiator
            ),
            self.ledger_inputs.clone(),
            publication_outputs,
        )?;
        if self.document.is_none() && self.sequence.is_none() {
            let (actor, params) = self.tool_activity(outcome)?;
            let activity = basecraft_ops::records::tool_run_entry(
                &publication,
                &self.started,
                actor,
                &self.operation.id,
                &self.operation.package.manifest.id,
                &self.operation.descriptor.implementation,
                params,
            );
            self.publication_input_record()?
                .publish_operation_activity(&self.run, &activity)?;
        }
        for artifact in &outcome.artifacts {
            self.run.bind(Path::new(&artifact.path))?;
        }
        Ok(())
    }
    /// Export one member of a published collection to a new file in the
    /// project, recording it in the outcome. Idempotent for a recorded export.
    fn publish_member(
        &self,
        role: &str,
        retained: &Path,
        hash: &str,
        destination: &str,
        outcome: &mut OperationOutcome,
    ) -> Result<()> {
        let destination_path = self.policy.root.join(destination);
        let path = destination_path.display().to_string();
        let intent = self.run.artifact(&format!(
            "member-{}-export.json",
            basecraft_io::hash_bytes(destination_path.as_os_str().as_encoded_bytes())
                .trim_start_matches("b3:")
        ))?;
        if outcome
            .artifacts
            .iter()
            .any(|a| a.path == path && a.digest == hash)
        {
            verify_member_receipt(&destination_path, hash, &intent)?;
        } else {
            if destination_path.exists() {
                verify_member_receipt(&destination_path, hash, &intent)?;
            } else {
                let destination = self.policy.path(destination, true)?;
                export_member(retained, &destination, hash, &intent)?;
            }
            outcome.artifacts.push(PublishedArtifact {
                role: role.into(),
                path,
                digest: hash.into(),
            });
            self.persist(outcome)?;
        }
        if destination_path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("nwk"))
        {
            self.finalise_exported_tree(&destination_path)?;
        }
        Ok(())
    }
}

fn record_collection(
    outcome: &mut OperationOutcome,
    role: &str,
    root: &Path,
    members: &BTreeMap<PathBuf, String>,
) {
    for (relative, hash) in members {
        let path = root.join(relative).display().to_string();
        if !outcome
            .artifacts
            .iter()
            .any(|a| a.path == path && &a.digest == hash)
        {
            outcome.artifacts.push(PublishedArtifact {
                role: role.into(),
                path,
                digest: hash.clone(),
            });
        }
    }
}
