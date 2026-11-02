use crate::{Error, Kind, Result, index::Query};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    CurrentFolder,
    AnalysisRoot,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Overview,
    #[default]
    Explorer,
    Large,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Representation {
    Treemap,
    City,
    Sunburst,
    Circles,
    Voronoi,
    #[default]
    Table,
    Grid,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    #[default]
    Logical,
    Allocated,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    #[default]
    Type,
    Size,
    Age,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    pub query: Query,
    pub scope: Scope,
    pub view: View,
    pub representation: Representation,
    pub metric: Metric,
    pub depth: u8,
    pub color: Color,
}
impl Default for ViewSpec {
    fn default() -> Self {
        Self {
            query: Query::default(),
            scope: Scope::default(),
            view: View::default(),
            representation: Representation::default(),
            metric: Metric::default(),
            depth: 6,
            color: Color::default(),
        }
    }
}
impl ViewSpec {
    pub fn validate(&self) -> Result<()> {
        let q = &self.query;
        q.compile()?;
        if q.parent.is_some() || q.scope.is_some() || q.offset != 0 || q.limit != 100 {
            return Err(Error::Invalid(
                "Saved views cannot contain analysis node IDs or pagination.".into(),
            ));
        }
        let large = self.view == View::Large;
        if !(1..=6).contains(&self.depth)
            || q.min_size > 9_007_199_254_740_991
            || (q.without_extension && !q.extension.is_empty())
            || q.modified_before
                .is_some_and(|v| v % 86400 != 0 || v > 253_402_214_400)
            || (large
                && (!matches!(q.kind, Some(Kind::File | Kind::Directory))
                    || q.files_only != (q.kind == Some(Kind::File))))
            || (!large && (q.kind.is_some() || q.files_only || q.min_size != 0))
            || (self.view == View::Overview
                && matches!(
                    self.representation,
                    Representation::Table | Representation::Grid
                ))
            || (large && self.representation != Representation::Table)
            || (q.sort == "allocated"
                && (self.view != View::Explorer || self.metric != Metric::Allocated))
            || (q.sort == "size" && self.view == View::Explorer && self.metric == Metric::Allocated)
        {
            return Err(Error::Invalid("This view contains filters or presentation values the desktop cannot restore exactly.".into()));
        }
        Ok(())
    }
}
