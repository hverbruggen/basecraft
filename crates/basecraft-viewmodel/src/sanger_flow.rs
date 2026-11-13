//! The steps that turn one to three reads into a review: preparing several
//! reads together, calling one read with a chosen signal cutoff, orienting
//! against a reference alignment and placing the consensus in it. The Sanger
//! controller runs each stage; this decides which runs next and keeps the
//! request so a failed one can be retried with the same inputs.

/// A stage the controller runs on a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Prepare,
    Single,
    Orient,
    Place,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Single => "single",
            Self::Orient => "orient",
            Self::Place => "place",
        }
    }
}

/// The default signal cutoff, in percent; a single read with it needs no
/// separate calling stage.
pub const DEFAULT_CUTOFF: i32 = 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub paths: Vec<String>,
    /// The retained reference alignment, or empty.
    pub reference: String,
    pub cutoff: i32,
    stage: Option<Stage>,
    cancelled: bool,
}

/// What the controller does next.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Submit(Stage),
    /// The review is ready; it was made against `reference` (may be empty).
    Finish {
        reference: String,
    },
    Fail,
}

/// How a new request begins: a single read is opened first.
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub open: Option<String>,
    pub then: Step,
}

#[derive(Default)]
pub struct Flow {
    pending: Option<Request>,
    failed: Option<Request>,
    pub error: String,
}

impl Flow {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn can_retry(&self) -> bool {
        self.failed.is_some()
    }

    /// The reads of the failed request, to show beside the error.
    pub fn failed_paths(&self) -> &[String] {
        self.failed.as_ref().map_or(&[], |request| &request.paths)
    }

    /// Begin preparing `paths`; None when a request is already running or
    /// there are no reads.
    pub fn start(&mut self, paths: Vec<String>, reference: String, cutoff: i32) -> Option<Start> {
        if self.pending.is_some() || paths.is_empty() {
            return None;
        }
        let open = (paths.len() == 1).then(|| paths[0].clone());
        let then = if paths.len() > 1 {
            Step::Submit(Stage::Prepare)
        } else if cutoff != DEFAULT_CUTOFF {
            Step::Submit(Stage::Single)
        } else if !reference.is_empty() {
            Step::Submit(Stage::Orient)
        } else {
            Step::Finish {
                reference: reference.clone(),
            }
        };
        self.pending = Some(Request {
            paths,
            reference,
            cutoff,
            stage: None,
            cancelled: false,
        });
        Some(Start { open, then })
    }

    /// The running request's reference, once retained.
    pub fn retain(&mut self, reference: String) {
        if let Some(request) = &mut self.pending {
            request.reference = reference;
        }
    }

    /// Orient the open review against another reference alignment.
    pub fn context(&mut self, paths: Vec<String>, reference: String, cutoff: i32) -> Option<Step> {
        self.start(vec![String::new()], reference, cutoff)?;
        if let Some(request) = &mut self.pending {
            request.paths = paths;
        }
        Some(Step::Submit(Stage::Orient))
    }

    /// The request a stage runs on, marked as running `stage`.
    pub fn submitting(&mut self, stage: Stage) -> Option<&Request> {
        let request = self.pending.as_mut()?;
        request.stage = Some(stage);
        Some(request)
    }

    /// What follows a finished stage; None when it is not the request's.
    pub fn finished(&mut self, stage: &str, success: bool) -> Option<Step> {
        let request = self.pending.as_ref()?;
        if request.stage.map(Stage::name) != Some(stage) {
            return None;
        }
        Some(if !success || request.cancelled {
            Step::Fail
        } else if stage == "orient" {
            Step::Submit(Stage::Place)
        } else if stage == "place" || request.reference.is_empty() {
            Step::Finish {
                reference: request.reference.clone(),
            }
        } else {
            Step::Submit(Stage::Orient)
        })
    }

