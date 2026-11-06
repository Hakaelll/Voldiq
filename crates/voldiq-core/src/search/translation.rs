use super::{QueryError, numbers};
use crate::index::Query;

#[derive(serde::Serialize, Debug)]
pub struct Translation {
    pub expression: String,
    pub conditions: usize,
    pub unparsed: Vec<UnparsedSpan>,
}

#[derive(serde::Serialize, Debug)]
pub struct UnparsedSpan {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

struct Token<'a> {
    start: usize,
    end: usize,
    value: &'a str,
    word: String,
    quoted: bool,
}

fn tokens(source: &str) -> Result<Vec<Token<'_>>, QueryError> {
    if source.len() > 4096 {
        return Err(QueryError::new(
            0,
            "Offline requests are limited to 4096 UTF-8 bytes.",
        ));
    }
    if let Some((offset, _)) = source
        .char_indices()
        .find(|(_, c)| c.is_control() && !c.is_whitespace())
    {
        return Err(QueryError::new(
            offset,
            "Control characters are unsupported.",
        ));
    }
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        offset += source[offset..].len() - source[offset..].trim_start().len();
        if offset == source.len() {
            break;
        }
        if result.len() == 128 {
            return Err(QueryError::new(
                offset,
                "Offline requests are limited to 128 words/quoted phrases.",
            ));
        }
        let start = offset;
        let quoted = source[offset..].starts_with('"');
        let value = if quoted {
            offset += 1;
            let width = source[offset..]
                .find('"')
                .ok_or_else(|| QueryError::new(start, "Unclosed double quote."))?;
            let value = &source[offset..offset + width];
            offset += width + 1;
            if value.is_empty()
                || source[offset..]
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_whitespace())
            {
                return Err(QueryError::new(
                    start,
                    "Use a nonempty quoted phrase separated by spaces.",
                ));
            }
            value
        } else {
            let width = source[offset..]
                .find(char::is_whitespace)
                .unwrap_or(source.len() - offset);
            offset += width;
            let value = &source[start..offset];
            if value.contains('"') {
                return Err(QueryError::new(
                    start,
                    "Start a quoted phrase after a space.",
                ));
            }
            value
        };
        result.push(Token {
            start,
            end: offset,
            value,
            word: value.to_lowercase(),
            quoted,
        });
    }
    Ok(result)
}

