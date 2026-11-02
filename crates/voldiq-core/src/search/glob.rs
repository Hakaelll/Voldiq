use super::QueryError;

pub(super) struct Glob {
    offset: usize,
    mode: Mode,
}
enum Mode {
    All,
    Exact(String),
    Prefix(String),
    Suffix(String),
    Contains(String),
    General(Vec<char>),
}

impl Glob {
    pub fn new(value: &str, offset: usize) -> Result<Self, QueryError> {
        if value.chars().count() > 256 || value.contains(['[', ']', '{', '}']) {
            return Err(QueryError::new(
                offset,
                "Globs support at most 256 characters and only * and ? wildcards; backslashes are literal.",
            ));
        }
        let mut pattern = Vec::new();
        for ch in value.to_lowercase().chars() {
            if ch != '*' || pattern.last() != Some(&'*') {
                pattern.push(ch);
            }
        }
        let stars = pattern.iter().filter(|&&ch| ch == '*').count();
        let mode = if pattern == ['*'] {
            Mode::All
        } else if pattern.contains(&'?') {
            Mode::General(pattern)
        } else if stars == 0 {
            Mode::Exact(pattern.iter().collect())
        } else if stars == 1 && pattern[0] == '*' {
            Mode::Suffix(pattern[1..].iter().collect())
        } else if stars == 1 && pattern.last() == Some(&'*') {
            Mode::Prefix(pattern[..pattern.len() - 1].iter().collect())
        } else if stars == 2 && pattern[0] == '*' && pattern.last() == Some(&'*') {
            Mode::Contains(pattern[1..pattern.len() - 1].iter().collect())
        } else {
            Mode::General(pattern)
        };
        Ok(Self { offset, mode })
    }

    pub fn matches(
        &self,
        value: &str,
        budget: &mut u64,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<bool>, QueryError> {
        let pattern = match &self.mode {
            Mode::All => return Ok(Some(true)),
            Mode::Exact(s) => return Ok(Some(value == s)),
            Mode::Prefix(s) => return Ok(Some(value.starts_with(s))),
            Mode::Suffix(s) => return Ok(Some(value.ends_with(s))),
            Mode::Contains(s) => return Ok(Some(value.contains(s))),
            Mode::General(pattern) => pattern,
        };
        let mut chars = value.chars();
        let mut position = 0;
        let mut star = None;
        let mut work = 0_u64;
        loop {
            if work.is_multiple_of(256) && cancelled() {
                return Ok(None);
            }
            *budget = budget.checked_sub(1).ok_or_else(|| {
                QueryError::new(
                    self.offset,
                    "Glob work limit exceeded. Simplify the pattern or narrow the scope.",
                )
            })?;
            work += 1;
            match pattern.get(position) {
                Some('*') => {
                    position += 1;
                    star = Some((position, chars.clone()));
                }
                token => {
                    let next = chars.next();
                    if next.is_none() && token.is_none() {
                        return Ok(Some(true));
                    }
                    if let (Some(ch), Some(expected)) = (next, token)
                        && (*expected == '?' || *expected == ch)
                    {
                        position += 1;
                        continue;
                    }
                    if let Some((restart, remaining)) = &mut star {
                        if remaining.next().is_none() {
                            return Ok(Some(false));
                        }
                        chars = remaining.clone();
                        position = *restart;
                    } else {
                        return Ok(Some(false));
                    }
                }
            }
        }
    }
}
