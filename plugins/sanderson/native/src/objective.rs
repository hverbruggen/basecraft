//! Compact tree-indexed PL objective and analytic gradient.

use crate::model::{GAP, Model, Penalty};
use argmin::core::{CostFunction, Error, Gradient};
use std::cell::{Cell, RefCell};
use std::time::Instant;

pub struct Problem<'a> {
    pub model: &'a Model,
    pub lambda: f64,
    pub holdout: Option<usize>,
    pub age_var: Vec<Option<usize>>,
    pub rate_var: Vec<Option<usize>>,
    pub dimension: usize,
    pub evaluations: Cell<u64>,
    optimizer_scale: Cell<f64>,
    started: Instant,
    scratch: RefCell<Scratch>,
    #[cfg(feature = "profile")]
    profile: ProfileStats,
}

#[cfg(feature = "profile")]
#[derive(Default)]
struct ProfileStats {
    cost_calls: Cell<u64>,
    gradient_calls: Cell<u64>,
    computes: Cell<u64>,
    age_ns: Cell<u64>,
    likelihood_ns: Cell<u64>,
    penalty_ns: Cell<u64>,
    assemble_ns: Cell<u64>,
}

#[cfg(feature = "profile")]
impl Drop for Problem<'_> {
    fn drop(&mut self) {
        if std::env::var_os("BASECRAFT_SANDERSON_PROFILE").is_some() {
            eprintln!(
                "profile nodes={} dimension={} holdout={:?} cost_calls={} gradient_calls={} computes={} age_ns={} likelihood_ns={} penalty_ns={} assemble_ns={}",
                self.model.nodes.len(),
                self.dimension,
                self.holdout,
                self.profile.cost_calls.get(),
                self.profile.gradient_calls.get(),
                self.profile.computes.get(),
                self.profile.age_ns.get(),
                self.profile.likelihood_ns.get(),
                self.profile.penalty_ns.get(),
                self.profile.assemble_ns.get()
            );
        }
    }
}

struct Scratch {
    last: Vec<f64>,
    cost: f64,
    gradient: Vec<f64>,
    age: Vec<f64>,
    rate: Vec<f64>, // total substitutions per unit of the supplied time scale
    age_gradient: Vec<f64>,
    rate_gradient: Vec<f64>, // derivative with respect to log(total rate * root age)
    parent_derivative: Vec<f64>,
    local_derivative: Vec<f64>,
}

pub type DecodedFit = (Vec<f64>, Vec<Option<f64>>, f64, f64);

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let e = value.exp();
        e / (1.0 + e)
    }
}

impl<'a> Problem<'a> {
    pub fn new(model: &'a Model, lambda: f64, holdout: Option<usize>) -> Result<Self, String> {
        if !lambda.is_finite() || lambda <= 0.0 {
            return Err("invalid PL lambda".into());
        }
        if holdout.is_some_and(|i| i == 0 || i >= model.nodes.len() || !model.is_tip[i]) {
            return Err("CV holdout must be one terminal branch".into());
        }
        let mut age_var = vec![None; model.nodes.len()];
        for (i, &node) in model.free_nodes.iter().enumerate() {
            age_var[node] = Some(i);
        }
        let mut rate_var = vec![None; model.nodes.len()];
        let mut dimension = model.free_nodes.len();
        for (i, slot) in rate_var.iter_mut().enumerate().skip(1) {
            if Some(i) != holdout {
                *slot = Some(dimension);
                dimension += 1;
            }
        }
        let n = model.nodes.len();
        Ok(Self {
            model,
            lambda,
            holdout,
            age_var,
            rate_var,
            dimension,
            evaluations: Cell::new(0),
            optimizer_scale: Cell::new(1.0),
            started: Instant::now(),
            scratch: RefCell::new(Scratch {
                last: Vec::new(),
                cost: f64::NAN,
                gradient: vec![0.0; dimension],
                age: vec![0.0; n],
                rate: vec![0.0; n],
                age_gradient: vec![0.0; n],
                rate_gradient: vec![0.0; n],
                parent_derivative: vec![0.0; n],
                local_derivative: vec![0.0; n],
            }),
            #[cfg(feature = "profile")]
            profile: ProfileStats::default(),
        })
    }

