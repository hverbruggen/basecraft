//! basecraft-ops integration tests, built as one test binary.
//! Suites that need the test-counters or test-heap features are separate binaries in tests/.
mod additional_record_formats;
mod alignment_save;
mod alphabet_answer;
mod alphabet_testdata;
mod analysis_store;
mod compact;
#[path = "../support/decided_save.rs"]
mod decided_save;
mod delimitation;
mod document_facts;
mod document_records;
mod fasta_classification;
mod guarded_evidence_publication;
#[path = "../support/indexed_kind.rs"]
mod indexed_kind;
mod integrity_binding_formats;
#[path = "../support/isolated_profile.rs"]
mod isolated_profile;
mod kind_conformance;
mod label_placement;
mod link_publication;
mod link_record_formats;
mod link_repair_record_formats;
mod load_owned_source;
mod metadata;
mod metadata_consumers;
mod multi_record;
mod new_sequence;
mod newer_records;
mod open_classified;
mod project;
mod project_records;
mod provenance;
mod provenance_boundaries;
mod record_names;
mod recovery_decisions;
#[cfg(windows)]
#[path = "../support/recovery_windows.rs"]
mod recovery_windows;
mod renames;
mod replay;
mod replay_retained;
mod report_record_formats;
mod review_completed_batch;
mod review_link_recovery;
mod review_tree_links;
mod rooting_corrections;
mod rooting_review;
#[path = "../support/sanger_fixture.rs"]
mod sanger_fixture;
mod sanger_secondary_evidence;
mod sanger_session;
mod save_history_size;
mod save_index;
mod save_reference_protections;
mod save_safety;
mod shared_delimitation_workflow;
mod species_analysis;
mod store_registry_formats;
mod submission;
#[path = "../support/submission_run.rs"]
mod submission_run;
mod submitted_input_record_formats;
mod support_import;
mod tool_run_record;
mod tree_annotations;
mod tree_presentation;
mod warned_save_decision;
mod writer_record_formats;