    pub fn cancel(&mut self) {
        if let Some(request) = &mut self.pending {
            request.cancelled = true;
        }
    }

    /// Keep the request for a retry and say why it stopped.
    pub fn fail(&mut self, error: String) {
        if let Some(request) = self.pending.take() {
            self.failed = Some(request);
            self.error = error;
        }
    }

    pub fn finish(&mut self) {
        self.pending = None;
        self.failed = None;
        self.error.clear();
    }

    /// The failed request's inputs, to start again.
    pub fn retry(&mut self) -> Option<(Vec<String>, String, i32)> {
        let request = self.failed.clone()?;
        Some((request.paths, request.reference, request.cutoff))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("/r{i}.ab1")).collect()
    }

    #[test]
    fn a_pair_prepares_then_orients_and_places_against_a_reference() {
        let mut flow = Flow::default();
        let start = flow.start(paths(2), "/ref.afa".into(), 20).unwrap();
        assert_eq!(
            start,
            Start {
                open: None,
                then: Step::Submit(Stage::Prepare)
            }
        );
        assert!(
            flow.start(paths(2), String::new(), 20).is_none(),
            "one request at a time"
        );
        flow.submitting(Stage::Prepare);
        assert_eq!(
            flow.finished("single", true),
            None,
            "another stage's completion"
        );
        assert_eq!(
            flow.finished("prepare", true),
            Some(Step::Submit(Stage::Orient))
        );
        flow.submitting(Stage::Orient);
        assert_eq!(
            flow.finished("orient", true),
            Some(Step::Submit(Stage::Place))
        );
        flow.submitting(Stage::Place);
        assert_eq!(
            flow.finished("place", true),
            Some(Step::Finish {
                reference: "/ref.afa".into()
            })
        );
        flow.finish();
        assert!(!flow.pending() && !flow.can_retry());
    }

    #[test]
    fn a_single_read_opens_first_and_calls_only_with_another_cutoff() {
        let mut flow = Flow::default();
        let plain = flow.start(paths(1), String::new(), 20).unwrap();
        assert_eq!(plain.open.as_deref(), Some("/r0.ab1"));
        assert_eq!(
            plain.then,
            Step::Finish {
                reference: String::new()
            }
        );
        flow.finish();
        assert_eq!(
            flow.start(paths(1), String::new(), 35).unwrap().then,
            Step::Submit(Stage::Single)
        );
        flow.submitting(Stage::Single);
        assert_eq!(
            flow.finished("single", true),
            Some(Step::Finish {
                reference: String::new()
            })
        );
        flow.finish();
        assert_eq!(
            flow.start(paths(1), "/ref.afa".into(), 20).unwrap().then,
            Step::Submit(Stage::Orient)
        );
    }

    #[test]
    fn failure_and_cancellation_keep_the_request_for_a_retry() {
        let mut flow = Flow::default();
        flow.start(paths(3), "/ref.afa".into(), 25).unwrap();
        flow.submitting(Stage::Prepare);
        flow.cancel();
        assert_eq!(flow.finished("prepare", true), Some(Step::Fail));
        flow.fail("Cancelled".into());
        assert!(!flow.pending() && flow.can_retry());
        assert_eq!(flow.error, "Cancelled");
        assert_eq!(flow.failed_paths(), paths(3));
        assert_eq!(flow.retry(), Some((paths(3), "/ref.afa".into(), 25)));
        flow.start(paths(3), "/ref.afa".into(), 25).unwrap();
        flow.submitting(Stage::Prepare);
        assert_eq!(flow.finished("prepare", false), Some(Step::Fail));
    }

    #[test]
    fn a_new_reference_orients_the_open_review() {
        let mut flow = Flow::default();
        assert_eq!(
            flow.context(paths(2), "/other.afa".into(), 20),
            Some(Step::Submit(Stage::Orient))
        );
        assert_eq!(flow.submitting(Stage::Orient).unwrap().paths, paths(2));
    }
}