fn prefix(tokens: &[Token<'_>], phrase: &str) -> Option<usize> {
    let mut width = 0;
    for word in phrase.split_whitespace() {
        let token = tokens.get(width)?;
        if token.quoted || token.word != word {
            return None;
        }
        width += 1;
    }
    Some(width)
}

fn value<'a>(tokens: &'a [Token<'_>], width: usize) -> Result<&'a Token<'a>, QueryError> {
    tokens
        .get(width)
        .ok_or_else(|| QueryError::new(tokens[0].start, "This phrase requires an explicit value."))
}

fn phrase(tokens: &[Token<'_>]) -> Result<Option<(usize, String)>, QueryError> {
    let first = &tokens[0];
    if !first.quoted {
        let kind = match first.word.as_str() {
            "files" | "file" | "archivos" | "archivo" | "ficheros" | "fichero" => Some("type:file"),
            "folders" | "folder" | "carpetas" | "carpeta" => Some("type:folder"),
            "videos" | "video" | "vídeos" | "vídeo" => Some("type:file category:video"),
            "images" | "image" | "photos" | "photo" | "imágenes" | "imagen" | "fotos" | "foto" => {
                Some("type:file category:image")
            }
            "audio" | "music" | "música" => Some("type:file category:audio"),
            _ => None,
        };
        if let Some(kind) = kind {
            return Ok(Some((1, kind.into())));
        }
    }
    for (source, expression) in [
        ("without extension", "ext:none"),
        ("sin extensión", "ext:none"),
        ("sin extension", "ext:none"),
        ("cloud placeholders", "cloud:placeholder"),
        ("marcadores de nube", "cloud:placeholder"),
        ("recall required", "cloud:recall_required"),
        ("unknown cloud state", "cloud:unknown"),
        ("application cache", "app-category:cache"),
        ("caché de aplicación", "app-category:cache"),
        ("cache de aplicación", "app-category:cache"),
    ] {
        if let Some(width) = prefix(tokens, source) {
            return Ok(Some((width, expression.into())));
        }
    }
    for (source, op) in [
        ("larger than", ">"),
        ("bigger than", ">"),
        ("mayores de", ">"),
        ("mayor de", ">"),
        ("más de", ">"),
        ("mas de", ">"),
        ("at least", ">="),
        ("al menos", ">="),
        ("smaller than", "<"),
        ("less than", "<"),
        ("menores de", "<"),
        ("menor de", "<"),
        ("menos de", "<"),
        ("at most", "<="),
        ("como máximo", "<="),
        ("como maximo", "<="),
    ] {
        if let Some(width) = prefix(tokens, source) {
            let amount = value(tokens, width)?;
            let mut size = amount.value.to_owned();
            let mut consumed = width + 1;
            if amount
                .value
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'.')
                && let Some(unit) = tokens.get(consumed).filter(|unit| {
                    !unit.quoted
                        && matches!(
                            unit.word.as_str(),
                            "b" | "kb"
                                | "mb"
                                | "gb"
                                | "tb"
                                | "pb"
                                | "eb"
                                | "kib"
                                | "mib"
                                | "gib"
                                | "tib"
                                | "pib"
                                | "eib"
                        )
                })
            {
                size.push_str(unit.value);
                consumed += 1;
            }
            numbers::bytes(&size, amount.start)?;
            return Ok(Some((consumed, format!("size{op}{size}"))));
        }
    }
    for source in ["modified between", "modificados entre", "modificadas entre"] {
        if let Some(width) = prefix(tokens, source) {
            let from = value(tokens, width)?;
            let connector = value(tokens, width + 1)?;
            let to = value(tokens, width + 2)?;
            let a = numbers::date(from.value, from.start)?;
            let b = numbers::date(to.value, to.start)?;
            if connector.quoted
                || !matches!(connector.word.as_str(), "and" | "y")
                || a.start > b.end.map_or(b.start, |end| end - 1)
            {
                return Err(QueryError::new(
                    from.start,
                    "Use an ordered UTC date range joined by and or y.",
                ));
            }
            return Ok(Some((
                width + 3,
                format!("modified>={} modified<={}", from.value, to.value),
            )));
        }
    }
    for (source, op) in [
        ("modified before", "<"),
        ("modificados antes de", "<"),
        ("modificadas antes de", "<"),
        ("modified after", ">"),
        ("modificados después de", ">"),
        ("modificados despues de", ">"),
        ("modified on", "="),
        ("modificados el", "="),
        ("modificadas el", "="),
    ] {
        if let Some(width) = prefix(tokens, source) {
            let date = value(tokens, width)?;
            numbers::date(date.value, date.start)?;
            return Ok(Some((width + 1, format!("modified{op}{}", date.value))));
        }
    }
    for (source, property) in [
        ("name contains", "name"),
        ("nombre contiene", "name"),
        ("named", "name"),
        ("llamados", "name"),
        ("llamadas", "name"),
        ("path contains", "path"),
        ("ruta contiene", "path"),
        ("with extension", "ext"),
        ("con extensión", "ext"),
        ("con extension", "ext"),
        ("extension", "ext"),
        ("extensión", "ext"),
        ("application named", "app"),
        ("aplicación llamada", "app"),
        ("aplicacion llamada", "app"),
    ] {
        if let Some(width) = prefix(tokens, source) {
            let value = value(tokens, width)?;
            return Ok(Some((width + 1, format!("{property}:\"{}\"", value.value))));
        }
    }
    Ok(None)
}

/// A proposal only. Unparsed source spans must remain visible before the human applies a query.
pub fn translate(source: &str) -> Result<Translation, QueryError> {
    let tokens = tokens(source)?;
    let mut consumed = vec![false; tokens.len()];
    let mut terms = Vec::new();
    let mut position = 0;
    while position < tokens.len() {
        if let Some((width, expression)) = phrase(&tokens[position..])? {
            consumed[position..position + width].fill(true);
            terms.push(expression);
            position += width;
        } else {
            position += 1;
        }
    }
    // Connectors are understood only between two interpreted phrases.
    for i in 1..tokens.len().saturating_sub(1) {
        if !tokens[i].quoted
            && matches!(tokens[i].word.as_str(), "and" | "y")
            && consumed[i - 1]
            && consumed[i + 1]
        {
            consumed[i] = true;
        }
    }
    let expression = terms.join(" ");
    let query = Query {
        expression: expression.clone(),
        ..Query::default()
    };
    let conditions = query.compile()?.search.terms.len();
    let mut unparsed = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if consumed[i] {
            i += 1;
            continue;
        }
        let start = tokens[i].start;
        while i + 1 < tokens.len() && !consumed[i + 1] {
            i += 1;
        }
        let end = tokens[i].end;
        unparsed.push(UnparsedSpan {
            start,
            end,
            text: source[start..end].to_owned(),
        });
        i += 1;
    }
    Ok(Translation {
        expression,
        conditions,
        unparsed,
    })
}
