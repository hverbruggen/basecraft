//! The steps of a species analysis asked for in a workspace: which tree to
//! attach or infer first, and what runs once that has finished. The
//! delimitation controller carries the steps out.

/// One analysis or display-tree run.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub method: String,
    pub input: String,
    pub rooted: bool,
    pub settings: String,
}

/// A PTP or mPTP fit on the workspace's display tree, rooted first unless
/// `rooting` is "keep".
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub method: String,
    pub settings: String,
    pub rooting: String,
}

/// What to do first.
#[derive(Clone, Debug, PartialEq)]
pub enum First {
    Nothing,
    Run(Run),
    /// Attach `input` as the display tree.
    Attach {
        input: String,
        rooted: bool,
    },
    /// Attach `input` unrooted, then fit on it if that worked.
    AttachThenFit {
        input: String,
        fit: Fit,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Create a manual hypothesis and make it active.
    pub create_manual: bool,
    /// Show the groups list (true) or the tree (false); None leaves it.
    pub show_groups: Option<bool>,
    pub first: First,
    /// Once the display tree job succeeds, fit on that tree.
    pub fit_after_display: Option<Fit>,
    /// Once the analysis succeeds, build this display tree.
    pub display_after: Option<Run>,
}

const DISPLAY_METHODS: [&str; 3] = ["upgma", "nj", "iqtree"];

/// Plan `method` on `input`. `display` is how the result is shown: an
/// inferred tree ("upgma", "nj", "iqtree"), the "existing" tree, or "none".
pub fn plan(method: &str, input: &str, rooted: bool, settings: &str, display: &str) -> Plan {
    let mut plan = Plan {
        create_manual: false,
        show_groups: None,
        first: First::Nothing,
        fit_after_display: None,
        display_after: None,
    };
    let inferred = DISPLAY_METHODS.contains(&display);
    let run = |method: String, rooted| Run {
        method,
        input: input.to_owned(),
        rooted,
        settings: settings.to_owned(),
    };
    match method {
        "manual" | "view" => {
            plan.create_manual = method == "manual";
            if inferred {
                plan.show_groups = Some(false);
                plan.first = First::Run(run(format!("display_{display}"), false));
            } else if display == "existing" {
                plan.show_groups = Some(false);
                plan.first = First::Attach {
                    input: input.to_owned(),
                    rooted,
                };
            } else {
                plan.show_groups = Some(true);
            }
        }
        // Both PTP inputs share rooting and failure gating. Inference is the
        // only extra step for an alignment input.
        "ptp" | "mptp" => {
            let mut fit_settings: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(settings).unwrap_or_default();
            let rooting = fit_settings
                .remove("rooting")
                .and_then(|value| value.as_str().map(str::to_owned))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    if display == "iqtree" {
                        "midpoint"
                    } else {
                        "keep"
                    }
                    .into()
                });
            fit_settings.remove("iqtree_model");
            let fit = Fit {
                method: method.to_owned(),
                settings: serde_json::Value::Object(fit_settings).to_string(),
                rooting,
            };
            if display == "iqtree" {
                plan.first = First::Run(run("display_iqtree".into(), false));
                plan.fit_after_display = Some(fit);
            } else if fit.rooting != "keep" || rooted {
                plan.first = First::AttachThenFit {
                    input: input.to_owned(),
                    fit,
                };
            }
        }
        // ASAP's display tree only illustrates its result, so it is built
        // after ASAP succeeds; a failed run leaves no tree behind.
        _ => {
            if method == "asap" && inferred {
                plan.display_after = Some(run(format!("display_{display}"), false));
            } else {
                plan.show_groups = Some(display == "none");
            }
            plan.first = First::Run(run(method.to_owned(), rooted));
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: &str = r#"{"rooting":"mrttv","iqtree_model":"GTR","workers":2}"#;

    #[test]
    fn ptp_on_an_alignment_infers_then_roots_and_fits() {
        for method in ["ptp", "mptp"] {
            let plan = plan(method, "/input.afa", true, SETTINGS, "iqtree");
            assert_eq!(
                plan.first,
                First::Run(Run {
                    method: "display_iqtree".into(),
                    input: "/input.afa".into(),
                    rooted: false,
                    settings: SETTINGS.into()
                })
            );
            assert_eq!(
                plan.fit_after_display,
                Some(Fit {
                    method: method.into(),
                    settings: r#"{"workers":2}"#.into(),
                    rooting: "mrttv".into()
                })
            );
        }
        let midpoint = plan("ptp", "/input.afa", true, r#"{"workers":2}"#, "iqtree");
        assert_eq!(midpoint.fit_after_display.unwrap().rooting, "midpoint");
    }

    #[test]
    fn ptp_on_a_supplied_tree_fits_only_when_it_is_or_gets_rooted() {
        let keep = plan(
            "ptp",
            "/supplied.nwk",
            false,
            r#"{"rooting":"keep"}"#,
            "existing",
        );
        assert_eq!(keep.first, First::Nothing);
        let rooted = plan(
            "mptp",
            "/supplied.nwk",
            true,
            r#"{"rooting":"keep"}"#,
            "existing",
        );
        assert!(
            matches!(rooted.first, First::AttachThenFit { ref fit, .. } if fit.rooting == "keep")
        );
        let rooting = plan(
            "ptp",
            "/supplied.nwk",
            false,
            r#"{"rooting":"midpoint"}"#,
            "existing",
        );
        assert!(
            matches!(rooting.first, First::AttachThenFit { ref input, .. } if input == "/supplied.nwk")
        );
    }

    #[test]
    fn asap_builds_its_display_tree_after_it_succeeds() {
        let plan = plan("asap", "/a.afa", false, "{}", "nj");
        assert!(matches!(plan.first, First::Run(ref run) if run.method == "asap"));
        assert_eq!(plan.display_after.unwrap().method, "display_nj");
        assert_eq!(plan.show_groups, None);
    }

    #[test]
    fn other_methods_run_once_and_choose_the_view() {
        let gmyc = plan(
            "gmyc_single",
            "/time.nwk",
            true,
            r#"{"seed":5}"#,
            "existing",
        );
        assert_eq!(
            gmyc.first,
            First::Run(Run {
                method: "gmyc_single".into(),
                input: "/time.nwk".into(),
                rooted: true,
                settings: r#"{"seed":5}"#.into()
            })
        );
        assert_eq!(gmyc.show_groups, Some(false));
        assert_eq!(
            plan("asap", "/a.afa", false, "{}", "none").show_groups,
            Some(true)
        );
    }

    #[test]
    fn manual_hypotheses_and_views_attach_or_infer_a_tree() {
        let manual = plan("manual", "/t.nwk", true, "{}", "existing");
        assert!(manual.create_manual);
        assert_eq!(
            manual.first,
            First::Attach {
                input: "/t.nwk".into(),
                rooted: true
            }
        );
        let view = plan("view", "/a.afa", false, "{}", "upgma");
        assert!(!view.create_manual);
        assert!(
            matches!(view.first, First::Run(ref run) if run.method == "display_upgma" && !run.rooted)
        );
        assert_eq!(
            plan("view", "", false, "{}", "none").show_groups,
            Some(true)
        );
    }
}
