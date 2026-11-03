mod fonts;

use super::{
    LogicalTotal, ReportSelection, ReportSummary, StreamOutcome, StreamProgress, ranked_summary,
};
use crate::{Error, Result, index::Index};
use krilla::{
    Document,
    color::rgb,
    geom::Point,
    metadata::Metadata,
    num::NormalizedF32,
    page::PageSettings,
    paint::Fill,
    text::{Font, GlyphId, KrillaGlyph, Tag},
};
use parley::{
    Layout, LayoutContext,
    layout::{Alignment, Line},
    style::{FontFamily, FontFamilyName, LineHeight, OverflowWrap, StyleProperty},
};
use std::{
    borrow::Cow,
    collections::HashMap,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

const WIDTH: f32 = 595.28;
const HEIGHT: f32 = 841.89;
const MARGIN: f32 = 44.0;
const CONTENT_WIDTH: f32 = WIDTH - 2.0 * MARGIN;
const BOTTOM: f32 = HEIGHT - 62.0;
const PAGE_LIMIT: usize = 256;
const TEXT_LIMIT: usize = 4 * 1024 * 1024;
fn ink() -> rgb::Color {
    rgb::Color::new(36, 44, 41)
}
fn secondary() -> rgb::Color {
    rgb::Color::new(80, 92, 84)
}
fn accent() -> rgb::Color {
    rgb::Color::new(0, 87, 217)
}

struct Block {
    text: String,
    size: f32,
    color: rgb::Color,
    gap: f32,
}
impl Block {
    fn body(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: 10.5,
            color: ink(),
            gap: 5.0,
        }
    }
    fn heading(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: 15.0,
            color: accent(),
            gap: 10.0,
        }
    }
}

/// C0/C1 and directional formatting controls are visible escapes, never active document structure.
fn display(text: &str) -> String {
    let mut result = String::new();
    for c in text.chars() {
        if c.is_control()
            || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            use std::fmt::Write;
            let _ = write!(result, "\\u{{{:04X}}}", c as u32);
        } else {
            result.push(c);
        }
    }
    result
}

fn blocks(summary: &ReportSummary) -> Result<Vec<Block>> {
    let scope = &summary.scope;
    let value = |key: &str| match &scope[key] {
        serde_json::Value::Null => "Unknown".to_owned(),
        serde_json::Value::String(text) => display(text),
        value => display(&value.to_string()),
    };
    let mut result = vec![
        Block {
            text: "VOLDIQ".into(),
            size: 11.0,
            color: accent(),
            gap: 9.0,
        },
        Block {
            text: "Storage report".into(),
            size: 26.0,
            color: ink(),
            gap: 13.0,
        },
        Block::body(format!(
            "{} matching {}. Showing {} largest recorded logical sizes; {} omitted.",
            summary.progress.rows,
            value("items"),
            summary.rows.len(),
            summary.omitted
        )),
        Block::body(
            "This is a ranked summary. CSV and XLSX include every matching record. Sizes below are exact bytes, not unique physical usage or reclaimable space.",
        ),
        Block::heading("Scope and observation"),
        Block::body(format!(
            "Analysis root: {}",
            if scope["include_paths"] == true {
                value("root_path")
            } else {
                "Withheld (path sharing is off)".into()
            }
        )),
        Block::body(format!(
            "Snapshot: {}. Generated: {} Unix seconds UTC.",
            value("snapshot_id"),
            value("generated_at")
        )),
        Block::body(format!("Recorded observation: {}", value("observation"))),
        Block::body(format!(
            "Coverage: {}. Exclusions: {}. Access issues: {}. Root incomplete: {}.",
            value("coverage"),
            value("exclusions"),
            value("access_issues"),
            value("root_incomplete")
        )),
        Block::body(format!(
            "Paths included: {}. Filter text included: {}.",
            value("include_paths"),
            value("filter_text_included")
        )),
        Block::body(format!("Selection filters: {}", value("filter"))),
        Block::heading("Matching records"),
        Block::body(match summary.logical {
            LogicalTotal::FilePaths { bytes } => format!(
                "Observed file-path total: {bytes} bytes. Aliases may repeat the same data; partial records are lower bounds."
            ),
            LogicalTotal::OverlappingFolders => {
                "Folder totals overlap. No sum is reported for ancestor and descendant folders."
                    .into()
            }
        }),
        Block::body(format!(
            "Incomplete: {}. Allocation unknown or partial: {}. Modified date unknown: {}. Recorded hard-link aliases: {}.",
            summary.incomplete_matches,
            summary.unknown_allocation_matches,
            summary.unknown_date_matches,
            summary.recorded_hard_link_matches
        )),
        Block::heading("Largest recorded entries"),
    ];
    if summary.rows.is_empty() {
        result.push(Block::body("No indexed record matches this selection."));
    }
    for (position, row) in summary.rows.iter().enumerate() {
        let entry = &row.entry;
        result.push(Block::heading(format!(
            "{:02}   {} bytes{}",
            position + 1,
            entry.logical,
            if entry.incomplete { " (partial)" } else { "" }
        )));
        result.push(Block::body(format!("Name: {}", display(&entry.name))));
        result.push(Block {
            text: format!(
                "Node {}. Recorded allocation: {}. Modified: {}. Hard-link alias: {}.",
                entry.id.0,
                if entry.allocation_known {
                    format!("{} bytes", entry.allocated)
                } else if entry.allocated > 0 {
                    format!("at least {} bytes (partial)", entry.allocated)
                } else {
                    "Unknown".into()
                },
                entry
                    .modified
                    .map(|time| format!("{time} Unix seconds UTC"))
                    .unwrap_or_else(|| "Unknown".into()),
                entry.hard_link
            ),
            size: 9.5,
            color: secondary(),
            gap: 6.0,
        });
        if let Some(path) = &row.path {
            result.push(Block::body(format!(
                "Path{}: {}",
                if path.exact {
                    ""
                } else {
                    " (display representation; not exact)"
                },
                display(&path.text)
            )));
        }
    }
    result.extend([
        Block::heading("Limitations"),
        Block::body(value("limitations")),
        Block::body("Ranking uses recorded logical sizes, with native index ID resolving ties. No source-file contents were read for this report. Unknown values are not zero. Cloud metadata does not prove resident content. This report grants no permission to change files."),
        Block::body("Names and paths are display strings. Control and directional formatting characters appear as visible Unicode escapes. Installed fonts are embedded as eligible subsets; unsupported text or resource limits cause an explicit export error."),
    ]);
    if result.iter().map(|block| block.text.len()).sum::<usize>() > TEXT_LIMIT {
        return Err(Error::Invalid(
            "PDF text exceeds the report resource limit. Narrow the selection or use CSV/XLSX."
                .into(),
        ));
    }
    Ok(result)
}

