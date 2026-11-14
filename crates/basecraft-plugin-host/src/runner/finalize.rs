//! Publish validated artifacts and retain an outcome for repair.
use super::{
    ArtifactDestination, ArtifactRevision, BTreeMap, CollectionExportIntent, CollectionIntent,
    Error, InvokeResult, OperationOutcome, Path, PathBuf, PreparedOperation, PublishedArtifact,
    Result, export_collection, records, verify_collection,
};

/// Files published into the project: destination, the run's copy if it
/// keeps one, and hash.
type PublishedFiles = Vec<(String, Option<PathBuf>, String)>;
/// Run paths of files moved into the project, and where they went.
type Moved = BTreeMap<PathBuf, PathBuf>;

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
        let (collections, mut exports, moved) =
            self.publish_collections(validated, outcome, repair)?;
        let mut publication_outputs =
            self.publish_artifacts(validated, &collections, &moved, outcome)?;
        self.publish_workspace_export(outcome, &mut exports)?;
        self.record_exports(&exports, &mut publication_outputs)?;
        self.record_outcome(result, publication_outputs, outcome)
    }

    fn publish_collections(
        &self,
        validated: &[(String, PathBuf, String)],
        outcome: &mut OperationOutcome,
        repair: bool,
    ) -> Result<(BTreeMap<String, PathBuf>, PublishedFiles, Moved)> {
        let mut collections = BTreeMap::new();
        let mut moved = Moved::new();
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
            let staging = self.run_path(&self.context.outputs[role]);
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
                // A repair finds members already moved to the project.
                let leaving = self.member_exports(role, &expected)?;
                let present: BTreeMap<_, _> = expected
                    .iter()
                    .filter(|(member, _)| {
                        retained.join(member).exists()
                            || !leaving.iter().any(|(gone, _)| gone == *member)
                    })
                    .map(|(member, hash)| (member.clone(), hash.clone()))
                    .collect();
                verify_collection(&retained, &present)?;
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
            // Members with a destination of their own leave the run (D26).
            let leaving = self.member_exports(role, &expected)?;
            let kept: BTreeMap<_, _> = expected
                .iter()
                .filter(|(member, _)| !leaving.iter().any(|(gone, _)| gone == *member))
                .map(|(member, hash)| (member.clone(), hash.clone()))
                .collect();
            record_collection(outcome, role, &retained, &kept);
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
            for (member, destination) in leaving {
                let hash = &expected[&member];
                self.publish_member(role, &retained.join(&member), hash, &destination, outcome)?;
                let path = self.policy.root.join(&destination);
                moved.insert(retained.join(&member), path);
                exports.push((destination, None, hash.clone()));
            }
            collections.insert(role.clone(), retained);
        }
        Ok((collections, exports, moved))
    }

    /// The members of collection `role` that go to project files of their
    /// own: the requested member destinations, a forest's component trees
    /// after its tree, and bootstrap trees asked for in the working folder.
    fn member_exports(
        &self,
        role: &str,
        expected: &BTreeMap<PathBuf, String>,
    ) -> Result<Vec<(PathBuf, String)>> {
        let mut leaving = Vec::new();
        for (key, destination) in &self.request.destinations {
            let Some((member_role, Some(member))) =
                crate::policy::destination_role(&self.operation, key)
            else {
                continue;
            };
            if member_role != role {
                continue;
            }
            if !expected.contains_key(member) {
                return Err(Error::Protocol(format!(
                    "invalid_arguments: {role} has no member {}",
                    member.display()
                )));
            }
            leaving.push((member.to_path_buf(), destination.clone()));
            // Component trees of a forest travel with their primary tree.
            if member == Path::new("tree.nwk") {
                let stem = Path::new(destination)
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
                for index in 1.. {
                    let component = PathBuf::from(format!("tree.component-{index}.nwk"));
                    if !expected.contains_key(&component) {
                        break;
                    }
                    let target = Path::new(destination)
                        .with_file_name(format!("{stem}.component-{index}.nwk"))
                        .display()
                        .to_string();
                    leaving.push((component, target));
                }
            }
        }
        let binding = &self.operation.descriptor.outputs[role];
        if binding.bootstrap_exports && self.parameters["bootstrap_destination"] == "working" {
            for member in expected.keys() {
                let name = member.to_string_lossy();
                if name.starts_with(&format!("bootstrap-{}", self.run.id()))
                    && name.ends_with(".nex")
                {
                    leaving.push((member.clone(), self.bootstrap_destination(member)));
                }
            }
        }
        Ok(leaving)
    }

    fn publish_artifacts(
        &self,
        validated: &[(String, PathBuf, String)],
        collections: &BTreeMap<String, PathBuf>,
        moved: &Moved,
        outcome: &mut OperationOutcome,
    ) -> Result<Vec<ArtifactRevision>> {
        let mut publication_outputs = Vec::new();
        for (index, (role, path, digest)) in validated.iter().enumerate() {
            let name = role.clone();
            if let Some(retained) = collections.get(role) {
                let relative = path
                    .strip_prefix(self.run_path(&self.context.outputs[role]))
                    .map_err(|_| Error::Protocol("collection member escapes allocation".into()))?;
                let retained = retained.join(relative);
                let label = format!("output-{index}");
                publication_outputs.push(match moved.get(&retained) {
                    // The run keeps only the moved file's path and hash.
                    Some(destination) => self.project_file(&label, destination, digest)?,
                    None => records::retained_file(
                        &self.run,
                        &label,
                        records::relative(&self.policy.root, &retained)?,
                        &retained,
                        digest.clone(),
                    )?,
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
                self.run.publish(&name, ArtifactDestination::Metadata)?;
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
            let store = self
                .run
                .directory()
                .ancestors()
                .nth(4)
                .ok_or_else(|| Error::Protocol("invalid run location".into()))?;
            publication_outputs.push(records::retained_file(
                &self.run,
                &format!("output-{index}"),
                records::relative(store, &retained)?,
                &retained,
                digest.clone(),
            )?);
        }
        Ok(publication_outputs)
    }

    fn publish_workspace_export(
        &self,
        outcome: &mut OperationOutcome,
        exports: &mut PublishedFiles,
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
                Some(self.run.artifact(&name.to_string_lossy())?),
                artifact.digest.clone(),
            ));
        }
        Ok(())
    }

    fn record_exports(
        &self,
        exports: &PublishedFiles,
        publication_outputs: &mut Vec<ArtifactRevision>,
    ) -> Result<()> {
        let mut exported = Vec::new();
        for (destination, retained, hash) in exports {
            let path = self.policy.root.join(destination);
            let label = format!("export-{}", exported.len());
            exported.push(match retained {
                Some(retained) => records::retained_file(
                    &self.run,
                    &label,
                    records::relative(&self.policy.root, &path)?,
                    retained,
                    hash.clone(),
                )?,
                None => self.project_file(&label, &path, hash)?,
            });
            // An exported tree's support semantics and alignment link travel
            // with it, so a complete export keeps them.
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nwk"))
            {
                for companion in [
                    basecraft_ops::files::companion_path(None, &path),
                    basecraft_ops::records::sidecar(&path),
                ] {
                    if companion.is_file() && companion.starts_with(self.run.directory()) {
                        exported.push(records::retained_file(
                            &self.run,
                            &format!("export-{}", exported.len()),
                            records::relative(&self.policy.root, &companion)?,
                            &companion,
                            basecraft_io::hash_file(&companion)?,
                        )?);
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
            let inputs: Vec<_> = publication
                .inputs
                .iter()
                .map(|input| (input.logical_name.clone(), input.hash.clone()))
                .collect();
            let (actor, record) = self.tool_run(&inputs, &[]);
            let base = self
                .project
                .as_ref()
                .map_or(self.policy.root.as_path(), |project| project.root());
            let activity = basecraft_ops::records::file_run_entry(
                record,
                base,
                &publication,
                &self.started,
                actor,
                &self.operation.id,
            )?;
            self.publication_input_record()?
                .publish_operation_activity(&self.run, &activity)?;
        }
        for artifact in &outcome.artifacts {
            self.run.bind(Path::new(&artifact.path))?;
        }
        Ok(())
    }
    /// A published output that lives in the project, not in the run: the
    /// record names it relative to the project and keeps its hash.
    fn project_file(&self, label: &str, path: &Path, hash: &str) -> Result<ArtifactRevision> {
        Ok(ArtifactRevision {
            id: format!("{}:{label}", self.run.id()),
            logical_name: records::relative(&self.policy.root, path)?,
            hash: hash.into(),
            size: std::fs::metadata(path)?.len(),
            retained: None,
            reference: None,
        })
    }

    /// Move one member of a published collection to a new file in the
    /// project (D26), recording it in the outcome. Idempotent for a recorded
    /// move.
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
            records::verify_export(&destination_path, hash, &intent)?;
        } else {
            if destination_path.exists() {
                records::verify_export(&destination_path, hash, &intent)?;
            } else {
                let destination = self.policy.path(destination, true)?;
                records::move_file(retained, &destination, hash, &intent)?;
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
            self.finalise_exported_tree(role, &destination_path)?;
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
