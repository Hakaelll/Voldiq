use super::*;
use crate::cleanup::{MIXED_REVIEW_RULE, Plan};
use std::path::Path;

pub struct Environment<'a> {
    pub temporary: &'a Path,
    pub local_data: &'a Path,
    pub caches: &'a [crate::developer::CacheLocation],
    pub state: &'a Path,
    pub now: u64,
    pub duplicates: Option<&'a crate::duplicates::Analysis>,
}

pub struct Prepared {
    pub plan: Plan,
    pub origins: Vec<Origin>,
}

pub fn prepare(
    index: &Index,
    groups: &[Group],
    env: &Environment<'_>,
    cancel: &AtomicBool,
) -> Result<Prepared> {
    validate(index, groups, cancel)?;
    let protections = crate::metadata::Store::open(env.state)?.protections()?;
    let mut catalog = None;
    let mut parts = Vec::new();
    for group in groups {
        super::super::check_cancelled(cancel)?;
        let selection = &group.selection;
        let plan = match &group.source {
            Source::Manual => Plan::prepare(index, selection, cancel),
            Source::Duplicates => {
                let analysis = env.duplicates.ok_or_else(|| {
                    Error::Invalid(
                        "Analyze duplicates in this analysis before preparing their review.".into(),
                    )
                })?;
                analysis.check_protections(&protections)?;
                Plan::duplicates(index, analysis, selection, cancel)
            }
            Source::UserTemp => {
                super::super::rules::preview(index, env.temporary, env.now, selection)
            }
            Source::OldFiles => super::super::rules::preview_old(index, env.now, selection),
            Source::ProjectTemp => {
                super::super::rules::preview_project(index, env.now, selection, env.caches)
            }
            Source::BrowserCache { application: None } => {
                super::super::rules::preview_browser_cache(
                    index,
                    env.local_data,
                    env.now,
                    selection,
                )
            }
            Source::BrowserCache {
                application: Some(context),
            } => {
                if catalog.is_none() {
                    catalog = Some(crate::applications::installed_programs(cancel)?);
                }
                crate::applications::review::preview(
                    index,
                    catalog
                        .as_ref()
                        .ok_or_else(|| Error::Invalid("Application evidence unavailable.".into()))?
                        .clone(),
                    context,
                    &crate::applications::review::Environment {
                        local_data: env.local_data,
                        now: env.now,
                        protections: &protections,
                    },
                    selection,
                    cancel,
                )
            }
        }?;
        plan.validate_retained()?;
        parts.push(plan);
    }
    let mut parts = parts.into_iter();
    let mut combined = parts
        .next()
        .ok_or_else(|| Error::Invalid("The review tray is empty.".into()))?;
    if groups.len() > 1 {
        combined.preview.rule = MIXED_REVIEW_RULE;
        for part in parts {
            if part.preview.policy != "recycle" || part.protections.is_some() {
                return Err(Error::Invalid(
                    "Incompatible cleanup review sources.".into(),
                ));
            }
            combined.files.extend(part.files);
            combined.retained.extend(part.retained);
            combined.preview.items.extend(part.preview.items);
        }
        combined.preview.logical = combined
            .preview
            .items
            .iter()
            .try_fold(0u64, |total, item| total.checked_add(item.logical))
            .ok_or_else(|| Error::Invalid("Review size overflow.".into()))?;
        combined.preview.reclaimable = combined
            .preview
            .items
            .iter()
            .try_fold(0u64, |total, item| total.checked_add(item.reclaimable))
            .ok_or_else(|| Error::Invalid("Review allocation overflow.".into()))?;
    }
    let selected: HashSet<_> = combined
        .files
        .iter()
        .map(|file| file.stamp.identity)
        .collect();
    let mut keepers = HashMap::new();
    for (file, digest) in &combined.retained {
        if selected.contains(&file.stamp.identity) {
            return Err(Error::Invalid("A retained duplicate copy is also selected in another review group. Remove it from the tray.".into()));
        }
        if let Some((stamp, previous)) = keepers.insert(file.stamp.identity, (&file.stamp, digest))
            && (stamp != &file.stamp || previous != digest)
        {
            return Err(Error::Invalid(
                "Retained-copy evidence changed between review groups.".into(),
            ));
        }
    }
    let mut identities = HashSet::new();
    combined
        .retained
        .retain(|(file, _)| identities.insert(file.stamp.identity));
    combined.preview.retained = combined
        .retained
        .iter()
        .map(|(file, _)| file.path.to_string_lossy().into_owned())
        .collect();
    combined.preview.retained.sort();
    combined.validate_retained()?;
    let plan = combined.with_protections(env.state)?;
    if plan
        .protections
        .as_ref()
        .is_none_or(|frame| frame.fingerprint != protections.fingerprint())
    {
        return Err(Error::Invalid(
            "Protections changed while preparing the tray.".into(),
        ));
    }
    super::super::check_cancelled(cancel)?;
    Ok(Prepared {
        plan,
        origins: groups.iter().map(|group| group.source.origin()).collect(),
    })
}