fn layout(
    block: &Block,
    fonts: &mut fonts::Fonts,
    context: &mut LayoutContext<rgb::Color>,
) -> Layout<rgb::Color> {
    let families: Vec<_> = fonts
        .families
        .iter()
        .map(|name| FontFamilyName::Named(Cow::Borrowed(name)))
        .collect();
    let mut builder = context.ranged_builder(&mut fonts.context, &block.text, 1.0, false);
    builder.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Borrowed(
        &families,
    ))));
    builder.push_default(StyleProperty::FontSize(block.size));
    builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.4)));
    builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
    builder.push_default(StyleProperty::Brush(block.color));
    let mut result = builder.build(&block.text);
    result.break_all_lines(Some(CONTENT_WIDTH));
    result.align(Alignment::Start, Default::default());
    result
}

type FontCache = HashMap<(u64, u32, Vec<i16>), Font>;

fn draw_line(
    surface: &mut krilla::surface::Surface<'_>,
    line: Line<'_, rgb::Color>,
    text: &str,
    y: f32,
    cache: &mut FontCache,
) -> Result<()> {
    let metrics = line.metrics();
    // Parley hangs trailing whitespace outside the line's visible width.
    let visible_advance = metrics.advance - metrics.trailing_whitespace;
    if !visible_advance.is_finite() || visible_advance > CONTENT_WIDTH + 0.5 {
        return Err(Error::Invalid(
            "A PDF text cluster cannot fit the page. Use CSV/XLSX for this selection.".into(),
        ));
    }
    let mut x = MARGIN + metrics.offset;
    let baseline = y + metrics.baseline - metrics.block_min_coord;
    for run in line.runs() {
        let font = run.font().clone();
        let (data, id) = font.data.into_raw_parts();
        let key = (id, font.index, run.normalized_coords().to_vec());
        if !cache.contains_key(&key) {
            let variations: Vec<_> = run
                .synthesis()
                .variation_settings()
                .iter()
                .map(|(tag, value)| (Tag::new(&tag.to_be_bytes()), *value))
                .collect();
            let converted = Font::new_variable(data.into(), font.index, &variations)
                .ok_or_else(|| Error::Invalid("Unable to embed a selected PDF font.".into()))?;
            cache.insert(key.clone(), converted);
        }
        let converted = &cache[&key];
        let mut glyphs = Vec::<KrillaGlyph>::new();
        let size = run.font_size();
        for cluster in run.visual_clusters() {
            if cluster.is_ligature_continuation() {
                if let Some(glyph) = glyphs.last_mut() {
                    glyph.text_range.end = cluster.text_range().end;
                }
                continue;
            }
            for glyph in cluster.glyphs() {
                if glyph.id == 0 {
                    return Err(Error::Invalid(
                        "PDF shaping produced a missing glyph. Use CSV/XLSX for this selection."
                            .into(),
                    ));
                }
                glyphs.push(KrillaGlyph::new(
                    GlyphId::new(glyph.id),
                    glyph.advance / size,
                    glyph.x / size,
                    glyph.y / size,
                    0.0,
                    cluster.text_range(),
                    None,
                ));
            }
        }
        surface.draw_glyphs(
            Point::from_xy(x, baseline),
            &glyphs,
            converted.clone(),
            text,
            size,
            false,
        );
        x += run.advance();
    }
    Ok(())
}

