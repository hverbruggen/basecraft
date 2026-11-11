//! Existing public import paths remain aliases of the protocol payloads.
use basecraft_docs::sanger::{ReviewReference, ReviewSettings};
use basecraft_ops::sanger_review::{MaskSettings, ReviewChoice, ReviewOp, SangerReview};
use basecraft_protocol::sanger;

#[test]
fn old_paths_accept_protocol_values_without_conversion() {
    let settings: ReviewSettings = sanger::ReviewSettings::default();
    let _: sanger::ReviewSettings = settings;
    let reference: Option<ReviewReference> = None::<sanger::ReviewReference>;
    let _: Option<sanger::ReviewReference> = reference;
    let choice: ReviewChoice = sanger::ReviewChoice::Edit('N');
    let mask: MaskSettings = sanger::MaskSettings {
        start: 0,
        end: 1,
        forward: String::new(),
        reverse: String::new(),
        minimum_overlap: 8,
        error_percent: 10,
        evidence: String::new(),
    };
    let _: sanger::ReviewChoice = choice;
    let op: ReviewOp = sanger::ReviewOp::Mask { settings: mask };
    let _: sanger::ReviewOp = op;
    let _: fn(&mut SangerReview, sanger::ReviewOp) -> Result<(), String> = SangerReview::apply;
}
