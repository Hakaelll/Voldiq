use super::*;

pub(super) fn visit<'a>(
    index: &Index,
    locations: impl Iterator<Item = &'a [locations::Location]>,
    scope: Option<NodeId>,
    cancelled: &mut impl FnMut() -> bool,
    mut accept: impl FnMut(&crate::Entry, &HashMap<usize, (usize, usize, Category)>) -> Result<()>,
) -> Result<()> {
    let mut claims: HashMap<NodeId, Vec<(usize, &locations::Location)>> = HashMap::new();
    for (slot, locations) in locations.enumerate() {
        for location in locations {
            claims
                .entry(location.root)
                .or_default()
                .push((slot, location));
        }
    }
    let mut owners = HashMap::new();
    for entry in &index.entries {
        if entry.id.0 % 256 == 0 && cancelled() {
            return Err(Error::Invalid("Application query cancelled.".into()));
        }
        if !entry.is_metadata_file() && entry.kind != Kind::Directory {
            continue;
        }
        owners.clear();
        let mut within_scope = scope.is_none();
        let mut parent = Some(entry.id);
        let mut depth = 0;
        let mut trail = [None; 3];
        while let Some(id) = parent {
            if depth % 256 == 0 && cancelled() {
                return Err(Error::Invalid("Application query cancelled.".into()));
            }
            if depth > index.entries.len() {
                return Err(Error::Invalid("Invalid index ancestry.".into()));
            }
            within_scope |= Some(id) == scope;
            if let Some(apps) = claims.get(&id) {
                for &(slot, location) in apps {
                    let category = location.category(&trail, depth, entry.is_metadata_file());
                    let observed =
                        owners
                            .entry(slot)
                            .or_insert((depth, location.priority, category));
                    if depth == observed.0 {
                        if location.priority > observed.1 {
                            *observed = (depth, location.priority, category)
                        } else if location.priority == observed.1 && category != observed.2 {
                            observed.2 = Category::Unknown
                        }
                    }
                }
            }
            trail = [Some(index.entry(id)?.name.as_str()), trail[0], trail[1]];
            parent = index.entry(id)?.parent;
            depth += 1;
        }
        if within_scope {
            accept(entry, &owners)?;
        }
    }
    if cancelled() {
        return Err(Error::Invalid("Application query cancelled.".into()));
    }
    Ok(())
}
