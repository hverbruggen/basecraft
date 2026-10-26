//! Deliberately direct O(n) objective evaluation used to check the optimized path.

use crate::model::{Model, Penalty};

pub fn objective(
    model: &Model,
    ages: &[f64],
    rates_per_site: &[Option<f64>],
    lambda: f64,
) -> Result<f64, String> {
    if ages.len() != model.nodes.len() || rates_per_site.len() != model.nodes.len() {
        return Err("reference result length mismatch".into());
    }
    let mut value = 0.0;
    for i in 1..model.nodes.len() {
        let parent = model.nodes[i].parent.ok_or("missing parent")?;
        let rate = rates_per_site[i].ok_or("missing fitted rate")? * model.sites;
        let mean = rate * (ages[parent] - ages[i]);
        if !mean.is_finite() || mean <= 0.0 {
            return Err("reference branch expectation is invalid".into());
        }
        let x = model.observed[i];
        value += mean - x * mean.ln() + model.ln_factorial[i];
    }
    let transform = |i: usize| -> Result<f64, String> {
        let q = rates_per_site[i].ok_or("missing fitted rate")? * model.sites;
        Ok(if model.penalty == Penalty::Log {
            q.ln()
        } else {
            q
        })
    };
    let mut roughness = 0.0;
    for i in 1..model.nodes.len() {
        let parent = model.nodes[i].parent.ok_or("missing parent")?;
        if parent != 0 {
            let delta = transform(i)? - transform(parent)?;
            roughness += delta * delta;
        }
    }
    let root_children = &model.nodes[0].children;
    let a = transform(root_children[0])?;
    let b = transform(root_children[1])?;
    let mean = (a + b) / 2.0;
    roughness += ((a - mean).powi(2) + (b - mean).powi(2)) / 2.0;
    Ok(value + lambda * roughness)
}
