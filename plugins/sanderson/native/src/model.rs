use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// Dimensionless separation in units of root_age; guards positive durations.
pub(crate) const GAP: f64 = 64.0 * f64::EPSILON;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputNode {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub label: String,
    pub length: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    pub node: usize,
    pub min_age: f64,
    pub max_age: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossValidation {
    pub log10_start: f64,
    pub log10_step: f64,
    pub count: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub branch_units: String,
    pub sites: u64,
    pub root_age: f64,
    pub time_unit: String,
    pub penalty: String,
    pub lambda: Option<f64>,
    pub starts: usize,
    pub seed: u64,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    pub cross_validation: Option<CrossValidation>,
    #[serde(default = "default_iterations")]
    pub max_iterations: u64,
    #[serde(default = "default_seconds")]
    pub max_seconds: u64,
}

const fn default_iterations() -> u64 {
    2000
}
const fn default_seconds() -> u64 {
    3600
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub nodes: Vec<InputNode>,
    pub parameters: Parameters,
    pub cancel_file: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Penalty {
    Add,
    Log,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub nodes: Vec<InputNode>,
    pub observed: Vec<f64>,
    pub ln_factorial: Vec<f64>,
    pub lower: Vec<f64>,
    pub upper: Vec<f64>,
    pub required_lower: Vec<f64>,
    pub free_nodes: Vec<usize>,
    pub penalty: Penalty,
    pub sites: f64,
    pub root_age: f64,
    pub max_iterations: u64,
    pub max_seconds: u64,
    pub starts: usize,
    pub seed: u64,
    pub cancel_file: Option<String>,
    pub cv: Option<CrossValidation>,
    pub requested_lambda: Option<f64>,
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

impl Model {
    pub fn new(input: Input) -> Result<Self, String> {
        let Input {
            nodes,
            parameters: p,
            cancel_file,
        } = input;
        if nodes.len() < 5
            || nodes.len() > 4095
            || nodes[0].parent.is_some()
            || nodes[0].length.is_some()
        {
            return Err(
                "expected one rooted binary tree with 3..2048 tips and no root stem".into(),
            );
        }
        if !positive(p.root_age) || p.sites == 0 || p.sites > 1_000_000_000 {
            return Err("invalid root age or effective site count".into());
        }
        if p.time_unit.trim().is_empty()
            || p.time_unit.len() > 80
            || p.time_unit.chars().any(char::is_control)
        {
            return Err("invalid time-unit label".into());
        }
        if !(1..=32).contains(&p.starts)
            || p.seed == 0
            || p.max_iterations == 0
            || p.max_iterations > 100_000
            || p.max_seconds == 0
            || p.max_seconds > 86_400
        {
            return Err("invalid optimizer starts, seed or budget".into());
        }
        let penalty = match p.penalty.as_str() {
            "add" => Penalty::Add,
            "log" => Penalty::Log,
            _ => return Err("penalty must be add or log".into()),
        };
        if p.branch_units != "substitutions_per_site" && p.branch_units != "substitution_counts" {
            return Err("invalid substitution branch units".into());
        }
        if p.lambda.is_some() == p.cross_validation.is_some() {
            return Err("exactly one of lambda and CV grid is required".into());
        }
        if let Some(lambda) = p.lambda {
            if !positive(lambda) || lambda > 1e8 {
                return Err("invalid lambda".into());
            }
        }
        if let Some(cv) = &p.cross_validation {
            if !cv.log10_start.is_finite()
                || !positive(cv.log10_step)
                || !(2..=30).contains(&cv.count)
                || cv.log10_start < -8.0
                || cv.log10_start + cv.log10_step * (cv.count - 1) as f64 > 8.0
            {
                return Err("invalid CV log-lambda grid".into());
            }
        }
        let mut tips = HashSet::new();
        let mut tip_count = 0;
        let mut indegree = vec![0_u8; nodes.len()];
        for (i, node) in nodes.iter().enumerate() {
            if i > 0
                && (node.parent.is_none_or(|parent| parent >= i)
                    || !node.length.is_some_and(positive))
            {
                return Err(format!(
                    "node {i} has invalid parent or substitution length"
                ));
            }
            if node.children.is_empty() {
                tip_count += 1;
                if node.label.is_empty() || !tips.insert(node.label.clone()) {
                    return Err("tip labels must be unique and nonempty".into());
                }
            } else if node.children.len() != 2 {
                return Err("only binary rooted topology is supported".into());
            }
            for &child in &node.children {
                if child <= i || child >= nodes.len() || nodes[child].parent != Some(i) {
                    return Err("inconsistent rooted topology".into());
                }
                indegree[child] = indegree[child].saturating_add(1);
            }
        }
        if tip_count < 3
            || tip_count * 2 - 1 != nodes.len()
            || indegree[0] != 0
            || indegree.iter().skip(1).any(|&degree| degree != 1)
        {
            return Err("tree is disconnected or not bifurcating".into());
        }
        let sites = p.sites as f64;
        let mut observed = vec![0.0; nodes.len()];
        let mut ln_factorial = vec![0.0; nodes.len()];
        for (i, node) in nodes.iter().enumerate().skip(1) {
            let x = node.length.unwrap_or(0.0)
                * if p.branch_units == "substitutions_per_site" {
                    sites
                } else {
                    1.0
                };
            if !positive(x) || x > 1e12 {
                return Err("scaled substitution length is outside supported finite range".into());
            }
            observed[i] = x;
            ln_factorial[i] = statrs::function::gamma::ln_gamma(x + 1.0);
        }
        let mut lower = vec![0.0; nodes.len()];
        let mut upper = vec![1.0; nodes.len()];
        lower[0] = 1.0;
        upper[0] = 1.0;
        let mut seen = HashSet::new();
        if p.constraints.len() > 128 {
            return Err("too many age constraints".into());
        }
        for constraint in &p.constraints {
            let i = constraint.node;
            if i == 0
                || i >= nodes.len()
                || nodes[i].children.is_empty()
                || !seen.insert(i)
                || !positive(constraint.min_age)
                || !positive(constraint.max_age)
                || constraint.min_age > constraint.max_age
                || constraint.max_age > p.root_age
            {
                return Err("invalid or repeated internal age constraint".into());
            }
            lower[i] = constraint.min_age / p.root_age;
            upper[i] = constraint.max_age / p.root_age;
        }
        let mut required_lower = lower.clone();
        for i in (0..nodes.len()).rev() {
            for &child in &nodes[i].children {
                required_lower[i] = required_lower[i].max(required_lower[child] + GAP);
            }
            if required_lower[i] > upper[i] {
                return Err("infeasible age constraints or tree depth".into());
            }
        }
        if required_lower[0] > 1.0 {
            return Err("age constraints exceed root age".into());
        }
        let free_nodes = (1..nodes.len())
            .filter(|&i| !nodes[i].children.is_empty() && required_lower[i] < upper[i])
            .collect();
        Ok(Self {
            nodes,
            observed,
            ln_factorial,
            lower,
            upper,
            required_lower,
            free_nodes,
            penalty,
            sites,
            root_age: p.root_age,
            max_iterations: p.max_iterations,
            max_seconds: p.max_seconds,
            starts: p.starts,
            seed: p.seed,
            cancel_file,
            cv: p.cross_validation,
            requested_lambda: p.lambda,
        })
    }
}
