//! basecraft-plugin-host integration tests, built as one test binary.
mod annotation_storage;
mod blast_submission;
mod clade_storage;
mod common;
mod credential_scope;
#[path = "../../../basecraft-ops/tests/support/decided_save.rs"]
mod decided_save;
mod delimitation_plugin;
mod distance_plugin;
mod fasttree_plugin;
mod genetic_code_tables;
mod iqtree_support;
#[path = "../../../basecraft-ops/tests/support/isolated_profile.rs"]
mod isolated_profile;
mod mafft_plugin;
mod operation_contract;
mod orf_plugin;
mod phylogenetics;
mod publication_repair;
mod pyrodigal_plugin;
mod referenced_inputs;
mod registered_jobs;
mod registry;
mod run_cost;
mod run_folder;
mod runtime_install;
mod sanger_caller;
mod saved_copy_inputs;
mod species_workspace_inputs;
mod table2asn_plugin;
mod third_party_services;
mod tree_distance_storage;
mod zenodo_submission;
