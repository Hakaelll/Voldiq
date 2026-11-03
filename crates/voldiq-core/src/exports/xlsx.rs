use super::{
    ReportFormat, ReportSelection, StreamOutcome, StreamProgress,
    stream::{Record, walk},
};
use crate::{Error, Kind, Result, index::Index};
use std::{
    io::{Seek, Write},
    sync::atomic::{AtomicBool, Ordering},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const ROW_LIMIT: u32 = 1_048_576;
const PRECISE_INTEGER: u64 = 999_999_999_999_999;
const COLUMNS: [&str; 10] = [
    "node_id",
    "kind",
    "name",
    "logical_bytes",
    "recorded_allocated_bytes",
    "allocation_known",
    "modified_unix_seconds",
    "path",
    "path_exact",
    "incomplete",
];

/// Streams fixed Open XML parts and inline strings; never constructs a formula or external link.
pub fn stream_xlsx<W: Write + Seek>(
    index: &Index,
    selection: &ReportSelection,
    writer: W,
    cancelled: &AtomicBool,
    progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(StreamOutcome::Cancelled(StreamProgress {
            inspected: 0,
            rows: 0,
            total_entries: index.entries.len().saturating_sub(1),
        }));
    }
    let mut book = Book {
        zip: ZipWriter::new(writer),
        sheets: 0,
        row: 0,
        scope: serde_json::Value::Null,
    };
    let result = walk(
        index,
        selection,
        ReportFormat::Xlsx,
        cancelled,
        progress,
        |record| {
            match record {
                Record::Scope(scope) => {
                    book.scope = scope;
                    book.start_data()?;
                }
                Record::Item(entry) => {
                    let details = if selection.include_paths {
                        let Some(details) = index
                            .details_cancellable(entry.id, || cancelled.load(Ordering::Relaxed))?
                        else {
                            return Ok(false);
                        };
                        Some(details)
                    } else {
                        None
                    };
                    if book.row == ROW_LIMIT {
                        book.end_data()?;
                        book.start_data()?;
                    }
                    book.row += 1;
                    write!(book.zip, "<row r=\"{}\">", book.row)?;
                    let row = book.row;
                    let kind = match entry.kind {
                        Kind::File => "file",
                        Kind::Directory => "directory",
                        Kind::Link => "link",
                    };
                    number(&mut book.zip, 'A', row, Some(entry.id.0.into()))?;
                    string(&mut book.zip, 'B', row, kind, false)?;
                    string(&mut book.zip, 'C', row, &entry.name, false)?;
                    number(&mut book.zip, 'D', row, Some(entry.logical))?;
                    number(
                        &mut book.zip,
                        'E',
                        row,
                        (entry.allocation_known || entry.allocated > 0).then_some(entry.allocated),
                    )?;
                    boolean(&mut book.zip, 'F', row, Some(entry.allocation_known))?;
                    number(&mut book.zip, 'G', row, entry.modified)?;
                    if let Some(details) = details {
                        string(&mut book.zip, 'H', row, &details.path, false)?;
                        boolean(&mut book.zip, 'I', row, Some(details.path_exact))?;
                    }
                    boolean(&mut book.zip, 'J', row, Some(entry.incomplete))?;
                    book.zip.write_all(b"</row>")?;
                }
                Record::Complete(state) => {
                    book.end_data()?;
                    book.metadata(state)?;
                }
            }
            Ok(true)
        },
    )?;
    if matches!(result, StreamOutcome::Complete(_)) {
        book.finish()?;
    }
    Ok(result)
}

