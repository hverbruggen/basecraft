//! Text tables: Basecraft's own TSV files, spreadsheets read for import
//! (CSV, TSV, `.ods`, `.xlsx`) and the `.ods` template writer.
//!
//! Basecraft TSV has a header row and one line per row. Within a field a
//! backslash, tab, newline or carriage return is written `\\`, `\t`, `\n`
//! or `\r`, so every line has exactly the header's number of fields.
use std::io::{Cursor, Write};
use std::path::Path;

use crate::{Error, Result};

fn invalid(message: impl Into<String>) -> Error {
    Error::Table(message.into())
}

/// Escape one field for Basecraft TSV.
pub fn escape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(field: &str, line: usize) -> Result<String> {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            other => {
                return Err(invalid(format!(
                    "line {line}: unknown escape \\{}",
                    other.map(String::from).unwrap_or_default()
                )));
            }
        }
    }
    Ok(out)
}

/// Write a Basecraft TSV table.
pub fn write_tsv(w: &mut dyn Write, header: &[&str], rows: &[Vec<String>]) -> std::io::Result<()> {
    let line = |fields: &mut dyn Iterator<Item = String>| fields.collect::<Vec<_>>().join("\t");
    writeln!(w, "{}", line(&mut header.iter().map(|h| escape(h))))?;
    for row in rows {
        writeln!(w, "{}", line(&mut row.iter().map(|f| escape(f))))?;
    }
    Ok(())
}

/// A table of text cells with a header row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sheet {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Cells (row, column) that a spreadsheet stored as a date or time.
    /// Their text is ISO 8601 at day precision or finer; a spreadsheet
    /// date can't say that only the year or month was known.
    pub date_cells: Vec<(usize, usize)>,
}

impl Sheet {
    pub fn column(&self, header: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == header)
    }
}

fn text(bytes: &[u8], what: &str) -> Result<String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8(bytes.to_vec()).map_err(|_| invalid(format!("{what} is not UTF-8 text")))
}

/// Read a Basecraft TSV table. Every row must have the header's width.
pub fn read_tsv(bytes: &[u8]) -> Result<Sheet> {
    let text = text(bytes, "table")?;
    let mut lines = text.split('\n').enumerate();
    let header = lines.next().map(|(_, l)| l.strip_suffix('\r').unwrap_or(l));
    let Some(header) = header.filter(|h| !h.is_empty()) else {
        return Err(invalid("table has no header row"));
    };
    let headers = header
        .split('\t')
        .map(|h| unescape(h, 1))
        .collect::<Result<Vec<_>>>()?;
    let mut rows = Vec::new();
    for (index, line) in lines {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let fields = line
            .split('\t')
            .map(|f| unescape(f, index + 1))
            .collect::<Result<Vec<_>>>()?;
        if fields.len() != headers.len() {
            return Err(invalid(format!(
                "line {}: {} fields, the header has {}",
                index + 1,
                fields.len(),
                headers.len()
            )));
        }
        rows.push(fields);
    }
    Ok(Sheet {
        headers,
        rows,
        date_cells: Vec::new(),
    })
}

/// Parse delimited text with RFC 4180 quoting.
pub fn read_delimited(bytes: &[u8], delimiter: char) -> Result<Sheet> {
    let text = text(bytes, "spreadsheet")?;
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut started = false;
    while let Some(c) = chars.next() {
        started = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == delimiter => record.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
                started = false;
            }
            c => field.push(c),
        }
    }
    if quoted {
        return Err(invalid("a quoted field is not closed"));
    }
    if started {
        record.push(field);
        records.push(record);
    }
    sheet_from_records(records, &[])
}

