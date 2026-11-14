//! The run a species analysis asks for: which operation, on which input,
//! with which parameters, written where. The window, the CLI and MCP build
//! the same run from the same request, then submit it as a job.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{DocId, Error, Result, Session};

/// A species analysis, or a display tree inferred for the workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    DisplayUpgma,
    DisplayNj,
    DisplayIqtree,
    GmycSingle,
    GmycMultiple,
    GmycAlignmentSingle,
    GmycAlignmentMultiple,
    Ptp,
    Mptp,
    Asap,
    RelativeTime,
    Haplotypes,
    GroupStatistics,
}

/// The settings a request may pass on to the operation. Its descriptor
/// then keeps only those it declares.
const SETTINGS: [&str; 13] = [
    "mapping",
    "workers",
    "seed",
    "replicates",
    "model",
    "reference_version",
    "reference_age_clamp",
    "unique_haplotypes",
    "deduplicate",
    "iqtree_model",
    "tree_index",
    "min_overlap",
    "candidate_budget",
];

impl Method {
    /// The method by its name, such as `gmyc_single`.
    pub fn parse(name: &str) -> Result<Self> {
        serde_json::from_value(Value::String(name.to_owned()))
            .map_err(|_| Error::Unsupported("Unknown analysis".into()))
    }

    /// The name `parse` reads.
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// The run draws a tree for the workspace and changes no species
    /// assignment.
    pub fn is_display(self) -> bool {
        matches!(
            self,
            Self::DisplayUpgma | Self::DisplayNj | Self::DisplayIqtree
        )
    }

    /// The run starts from the alignment: the host infers the tree first.
    fn starts_from_alignment(self) -> bool {
        matches!(
            self,
            Self::GmycAlignmentSingle | Self::GmycAlignmentMultiple | Self::Asap
        )
    }

    /// The registered operation that carries the method out.
    pub fn operation(self) -> String {
        match self {
            Self::DisplayIqtree => "basecraft.iqtree/tree@1".into(),
            Self::DisplayUpgma | Self::DisplayNj => "basecraft.distance/tree@1".into(),
            Self::Asap => "basecraft.host/asap_alignment@1".into(),
            Self::GmycAlignmentSingle | Self::GmycAlignmentMultiple => {
                format!("basecraft.host/{}@1", self.name())
            }
            _ => format!("basecraft.delimitation/{}@1", self.name()),
        }
    }
}

/// What the person asked to run.
pub struct Request<'a> {
    pub method: Method,
    /// The tree or alignment named in the request.
    pub input: &'a Path,
    pub rooted: bool,
    /// Settings by name; empty strings are left out.
    pub settings: Map<String, Value>,
    /// The hypothesis group statistics describe.
    pub hypothesis: &'a str,
    /// The tree the workspace shows now, which stands for its display tree.
    pub shown_tree: &'a Path,
}

/// A run ready to submit as a job.
#[derive(Debug)]
pub struct Run {
    pub operation: String,
    /// The file the operation reads: the request's input, or the
    /// workspace's checked alignment or display tree it stands for.
    pub input: PathBuf,
    pub parameters: Value,
    /// Where the result goes: a new file beside the workspace.
    pub output: PathBuf,
}

