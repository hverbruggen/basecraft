//! Emit portable fixtures for independent `SpartParser` validation.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("output directory required")?;
    let path = std::path::Path::new(&path);
    std::fs::create_dir_all(path)?;
    let work = basecraft_io::spart::parse_text(include_str!(
        "../../../testdata/fixtures/delimitation/manual.spart"
    ))?;
    std::fs::write(
        path.join("partial.xml"),
        basecraft_io::spart::write_xml(&work),
    )?;
    let mut complete = work.clone();
    complete.hypotheses.truncate(2);
    std::fs::write(
        path.join("complete.spart"),
        basecraft_io::spart::write_text(&complete).0,
    )?;
    Ok(())
}
