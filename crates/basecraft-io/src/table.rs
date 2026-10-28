//! Text tables: Basecraft's own TSV files, spreadsheets read for import
//! (CSV, TSV, `.ods`, `.xlsx`) and the `.ods` template writer.
//!
//! Basecraft TSV has a header row and one line per row. Within a field a
//! backslash, tab, newline or carriage return is written `\\`, `\t`, `\n`
//! or `\r`, so every line has exactly the header's number of fields.
use std::io::Write;
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
        "ods" | "xlsx" | "xlsm" => read_workbook(path, &ext),
        _ => Err(invalid(format!(
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
}

impl Cells {
    const LIMIT: usize = 10_000_000;

    fn set(&mut self, row: usize, column: usize, text: String, date: bool) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if row.saturating_mul(column.max(1)) > Self::LIMIT || column > 16_384 {
            return Err(invalid("the spreadsheet is too large to import"));
        }
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

fn xml_document(bytes: &[u8], part: &str) -> Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| invalid(format!("{part} is not UTF-8")))
}

fn parse_xml<'a>(text: &'a str, part: &str) -> Result<roxmltree::Document<'a>> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            nodes_limit: 20_000_000,
            ..Default::default()
        },
    )
    .map_err(|e| invalid(format!("{part} could not be read: {e}")))
}

/// An attribute by local name, whatever its namespace prefix.
fn attr<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

fn named<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    name: &'static str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> {
    node.children()
        .filter(move |n| n.is_element() && n.tag_name().name() == name)
}

/// The first worksheet of an `.ods` or `.xlsx` file.
fn read_workbook(path: &Path, ext: &str) -> Result<Sheet> {
    let bytes = std::fs::read(path)?;
    let archive = crate::archive::Archive::new(&bytes)
        .map_err(|e| invalid(format!("could not read {}: {e}", path.display())))?;
    let cells = if ext == "ods" {
        read_ods(&archive)?
    } else {
        read_xlsx(&archive)?
    };
    cells.into_sheet()
}

fn ods_text(node: roxmltree::Node<'_, '_>, out: &mut String) {
    for child in node.children() {
        if child.is_text() {
            out.push_str(child.text().unwrap_or_default());
            continue;
        }
        match child.tag_name().name() {
            "s" => {
                let count: usize = attr(child, "c").and_then(|c| c.parse().ok()).unwrap_or(1);
                out.extend(std::iter::repeat_n(' ', count.min(1000)));
            }
            "tab" => out.push('\t'),
            "line-break" => out.push('\n'),
            "annotation" => {}
            _ => ods_text(child, out),
        }
    }
}

/// A sheet's rows in order; rows can sit in row groups and header rows.
fn ods_rows<'a, 'i>(node: roxmltree::Node<'a, 'i>, rows: &mut Vec<roxmltree::Node<'a, 'i>>) {
    for child in node.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "table-row" => rows.push(child),
            "table-row-group" | "table-header-rows" | "table-rows" => ods_rows(child, rows),
            _ => {}
        }
    }
}

fn read_ods(archive: &crate::archive::Archive<'_>) -> Result<Cells> {
    let content = archive
        .read("content.xml")?
        .ok_or_else(|| invalid("the .ods file has no content.xml"))?;
    let text = xml_document(&content, "content.xml")?;
    let doc = parse_xml(&text, "content.xml")?;
    let table = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "table")
        .ok_or_else(|| invalid("the .ods file has no sheet"))?;
    let mut rows = Vec::new();
    ods_rows(table, &mut rows);
    let mut cells = Cells::default();
    let mut row_index = 0usize;
    for row in rows {
        let repeat: usize = attr(row, "number-rows-repeated")
            .and_then(|r| r.parse().ok())
            .unwrap_or(1);
        let mut values = Vec::new();
        let mut column = 0usize;
        for cell in row.children().filter(|n| {
            n.is_element() && matches!(n.tag_name().name(), "table-cell" | "covered-table-cell")
        }) {
            let span: usize = attr(cell, "number-columns-repeated")
                .and_then(|r| r.parse().ok())
                .unwrap_or(1);
            let (value, date) = match attr(cell, "value-type") {
                Some("float" | "percentage" | "currency") => {
                    (attr(cell, "value").unwrap_or_default().to_string(), false)
                }
                Some("date") => {
                    let value = attr(cell, "date-value").unwrap_or_default();
                    (
                        value.strip_suffix("T00:00:00").unwrap_or(value).to_string(),
                        true,
                    )
                }
                Some("time") => (
                    attr(cell, "time-value").unwrap_or_default().to_string(),
                    false,
                ),
                Some("boolean") => (
                    attr(cell, "boolean-value").unwrap_or_default().to_string(),
                    false,
                ),
                _ => {
                    let mut text = String::new();
                    for (i, p) in named(cell, "p").enumerate() {
                        if i > 0 {
                            text.push('\n');
                        }
                        ods_text(p, &mut text);
                    }
                    (text, false)
                }
            };
            if value.is_empty() {
                column = column.saturating_add(span);
            } else {
                for _ in 0..span.min(16_384) {
                    values.push((column, value.clone(), date));
                    column += 1;
                }
            }
        }
        if values.is_empty() {
            // Empty rows only separate content; long repeats are padding.
            row_index = row_index.saturating_add(repeat.min(1));
            continue;
        }
        for _ in 0..repeat.min(10_000) {
            for (column, value, date) in &values {
                cells.set(row_index, *column, value.clone(), *date)?;
            }
            row_index += 1;
        }
    }
    Ok(cells)
}

