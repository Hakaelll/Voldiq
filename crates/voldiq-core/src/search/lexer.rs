use super::QueryError;

pub(super) struct Token {
    pub value: String,
    pub offset: usize,
    pub literal: bool,
}

pub(super) fn tokens(source: &str) -> Result<Vec<Token>, QueryError> {
    let mut result = Vec::new();
    let mut value = String::new();
    let mut start = None;
    let mut quoted = false;
    let mut bare = false;
    for (offset, ch) in source.char_indices() {
        if ch.is_control() && !ch.is_whitespace() {
            return Err(QueryError::new(
                offset,
                "Control characters are unsupported.",
            ));
        }
        if !quoted && ch.is_whitespace() {
            if let Some(offset) = start.take() {
                push(&mut result, std::mem::take(&mut value), offset, !bare)?;
                bare = false;
            }
            continue;
        }
        start.get_or_insert(offset);
        if ch == '"' {
            quoted = !quoted;
        } else {
            bare |= !quoted;
            value.push(ch);
        }
    }
    if quoted {
        return Err(QueryError::new(
            start.unwrap_or(0),
            "Unclosed double quote.",
        ));
    }
    if let Some(offset) = start {
        push(&mut result, value, offset, !bare)?;
    }
    Ok(result)
}

fn push(
    result: &mut Vec<Token>,
    value: String,
    offset: usize,
    literal: bool,
) -> Result<(), QueryError> {
    if value.is_empty() {
        return Err(QueryError::new(offset, "A search term cannot be empty."));
    }
    if result.len() == 64 {
        return Err(QueryError::new(
            offset,
            "At most 64 expression terms are supported.",
        ));
    }
    result.push(Token {
        value,
        offset,
        literal,
    });
    Ok(())
}
