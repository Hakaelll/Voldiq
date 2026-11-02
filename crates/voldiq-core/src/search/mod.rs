mod glob;
mod lexer;
mod numbers;

use crate::{Entry, Kind, cloud::CloudFilter, index::Query};
use glob::Glob;
use numbers::{ByteThreshold, DateValue};
use std::cmp::Ordering;

#[derive(Debug, thiserror::Error)]
#[error("Invalid query at byte {offset}: {message}")]
pub struct QueryError {
    pub offset: usize,
    pub message: &'static str,
}
impl QueryError {
    fn new(offset: usize, message: &'static str) -> Self {
        Self { offset, message }
    }
}

pub struct CompiledQuery<'a> {
    pub(crate) query: &'a Query,
    pub(crate) search: Search,
}

#[derive(Clone, Copy)]
enum Compare {
    Equal,
    NotEqual,
    Less,
    AtMost,
    Greater,
    AtLeast,
}
impl Compare {
    fn matches(self, order: Ordering) -> bool {
        match self {
            Self::Equal => order.is_eq(),
            Self::NotEqual => !order.is_eq(),
            Self::Less => order.is_lt(),
            Self::AtMost => !order.is_gt(),
            Self::Greater => order.is_gt(),
            Self::AtLeast => !order.is_lt(),
        }
    }
}
enum Predicate {
    Name(String),
    NameGlob(Glob),
    Path(String),
    PathGlob(Glob),
    Extension(String),
    Extensionless,
    Size(Compare, ByteThreshold),
    Modified(Compare, DateValue),
    Kind(Kind),
    Cloud(CloudFilter),
}

pub(crate) struct Search {
    terms: Vec<Predicate>,
    needs_path: bool,
}
pub(crate) const GLOB_WORK_LIMIT: u64 = 100_000_000;

impl Query {
    pub fn compile(&self) -> Result<CompiledQuery<'_>, QueryError> {
        if self.expression.len() > 4096 || self.text.len() > 4096 || self.extension.len() > 128 {
            return Err(QueryError::new(
                0,
                "Expression/text must be at most 4096 bytes and extension at most 128 bytes.",
            ));
        }
        if !matches!(
            self.sort.as_str(),
            "size" | "allocated" | "name" | "modified"
        ) {
            return Err(QueryError::new(
                0,
                "Unsupported sort. Use size, allocated, name or modified.",
            ));
        }
        let mut terms = Vec::new();
        if self.files_only {
            terms.push(Predicate::Kind(Kind::File));
        }
        if let Some(kind) = self.kind {
            terms.push(Predicate::Kind(kind));
        }
        if let Some(cloud) = self.cloud {
            terms.push(Predicate::Cloud(cloud));
        }
        if self.min_size > 0 {
            terms.push(Predicate::Size(
                Compare::AtLeast,
                ByteThreshold {
                    whole: self.min_size,
                    fractional: false,
                },
            ));
        }
        if let Some(before) = self.modified_before {
            terms.push(Predicate::Modified(
                Compare::Less,
                DateValue {
                    start: before,
                    end: None,
                },
            ));
        }
        if self.without_extension {
            terms.push(Predicate::Extensionless);
        }
        if !self.extension.is_empty() {
            if self.extension.trim_start_matches('.').is_empty() {
                return Err(QueryError::new(
                    0,
                    "An extension filter cannot contain only dots.",
                ));
            }
            terms.push(Predicate::Extension(
                self.extension.trim_start_matches('.').to_lowercase(),
            ));
        }
        if !self.text.is_empty() {
            terms.push(Predicate::Name(self.text.to_lowercase()));
        }
        for token in lexer::tokens(&self.expression)? {
            terms.push(parse(token)?);
        }
        Ok(CompiledQuery {
            query: self,
            search: Search {
                needs_path: terms
                    .iter()
                    .any(|p| matches!(p, Predicate::Path(_) | Predicate::PathGlob(_))),
                terms,
            },
        })
    }
}