    pub fn initial(&self, perturb: u64) -> Result<Vec<f64>, String> {
        let mut x = vec![0.0; self.dimension];
        let mut state = perturb ^ self.model.seed;
        for value in x.iter_mut().take(self.model.free_nodes.len()) {
            if perturb != 0 {
                *value = 0.5 * (unit_random(&mut state) - 0.5);
            }
        }
        let mut scratch = self.scratch.borrow_mut();
        self.map_ages(&x, &mut scratch)?;
        for i in 1..self.model.nodes.len() {
            if let Some(j) = self.rate_var[i] {
                let parent = self.model.parents[i];
                let duration = scratch.age[parent] - scratch.age[i];
                if duration <= 0.0 {
                    return Err("initial ages violate strict ancestry".into());
                }
                x[j] = (self.model.observed[i] / duration).ln();
                if perturb != 0 {
                    x[j] += 0.2 * (unit_random(&mut state) - 0.5);
                }
            }
        }
        Ok(x)
    }

    pub fn scale_optimizer_at(&self, x: &[f64]) -> Result<f64, String> {
        let (_, gradient) = self.value_and_gradient(x)?;
        let largest = gradient.iter().fold(0.0_f64, |a, b| a.max(b.abs()));
        if !largest.is_finite() {
            return Err("nonfinite initial PL gradient".into());
        }
        // A positive constant rescales the solver's objective, not the PL
        // minimizer. It keeps the first L-BFGS trial step well conditioned.
        let scale = largest.max(1.0);
        self.optimizer_scale.set(scale);
        Ok(scale)
    }

    fn map_ages(&self, x: &[f64], s: &mut Scratch) -> Result<(), String> {
        s.age[0] = 1.0;
        s.parent_derivative.fill(0.0);
        s.local_derivative.fill(0.0);
        for i in 1..self.model.nodes.len() {
            let parent = self.model.parents[i];
            if self.model.is_tip[i] {
                s.age[i] = 0.0;
                continue;
            }
            if self.model.required_lower[i] == self.model.upper[i] {
                s.age[i] = self.model.required_lower[i];
                if s.age[parent] - s.age[i] < GAP {
                    return Err("fixed age violates strict ancestry".into());
                }
                continue;
            }
            let parent_cap = s.age[parent] - GAP;
            let upper = self.model.upper[i].min(parent_cap);
            let lower = self.model.required_lower[i];
            if upper < lower || !upper.is_finite() {
                return Err("age bounds became infeasible".into());
            }
            let z = x[*self.age_var[i].as_ref().ok_or("missing age parameter")?];
            let fraction = sigmoid(z);
            s.age[i] = lower + (upper - lower) * fraction;
            s.local_derivative[i] = (upper - lower) * fraction * (1.0 - fraction);
            if parent_cap < self.model.upper[i] {
                s.parent_derivative[i] = fraction;
            }
        }
        Ok(())
    }