/// Build the run for `request` on the species workspace `doc`. `declared` is
/// the set of parameters the operation's descriptor declares; only those
/// are passed on.
pub fn run(
    session: &Session,
    doc: DocId,
    request: &Request,
    declared: &BTreeSet<String>,
) -> Result<Run> {
    let method = request.method;
    let workspace = session.delimitation(doc)?;
    let workspace_path = session.root().join(session.doc_key(doc)?);
    let retained = workspace
        .exchange_fields
        .get("basecraft_alignment_snapshot")
        .is_some_and(|path| Path::new(path) == request.input);
    let input = if retained {
        let expected = workspace
            .exchange_fields
            .get("basecraft_alignment_hash")
            .ok_or_else(|| Error::Damaged {
                path: workspace_path.clone(),
                detail: "Retained alignment hash is missing".into(),
            })?;
        let alignment = crate::delimitation::checked_alignment(workspace, &workspace_path)?
            .ok_or_else(|| Error::NotReady("No linked alignment available".into()))?;
        if &basecraft_io::hash_file(&alignment)? != expected {
            return Err(Error::Outdated(
                "The retained alignment changed; select an intact input explicitly".into(),
            ));
        }
        alignment
    } else if !method.is_display()
        && (workspace.view.display_tree.as_deref().map(Path::new) == Some(request.input)
            || request.shown_tree == request.input)
    {
        crate::delimitation::checked_display_tree(workspace)?
            .ok_or_else(|| Error::NotReady("No analysis tree available".into()))?
    } else {
        request.input.to_path_buf()
    };

    let mut params = Map::new();
    params.insert("rooted".into(), request.rooted.into());
    params.insert("workers".into(), 1.into());
    params.insert("seed".into(), 1.into());
    for (key, value) in &request.settings {
        if SETTINGS.contains(&key.as_str()) && !value.as_str().is_some_and(str::is_empty) {
            params.insert(key.clone(), value.clone());
        }
    }
    if !method.starts_from_alignment()
        && !method.is_display()
        && params.get("deduplicate").and_then(Value::as_bool) == Some(true)
    {
        return Err(Error::Unsupported("Exact sequence deduplication requires a verified alignment and representative tree. Tree-only preprocessing is unavailable; use alignment GMYC/ASAP, or prepare and root a representative tree with its mapping before fitting. No tips were removed.".into()));
    }
    if method.starts_from_alignment() {
        let unique = params
            .get("deduplicate")
            .or_else(|| params.get("unique_haplotypes"))
            .and_then(Value::as_bool)
            .unwrap_or(method != Method::Asap);
        params.insert("unique_haplotypes".into(), unique.into());
    }
    if method.is_display() {
        params.insert(
            "tree_role".into(),
            "display only; does not change species assignments".into(),
        );
        let tree_method = if method == Method::DisplayUpgma {
            "upgma"
        } else {
            "njstar"
        };
        params.insert("method".into(), tree_method.into());
    }
    if method == Method::DisplayIqtree {
        let model = params
            .remove("iqtree_model")
            .unwrap_or_else(|| "MFP".into());
        params.insert("model".into(), model);
        params.insert("keep_identical".into(), true.into());
    }
    if method == Method::GroupStatistics {
        let hypothesis = workspace
            .hypotheses
            .iter()
            .find(|h| h.id == request.hypothesis)
            .ok_or_else(|| Error::MissingChoice("Choose a hypothesis".into()))?;
        let groups: BTreeMap<_, _> = hypothesis
            .groups
            .iter()
            .flat_map(|g| g.members.iter().map(move |id| (id.clone(), g.id.clone())))
            .collect();
        params.insert("groups_data".into(), serde_json::to_value(groups)?);
    }
    params.retain(|key, _| declared.contains(key));

    let extension = if method.is_display() { "nwk" } else { "json" };
    let output = workspace_path
        .parent()
        .unwrap_or(session.root())
        .join(format!("analysis-{}.{extension}", ulid::Ulid::generate()));
    Ok(Run {
        operation: method.operation(),
        input,
        parameters: Value::Object(params),
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_reads_back_from_its_name() {
        for method in [
            Method::DisplayUpgma,
            Method::DisplayNj,
            Method::DisplayIqtree,
            Method::GmycSingle,
            Method::GmycMultiple,
            Method::GmycAlignmentSingle,
            Method::GmycAlignmentMultiple,
            Method::Ptp,
            Method::Mptp,
            Method::Asap,
            Method::RelativeTime,
            Method::Haplotypes,
            Method::GroupStatistics,
        ] {
            assert_eq!(Method::parse(&method.name()).unwrap(), method);
        }
        assert!(matches!(
            Method::parse("bpp"),
            Err(Error::Unsupported(text)) if text == "Unknown analysis"
        ));
    }

    #[test]
    fn methods_run_their_registered_operations() {
        assert_eq!(Method::Ptp.operation(), "basecraft.delimitation/ptp@1");
        assert_eq!(Method::DisplayNj.operation(), "basecraft.distance/tree@1");
        assert_eq!(Method::DisplayIqtree.operation(), "basecraft.iqtree/tree@1");
        assert_eq!(Method::Asap.operation(), "basecraft.host/asap_alignment@1");
        assert_eq!(
            Method::GmycAlignmentSingle.operation(),
            "basecraft.host/gmyc_alignment_single@1"
        );
    }
}
