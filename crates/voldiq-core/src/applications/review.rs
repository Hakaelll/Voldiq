//! Application claims narrow discovery; the existing file rule and plan remain authoritative.
use super::*;
use crate::{
    cleanup::{Plan, rules},
    metadata::Protections,
};
use std::path::Path;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub scope: NodeId,
    pub application_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub context: Context,
    pub offset: usize,
    pub limit: usize,
}

pub struct Environment<'a> {
    pub local_data: &'a Path,
    pub now: u64,
    pub protections: &'a Protections,
}

#[derive(Serialize)]
pub struct Page {
    pub application: Application,
    pub candidates: rules::Page,
}

fn resolve(
    index: &Index,
    catalog: Catalog,
    context: &Context,
    cancel: &AtomicBool,
) -> Result<(Application, NodeId)> {
    if index.snapshot_id.is_some()
        || context.application_id.len() != 64
        || !context
            .application_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Invalid(
            "Review a current application in the live analysis.".into(),
        ));
    }
    let query = Query {
        scope: context.scope,
        limit: 1,
        ..Default::default()
    };
    let page = query_selected(
        index,
        catalog,
        &query,
        cancel,
        crate::agents::pin_directory,
        Some(&context.application_id),
    )?;
    let application = page.applications.into_iter().next().ok_or_else(|| {
        Error::Invalid(
            "Application evidence changed. Refresh Applications before reviewing.".into(),
        )
    })?;
    let profile = application.profile_scope.ok_or_else(|| {
        Error::Invalid("A unique indexed profile location is unavailable.".into())
    })?;
    if matches!(application.confidence, Confidence::Ambiguous)
        || application.overlapping.files != 0
        || !application
            .breakdown
            .iter()
            .any(|part| part.category == Category::Cache && part.exclusive.files > 0)
    {
        return Err(Error::Invalid(
            "Application cache claims are unavailable or overlapping. Review files manually."
                .into(),
        ));
    }
    let scope = if index.in_scope(profile, context.scope) {
        profile
    } else if index.in_scope(context.scope, profile) {
        context.scope
    } else {
        return Err(Error::Invalid(
            "Profile is outside the requested scope.".into(),
        ));
    };
    Ok((application, scope))
}

pub fn query(
    index: &Index,
    catalog: Catalog,
    request: &Request,
    environment: &Environment<'_>,
    cancel: &AtomicBool,
) -> Result<Page> {
    if request.limit == 0 || request.limit > 100 || request.offset > index.entries.len() {
        return Err(Error::Invalid(
            "Invalid application cache page bounds.".into(),
        ));
    }
    let (application, scope) = resolve(index, catalog, &request.context, cancel)?;
    let candidates = rules::query_browser_cache_scoped(
        index,
        environment.local_data,
        environment.now,
        &rules::BrowserCacheQuery {
            scope,
            offset: request.offset,
            limit: request.limit,
        },
        environment.protections,
        cancel,
    )?;
    check(cancel)?;
    Ok(Page {
        application,
        candidates,
    })
}

pub fn preview(
    index: &Index,
    catalog: Catalog,
    context: &Context,
    environment: &Environment<'_>,
    selection: &[NodeId],
    cancel: &AtomicBool,
) -> Result<Plan> {
    if selection.is_empty()
        || selection.len() > 500
        || selection.iter().collect::<HashSet<_>>().len() != selection.len()
    {
        return Err(Error::Invalid(
            "Select between 1 and 500 distinct files.".into(),
        ));
    }
    let (_, scope) = resolve(index, catalog, context, cancel)?;
    let mask = environment.protections.for_index(index, cancel)?;
    for &id in selection {
        index.entry(id)?;
        if !index.in_scope(id, scope) || !mask.allows(id) {
            return Err(Error::Invalid(
                "Selected file is outside this application review or is protected.".into(),
            ));
        }
    }
    check(cancel)?;
    rules::preview_browser_cache(index, environment.local_data, environment.now, selection)
}