    fn evaluate(&self, x: &[f64], s: &mut Scratch) -> Result<(), String> {
        if x.len() != self.dimension || x.iter().any(|value| !value.is_finite()) {
            return Err("invalid optimizer vector".into());
        }
        #[cfg(feature = "profile")]
        let phase_start = Instant::now();
        self.map_ages(x, s)?;
        #[cfg(feature = "profile")]
        let phase_ages = Instant::now();
        s.age_gradient.fill(0.0);
        s.rate_gradient.fill(0.0);
        s.gradient.fill(0.0);
        let n = self.model.nodes.len();
        let mut likelihood = 0.0;
        for i in 1..n {
            if Some(i) == self.holdout {
                continue;
            }
            let j = self.rate_var[i].ok_or("missing rate parameter")?;
            let q_scaled = x[j].exp();
            if !q_scaled.is_finite() || q_scaled <= 0.0 {
                return Err("nonfinite fitted rate".into());
            }
            s.rate[i] = q_scaled / self.model.root_age;
            let parent = self.model.parents[i];
            let duration = s.age[parent] - s.age[i];
            let mean = q_scaled * duration;
            if !mean.is_finite() || mean <= 0.0 {
                return Err("nonpositive branch expectation".into());
            }
            let observed = self.model.observed[i];
            likelihood += mean - observed * mean.ln();
            s.rate_gradient[i] += mean - observed;
            let age_derivative = q_scaled - observed / duration;
            s.age_gradient[parent] += age_derivative;
            s.age_gradient[i] -= age_derivative;
        }
        #[cfg(feature = "profile")]
        let phase_likelihood = Instant::now();
        let mut roughness = 0.0;
        for &(i, parent) in &self.model.nonbasal_edges {
            if Some(i) == self.holdout {
                continue;
            }
            let left = match self.model.penalty {
                Penalty::Add => s.rate[i],
                Penalty::Log => x[self.rate_var[i].ok_or("missing rate parameter")?],
            };
            let right = match self.model.penalty {
                Penalty::Add => s.rate[parent],
                Penalty::Log => x[self.rate_var[parent].ok_or("missing rate parameter")?],
            };
            let difference = left - right;
            roughness += difference * difference;
            let left_chain = if self.model.penalty == Penalty::Add {
                s.rate[i]
            } else {
                1.0
            };
            let right_chain = if self.model.penalty == Penalty::Add {
                s.rate[parent]
            } else {
                1.0
            };
            s.rate_gradient[i] += self.lambda * 2.0 * difference * left_chain;
            s.rate_gradient[parent] -= self.lambda * 2.0 * difference * right_chain;
        }
        let basal = self.model.basal;
        if self.holdout != Some(basal[0]) && self.holdout != Some(basal[1]) {
            let values = basal.map(|i| {
                if self.model.penalty == Penalty::Add {
                    s.rate[i]
                } else {
                    x[self.rate_var[i].unwrap_or(0)]
                }
            });
            let mean = (values[0] + values[1]) / 2.0;
            for (&i, &value) in basal.iter().zip(&values) {
                let delta = value - mean;
                roughness += delta * delta / 2.0;
                let chain = if self.model.penalty == Penalty::Add {
                    s.rate[i]
                } else {
                    1.0
                };
                s.rate_gradient[i] += self.lambda * delta * chain;
            }
        }
        #[cfg(feature = "profile")]
        let phase_penalty = Instant::now();
        for i in (1..n).rev() {
            if let Some(j) = self.age_var[i] {
                s.gradient[j] = s.age_gradient[i] * s.local_derivative[i];
                let parent = self.model.parents[i];
                s.age_gradient[parent] += s.age_gradient[i] * s.parent_derivative[i];
            }
        }
        for i in 1..n {
            if let Some(j) = self.rate_var[i] {
                s.gradient[j] = s.rate_gradient[i];
            }
        }
        s.cost = likelihood + self.lambda * roughness;
        if !s.cost.is_finite() || s.gradient.iter().any(|value| !value.is_finite()) {
            return Err("nonfinite objective or gradient".into());
        }
        s.last.clear();
        s.last.extend_from_slice(x);
        #[cfg(feature = "profile")]
        {
            self.profile.computes.set(self.profile.computes.get() + 1);
            self.profile.age_ns.set(
                self.profile.age_ns.get()
                    + phase_ages.duration_since(phase_start).as_nanos() as u64,
            );
            self.profile.likelihood_ns.set(
                self.profile.likelihood_ns.get()
                    + phase_likelihood.duration_since(phase_ages).as_nanos() as u64,
            );
            self.profile.penalty_ns.set(
                self.profile.penalty_ns.get()
                    + phase_penalty.duration_since(phase_likelihood).as_nanos() as u64,
            );
            self.profile.assemble_ns.set(
                self.profile.assemble_ns.get()
                    + Instant::now().duration_since(phase_penalty).as_nanos() as u64,
            );
        }
        Ok(())
    }

