use super::{SvgView, invalid};
use crate::Result;
use quick_xml::{
    Reader, Writer, XmlVersion,
    events::{BytesEnd, BytesStart, BytesText, Event},
};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

fn numeric(value: &str) -> Result<f64> {
    let result = value.parse::<f64>().map_err(|_| invalid())?;
    if !result.is_finite() || result.abs() > 10_000_000.0 {
        return Err(invalid());
    }
    Ok(result)
}
fn unit(value: &str) -> Result<()> {
    if !(0.0..=1.0).contains(&numeric(value)?) {
        return Err(invalid());
    }
    Ok(())
}
fn rgb(value: &str) -> Result<()> {
    if value == "none"
        || (value.len() == 7
            && value.starts_with('#')
            && value[1..].bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Ok(());
    }
    let Some(body) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) else {
        return Err(invalid());
    };
    let parts: Vec<_> = body.split(',').collect();
    if parts.len() != 3 || !parts.iter().all(|v| v.trim().parse::<u8>().is_ok()) {
        return Err(invalid());
    }
    Ok(())
}
fn clip_id(value: &str) -> Result<()> {
    if value
        .strip_prefix("map-clip-")
        .and_then(|s| s.parse::<u32>().ok())
        .is_none()
    {
        return Err(invalid());
    }
    Ok(())
}
fn path(value: &str, cancelled: &AtomicBool) -> Result<bool> {
    let mut tokens = value.split_ascii_whitespace();
    let mut first = true;
    let mut commands = 0;
    while let Some(token) = tokens.next() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(false);
        }
        commands += 1;
        if commands > 500_000 {
            return Err(invalid());
        }
        let command = token.as_bytes().first().copied().ok_or_else(invalid)?;
        if first && command != b'M' {
            return Err(invalid());
        }
        first = false;
        if command == b'Z' {
            if token != "Z" {
                return Err(invalid());
            }
            continue;
        }
        let count = match command {
            b'M' | b'L' => 2,
            b'A' => 7,
            _ => return Err(invalid()),
        };
        let mut values = [0.0; 7];
        values[0] = numeric(token.get(1..).ok_or_else(invalid)?)?;
        for value in values.iter_mut().take(count).skip(1) {
            *value = numeric(tokens.next().ok_or_else(invalid)?)?;
        }
        if command == b'A'
            && (values[0] < 0.0
                || values[1] < 0.0
                || values[2] != 0.0
                || ![0.0, 1.0].contains(&values[3])
                || ![0.0, 1.0].contains(&values[4]))
        {
            return Err(invalid());
        }
    }
    Ok(true)
}
fn child(parent: Option<&str>, name: &str) -> bool {
    match parent {
        None => name == "svg",
        Some("svg") => matches!(
            name,
            "title" | "desc" | "defs" | "g" | "path" | "rect" | "text"
        ),
        Some("defs") => name == "clipPath",
        Some("clipPath") => name == "path",
        Some("g") => matches!(name, "g" | "path" | "rect" | "text"),
        _ => false,
    }
}
fn attributes(
    tag: &BytesStart<'_>,
    view: &SvgView,
    ids: &mut HashSet<String>,
    references: &mut HashSet<String>,
    cancelled: &AtomicBool,
) -> Result<bool> {
    let name = tag.name();
    let name = name.as_ref();
    let mut values = HashMap::new();
    for attribute in tag.attributes().with_checks(true) {
        let attribute = attribute.map_err(|_| invalid())?;
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|_| invalid())?
            .into_owned();
        if values
            .insert(attribute.key.as_ref().to_owned(), value)
            .is_some()
            || values.len() > 20
        {
            return Err(invalid());
        }
    }
    let keys: &[&str] = match name {
        "svg" => &["xmlns", "width", "height", "viewBox", "overflow"],
        "title" | "desc" | "defs" => &[],
        "clipPath" => &["id", "clipPathUnits"],
        "g" => &["clip-path"],
        "path" => &[
            "d",
            "fill",
            "fill-opacity",
            "stroke",
            "stroke-opacity",
            "stroke-width",
            "stroke-miterlimit",
            "opacity",
        ],
        "rect" => &[
            "x",
            "y",
            "width",
            "height",
            "fill",
            "fill-opacity",
            "opacity",
        ],
        "text" => &[
            "x",
            "y",
            "fill",
            "fill-opacity",
            "opacity",
            "font-family",
            "font-size",
            "font-weight",
            "text-anchor",
            "dominant-baseline",
            "xml:space",
        ],
        _ => return Err(invalid()),
    };
    if values.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(invalid());
    }
    if name == "svg" {
        if values.get("xmlns").map(String::as_str) != Some("http://www.w3.org/2000/svg")
            || values.get("overflow").map(String::as_str) != Some("hidden")
            || numeric(values.get("width").ok_or_else(invalid)?)? != view.width
            || numeric(values.get("height").ok_or_else(invalid)?)? != view.height
        {
            return Err(invalid());
        }
        let bounds = values
            .get("viewBox")
            .ok_or_else(invalid)?
            .split_ascii_whitespace()
            .map(numeric)
            .collect::<Result<Vec<_>>>()?;
        if bounds != [0.0, 0.0, view.width, view.height] {
            return Err(invalid());
        }
    }
    if name == "clipPath" {
        let id = values.get("id").ok_or_else(invalid)?;
        clip_id(id)?;
        if !ids.insert(id.clone())
            || ids.len() > 20_000
            || values.get("clipPathUnits").map(String::as_str) != Some("userSpaceOnUse")
        {
            return Err(invalid());
        }
    }
    if name == "g" {
        let id = values
            .get("clip-path")
            .and_then(|v| v.strip_prefix("url(#"))
            .and_then(|v| v.strip_suffix(')'))
            .ok_or_else(invalid)?;
        clip_id(id)?;
        references.insert(id.to_owned());
        if references.len() > 20_000 {
            return Err(invalid());
        }
    }
    if name == "path" && !path(values.get("d").ok_or_else(invalid)?, cancelled)? {
        return Ok(false);
    }
    for (key, value) in values {
        match key.as_str() {
            "x" | "y" => {
                numeric(&value)?;
            }
            "width" | "height" => {
                if numeric(&value)? < 0.0 {
                    return Err(invalid());
                }
            }
            "fill" | "stroke" => rgb(&value)?,
            "fill-opacity" | "stroke-opacity" | "opacity" => unit(&value)?,
            "stroke-width" => {
                if !(f64::MIN_POSITIVE..=8192.0).contains(&numeric(&value)?) {
                    return Err(invalid());
                }
            }
            "stroke-miterlimit" => {
                if value != "10" {
                    return Err(invalid());
                }
            }
            "font-family" => {
                if !matches!(value.as_str(), "Segoe UI, sans-serif" | "sans-serif") {
                    return Err(invalid());
                }
            }
            "font-size" => {
                if !(1.0..=128.0).contains(&numeric(&value)?) {
                    return Err(invalid());
                }
            }
            "font-weight" => {
                if !matches!(
                    value.as_str(),
                    "100" | "200" | "300" | "400" | "500" | "600" | "700" | "800" | "900"
                ) {
                    return Err(invalid());
                }
            }
            "text-anchor" => {
                if !matches!(value.as_str(), "start" | "middle" | "end") {
                    return Err(invalid());
                }
            }
            "dominant-baseline" => {
                if value != "alphabetic" {
                    return Err(invalid());
                }
            }
            "xml:space" if value != "preserve" => return Err(invalid()),
            _ => {}
        }
    }
    Ok(true)
}
struct Node {
    name: String,
    text_units: usize,
}
fn text(nodes: &mut [Node], value: &str) -> Result<()> {
    if value.contains("]]>") {
        return Err(invalid());
    }
    if value.chars().any(|c| {
        !matches!(c, '\t' | '\n' | '\r') && (c < ' ' || matches!(c, '\u{fffe}' | '\u{ffff}'))
    }) {
        return Err(invalid());
    }
    if let Some(parent) = nodes.last_mut()
        && matches!(parent.name.as_str(), "text" | "title" | "desc")
    {
        parent.text_units += value.encode_utf16().count();
        if parent.text_units > 65_536 {
            return Err(invalid());
        }
        return Ok(());
    }
    if !value.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
        return Err(invalid());
    }
    Ok(())
}
fn context(writer: &mut Writer<Vec<u8>>, metadata: &serde_json::Value) -> Result<()> {
    for (name, value) in [
        ("title", "Voldiq storage map".to_owned()),
        (
            "desc",
            format!(
                "Recorded storage-map view; scope node {}. Units: bytes and Unix seconds UTC. Coverage: {}. Search highlights do not change area sizes. Detailed context and limitations are in metadata; this illustration grants no file-operation authority.",
                metadata["scope_node"],
                metadata["coverage"].as_str().unwrap_or("unknown")
            ),
        ),
        ("metadata", metadata.to_string()),
    ] {
        writer
            .write_event(Event::Start(BytesStart::new(name)))
            .map_err(|_| invalid())?;
        writer
            .write_event(Event::Text(BytesText::new(&value)))
            .map_err(|_| invalid())?;
        writer
            .write_event(Event::End(BytesEnd::new(name)))
            .map_err(|_| invalid())?;
    }
    Ok(())
}