/// Column index of an `A1` reference.
fn column_of(reference: &str) -> Option<usize> {
    let letters: String = reference
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n = 0usize;
    for c in letters.to_ascii_uppercase().bytes() {
        n = n * 26 + usize::from(c - b'A') + 1;
    }
    Some(n - 1)
}

/// Days since 1970-01-01 to a civil date (proleptic Gregorian).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// An Excel serial date as ISO 8601.
fn excel_date(serial: f64, date1904: bool) -> Option<String> {
    if !serial.is_finite() || !(0.0..3_000_000.0).contains(&serial) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let whole = serial.floor() as i64;
    // 1900 system: serial 25569 is 1970-01-01 (Excel's 1900 leap-day bug
    // only affects dates before March 1900).
    let days = if date1904 {
        whole - 24_107
    } else {
        whole - 25_569
    };
    let (y, m, d) = civil(days);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let seconds = ((serial - serial.floor()) * 86_400.0).round() as u32;
    Some(if seconds == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    })
}

/// Whether a number format shows a date: built-in date formats, or a
/// custom code with day, month or year outside quotes and brackets.
fn date_format(id: u32, code: Option<&str>) -> bool {
    if matches!(id, 14..=22 | 45..=47) {
        return true;
    }
    let Some(code) = code else { return false };
    let mut quoted = false;
    let mut bracket = false;
    for c in code.chars() {
        match c {
            '"' => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            'd' | 'D' | 'm' | 'M' | 'y' | 'Y' if !quoted && !bracket => return true,
            _ => {}
        }
    }
    false
}

fn xlsx_text(node: roxmltree::Node<'_, '_>) -> String {
    let mut out = String::new();
    for t in node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "t")
    {
        if t.ancestors().any(|a| a.tag_name().name() == "rPh") {
            continue;
        }
        out.push_str(t.text().unwrap_or_default());
    }
    out
}

