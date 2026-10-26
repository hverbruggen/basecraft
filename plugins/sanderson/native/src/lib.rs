//! Native Sanderson PL calculation. Plugin transport and artifact publication live in Python/host.

mod model;
mod objective;
mod reference;

pub use model::{Input, InputNode};

use argmin::core::{Executor, State, TerminationReason};
use argmin::solver::linesearch::MoreThuenteLineSearch;
use argmin::solver::quasinewton::LBFGS;
use objective::Problem;
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize)]
pub struct StartDiagnostic {
    start: usize,
    objective: Option<f64>,
    gradient_norm: Option<f64>,
    iterations: Option<u64>,
    termination: String,
}

#[derive(Debug, Serialize)]
pub struct CvScore {
    log10_lambda: f64,
    lambda: f64,
    raw_squared_error: f64,
    chi_square_error: f64,
    predictions: Vec<f64>,
}

#[derive(Debug, Serialize)]
pub struct Output {
    pub ages: Vec<f64>,
    pub rates_per_site: Vec<Option<f64>>,
    pub objective: f64,
    pub gradient_norm: f64,
    pub lambda: f64,
    pub penalty: model::Penalty,
    pub sites: f64,
    pub root_age: f64,
    pub optimizer: &'static str,
    pub starts: Vec<StartDiagnostic>,
    pub cross_validation: Option<Vec<CvScore>>,
    pub observation_convention: &'static str,
    pub root_penalty: &'static str,
}

struct Fit {
    parameters: Vec<f64>,
    objective: f64,
    gradient_norm: f64,
    starts: Vec<StartDiagnostic>,
}

fn fit(
    model: &model::Model,
    lambda: f64,
    holdout: Option<usize>,
    deadline: Instant,
) -> Result<Fit, String> {
    let mut best: Option<Fit> = None;
    let mut all_starts = Vec::new();
    for start in 0..model.starts {
        if Instant::now() >= deadline {
            return Err("PL total time budget exceeded".into());
        }
        let problem = Problem::new(model, lambda, holdout)?;
        let initial = problem.initial(start as u64)?;
        let solver = LBFGS::new(MoreThuenteLineSearch::new(), 10)
            .with_tolerance_grad(1e-7)
            .map_err(|error| error.to_string())?
            .with_tolerance_cost(1e-11)
            .map_err(|error| error.to_string())?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let result = Executor::new(problem, solver)
            .configure(|state| {
                state
                    .param(initial)
                    .max_iters(model.max_iterations)
                    .counting(true)
            })
            .timeout(remaining)
            .run();
        match result {
            Err(error) => all_starts.push(StartDiagnostic {
                start: start + 1,
                objective: None,
                gradient_norm: None,
                iterations: None,
                termination: error.to_string(),
            }),
            Ok(result) => {
                let state = result.state();
                let reason = state.get_termination_reason();
                let iter = state.get_iter();
                let Some(parameters) = state.get_best_param() else {
                    all_starts.push(StartDiagnostic {
                        start: start + 1,
                        objective: None,
                        gradient_norm: None,
                        iterations: Some(iter),
                        termination: "no best parameters".into(),
                    });
                    continue;
                };
                let problem = result
                    .problem
                    .problem
                    .as_ref()
                    .ok_or("optimizer lost its problem state")?;
                let (objective, gradient) = problem.value_and_gradient(parameters)?;
                let gradient_norm = gradient
                    .iter()
                    .map(|value| value * value)
                    .sum::<f64>()
                    .sqrt();
                let converged = matches!(reason, Some(TerminationReason::SolverConverged))
                    && gradient_norm <= 1e-4 * (parameters.len() as f64).sqrt();
                all_starts.push(StartDiagnostic {
                    start: start + 1,
                    objective: Some(objective),
                    gradient_norm: Some(gradient_norm),
                    iterations: Some(iter),
                    termination: format!(
                        "{}{}",
                        reason.map_or("unknown".into(), ToString::to_string),
                        if converged { "" } else { "; rejected" }
                    ),
                });
                if converged
                    && best
                        .as_ref()
                        .is_none_or(|candidate| objective < candidate.objective)
                {
                    best = Some(Fit {
                        parameters: parameters.clone(),
                        objective,
                        gradient_norm,
                        starts: Vec::new(),
                    });
                }
            }
        }
    }
    let mut best = best.ok_or_else(|| format!("no converged PL start: {all_starts:?}"))?;
    best.starts = all_starts;
    Ok(best)
}

