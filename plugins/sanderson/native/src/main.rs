use std::io::{self, Read};

use basecraft_sanderson_native::{Input, solve};

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut body = String::new();
    io::stdin()
        .take(8_000_001)
        .read_to_string(&mut body)
        .map_err(|error| format!("cannot read request: {error}"))?;
    if body.len() > 8_000_000 {
        return Err("request exceeds 8 MB".into());
    }
    let input: Input =
        serde_json::from_str(&body).map_err(|error| format!("invalid request: {error}"))?;
    let result = solve(input)?;
    serde_json::to_writer(io::stdout(), &result)
        .map_err(|error| format!("cannot write result: {error}"))?;
    println!();
    Ok(())
}