struct Book<W: Write + Seek> {
    zip: ZipWriter<W>,
    sheets: usize,
    row: u32,
    scope: serde_json::Value,
}
impl<W: Write + Seek> Book<W> {
    fn part(&mut self, name: &str) -> Result<()> {
        self.zip
            .start_file(
                name,
                SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Deflated)
                    .compression_level(Some(3))
                    .large_file(true),
            )
            .map_err(archive_error)?;
        Ok(())
    }
    fn start_data(&mut self) -> Result<()> {
        self.sheets += 1;
        self.row = 1;
        self.part(&format!("xl/worksheets/sheet{}.xml", self.sheets + 1))?;
        write!(
            self.zip,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"{NS}\"><sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews><cols><col min=\"1\" max=\"2\" width=\"15\" customWidth=\"1\"/><col min=\"3\" max=\"3\" width=\"48\" customWidth=\"1\"/><col min=\"4\" max=\"7\" width=\"26\" customWidth=\"1\"/><col min=\"8\" max=\"8\" width=\"80\" customWidth=\"1\"/><col min=\"9\" max=\"10\" width=\"18\" customWidth=\"1\"/></cols><sheetData><row r=\"1\">"
        )?;
        for (position, title) in COLUMNS.iter().enumerate() {
            string(
                &mut self.zip,
                char::from(b'A' + position as u8),
                1,
                title,
                true,
            )?;
        }
        self.zip.write_all(b"</row>")?;
        Ok(())
    }
    fn end_data(&mut self) -> Result<()> {
        write!(
            self.zip,
            "</sheetData><autoFilter ref=\"A1:J{}\"/></worksheet>",
            self.row
        )?;
        Ok(())
    }
    fn metadata(&mut self, progress: StreamProgress) -> Result<()> {
        self.part("xl/worksheets/sheet1.xml")?;
        write!(
            self.zip,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"{NS}\"><cols><col min=\"1\" max=\"1\" width=\"28\" customWidth=\"1\"/><col min=\"2\" max=\"2\" width=\"110\" customWidth=\"1\"/></cols><sheetData><row r=\"1\">"
        )?;
        string(&mut self.zip, 'A', 1, "Field", true)?;
        string(&mut self.zip, 'B', 1, "Value", true)?;
        self.zip.write_all(b"</row>")?;
        self.scope["emitted_rows"] = progress.rows.into();
        self.scope["inspected_records"] = progress.inspected.into();
        self.scope["complete"] = true.into();
        let fields = self
            .scope
            .as_object()
            .ok_or_else(|| Error::Invalid("Missing XLSX scope metadata.".into()))?;
        for (position, (key, value)) in fields.iter().enumerate() {
            let row = position as u32 + 2;
            write!(self.zip, "<row r=\"{row}\">")?;
            string(&mut self.zip, 'A', row, key, false)?;
            match value {
                serde_json::Value::Null => string(
                    &mut self.zip,
                    'B',
                    row,
                    if key == "root_path" {
                        "Withheld"
                    } else {
                        "Unknown"
                    },
                    false,
                )?,
                serde_json::Value::String(text) => string(&mut self.zip, 'B', row, text, false)?,
                serde_json::Value::Bool(value) => boolean(&mut self.zip, 'B', row, Some(*value))?,
                value if value.as_u64().is_some() => {
                    number(&mut self.zip, 'B', row, value.as_u64())?
                }
                value => string(&mut self.zip, 'B', row, &value.to_string(), false)?,
            }
            self.zip.write_all(b"</row>")?;
        }
        self.zip.write_all(b"</sheetData></worksheet>")?;
        Ok(())
    }
    fn finish(mut self) -> Result<()> {
        self.part("[Content_Types].xml")?;
        self.zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>")?;
        for id in 1..=self.sheets + 1 {
            write!(
                self.zip,
                "<Override PartName=\"/xl/worksheets/sheet{id}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
            )?;
        }
        self.zip.write_all(b"</Types>")?;
        self.part("_rels/.rels")?;
        write!(
            self.zip,
            "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
        )?;
        self.part("xl/_rels/workbook.xml.rels")?;
        self.zip.write_all(b"<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">")?;
        for id in 1..=self.sheets + 1 {
            write!(
                self.zip,
                "<Relationship Id=\"rId{id}\" Type=\"{REL}/worksheet\" Target=\"worksheets/sheet{id}.xml\"/>"
            )?;
        }
        write!(
            self.zip,
            "<Relationship Id=\"rId{}\" Type=\"{REL}/styles\" Target=\"styles.xml\"/></Relationships>",
            self.sheets + 2
        )?;
        self.part("xl/workbook.xml")?;
        write!(
            self.zip,
            "<?xml version=\"1.0\"?><workbook xmlns=\"{NS}\" xmlns:r=\"{REL}\"><sheets><sheet name=\"Metadata\" sheetId=\"1\" r:id=\"rId1\"/>"
        )?;
        for id in 2..=self.sheets + 1 {
            write!(
                self.zip,
                "<sheet name=\"Records {}\" sheetId=\"{id}\" r:id=\"rId{id}\"/>",
                id - 1
            )?;
        }
        self.zip.write_all(b"</sheets></workbook>")?;
        self.part("xl/styles.xml")?;
        write!(
            self.zip,
            "<?xml version=\"1.0\"?><styleSheet xmlns=\"{NS}\"><fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font><font><b/><sz val=\"11\"/><color rgb=\"FFFFFFFF\"/><name val=\"Calibri\"/></font></fonts><fills count=\"3\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill><fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF0057D9\"/><bgColor indexed=\"64\"/></patternFill></fill></fills><borders count=\"1\"><border/></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\" fillId=\"2\" borderId=\"0\" xfId=\"0\" applyFont=\"1\" applyFill=\"1\"/></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>"
        )?;
        self.zip.finish().map_err(archive_error)?;
        Ok(())
    }
}

