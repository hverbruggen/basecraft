//! Prints anonymous memory after each step of opening a file:
//! `cargo run --release -p basecraft-ops --example open_memory -- FILE`.

fn anon_mb() -> f64 {
    std::fs::read_to_string("/proc/self/smaps_rollup")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("Anonymous:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}

fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("file"));
    println!("start            {:7.1} MB", anon_mb());
    let gff =
        basecraft_io::gff3::parse(&std::fs::read(path.with_extension("gff3")).unwrap_or_default())
            .unwrap();
    println!("gff parsed       {:7.1} MB", anon_mb());
    drop(gff);
    println!("gff dropped      {:7.1} MB", anon_mb());
    let opened = basecraft_ops::files::open(&path).unwrap();
    println!(
        "opened           {:7.1} MB  ({} features)",
        anon_mb(),
        opened.doc.feature_count()
    );
    let idx = opened.doc.index();
    println!(
        "interval index   {:7.1} MB  ({} entries)",
        anon_mb(),
        idx.len()
    );
    let s = opened.doc.summary();
    println!("gc summary       {:7.1} MB  ({} bp)", anon_mb(), s.len);
}