pub fn solve(input: Input) -> Result<Output, String> {
    let model = model::Model::new(input)?;
    if model.cv.is_some()
        && model
            .nodes
            .iter()
            .filter(|node| node.children.is_empty())
            .count()
            > 128
    {
        return Err("full terminal-branch cross-validation is limited to 128 tips".into());
    }
    let deadline = Instant::now() + Duration::from_secs(model.max_seconds);
    let mut scores = None;
    let mut selected = model.requested_lambda;
    if let Some(grid) = &model.cv {
        let mut rows = Vec::with_capacity(grid.count);
        let mut winner: Option<(f64, f64)> = None;
        for index in 0..grid.count {
            let log_lambda = grid.log10_start + grid.log10_step * index as f64;
            let lambda = 10_f64.powf(log_lambda);
            let mut raw = 0.0;
            let mut chi = 0.0;
            let mut predictions = Vec::new();
            for (tip, node) in model.nodes.iter().enumerate().skip(1) {
                if !node.children.is_empty() {
                    continue;
                }
                let fitted = fit(&model, lambda, Some(tip), deadline)?;
                let fold = Problem::new(&model, lambda, Some(tip))?;
                let expected = fold.predicted_holdout(&fitted.parameters)?;
                let error = expected - model.observed[tip];
                raw += error * error;
                chi += error * error / expected;
                predictions.push(expected);
            }
            if !raw.is_finite() || !chi.is_finite() {
                return Err("nonfinite CV score".into());
            }
            if winner.is_none_or(|(score, _)| chi < score) {
                winner = Some((chi, lambda));
            }
            rows.push(CvScore {
                log10_lambda: log_lambda,
                lambda,
                raw_squared_error: raw,
                chi_square_error: chi,
                predictions,
            });
        }
        selected = winner.map(|(_, lambda)| lambda);
        scores = Some(rows);
    }
    let lambda = selected.ok_or("no PL smoothing value")?;
    let fitted = fit(&model, lambda, None, deadline)?;
    let problem = Problem::new(&model, lambda, None)?;
    let (ages, rates, objective, gradient_norm) = problem.unpack(&fitted.parameters)?;
    let reference = reference::objective(&model, &ages, &rates, lambda)?;
    if (objective - reference).abs() > 1e-8 * reference.abs().max(1.0) {
        return Err("independent PL objective recheck disagrees".into());
    }
    for (i, node) in model.nodes.iter().enumerate().skip(1) {
        let parent = node.parent.ok_or("missing parent")?;
        if !ages[i].is_finite() || ages[parent] - ages[i] <= 0.0 {
            return Err("nonpositive fitted duration".into());
        }
        if ages[i] < model.lower[i] * model.root_age - 1e-8
            || ages[i] > model.upper[i] * model.root_age + 1e-8
        {
            return Err("fitted age violates a hard bound".into());
        }
    }
    if !objective.is_finite()
        || !gradient_norm.is_finite()
        || rates.iter().flatten().any(|r| !r.is_finite() || *r <= 0.0)
    {
        return Err("nonfinite or nonpositive PL result".into());
    }
    Ok(Output {
        ages,
        rates_per_site: rates,
        objective,
        gradient_norm,
        lambda,
        penalty: model.penalty,
        sites: model.sites,
        root_age: model.root_age,
        optimizer: "argmin 0.11 L-BFGS with More-Thuente line search",
        starts: fitted.starts,
        cross_validation: scores,
        observation_convention: "continuous substitutions; gamma-extended Poisson quasi-likelihood",
        root_penalty: "population variance of root-child total rates",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InputNode, Parameters};

    fn sample(penalty: &str) -> Input {
        Input {
            nodes: vec![
                InputNode {
                    parent: None,
                    children: vec![1, 4],
                    label: "root".into(),
                    length: None,
                },
                InputNode {
                    parent: Some(0),
                    children: vec![2, 3],
                    label: "clade".into(),
                    length: Some(0.21),
                },
                InputNode {
                    parent: Some(1),
                    children: vec![],
                    label: "A".into(),
                    length: Some(0.11),
                },
                InputNode {
                    parent: Some(1),
                    children: vec![],
                    label: "B".into(),
                    length: Some(0.12),
                },
                InputNode {
                    parent: Some(0),
                    children: vec![],
                    label: "C".into(),
                    length: Some(0.3),
                },
            ],
            parameters: Parameters {
                branch_units: "substitutions_per_site".into(),
                sites: 953,
                root_age: 1.0,
                time_unit: "relative units".into(),
                penalty: penalty.into(),
                lambda: Some(0.01),
                starts: 2,
                seed: 7,
                constraints: vec![],
                cross_validation: None,
                max_iterations: 2000,
                max_seconds: 60,
            },
            cancel_file: None,
        }
    }

    #[test]
    fn continuous_lengths_are_preserved() {
        let model = model::Model::new(sample("add")).unwrap();
        assert!((model.observed[2] - 104.83).abs() < 1e-12);
        assert!((model.observed[3] - 114.36).abs() < 1e-12);
        let problem = Problem::new(&model, 0.01, None).unwrap();
        let x = problem.initial(0).unwrap();
        let (ages, rates, fast, _) = problem.unpack(&x).unwrap();
        let slow = reference::objective(&model, &ages, &rates, 0.01).unwrap();
        assert!((fast - slow).abs() < 1e-9 * slow.abs().max(1.0));
    }

    #[test]
    fn analytic_gradient_matches_central_difference_for_both_penalties() {
        for penalty in ["add", "log"] {
            let model = model::Model::new(sample(penalty)).unwrap();
            let problem = Problem::new(&model, 0.01, None).unwrap();
            let mut x = problem.initial(2).unwrap();
            let (_, gradient) = problem.value_and_gradient(&x).unwrap();
            for i in 0..x.len() {
                let old = x[i];
                let h = 1e-6;
                x[i] = old + h;
                let positive = problem.value(&x).unwrap();
                x[i] = old - h;
                let negative = problem.value(&x).unwrap();
                x[i] = old;
                let numerical = (positive - negative) / (2.0 * h);
                let tolerance = 1e-5 * numerical.abs().max(1.0);
                assert!(
                    (gradient[i] - numerical).abs() < tolerance,
                    "{penalty} parameter {i}: analytic={} numerical={numerical}",
                    gradient[i]
                );
            }
        }
    }

    #[test]
    fn held_out_tip_uses_parent_or_sister_rate() {
        let model = model::Model::new(sample("log")).unwrap();
        for tip in [2, 4] {
            let problem = Problem::new(&model, 0.1, Some(tip)).unwrap();
            let initial = problem.initial(0).unwrap();
            let prediction = problem.predicted_holdout(&initial).unwrap();
            assert!(prediction.is_finite() && prediction > 0.0);
        }
    }

    #[test]
    fn rejects_invalid_lengths_and_bounds() {
        let mut input = sample("add");
        input.nodes[2].length = Some(0.0);
        assert!(model::Model::new(input).is_err());
        let mut input = sample("add");
        input.parameters.constraints.push(model::Constraint {
            node: 1,
            min_age: 1.0,
            max_age: 1.0,
        });
        assert!(model::Model::new(input).is_err());
    }

    #[test]
    #[ignore = "run only on build-host after the coordinator lifts the compile hold"]
    fn benchmark_objective_against_direct_reference() {
        let path = std::env::var("BASECRAFT_SANDERSON_BENCH_REQUEST").expect("fixture path");
        let request = std::fs::read_to_string(path).unwrap();
        let input: Input = serde_json::from_str(&request).unwrap();
        let model = model::Model::new(input).unwrap();
        let problem = Problem::new(&model, 1.0, None).unwrap();
        let a = problem.initial(0).unwrap();
        let mut b = a.clone();
        b[0] += 1e-5;
        let (age_a, rate_a, cost_a, _) = problem.unpack(&a).unwrap();
        let (age_b, rate_b, cost_b, _) = problem.unpack(&b).unwrap();
        for (ages, rates, cost) in [(&age_a, &rate_a, cost_a), (&age_b, &rate_b, cost_b)] {
            let direct = reference::objective(&model, ages, rates, 1.0).unwrap();
            assert!((direct - cost).abs() <= 1e-8 * direct.abs().max(1.0));
        }
        let repetitions = if model.nodes.len() > 200 { 300 } else { 2000 };
        let start = Instant::now();
        for i in 0..repetitions {
            std::hint::black_box(problem.value(if i % 2 == 0 { &a } else { &b }).unwrap());
        }
        let optimized = start.elapsed();
        let start = Instant::now();
        for i in 0..repetitions {
            let (ages, rates) = if i % 2 == 0 {
                (&age_a, &rate_a)
            } else {
                (&age_b, &rate_b)
            };
            std::hint::black_box(reference::objective(&model, ages, rates, 1.0).unwrap());
        }
        let direct = start.elapsed();
        println!(
            "nodes={} evaluations={} indexed={optimized:?} direct={direct:?}",
            model.nodes.len(),
            repetitions
        );
    }
}