fn string(
    writer: &mut impl Write,
    column: char,
    row: u32,
    value: &str,
    header: bool,
) -> Result<()> {
    if value.encode_utf16().count() > 32_767 {
        return Err(Error::Invalid(
            "XLSX cell exceeds Excel's text limit; choose CSV or narrow the report.".into(),
        ));
    }
    write!(
        writer,
        "<c r=\"{column}{row}\" t=\"inlineStr\" s=\"{}\"><is><t xml:space=\"preserve\">",
        u8::from(header)
    )?;
    let mut escaped = String::with_capacity(value.len());
    for (position, character) in value.char_indices() {
        let suffix = &value.as_bytes()[position..];
        if character == '_'
            && suffix.get(..7).is_some_and(|token| {
                token[1] == b'x'
                    && token[2..6].iter().all(u8::is_ascii_hexdigit)
                    && token[6] == b'_'
            })
        {
            escaped.push_str("_x005F_");
        } else {
            match character {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                '"' => escaped.push_str("&quot;"),
                '\'' => escaped.push_str("&apos;"),
                '\r'
                | '\u{0}'..='\u{8}'
                | '\u{b}'
                | '\u{c}'
                | '\u{e}'..='\u{1f}'
                | '\u{fffe}'
                | '\u{ffff}' => escaped.push_str(&format!("_x{:04X}_", u32::from(character))),
                character => {
                    escaped.push(character);
                }
            }
        }
    }
    writer.write_all(escaped.as_bytes())?;
    writer.write_all(b"</t></is></c>")?;
    Ok(())
}
fn number(writer: &mut impl Write, column: char, row: u32, value: Option<u64>) -> Result<()> {
    match value {
        Some(value) if value > PRECISE_INTEGER => {
            string(writer, column, row, &value.to_string(), false)?
        }
        Some(value) => write!(writer, "<c r=\"{column}{row}\" t=\"n\"><v>{value}</v></c>")?,
        None => write!(writer, "<c r=\"{column}{row}\"/>")?,
    }
    Ok(())
}
fn boolean(writer: &mut impl Write, column: char, row: u32, value: Option<bool>) -> Result<()> {
    if let Some(value) = value {
        write!(
            writer,
            "<c r=\"{column}{row}\" t=\"b\"><v>{}</v></c>",
            u8::from(value)
        )?;
    }
    Ok(())
}
fn archive_error(error: zip::result::ZipError) -> Error {
    Error::Invalid(format!("XLSX archive failed: {error}"))
}
