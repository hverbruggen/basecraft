//! The `basecraft` command-line tool. Its commands are made from the command
//! table of `basecraft_ops::commands` and the plugin-host commands in `host`;
//! `basecraft mcp` serves the same table as MCP tools.

mod host;
mod mcp;
mod save;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use basecraft_ops::commands::{self, Call, Command, Kind, Routes};
use clap::{ArgAction, ArgMatches, Args, FromArgMatches};
use serde_json::{Map, Value, json};

// The options every command takes.
#[derive(Args)]
struct Globals {
    /// The project folder you chose. Files inside it keep their history in the
    /// project; without it every file is a loose file. Basecraft never guesses.
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    #[command(flatten)]
    save_options: save::Options,
}

// `basecraft mcp`: the transport, not a command of the table.
#[derive(Args)]
struct McpArgs {
    /// The project folder: the session's root and access boundary.
    #[arg(value_name = "PROJECT")]
    root: PathBuf,
    #[arg(long)]
    allow_writes: bool,
    /// Client name recorded with each edit and operation run: the
    /// assistant's model ID, or the script's name with `--client-kind script`.
    #[arg(long, alias = "client", default_value = "mcp-client")]
    model_id: String,
    /// Record edits as made by an assistant (`agent`) or a script (`script`).
    #[arg(long, value_enum, default_value_t)]
    client_kind: mcp::ClientKind,
}