fn sheet_from_records(records: Vec<Vec<String>>, date_cells: &[(usize, usize)]) -> Result<Sheet> {
    let mut records = records.into_iter();
    let mut headers: Vec<String> = records
        .next()
        .ok_or_else(|| invalid("spreadsheet is empty"))?
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect();
    // Columns after the last header are not part of the table.
    while headers.last().is_some_and(String::is_empty) {
        headers.pop();
    }
    if headers.is_empty() {
        return Err(invalid(
            "spreadsheet has no column headers in its first row",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for header in &headers {
        if !header.is_empty() && !seen.insert(header.as_str()) {
            return Err(invalid(format!("column header {header:?} appears twice")));
        }
    }
    let width = headers.len();
    let mut rows = Vec::new();
    let mut kept_dates = Vec::new();
    for (index, mut record) in records.enumerate() {
        if record.iter().all(|f| f.trim().is_empty()) {
            continue;
        }
        record.resize(width, String::new());
        for &(_, c) in date_cells.iter().filter(|(r, c)| *r == index && *c < width) {
            kept_dates.push((rows.len(), c));
        }
        rows.push(record);
    }
    Ok(Sheet {
        headers,
        rows,
        date_cells: kept_dates,
    })
}

/// The first worksheet of an `.ods`, `.xlsx`, `.xls` or `.xlsb` file, or
/// a CSV or TSV file, as text cells.
pub fn read_spreadsheet(path: &Path) -> Result<Sheet> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "csv" => {
            let bytes = std::fs::read(path)?;
            let first = bytes.split(|b| *b == b'\n').next().unwrap_or_default();
            let commas = memchr::memchr_iter(b',', first).count();
            let semicolons = memchr::memchr_iter(b';', first).count();
            read_delimited(&bytes, if semicolons > commas { ';' } else { ',' })
        }
        "tsv" | "tab" | "txt" => read_delimited(&std::fs::read(path)?, '\t'),
        "ods" | "xlsx" | "xlsm" | "xls" | "xlsb" => read_workbook(path),
        _ => Err(invalid(format!(
            "{} is not a spreadsheet Basecraft reads (.ods, .xlsx, .csv or .tsv)",
            path.display()
        ))),
    }
}

fn read_workbook(path: &Path) -> Result<Sheet> {
    use calamine::{Data, Reader};
    let mut workbook = calamine::open_workbook_auto(path)
        .map_err(|e| invalid(format!("could not read {}: {e}", path.display())))?;
    let name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| invalid("the workbook has no sheets"))?;
    let range = workbook
        .worksheet_range(&name)
        .map_err(|e| invalid(format!("could not read sheet {name}: {e}")))?;
    let (first_row, first_column) = range.start().unwrap_or((0, 0));
    let mut records = Vec::new();
    let mut date_cells = Vec::new();
    // Rows and columns before the used range are empty, but keep the
    // header in the first row of the sheet as people see it.
    for _ in 0..first_row {
        records.push(Vec::new());
    }
    for (r, row) in range.rows().enumerate() {
        let mut record = vec![String::new(); first_column as usize];
        for (c, cell) in row.iter().enumerate() {
            let value = match cell {
                Data::Empty => String::new(),
                Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
                Data::Int(i) => i.to_string(),
                Data::Float(f) => format!("{f}"),
                Data::Bool(b) => b.to_string(),
                Data::Error(e) => format!("#{e}"),
                Data::DateTime(d) => {
                    let (y, mo, da, h, mi, s, ms) = d.to_ymd_hms_milli();
                    if d.is_duration() {
                        format!("{}", d.as_f64())
                    } else {
                        date_cells.push((first_row as usize + r, first_column as usize + c));
                        if (h, mi, s, ms) == (0, 0, 0, 0) {
                            format!("{y:04}-{mo:02}-{da:02}")
                        } else {
                            format!("{y:04}-{mo:02}-{da:02}T{h:02}:{mi:02}:{s:02}")
                        }
                    }
                }
            };
            record.push(value);
        }
        records.push(record);
    }
    // Drop leading empty rows; dates are counted from the header row.
    let skipped = records
        .iter()
        .take_while(|r| r.iter().all(|f| f.trim().is_empty()))
        .count();
    let records: Vec<Vec<String>> = records.into_iter().skip(skipped).collect();
    let date_cells: Vec<(usize, usize)> = date_cells
        .into_iter()
        .filter(|(r, _)| *r > skipped)
        .map(|(r, c)| (r - skipped - 1, c))
        .collect();
    sheet_from_records(records, &date_cells)
}

/// A worksheet for an `.ods` file. Every cell is text, so spreadsheet
/// programs don't turn IDs or partial dates into numbers or full dates.
pub struct OdsSheet<'a> {
    pub name: &'a str,
    pub headers: &'a [String],
    pub rows: &'a [Vec<String>],
}

fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if c.is_control() && c != '\t' && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// An `OpenDocument` spreadsheet holding `sheets`, header rows in bold.
pub fn write_ods(sheets: &[OdsSheet<'_>]) -> Result<Vec<u8>> {
    use std::fmt::Write as _;
    use zip::write::SimpleFileOptions;
    let mut content = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" office:version="1.2">
<office:automatic-styles>
<number:text-style style:name="Ntext"><number:text-content/></number:text-style>
<style:style style:name="text" style:family="table-cell" style:data-style-name="Ntext"/>
<style:style style:name="head" style:family="table-cell" style:data-style-name="Ntext"><style:text-properties fo:font-weight="bold"/></style:style>
<style:style style:name="col" style:family="table-column"><style:table-column-properties style:column-width="1.2in"/></style:style>
</office:automatic-styles>
<office:body><office:spreadsheet>
"#,
    );
    for sheet in sheets {
        let width = sheet.headers.len().max(1);
        let _ = writeln!(
            content,
            "<table:table table:name=\"{}\"><table:table-column table:style-name=\"col\" table:default-cell-style-name=\"text\" table:number-columns-repeated=\"{width}\"/>",
            xml(sheet.name)
        );
        let row = |cells: &[String], style: &str| {
            let mut out = String::from("<table:table-row>");
            for cell in cells {
                if cell.is_empty() {
                    let _ = write!(out, "<table:table-cell table:style-name=\"{style}\"/>");
                } else {
                    let _ = write!(
                        out,
                        "<table:table-cell table:style-name=\"{style}\" office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>",
                        xml(cell)
                    );
                }
            }
            out.push_str("</table:table-row>\n");
            out
        };
        content.push_str(&row(sheet.headers, "head"));
        for cells in sheet.rows {
            content.push_str(&row(cells, "text"));
        }
        content.push_str("</table:table>\n");
    }
    content.push_str("</office:spreadsheet></office:body></office:document-content>\n");
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>
<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>
</manifest:manifest>
"#;
    let zip_error = |e: zip::result::ZipError| invalid(format!("could not write spreadsheet: {e}"));
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // The mimetype comes first and uncompressed, as OpenDocument requires.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("mimetype", stored).map_err(zip_error)?;
    zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet")?;
    zip.start_file("content.xml", deflated).map_err(zip_error)?;
    zip.write_all(content.as_bytes())?;
    zip.start_file("META-INF/manifest.xml", deflated)
        .map_err(zip_error)?;
    zip.write_all(manifest.as_bytes())?;
    Ok(zip.finish().map_err(zip_error)?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_round_trips_awkward_text() {
        let rows = vec![vec!["a\tb".to_string(), "line\nbreak \\ back".into()]];
        let mut out = Vec::new();
        write_tsv(&mut out, &["one", "two"], &rows).unwrap();
        assert_eq!(
            String::from_utf8(out.clone()).unwrap(),
            "one\ttwo\na\\tb\tline\\nbreak \\\\ back\n"
        );
        let sheet = read_tsv(&out).unwrap();
        assert_eq!(sheet.headers, ["one", "two"]);
        assert_eq!(sheet.rows, rows);
        assert!(read_tsv(b"a\tb\nonly\n").is_err());
        assert!(read_tsv(b"a\n\\x\n").is_err());
    }

    #[test]
    fn delimited_text_handles_quotes() {
        let sheet = read_delimited(
            b"\xEF\xBB\xBFid,name\r\nHV1,\"Caulerpa, \"\"sp\"\"\"\n\nHV2,\"two\nlines\"",
            ',',
        )
        .unwrap();
        assert_eq!(sheet.headers, ["id", "name"]);
        assert_eq!(
            sheet.rows,
            [
                vec!["HV1".to_string(), "Caulerpa, \"sp\"".into()],
                vec!["HV2".into(), "two\nlines".into()]
            ]
        );
        assert!(read_delimited(b"a,b\n\"open", ',').is_err());
        assert!(read_delimited(b"a,a\n1,2", ',').is_err());
    }

    #[test]
    fn ods_template_reads_back_as_text() {
        let headers = vec!["specimen_id".to_string(), "eventDate".into()];
        let rows = vec![
            vec!["0012".to_string(), "2019-05".into()],
            vec!["A<1>".into(), String::new()],
        ];
        let bytes = write_ods(&[OdsSheet {
            name: "Specimens",
            headers: &headers,
            rows: &rows,
        }])
        .unwrap();
        assert!(bytes[30..].starts_with(b"mimetypeapplication/vnd.oasis.opendocument.spreadsheet"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.ods");
        std::fs::write(&path, bytes).unwrap();
        let sheet = read_spreadsheet(&path).unwrap();
        assert_eq!(sheet.headers, headers);
        assert_eq!(sheet.rows, rows);
        assert_eq!(sheet.date_cells, Vec::<(usize, usize)>::new());
    }
}
