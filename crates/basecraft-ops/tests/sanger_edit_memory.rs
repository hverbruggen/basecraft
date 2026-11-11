#![cfg(feature = "test-heap")]
#[global_allocator]
static ALLOCATOR: basecraft_io::heap::Allocator = basecraft_io::heap::Allocator;
#[path = "support/isolated_profile.rs"]
mod isolated_profile;
#[path = "support/sanger_fixture.rs"]
mod sanger_fixture;

#[test]
fn two_hundred_selections_retain_less_than_one_mib() {
    let Some(root) = isolated_profile::enter("two_hundred_selections_retain_less_than_one_mib")
    else {
        return;
    };
    let path = sanger_fixture::copy_to(&root.join("fixture"), "multi.json");
    let review = basecraft_ops::sanger_review::SangerReview::load(&path).unwrap();
    let mut session = basecraft_ops::Session::new(&root);
    let id = session.create_sanger_review(&review).unwrap();
    let sets: Vec<_> = review
        .call_sets()
        .into_iter()
        .map(|set| set.0.to_owned())
        .collect();
    let baseline = basecraft_io::heap::live_bytes().unwrap();
    for i in 0..200 {
        session
            .apply(
                id,
                basecraft_protocol::Op::SangerSelect {
                    call_set: sets[i % 2].clone(),
                },
            )
            .unwrap();
    }
    let growth = basecraft_io::heap::live_bytes()
        .unwrap()
        .saturating_sub(baseline);
    assert!(
        growth < 1024 * 1024,
        "200 selections retained {growth} bytes"
    );
}