    fn check(&self) -> Result<(), String> {
        let count = self.evaluations.get() + 1;
        self.evaluations.set(count);
        if count > self.model.max_iterations.saturating_mul(100) {
            return Err("objective evaluation budget exceeded".into());
        }
        if self.started.elapsed().as_secs() >= self.model.max_seconds {
            return Err("analysis time budget exceeded".into());
        }
        if self
            .model
            .cancel_file
            .as_ref()
            .is_some_and(|path| std::path::Path::new(path).exists())
        {
            return Err("analysis cancelled".into());
        }
        Ok(())
    }

    pub fn value_and_gradient(&self, x: &[f64]) -> Result<(f64, Vec<f64>), String> {
        self.check()?;
        let mut s = self.scratch.borrow_mut();
        if s.last != x {
            self.evaluate(x, &mut s)?;
        }
        Ok((s.cost, s.gradient.clone()))
    }

    pub fn value(&self, x: &[f64]) -> Result<f64, String> {
        self.check()?;
        let mut s = self.scratch.borrow_mut();
        if s.last != x {
            self.evaluate(x, &mut s)?;
        }
        Ok(s.cost)
    }

    pub fn gradient_vector(&self, x: &[f64]) -> Result<Vec<f64>, String> {
        self.check()?;
        let mut s = self.scratch.borrow_mut();
        if s.last != x {
            self.evaluate(x, &mut s)?;
        }
        Ok(s.gradient.clone())
    }

    pub fn unpack(&self, x: &[f64]) -> Result<DecodedFit, String> {
        let (cost, gradient) = self.value_and_gradient(x)?;
        let s = self.scratch.borrow();
        let mut rates = vec![None; self.model.nodes.len()];
        for (i, rate) in rates.iter_mut().enumerate().skip(1) {
            if self.rate_var[i].is_some() {
                *rate = Some(s.rate[i] / self.model.sites);
            }
        }
        let ages = s.age.iter().map(|age| age * self.model.root_age).collect();
        let full_cost = cost
            + self
                .model
                .ln_factorial
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != self.holdout)
                .map(|(_, value)| value)
                .sum::<f64>();
        let norm = gradient
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        Ok((ages, rates, full_cost, norm))
    }

    pub fn predicted_holdout(&self, x: &[f64]) -> Result<f64, String> {
        let tip = self.holdout.ok_or("not a CV fold")?;
        self.value_and_gradient(x)?;
        let s = self.scratch.borrow();
        let parent = self.model.parents[tip];
        let donor = if parent == 0 {
            self.model
                .basal
                .iter()
                .copied()
                .find(|&sibling| sibling != tip)
                .ok_or("missing root sister")?
        } else {
            parent
        };
        let prediction = s.rate[donor] * self.model.root_age * (s.age[parent] - s.age[tip]);
        if !prediction.is_finite() || prediction <= 0.0 {
            return Err("invalid held-out prediction".into());
        }
        Ok(prediction)
    }
}

fn unit_random(state: &mut u64) -> f64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((z ^ (z >> 31)) >> 11) as f64 / ((1_u64 << 53) as f64)
}

impl CostFunction for Problem<'_> {
    type Param = Vec<f64>;
    type Output = f64;
    fn cost(&self, param: &Self::Param) -> Result<Self::Output, Error> {
        #[cfg(feature = "profile")]
        self.profile
            .cost_calls
            .set(self.profile.cost_calls.get() + 1);
        self.value(param)
            .map(|cost| cost / self.optimizer_scale.get())
            .map_err(|message| anyhow::anyhow!(message))
    }
}

impl Gradient for Problem<'_> {
    type Param = Vec<f64>;
    type Gradient = Vec<f64>;
    fn gradient(&self, param: &Self::Param) -> Result<Self::Gradient, Error> {
        #[cfg(feature = "profile")]
        self.profile
            .gradient_calls
            .set(self.profile.gradient_calls.get() + 1);
        self.gradient_vector(param)
            .map(|gradient| {
                gradient
                    .into_iter()
                    .map(|value| value / self.optimizer_scale.get())
                    .collect()
            })
            .map_err(|message| anyhow::anyhow!(message))
    }
}
