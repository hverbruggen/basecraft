//! Thin adapters for the shared saved-hypothesis workflow.
use std::path::PathBuf;

use basecraft_ops::delimitation::{self, CombineSelection};
use clap::Subcommand;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Subcommand)]
pub enum DelimitationCommand {
    /// Inspect an unverified alignment hint; does not read or validate the hint.
    LinkedAlignment {
        path: PathBuf,
        /// Explicit root of the read-only session; no membership is inferred.
        #[arg(long)]
        project: PathBuf,
    },
    /// Inspect hypotheses in recommendation order without writing records.
    Candidates { paths: Vec<PathBuf> },
    /// Suggest a free combined SPART filename beside the last input.
    Name { paths: Vec<PathBuf> },
    /// Combine selected saved hypotheses into a new SPART or SPART XML file.
    Combine {
        #[arg(required = true, num_args = 2..)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        /// JSON object mapping input paths to hypothesis IDs. Omit to keep all.
        #[arg(long)]
        selection: Option<String>,
        /// Root of this recoverable CLI session; no project membership is inferred.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Attach the tree in TREE (one tree, Newick or NEXUS) as the display tree
    /// of the species workspace WORKSPACE and save it. The tree file is kept as
    /// the workspace's evidence.
    DisplayTree {
        workspace: PathBuf,
        #[arg(long)]
        tree: PathBuf,
    },
    /// Write the comparison figure of WORKSPACE on its display tree as SVG in
    /// FOLDER, in publication style: one with every hypothesis
    /// (NAME.svg) and one per hypothesis (NAME.ID.svg).
    Figures {
        workspace: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
}

/// The batch figures of species workspace `doc` in `folder`
/// (`basecraft_viewmodel::delimitation::batch_figures`), each recorded as an
/// export.
pub fn figures(
    session: &mut basecraft_ops::Session,
    doc: basecraft_ops::DocId,
    folder: &std::path::Path,
) -> Result<Vec<PathBuf>> {
    let mut tree = delimitation::attached_display_tree(session, doc)?.tree;
    let work = session.delimitation(doc)?.clone();
    basecraft_ops::session::apply_drawing_edits(&mut tree, &work.view.tree_edits);
    let stem = std::path::Path::new(session.doc_key(doc)?)
        .file_stem()
        .map_or_else(|| "species".into(), |s| s.to_string_lossy().into_owned());
    let theme = basecraft_viewmodel::Theme::publication();
    std::fs::create_dir_all(folder)?;
    let mut written = Vec::new();
    for (name, svg) in
        basecraft_viewmodel::delimitation::batch_figures(&work, &tree, &theme, FONT, &stem)
    {
        let path = folder.join(name);
        session.export(doc, &path, "svg", |w| w.write_all(svg.as_bytes()))?;
        written.push(path);
    }
    Ok(written)
}

/// The figures' font: a generic family every SVG viewer has.
const FONT: &str = "sans-serif";

fn absolute(path: PathBuf) -> std::io::Result<PathBuf> {
    std::path::absolute(path)
}

pub fn run(
    command: DelimitationCommand,
    project: Option<&basecraft_ops::project::Project>,
    save_options: crate::save::Options,
) -> Result<()> {
    let value = match command {
        DelimitationCommand::LinkedAlignment { path, project } => {
            let project = basecraft_ops::project::Project::new(absolute(project)?)?;
            let mut session =
                basecraft_ops::Session::unlogged(project.root()).in_project(Some(project));
            let doc = session.open(&absolute(path)?)?;
            serde_json::json!({"path": delimitation::linked_alignment(&session, doc)?})
        }
        DelimitationCommand::Candidates { paths } => {
            // No --project here: the files are read as loose files.
            serde_json::to_value(delimitation::combine_candidates(None, &paths)?)?
        }
        DelimitationCommand::Name { paths } => {
            serde_json::json!({"path": delimitation::combined_path(&paths)})
        }
        DelimitationCommand::Combine {
            paths,
            output,
            selection,
            project,
        } => {
            let output = absolute(output)?;
            let keep: Option<CombineSelection> =
                selection.map(|s| serde_json::from_str(&s)).transpose()?;
            // Existing inputs and selection aliases are resolved together by
            // the shared workflow. Output/project handling stays unchanged.
            let mut session = crate::project_session(&absolute(project)?)?;
            delimitation::combine_to(&mut session, &paths, &output, keep.as_ref())?;
            serde_json::json!({"output": output})
        }
        DelimitationCommand::DisplayTree { workspace, tree } => {
            let workspace = workspace.canonicalize()?;
            let root = workspace.parent().ok_or("the workspace has no folder")?;
            let mut session = crate::script_session(root.to_path_buf(), project)?;
            let doc = session.open(&workspace)?;
            delimitation::attach_display_tree(&mut session, doc, &absolute(tree)?)?;
            save_options.save(&mut session, doc, None, false)?;
            let shown = &session.delimitation(doc)?.view.display_tree;
            serde_json::json!({"workspace": workspace, "display_tree": shown})
        }
        DelimitationCommand::Figures { workspace, output } => {
            let workspace = workspace.canonicalize()?;
            let root = workspace.parent().ok_or("the workspace has no folder")?;
            let mut session = crate::script_session(root.to_path_buf(), project)?;
            let doc = session.open(&workspace)?;
            serde_json::json!({"figures": figures(&mut session, doc, &absolute(output)?)?})
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