/// Every command, from both tables.
pub(crate) fn all_commands() -> Vec<&'static Command> {
    commands::all().chain(host::COMMANDS).collect()
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            match e.downcast_ref::<basecraft_ops::Error>() {
                Some(basecraft_ops::Error::Host(inner)) => eprintln!("basecraft: {inner}"),
                Some(error) => eprintln!("{}", save::error_value(error)),
                None => eprintln!("basecraft: {e}"),
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let table: Vec<_> = all_commands()
        .into_iter()
        .filter(|command| command.routes != Routes::LiveSession)
        .collect();
    let root = cli(&table).get_matches();
    let globals = Globals::from_arg_matches(&root)?;
    let (mut name, mut matches) = root.subcommand().ok_or("give a command")?;
    if name == "mcp" {
        // The server's project is its argument, and its client decides saves.
        let given = [
            "project",
            "acknowledge_incomplete_evidence",
            "save_known_policy",
        ]
        .into_iter()
        .chain(["drop_newer_fields"])
        .find(|id| matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine));
        if let Some(id) = given {
            return Err(format!("mcp does not take --{}", id.replace('_', "-")).into());
        }
        let args = McpArgs::from_arg_matches(matches)?;
        mcp::serve(
            &args.root,
            args.allow_writes,
            args.client_kind,
            &args.model_id,
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    let mut words = name.to_owned();
    while let Some((word, sub)) = matches.subcommand() {
        (name, matches) = (word, sub);
        words = format!("{words} {name}");
    }
    let command = commands::find(&table, &words).ok_or("unknown command")?;
    run_command(command, matches, &globals)
}

/// The clap command line made from the table.
fn cli(table: &[&'static Command]) -> clap::Command {
    let root = clap::Command::new("basecraft")
        .version(basecraft_protocol::VERSION)
        .about("Basecraft command-line tool")
        .subcommand_required(true);
    let mut root = Globals::augment_args(root)
        .subcommand(McpArgs::augment_args(clap::Command::new("mcp").about(
            "Serve a headless project session over MCP stdio (2025-11-25).",
        )));
    let mut groups: Vec<(&str, clap::Command)> = Vec::new();
    for command in table {
        let Some((group, word)) = command.name.split_once(' ') else {
            root = root.subcommand(subcommand(command.name, command));
            continue;
        };
        let at = groups
            .iter()
            .position(|(g, _)| *g == group)
            .unwrap_or_else(|| {
                let help = commands::GROUPS
                    .iter()
                    .chain(host::GROUPS)
                    .find(|(g, _)| *g == group)
                    .map_or("", |(_, help)| help);
                let new = clap::Command::new(group)
                    .about(help)
                    .subcommand_required(true);
                groups.push((group, new));
                groups.len() - 1
            });
        let parent = std::mem::replace(&mut groups[at].1, clap::Command::new(group));
        groups[at].1 = parent.subcommand(subcommand(word, command));
    }
    root.subcommands(groups.into_iter().map(|(_, group)| group))
}

fn subcommand(word: &'static str, command: &'static Command) -> clap::Command {
    let args = command.args.iter().map(|arg| {
        let mut built = clap::Arg::new(arg.name)
            .help(arg.help)
            .required(arg.required);
        built = built.value_name(if arg.kind == Kind::Document {
            "FILE"
        } else {
            leak(arg.name.to_uppercase())
        });
        if !arg.positional {
            built = built.long(leak(arg.name.replace('_', "-")));
        }
        if let Some(short) = arg.short {
            built = built.short(short);
        }
        built = match arg.kind {
            Kind::Flag => built.action(ArgAction::SetTrue),
            Kind::Texts | Kind::Files if arg.positional => {
                built.num_args(1..).action(ArgAction::Append)
            }
            Kind::Texts | Kind::Files => built.action(ArgAction::Append),
            Kind::List => built.value_delimiter(',').action(ArgAction::Append),
            Kind::Number => built.value_parser(clap::value_parser!(u64)),
            Kind::Choice(words) => built.value_parser(words.to_vec()),
            _ => built,
        };
        if let Some(default) = arg.default {
            built = built.default_value(default);
        }
        built
    });
    clap::Command::new(word).about(command.help).args(args)
}

/// Clap keeps names for the life of the process.
fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The arguments clap read, as the JSON the command's function reads.
fn arguments(command: &Command, matches: &ArgMatches) -> basecraft_ops::Result<Map<String, Value>> {
    let mut args = Map::new();
    for arg in command.args {
        let name = arg.name;
        let value = match arg.kind {
            Kind::Flag => Some(Value::Bool(matches.get_flag(name))),
            Kind::Number => matches.get_one::<u64>(name).map(|n| Value::from(*n)),
            Kind::Texts | Kind::List | Kind::Files => matches
                .get_many::<String>(name)
                .map(|values| json!(values.collect::<Vec<_>>())),
            Kind::Json(_) | Kind::Operations => matches
                .get_one::<String>(name)
                .map(|text| commands::json_text(text))
                .transpose()?,
            _ => matches
                .get_one::<String>(name)
                .map(|text| Value::from(text.as_str())),
        };
        if let Some(value) = value {
            args.insert(name.to_owned(), value);
        }
    }
    Ok(args)
}

/// The folder of the first file a command names: the session's root for a
/// loose file.
fn first_folder(command: &Command, args: &Map<String, Value>) -> Option<PathBuf> {
    let first = command.args.iter().find_map(|arg| match arg.kind {
        Kind::Document | Kind::File | Kind::NewFile => args.get(arg.name)?.as_str(),
        Kind::Files => args.get(arg.name)?.get(0)?.as_str(),
        _ => None,
    })?;
    let path = std::path::absolute(first).ok()?;
    let path = path.canonicalize().unwrap_or(path);
    path.parent().map(Path::to_path_buf)
}

/// Run one command from the command line: open what it names, run it, save
/// every document it changed, print its answer.
fn run_command(
    command: &'static Command,
    matches: &ArgMatches,
    globals: &Globals,
) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let project = globals
        .project
        .as_deref()
        .map(basecraft_ops::project::Project::new)
        .transpose()?;
    let args = arguments(command, matches)?;
    let recent = basecraft_viewmodel::projects::recent();
    let folder = first_folder(command, &args);
    let root = match (&project, folder) {
        (Some(project), _) => project.root().to_path_buf(),
        (None, Some(folder)) => {
            name_recent_project(&folder, &recent);
            folder
        }
        (None, None) => std::env::current_dir()?,
    };
    let mut session = basecraft_ops::Session::recoverable(root)?.in_project(project);
    session.set_actor(basecraft_protocol::Actor::Script {
        client: "CLI".into(),
    });
    let answers = globals.save_options.answers();
    let mut call = Call::command_line(&mut session, command, args, &recent, answers);
    let value = (command.run)(&mut call)?;
    call.save_edited()?;
    // Commands keep stdout for their answer; a save that warned still saved.
    if !call.warnings().is_empty() {
        eprintln!("{}", json!({"state": "saved", "warnings": call.warnings()}));
    }
    let failed = call.failed();
    match value {
        Value::Null => {}
        Value::String(text) => println!("{text}"),
        value => println!("{}", serde_json::to_string_pretty(&value)?),
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// A file used without --project that lies in a recent project: say which,
/// without choosing it (D16; with --project the decision asks nothing).
fn name_recent_project(folder: &Path, recent: &[PathBuf]) {
    if let Some(holder) = basecraft_ops::project::Project::recent_holding(folder, None, recent) {
        let root = holder.root().display();
        eprintln!(
            "This file is in the recent project {root}. Without --project it is used as a loose file; add --project {root} to use the project."
        );
    }
}

/// Parse an input-conflict decision by its shared serialized name.
fn save_policy(name: &str) -> Result<basecraft_ops::records::SavePolicy, String> {
    serde_json::from_value(Value::from(name)).map_err(|_| {
        "expected cancel, preserve_copy, remove_analysis or break_evidence".to_string()
    })
}