/// Generates a bounded, paginated text report on demand. No destination or source content is opened.
pub fn render_pdf(summary: &ReportSummary, cancelled: &AtomicBool) -> Result<Option<Vec<u8>>> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let blocks = blocks(summary)?;
    let mut characters = String::from("VOLDIQ / Storage report / Page 0123456789");
    for block in &blocks {
        characters.push_str(&block.text);
    }
    let Some(mut fonts) = fonts::load(&characters, cancelled)? else {
        return Ok(None);
    };
    drop(characters);
    let mut context = LayoutContext::new();
    let mut layouts = Vec::with_capacity(blocks.len());
    let mut pages: Vec<Vec<(usize, usize, f32)>> = vec![Vec::new()];
    let mut y = MARGIN;
    for (block_index, block) in blocks.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let paragraph = layout(block, &mut fonts, &mut context);
        for (line_index, line) in paragraph.lines().enumerate() {
            let metrics = line.metrics();
            let height = metrics
                .line_height
                .max(metrics.block_max_coord - metrics.block_min_coord);
            if !height.is_finite() || height <= 0.0 || height > BOTTOM - MARGIN {
                return Err(Error::Invalid(
                    "PDF line exceeds the page resource limit.".into(),
                ));
            }
            let keep_next = if line_index == 0 && block.size >= 15.0 {
                32.0
            } else {
                0.0
            };
            if y + height + keep_next > BOTTOM && !pages.last().unwrap().is_empty() {
                if pages.len() >= PAGE_LIMIT {
                    return Err(Error::Invalid(
                        "PDF exceeds 256 pages. Narrow the selection or use CSV/XLSX.".into(),
                    ));
                }
                pages.push(Vec::new());
                y = MARGIN;
            }
            pages.last_mut().unwrap().push((block_index, line_index, y));
            y += height;
        }
        y += block.gap;
        layouts.push(paragraph);
    }
    let mut document = Document::new();
    document.set_metadata(
        Metadata::new()
            .title("Voldiq storage report".into())
            .creator("Voldiq".into())
            .language("en".into()),
    );
    let mut cache = FontCache::new();
    for (number, lines) in pages.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let mut page = document.start_page_with(PageSettings::from_wh(WIDTH, HEIGHT).unwrap());
        let mut surface = page.surface();
        for &(block_index, line_index, y) in lines {
            if cancelled.load(Ordering::Relaxed) {
                return Ok(None);
            }
            let block = &blocks[block_index];
            surface.set_fill(Some(Fill {
                paint: block.color.into(),
                opacity: NormalizedF32::ONE,
                rule: Default::default(),
            }));
            draw_line(
                &mut surface,
                layouts[block_index].lines().nth(line_index).unwrap(),
                &block.text,
                y,
                &mut cache,
            )?;
        }
        let footer = Block {
            text: format!(
                "VOLDIQ / Storage report / Page {} of {}",
                number + 1,
                pages.len()
            ),
            size: 9.0,
            color: secondary(),
            gap: 0.0,
        };
        let footer_layout = layout(&footer, &mut fonts, &mut context);
        surface.set_fill(Some(Fill {
            paint: secondary().into(),
            opacity: NormalizedF32::ONE,
            rule: Default::default(),
        }));
        for line in footer_layout.lines() {
            draw_line(&mut surface, line, &footer.text, HEIGHT - 39.0, &mut cache)?;
        }
        surface.finish();
        page.finish();
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let bytes = document
        .finish()
        .map_err(|_| Error::Invalid("PDF encoding failed; no report was published.".into()))?;
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Error::Invalid(
            "Encoded PDF exceeds the report resource limit.".into(),
        ));
    }
    Ok(Some(bytes))
}

pub fn stream_pdf(
    index: &Index,
    request: &ReportSelection,
    writer: &mut impl Write,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(StreamProgress),
) -> Result<StreamOutcome> {
    let mut last = StreamProgress {
        inspected: 0,
        rows: 0,
        total_entries: index.entries.len().saturating_sub(1),
    };
    let summary = ranked_summary(index, request, cancelled, |value| {
        last = value;
        progress(value);
    })?;
    let Some(summary) = summary else {
        return Ok(StreamOutcome::Cancelled(last));
    };
    let Some(bytes) = render_pdf(&summary, cancelled)? else {
        return Ok(StreamOutcome::Cancelled(last));
    };
    for chunk in bytes.chunks(64 * 1024) {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(StreamOutcome::Cancelled(last));
        }
        writer.write_all(chunk)?;
    }
    Ok(if cancelled.load(Ordering::Relaxed) {
        StreamOutcome::Cancelled(last)
    } else {
        StreamOutcome::Complete(last)
    })
}
