//! basecraft-cli integration tests, built as one test binary.
mod analysis_evidence;
mod annotation_routes;
mod display_tree_command;
#[path = "../../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
mod mcp;
mod new_sequence;
mod newer_records;
mod operation_exports;
mod plugin_command;
mod plugin_registry;
mod preferences;
mod project;
mod run_command;
mod sanger_commands;
#[path = "../../../basecraft-ops/tests/support/sanger_fixture.rs"]
mod sanger_fixture;
mod saved_records;
mod tree_commands;
mod warned_saves;