pub(super) fn encode(
    drawing: &str,
    view: &SvgView,
    metadata: &serde_json::Value,
    cancelled: &AtomicBool,
) -> Result<Option<Vec<u8>>> {
    let mut reader = Reader::from_str(drawing);
    let mut writer = Writer::new(Vec::new());
    let mut nodes = Vec::<Node>::new();
    let mut ids = HashSet::new();
    let mut references = HashSet::new();
    let mut root = false;
    let mut closed = false;
    let mut events = 0usize;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        events += 1;
        if events > 500_000 || writer.get_ref().len() > 10 * 1024 * 1024 {
            return Err(invalid());
        }
        let event = reader.read_event().map_err(|_| invalid())?;
        match &event {
            Event::Start(tag) | Event::Empty(tag) => {
                let name = tag.name();
                let name = name.as_ref();
                if closed
                    || nodes.len() >= 64
                    || !child(nodes.last().map(|n| n.name.as_str()), name)
                {
                    return Err(invalid());
                }
                if !attributes(tag, view, &mut ids, &mut references, cancelled)? {
                    return Ok(None);
                }
                if name == "svg" {
                    if root || matches!(event, Event::Empty(_)) {
                        return Err(invalid());
                    }
                    root = true;
                }
                if !matches!(name, "title" | "desc") {
                    writer.write_event(event.borrow()).map_err(|_| invalid())?;
                }
                if name == "svg" {
                    context(&mut writer, metadata)?;
                }
                if matches!(event, Event::Start(_)) {
                    nodes.push(Node {
                        name: name.to_owned(),
                        text_units: 0,
                    });
                }
            }
            Event::End(tag) => {
                let parent = nodes.pop().ok_or_else(invalid)?;
                if parent.name != tag.name().as_ref() {
                    return Err(invalid());
                }
                if parent.name == "svg" {
                    closed = true;
                }
                if !matches!(parent.name.as_str(), "title" | "desc") {
                    writer.write_event(event.borrow()).map_err(|_| invalid())?;
                }
            }
            Event::Text(value) => {
                text(&mut nodes, value.as_ref())?;
                if !nodes
                    .last()
                    .is_some_and(|n| matches!(n.name.as_str(), "title" | "desc"))
                {
                    writer.write_event(event.borrow()).map_err(|_| invalid())?;
                }
            }
            Event::GeneralRef(value) => {
                if !nodes
                    .last()
                    .is_some_and(|n| matches!(n.name.as_str(), "text" | "title" | "desc"))
                {
                    return Err(invalid());
                }
                let resolved = if let Some(c) = value.resolve_char_ref().map_err(|_| invalid())? {
                    c.to_string()
                } else {
                    quick_xml::escape::resolve_predefined_entity(value.as_ref())
                        .ok_or_else(invalid)?
                        .to_owned()
                };
                text(&mut nodes, &resolved)?;
                if !nodes
                    .last()
                    .is_some_and(|n| matches!(n.name.as_str(), "title" | "desc"))
                {
                    writer.write_event(event.borrow()).map_err(|_| invalid())?;
                }
            }
            Event::Eof => break,
            _ => return Err(invalid()),
        }
    }
    if !root || !closed || !nodes.is_empty() || !references.is_subset(&ids) {
        return Err(invalid());
    }
    Ok(Some(writer.into_inner()))
}