fn parse(token: lexer::Token) -> Result<Predicate, QueryError> {
    let error = |message| QueryError::new(token.offset, message);
    if token.literal {
        return Ok(Predicate::Name(token.value.to_lowercase()));
    }
    let Some((position, separator)) = token
        .value
        .char_indices()
        .find(|(_, c)| matches!(c, ':' | '=' | '!' | '<' | '>' | '~'))
    else {
        if matches!(token.value.as_str(), "AND" | "OR" | "NOT") || token.value.contains(['(', ')'])
        {
            return Err(error(
                "Terms are combined by AND using spaces. Boolean operators/parentheses are unsupported; quote literal text.",
            ));
        }
        return Ok(Predicate::Name(token.value.to_lowercase()));
    };
    let property = token.value[..position].to_ascii_lowercase();
    let tail = &token.value[position..];
    let (op, width) = if separator == ':' {
        (Compare::Equal, 1)
    } else {
        match tail {
            s if s.starts_with("!=") => (Compare::NotEqual, 2),
            s if s.starts_with("<=") => (Compare::AtMost, 2),
            s if s.starts_with(">=") => (Compare::AtLeast, 2),
            s if s.starts_with('=') => (Compare::Equal, 1),
            s if s.starts_with('<') => (Compare::Less, 1),
            s if s.starts_with('>') => (Compare::Greater, 1),
            _ => return Err(error("Unsupported comparison operator.")),
        }
    };
    let value = &tail[width..];
    if value.is_empty() {
        return Err(error("A condition requires a value."));
    }
    if matches!(property.as_str(), "size" | "modified") {
        return if property == "size" {
            Ok(Predicate::Size(op, numbers::bytes(value, token.offset)?))
        } else {
            Ok(Predicate::Modified(op, numbers::date(value, token.offset)?))
        };
    }
    if separator != ':' {
        return Err(error(
            "This property requires a colon, not a comparison operator.",
        ));
    }
    Ok(match property.as_str() {
        "name" => Predicate::Name(value.to_lowercase()),
        "glob" => Predicate::NameGlob(Glob::new(value, token.offset)?),
        "path" => Predicate::Path(value.to_lowercase()),
        "pathglob" => Predicate::PathGlob(Glob::new(value, token.offset)?),
        "ext" if value.eq_ignore_ascii_case("none") => Predicate::Extensionless,
        "ext" if !value.trim_start_matches('.').is_empty() => {
            Predicate::Extension(value.trim_start_matches('.').to_lowercase())
        }
        "ext" => {
            return Err(error(
                "An extension requires characters after the dot, or none.",
            ));
        }
        "type" => Predicate::Kind(match value.to_ascii_lowercase().as_str() {
            "file" => Kind::File,
            "folder" | "directory" => Kind::Directory,
            "link" => Kind::Link,
            _ => return Err(error("Use type:file, type:folder or type:link.")),
        }),
        "cloud" => Predicate::Cloud(match value.to_ascii_lowercase().as_str() {
            "placeholder" => CloudFilter::Placeholder,
            "recall_required" => CloudFilter::RecallRequired,
            "no_recall_flag" => CloudFilter::NoRecallFlag,
            "not_placeholder" => CloudFilter::NotPlaceholder,
            "unknown" => CloudFilter::Unknown,
            _ => return Err(error("Unsupported observed cloud state.")),
        }),
        _ => {
            return Err(error(
                "Unsupported property. Use name, glob, path, pathglob, ext, size, modified, type or cloud. App attribution is not yet available.",
            ));
        }
    })
}

impl Search {
    pub fn needs_path(&self) -> bool {
        self.needs_path
    }

    pub fn matches_entry(
        &self,
        entry: &Entry,
        budget: &mut u64,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<bool>, QueryError> {
        let mut lower_name = None;
        for term in &self.terms {
            let matched = match term {
                Predicate::Name(value) => contains(&entry.name, value),
                Predicate::NameGlob(glob) => {
                    let Some(matched) = glob.matches(
                        lower_name.get_or_insert_with(|| entry.name.to_lowercase()),
                        budget,
                        cancelled,
                    )?
                    else {
                        return Ok(None);
                    };
                    matched
                }
                Predicate::Extension(value) => std::path::Path::new(&entry.name)
                    .extension()
                    .is_some_and(|ext| ext.to_string_lossy().to_lowercase() == *value),
                Predicate::Extensionless => {
                    entry.is_metadata_file()
                        && std::path::Path::new(&entry.name).extension().is_none()
                }
                Predicate::Size(op, value) => {
                    if (entry.kind == Kind::Link && !entry.cloud.is_file())
                        || (entry.kind == Kind::Directory
                            && entry.incomplete
                            && !matches!(op, Compare::Greater | Compare::AtLeast))
                    {
                        return Ok(Some(false));
                    }
                    let order = entry.logical.cmp(&value.whole);
                    op.matches(if order.is_eq() && value.fractional {
                        Ordering::Less
                    } else {
                        order
                    })
                }
                Predicate::Modified(op, value) => entry.modified.is_some_and(|modified| {
                    if let Some(end) = value.end {
                        match op {
                            Compare::Equal => modified >= value.start && modified < end,
                            Compare::NotEqual => modified < value.start || modified >= end,
                            Compare::Less => modified < value.start,
                            Compare::AtMost => modified < end,
                            Compare::Greater => modified >= end,
                            Compare::AtLeast => modified >= value.start,
                        }
                    } else {
                        op.matches(modified.cmp(&value.start))
                    }
                }),
                Predicate::Kind(kind) => match kind {
                    Kind::File => entry.is_metadata_file(),
                    Kind::Directory => entry.is_metadata_directory(),
                    Kind::Link => entry.kind == Kind::Link,
                },
                Predicate::Cloud(filter) => filter.matches(entry.cloud),
                Predicate::Path(_) | Predicate::PathGlob(_) => continue,
            };
            if !matched {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }

    pub fn matches_path(
        &self,
        path: Option<&str>,
        budget: &mut u64,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Option<bool>, QueryError> {
        let Some(path) = path else {
            return Ok(Some(false));
        };
        let path = path.to_lowercase();
        for term in &self.terms {
            let matched = match term {
                Predicate::Path(value) => path.contains(value),
                Predicate::PathGlob(glob) => {
                    let Some(matched) = glob.matches(&path, budget, cancelled)? else {
                        return Ok(None);
                    };
                    matched
                }
                _ => continue,
            };
            if !matched {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }
}

fn contains(name: &str, lowercase: &str) -> bool {
    if name.is_ascii() && !name.bytes().any(|b| b.is_ascii_uppercase()) {
        name.contains(lowercase)
    } else {
        name.to_lowercase().contains(lowercase)
    }
}