fn read_xlsx(archive: &crate::archive::Archive<'_>) -> Result<Cells> {
    let part = |name: &str| -> Result<Option<String>> {
        archive
            .read(name)?
            .map(|bytes| xml_document(&bytes, name))
            .transpose()
    };
    let workbook =
        part("xl/workbook.xml")?.ok_or_else(|| invalid("the .xlsx file has no workbook"))?;
    let workbook = parse_xml(&workbook, "workbook.xml")?;
    let date1904 = workbook
        .descendants()
        .find(|n| n.tag_name().name() == "workbookPr")
        .and_then(|n| attr(n, "date1904"))
        .is_some_and(|v| v == "1" || v == "true");
    let sheet = workbook
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "sheet")
        .ok_or_else(|| invalid("the workbook has no sheets"))?;
    let id = attr(sheet, "id").ok_or_else(|| invalid("the first sheet has no relationship"))?;
    let rels = part("xl/_rels/workbook.xml.rels")?
        .ok_or_else(|| invalid("the .xlsx file has no workbook relationships"))?;
    let rels = parse_xml(&rels, "workbook.xml.rels")?;
    let target = rels
        .descendants()
        .find(|n| n.tag_name().name() == "Relationship" && attr(*n, "Id") == Some(id))
        .and_then(|n| attr(n, "Target"))
        .ok_or_else(|| invalid("the first sheet's part is missing"))?;
    let target = target
        .strip_prefix('/')
        .map_or_else(|| format!("xl/{target}"), str::to_string);
    let shared: Vec<String> = match part("xl/sharedStrings.xml")? {
        Some(text) => parse_xml(&text, "sharedStrings.xml")?
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "si")
            .map(xlsx_text)
            .collect(),
        None => Vec::new(),
    };
    let mut date_styles = Vec::new();
    if let Some(text) = part("xl/styles.xml")? {
        let styles = parse_xml(&text, "styles.xml")?;
        let custom: std::collections::BTreeMap<u32, &str> = styles
            .descendants()
            .filter(|n| n.tag_name().name() == "numFmt")
            .filter_map(|n| Some((attr(n, "numFmtId")?.parse().ok()?, attr(n, "formatCode")?)))
            .collect();
        if let Some(xfs) = styles
            .descendants()
            .find(|n| n.tag_name().name() == "cellXfs")
        {
            for xf in named(xfs, "xf") {
                let id: u32 = attr(xf, "numFmtId")
                    .and_then(|i| i.parse().ok())
                    .unwrap_or(0);
                date_styles.push(date_format(id, custom.get(&id).copied()));
            }
        }
    }
    let sheet = part(&target)?.ok_or_else(|| invalid(format!("{target} is missing")))?;
    let sheet = parse_xml(&sheet, &target)?;
    let mut cells = Cells::default();
    let mut next_row = 0usize;
    for row in sheet
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "row")
    {
        let index = attr(row, "r")
            .and_then(|r| r.parse::<usize>().ok())
            .map_or(next_row, |r| r.saturating_sub(1));
        next_row = index + 1;
        let mut next_column = 0usize;
        for cell in named(row, "c") {
            let column = attr(cell, "r").and_then(column_of).unwrap_or(next_column);
            next_column = column + 1;
            let value = named(cell, "v")
                .next()
                .and_then(|v| v.text())
                .unwrap_or_default();
            let mut date = false;
            let text = match attr(cell, "t") {
                Some("s") => value
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| shared.get(i).cloned())
                    .unwrap_or_default(),
                Some("inlineStr") => named(cell, "is").next().map(xlsx_text).unwrap_or_default(),
                Some("b") => (if value == "1" { "true" } else { "false" }).to_string(),
                Some("e") => format!("#{value}"),
                Some("str" | "d") => value.to_string(),
                _ => {
                    let style: usize = attr(cell, "s").and_then(|s| s.parse().ok()).unwrap_or(0);
                    match value.parse::<f64>() {
                        Ok(serial) if date_styles.get(style).copied().unwrap_or(false) => {
                            date = true;
                            excel_date(serial, date1904).unwrap_or_else(|| value.to_string())
                        }
                        Ok(number) => format!("{number}"),
                        Err(_) => value.to_string(),
                    }
                }
            };
            cells.set(index, column, text, date)?;
        }
    }
    Ok(cells)
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

    #[test]
    fn xlsx_cells_dates_and_shared_strings() {
        let mut zip = crate::archive::Writer::default();
        let part = |zip: &mut crate::archive::Writer, name: &str, xml: &str| {
            zip.add(name, xml.as_bytes(), false).unwrap();
        };
        part(
            &mut zip,
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        );
        part(
            &mut zip,
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#,
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
        part(
            &mut zip,
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData><row r="2"><c r="B2" t="s"><v>0</v></c><c r="C2" t="s"><v>1</v></c><c r="D2" t="inlineStr"><is><t>n</t></is></c></row><row r="3"><c r="B3" t="s"><v>2</v></c><c r="C3" s="1"><v>43597</v></c><c r="D3"><v>12.5</v></c></row></sheetData></worksheet>"#,
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.xlsx");
        std::fs::write(&path, zip.finish().unwrap()).unwrap();
        let sheet = read_spreadsheet(&path).unwrap();
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
        let content = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><table:table><table:table-header-rows><table:table-row><table:table-cell office:value-type="string"><text:p>id</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>date</text:p></table:table-cell><table:table-cell table:number-columns-repeated="1000"/></table:table-row></table:table-header-rows><table:table-row table:number-rows-repeated="2"><table:table-cell office:value-type="string"><text:p>A<text:s text:c="2"/>B</text:p></table:table-cell><table:table-cell office:value-type="date" office:date-value="2019-05-12T00:00:00"/></table:table-row><table:table-row table:number-rows-repeated="1048570"><table:table-cell table:number-columns-repeated="1024"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#;
        let mut zip = crate::archive::Writer::default();
        zip.add(
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
            true,
        )
        .unwrap();
        zip.add("content.xml", content.as_bytes(), false).unwrap();
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
}
