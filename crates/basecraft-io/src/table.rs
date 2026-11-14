//! Text tables: Basecraft's own TSV files, spreadsheets read for import
//! (CSV, TSV, `.ods`, `.xlsx`) and the `.ods` template writer.
//!
//! Basecraft TSV has a header row and one line per row. Within a field a
//! backslash, tab, newline or carriage return is written `\\`, `\t`, `\n`
//! or `\r`, so every line has exactly the header's number of fields.
use std::io::Write;
use std::path::Path;

use crate::{Error, Result};

fn unreadable(message: impl Into<String>) -> Error {
    Error::Table(message.into())
}

fn too_large(message: impl Into<String>) -> Error {
    Error::TooLarge(message.into())
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
                return Err(unreadable(format!(
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
    String::from_utf8(bytes.to_vec()).map_err(|_| unreadable(format!("{what} is not UTF-8 text")))
}

/// Read a Basecraft TSV table. Every row must have the header's width.
pub fn read_tsv(bytes: &[u8]) -> Result<Sheet> {
    let text = text(bytes, "table")?;
    let mut lines = text.split('\n').enumerate();
    let header = lines.next().map(|(_, l)| l.strip_suffix('\r').unwrap_or(l));
    let Some(header) = header.filter(|h| !h.is_empty()) else {
        return Err(unreadable("table has no header row"));
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
            return Err(unreadable(format!(
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
        return Err(unreadable("a quoted field is not closed"));
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
        .ok_or_else(|| unreadable("spreadsheet is empty"))?
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect();
    // Columns after the last header are not part of the table.
    while headers.last().is_some_and(String::is_empty) {
        headers.pop();
    }
    if headers.is_empty() {
        return Err(unreadable(
            "spreadsheet has no column headers in its first row",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for header in &headers {
        if !header.is_empty() && !seen.insert(header.as_str()) {
            return Err(unreadable(format!("column header {header:?} appears twice")));
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

/// The first worksheet of an `.ods`, `.xlsx` or `.xlsm` file, or
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
        "ods" | "xlsx" | "xlsm" => read_workbook(path, &ext),
        _ => Err(unreadable(format!(
            "{} is not a spreadsheet Basecraft reads (.ods, .xlsx, .csv or .tsv)",
            path.display()
        ))),
    }
}

/// Cells of one worksheet: (row, column) to text, and which held dates.
#[derive(Default)]
struct Cells {
    rows: Vec<Vec<String>>,
    dates: Vec<(usize, usize)>,
    width: usize,
    text_bytes: u64,
}

impl Cells {
    const LIMIT: usize = 10_000_000;

    fn set(&mut self, row: usize, column: usize, text: String, date: bool) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let width = self.width.max(column.saturating_add(1));
        let height = self.rows.len().max(row.saturating_add(1));
        self.text_bytes = self.text_bytes.saturating_add(text.len() as u64);
        if height.saturating_mul(width) > Self::LIMIT
            || column >= 16_384
            || self.text_bytes > crate::archive::MEMBER_LIMIT
        {
            return Err(too_large("the spreadsheet is too large to import"));
        }
        self.width = width;
        if self.rows.len() <= row {
            self.rows.resize(row + 1, Vec::new());
        }
        let cells = &mut self.rows[row];
        if cells.len() <= column {
            cells.resize(column + 1, String::new());
        }
        cells[column] = text;
        if date {
            self.dates.push((row, column));
        }
        Ok(())
    }

    /// Drop leading empty rows: the first row with content is the header.
    fn into_sheet(self) -> Result<Sheet> {
        let skipped = self
            .rows
            .iter()
            .take_while(|r| r.iter().all(|f| f.trim().is_empty()))
            .count();
        let dates: Vec<(usize, usize)> = self
            .dates
            .into_iter()
            .filter(|(r, _)| *r > skipped)
            .map(|(r, c)| (r - skipped - 1, c))
            .collect();
        let records: Vec<Vec<String>> = self.rows.into_iter().skip(skipped).collect();
        sheet_from_records(records, &dates)
    }
}

/// Calamine owns workbook relationships, strings, styles and cell decoding.
/// The adapter retains Basecraft's first-sheet/header and date-warning rules.
fn read_workbook(path: &Path, ext: &str) -> Result<Sheet> {
    use calamine::{Ods, Reader, Xlsx};
    use std::io::{Cursor, Read};

    let result = || -> Result<Sheet> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(crate::archive::MEMBER_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > crate::archive::MEMBER_LIMIT {
            return Err(too_large("the spreadsheet file is too large to import"));
        }
        let mut archive = crate::archive::Archive::new(&bytes)?;
        archive.validate()?;
        let mut cells = Cells::default();
        if ext == "ods" {
            let content = archive
                .read("content.xml")?
                .ok_or_else(|| unreadable("the .ods file has no content.xml"))?;
            preflight_ods(&content)?;
            let mut workbook = Ods::new(Cursor::new(&bytes)).map_err(workbook_error)?;
            let name = workbook
                .sheet_names()
                .first()
                .cloned()
                .ok_or_else(|| unreadable("the workbook has no sheets"))?;
            let range = workbook.worksheet_range(&name).map_err(workbook_error)?;
            if let Some((row, column)) = range.start() {
                for (r, c, value) in range.used_cells() {
                    let (text, date) = cell_text(value.clone())?;
                    cells.set(row as usize + r, column as usize + c, text, date)?;
                }
            }
        } else {
            let mut workbook = Xlsx::new(Cursor::new(&bytes)).map_err(workbook_error)?;
            let name = workbook
                .sheet_names()
                .first()
                .cloned()
                .ok_or_else(|| unreadable("the workbook has no sheets"))?;
            let mut reader = workbook
                .worksheet_cells_reader(&name)
                .map_err(workbook_error)?;
            while let Some(cell) = reader.next_cell().map_err(workbook_error)? {
                let (row, column) = cell.get_position();
                let (text, date) = cell_text(calamine::Data::from(cell.get_value().clone()))?;
                cells.set(row as usize, column as usize, text, date)?;
            }
        }
        cells.into_sheet()
    };
    let context = |e: &dyn std::fmt::Display| format!("could not read {}: {e}", path.display());
    result().map_err(|e| match e {
        Error::TooLarge(m) => Error::TooLarge(context(&m)),
        Error::Io(io) => Error::Io(std::io::Error::new(io.kind(), context(&io))),
        other => Error::Table(context(&other)),
    })
}

fn workbook_error(error: impl std::fmt::Display) -> Error {
    unreadable(format!("spreadsheet could not be decoded: {error}"))
}

fn cell_text(value: calamine::Data) -> Result<(String, bool)> {
    use calamine::Data;
    Ok(match value {
        Data::Empty => (String::new(), false),
        Data::String(text) | Data::DurationIso(text) => (text, false),
        Data::DateTimeIso(mut text) => {
            if text.ends_with("T00:00:00") {
                text.truncate(text.len() - "T00:00:00".len());
            }
            (text, true)
        }
        Data::DateTime(value) => {
            let serial = value.as_f64();
            if !serial.is_finite() || !(0.0..3_000_000.0).contains(&serial) {
                return Err(unreadable("a spreadsheet date is outside the supported range"));
            }
            let date = value
                .as_datetime()
                .ok_or_else(|| unreadable("a spreadsheet date cannot be represented"))?;
            let text = format!("{date:?}");
            (
                text.strip_suffix("T00:00:00").unwrap_or(&text).to_string(),
                true,
            )
        }
        // Calamine's error display already includes Excel's leading '#'.
        other => (other.to_string(), false),
    })
}

/// ODS is eager in calamine. Bound every sheet (including formulas), repeated
/// text and whitespace before handing it over. This inspects allocation sizes;
/// cell values and workbook semantics are decoded only by calamine.
fn preflight_ods(bytes: &[u8]) -> Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| unreadable("content.xml is not UTF-8"))?;
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            nodes_limit: 20_000_000,
            ..Default::default()
        },
    )
    .map_err(|e| unreadable(format!("content.xml could not be read: {e}")))?;
    let mut total_cells = 0usize;
    let mut total_text = 0u64;
    let mut stored_cells = 0usize;
    for table in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "table")
    {
        let mut rows = 0usize;
        let mut width = 0usize;
        let mut used_rows = 0usize;
        for row in table
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "table-row")
        {
            let repeat = ods_repeat(row, "number-rows-repeated")?;
            rows = rows.saturating_add(repeat);
            let mut column = 0usize;
            let mut row_stored = 0usize;
            for cell in row.children().filter(|n| {
                n.is_element() && matches!(n.tag_name().name(), "table-cell" | "covered-table-cell")
            }) {
                let previous_column = column;
                let span = ods_repeat(cell, "number-columns-repeated")?;
                column = column.saturating_add(span);
                let mut text_size = 0u64;
                for node in cell.descendants() {
                    if node.is_text() {
                        text_size =
                            text_size.saturating_add(node.text().unwrap_or_default().len() as u64);
                    } else if node.is_element() && node.tag_name().name() == "s" {
                        text_size = text_size.saturating_add(ods_repeat(node, "c")? as u64);
                    }
                }
                for attribute in cell.attributes() {
                    text_size = text_size.saturating_add(attribute.value().len() as u64);
                }
                // An untyped empty cell may still have style attributes.
                let used = cell.attributes().any(|a| {
                    matches!(
                        a.name(),
                        "value"
                            | "string-value"
                            | "date-value"
                            | "time-value"
                            | "boolean-value"
                            | "formula"
                    )
                }) || cell
                    .descendants()
                    .any(|n| n.is_text() && !n.text().unwrap_or_default().is_empty())
                    || cell
                        .descendants()
                        .any(|n| n.is_element() && n.tag_name().name() == "s");
                row_stored = if used { column } else { previous_column }.min(16_384);
                if used {
                    if column > 16_384 || rows > 1_048_576 {
                        return Err(too_large("the spreadsheet is too large to import"));
                    }
                    width = width.max(column);
                    used_rows = rows;
                    total_text = total_text.saturating_add(
                        text_size
                            .saturating_mul(span as u64)
                            .saturating_mul(repeat as u64),
                    );
                }
                if text_size > crate::archive::MEMBER_LIMIT
                    || total_text > crate::archive::MEMBER_LIMIT
                {
                    return Err(too_large(
                        "the spreadsheet's expanded text is too large to import",
                    ));
                }
            }
            stored_cells = stored_cells.saturating_add(row_stored);
            if stored_cells > Cells::LIMIT || used_rows.saturating_mul(width) > Cells::LIMIT {
                return Err(too_large("the spreadsheet is too large to import"));
            }
        }
        total_cells = total_cells.saturating_add(used_rows.saturating_mul(width));
        if total_cells > Cells::LIMIT {
            return Err(too_large("the spreadsheet is too large to import"));
        }
    }
    Ok(())
}

fn ods_repeat(node: roxmltree::Node<'_, '_>, name: &str) -> Result<usize> {
    node.attributes()
        .find(|a| a.name() == name)
        .map_or(Ok(1), |a| {
            a.value()
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| unreadable(format!("invalid ODS {name}")))
        })
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
    let mut zip = crate::archive::Writer::default();
    // The mimetype comes first and uncompressed, as OpenDocument requires.
    zip.add(
        "mimetype",
        b"application/vnd.oasis.opendocument.spreadsheet",
        true,
    )?;
    zip.add("content.xml", content.as_bytes(), false)?;
    zip.add("META-INF/manifest.xml", manifest.as_bytes(), false)?;
    zip.finish()
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

    fn xlsx_fixture(sheet_xml: &str, date1904: bool) -> Vec<u8> {
        let mut zip = crate::archive::Writer::default();
        let part = |zip: &mut crate::archive::Writer, name: &str, xml: &str| {
            zip.add(name, xml.as_bytes(), false).unwrap();
        };
        part(
            &mut zip,
            "_rels/.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId0" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        );
        part(
            &mut zip,
            "xl/workbook.xml",
            &format!(
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="{}"/><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
                u8::from(date1904)
            ),
        );
        part(
            &mut zip,
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        );
        part(
            &mut zip,
            "xl/sharedStrings.xml",
            "<sst><si><t>specimen_id</t></si><si><t>eventDate</t></si><si><r><t>HV</t></r><r><t>1</t></r></si></sst>",
        );
        part(
            &mut zip,
            "xl/styles.xml",
            r#"<styleSheet><cellXfs><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#,
        );
        part(&mut zip, "xl/worksheets/sheet1.xml", sheet_xml);
        zip.finish().unwrap()
    }

    fn read_fixture(bytes: &[u8], extension: &str) -> Result<Sheet> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("import.{extension}"));
        std::fs::write(&path, bytes).unwrap();
        let result = read_spreadsheet(&path);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        result
    }

    #[test]
    fn xlsx_cells_dates_and_shared_strings() {
        let bytes = xlsx_fixture(
            r#"<worksheet><sheetData><row r="2"><c r="B2" t="s"><v>0</v></c><c r="C2" t="s"><v>1</v></c><c r="D2" t="inlineStr"><is><t>n</t></is></c></row><row r="3"><c r="B3" t="s"><v>2</v></c><c r="C3" s="1"><v>43597</v></c><c r="D3"><v>12.5</v></c></row></sheetData></worksheet>"#,
            false,
        );
        let sheet = read_fixture(&bytes, "xlsx").unwrap();
        assert_eq!(sheet.headers, ["", "specimen_id", "eventDate", "n"]);
        assert_eq!(
            sheet.rows,
            [vec![
                String::new(),
                "HV1".into(),
                "2019-05-12".into(),
                "12.5".into()
            ]]
        );
        assert_eq!(sheet.date_cells, [(0, 2)]);
    }

    #[test]
    fn libreoffice_files_read_alike() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/fixtures/metadata");
        let csv = read_spreadsheet(&dir.join("specimens.csv")).unwrap();
        for name in ["specimens.ods", "specimens.xlsx"] {
            let sheet = read_spreadsheet(&dir.join(name)).unwrap();
            assert_eq!(sheet.headers, csv.headers, "{name}");
            assert_eq!(sheet.rows[0][1], "Codium fragile", "{name}");
            assert_eq!(sheet.rows[0][2], "2019-05-12", "{name}");
            assert_eq!(sheet.rows[0][3], "-38.1", "{name}");
            // The spreadsheet program made the voucher number a number.
            assert_eq!(sheet.rows[0][4], "12", "{name}");
            assert_eq!(sheet.rows[0][5], "Port Phillip Bay, Victoria", "{name}");
            assert_eq!(sheet.rows[1][2], "not collected", "{name}");
            assert_eq!(sheet.date_cells, [(0, 2)], "{name}");
        }
    }

    #[test]
    fn ods_repeats_groups_and_dates() {
        let content = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><table:table table:name="S"><table:table-header-rows><table:table-row><table:table-cell office:value-type="string"><text:p>id</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>date</text:p></table:table-cell><table:table-cell table:number-columns-repeated="1000"/></table:table-row></table:table-header-rows><table:table-row table:number-rows-repeated="2"><table:table-cell office:value-type="string"><text:p>A<text:s text:c="2"/>B</text:p></table:table-cell><table:table-cell office:value-type="date" office:date-value="2019-05-12T00:00:00"/></table:table-row><table:table-row table:number-rows-repeated="1048570"><table:table-cell table:number-columns-repeated="1024"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#;
        let mut zip = crate::archive::Writer::default();
        zip.add(
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
            true,
        )
        .unwrap();
        zip.add("content.xml", content.as_bytes(), false).unwrap();
        zip.add("META-INF/manifest.xml", b"<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\"/>", false).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.ods");
        std::fs::write(&path, zip.finish().unwrap()).unwrap();
        let sheet = read_spreadsheet(&path).unwrap();
        assert_eq!(sheet.headers, ["id", "date"]);
        assert_eq!(
            sheet.rows,
            [
                vec!["A  B".to_string(), "2019-05-12".into()],
                vec!["A  B".to_string(), "2019-05-12".into()]
            ]
        );
        assert_eq!(sheet.date_cells, [(0, 1), (1, 1)]);
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

    #[test]
    fn xlsx_keeps_text_ids_holes_and_date_positions_after_blank_rows() {
        let bytes = xlsx_fixture(
            r#"<worksheet><dimension ref="A1:XFD1048576"/><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>1</v></c></row>
            <row r="2"><c r="A2" t="inlineStr"><is><t>0012</t></is></c><c r="C2" t="d"><v>2019-05-12T00:00:00</v></c></row>
            <row r="4"><c r="A4" t="inlineStr"><is><t>0003</t></is></c><c r="C4" s="1"><v>43597.5</v></c></row>
            </sheetData></worksheet>"#,
            false,
        );
        let sheet = read_fixture(&bytes, "xlsx").unwrap();
        assert_eq!(sheet.headers, ["specimen_id", "", "eventDate"]);
        assert_eq!(
            sheet.rows,
            [
                vec!["0012".to_string(), String::new(), "2019-05-12".into()],
                vec![
                    "0003".to_string(),
                    String::new(),
                    "2019-05-12T12:00:00".into()
                ],
            ]
        );
        assert_eq!(sheet.date_cells, [(0, 2), (1, 2)]);
    }

    #[test]
    fn xlsx_1904_epoch_is_respected() {
        let bytes = xlsx_fixture(
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>1</v></c></row><row r="2"><c r="A2" s="1"><v>42135</v></c></row></sheetData></worksheet>"#,
            true,
        );
        let sheet = read_fixture(&bytes, "xlsx").unwrap();
        assert_eq!(sheet.rows, [vec!["2019-05-12".to_string()]]);
        assert_eq!(sheet.date_cells, [(0, 0)]);
    }

    #[test]
    fn malformed_and_oversized_workbooks_give_contextual_errors() {
        let error = read_fixture(b"not a zip", "xlsx").unwrap_err().to_string();
        assert!(error.contains("import.xlsx"), "{error}");
        let bytes = xlsx_fixture(
            r#"<worksheet><sheetData><row r="1"><c r="XFD1" t="s"><v>0</v></c></row><row r="1048576"><c r="A1048576" t="s"><v>2</v></c></row></sheetData></worksheet>"#,
            false,
        );
        let error = read_fixture(&bytes, "xlsx").unwrap_err();
        assert!(matches!(error, Error::TooLarge(_)), "{error}");
        assert!(error.to_string().contains("import.xlsx"), "{error}");
        assert!(read_fixture(&xlsx_fixture("<worksheet>", false), "xlsx").is_err());
        // Library support for other Excel formats does not expand the API.
        assert!(read_fixture(b"", "xls").is_err());
    }

    #[test]
    fn ods_preflight_rejects_expansion_before_library_allocation() {
        let wrap = |body: &str| {
            format!(
                r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><table:table table:name="S">{body}</table:table></office:document-content>"#
            )
        };
        for body in [
            r#"<table:table-row table:number-rows-repeated="1000"><table:table-cell table:number-columns-repeated="10001" office:value="1"/></table:table-row>"#,
            r#"<table:table-row><table:table-cell office:value-type="string"><text:p><text:s text:c="268435457"/></text:p></table:table-cell></table:table-row>"#,
            r#"<table:table-row><table:table-cell table:number-columns-repeated="16385" office:value="1"/></table:table-row>"#,
            r#"<table:table-row table:number-rows-repeated="0"/>"#,
            r#"<table:table-row table:number-rows-repeated="18446744073709551616"/>"#,
        ] {
            assert!(preflight_ods(wrap(body).as_bytes()).is_err(), "{body}");
        }
        assert!(preflight_ods(b"<broken>").is_err());
        // A later sheet is read eagerly too: limits must include it.
        let body = r#"<table:table-row table:number-rows-repeated="1000"><table:table-cell table:number-columns-repeated="6000" office:value="1"/></table:table-row>"#;
        let mut two_sheets = wrap(body);
        two_sheets = two_sheets.replace(
            "</office:document-content>",
            &format!(
                "<table:table table:name=\"Second\">{body}</table:table></office:document-content>"
            ),
        );
        assert!(preflight_ods(two_sheets.as_bytes()).is_err());
    }

    #[test]
    fn ods_imports_first_sheet_and_preserves_text_and_empty_columns() {
        let headers = vec!["id".into(), "empty".into(), "notes".into()];
        let rows = vec![vec!["0007".into(), String::new(), "a & b\nnext".into()]];
        let first = OdsSheet {
            name: "Z first",
            headers: &headers,
            rows: &rows,
        };
        let second_headers = vec!["other".into()];
        let second = OdsSheet {
            name: "A second",
            headers: &second_headers,
            rows: &[],
        };
        let bytes = write_ods(&[first, second]).unwrap();
        let sheet = read_fixture(&bytes, "ods").unwrap();
        assert_eq!(sheet.headers, headers);
        assert_eq!(sheet.rows, rows);
        assert_eq!(sheet.date_cells, [] as [(usize, usize); 0]);
    }
}
