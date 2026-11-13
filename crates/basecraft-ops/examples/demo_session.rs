//! Records a short editing session in a project directory, for looking at
//! the log format: `cargo run -p basecraft-ops --example demo_session -- DIR FILE`.

use basecraft_ops::Session;
use basecraft_protocol::Op;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (root, file) = (&args[1], &args[2]);
    let mut s = Session::new(root);
    let d = s.open(std::path::Path::new(file))?;
    for (i, c) in "GATTACA".chars().enumerate() {
        let p = 1000 + i as u64;
        s.apply(d, Op::seq_replace(p, p, c.to_string()))?;
    }
    s.apply(d, Op::seq_replace(2000, 2400, String::new()))?;
    s.undo(d)?;
    s.save(d, None)?;
    Ok(())
}
