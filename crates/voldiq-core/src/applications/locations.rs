use super::*;
use std::path::Component;
#[cfg(windows)]
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Installation,
    Data,
    Cache,
    Logs,
    Unknown,
}

#[derive(Clone, Serialize)]
pub struct CategoryMeasurement {
    pub category: Category,
    pub exclusive: Footprint,
    pub overlapping: Footprint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserLayout {
    ChromeStable,
    EdgeStable,
}

#[derive(Clone, Debug)]
pub struct ProfileLocation {
    pub path: PathBuf,
    pub layout: BrowserLayout,
}

impl BrowserLayout {
    pub(super) fn registered(record: &Registration) -> Option<Self> {
        let publisher = record.publisher.as_deref()?;
        if record.key.eq_ignore_ascii_case("Google Chrome")
            && record.name.eq_ignore_ascii_case("Google Chrome")
            && ["Google LLC", "Google Inc."]
                .iter()
                .any(|name| publisher.eq_ignore_ascii_case(name))
        {
            Some(Self::ChromeStable)
        } else if record.key.eq_ignore_ascii_case("Microsoft Edge")
            && record.name.eq_ignore_ascii_case("Microsoft Edge")
            && publisher.eq_ignore_ascii_case("Microsoft Corporation")
        {
            Some(Self::EdgeStable)
        } else {
            None
        }
    }

    #[cfg(windows)]
    pub(super) fn default_path(self, local: &Path) -> PathBuf {
        match self {
            Self::ChromeStable => local.join("Google/Chrome/User Data"),
            Self::EdgeStable => local.join("Microsoft/Edge/User Data"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Location {
    pub root: NodeId,
    pub priority: usize,
    pub(super) profile: Option<BrowserLayout>,
    pub(super) prefix: Vec<String>,
    pub(super) prefix_depth: usize,
    pub(super) parent_hops: usize,
}

impl Location {
    pub(super) fn bind(
        root_path: &std::path::Path,
        identities: &HashMap<(u64, u64), NodeId>,
        ancestors: &[(u64, u64)],
        identity: (u64, u64),
        profile: Option<BrowserLayout>,
    ) -> Option<Self> {
        let ancestor = ancestors.iter().position(|value| *value == identity);
        let root = identities
            .get(&identity)
            .copied()
            .or_else(|| ancestor.map(|_| NodeId(0)))?;
        let prefix_depth = if profile.is_some() && root == NodeId(0) {
            ancestors.len() - 1 - ancestor?
        } else {
            0
        };
        let mut names: Vec<_> = root_path
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .rev()
            .take(prefix_depth)
            .collect();
        names.reverse();
        names.truncate(3);
        Some(Self {
            root,
            priority: ancestor.unwrap_or(ancestors.len()),
            profile,
            prefix: names,
            prefix_depth,
            parent_hops: ancestor.map_or(0, |position| ancestors.len() - 1 - position),
        })
    }

    pub(super) fn category(
        &self,
        trail: &[Option<&str>; 3],
        depth: usize,
        is_file: bool,
    ) -> Category {
        let Some(layout) = self.profile else {
            return Category::Installation;
        };
        let relative = |slot: usize| {
            if slot < self.prefix_depth {
                self.prefix.get(slot).map(String::as_str)
            } else {
                trail.get(slot - self.prefix_depth).copied().flatten()
            }
        };
        let profile = relative(0);
        let known_profile = profile.is_some_and(|name| {
            name.eq_ignore_ascii_case("Default")
                || name.split_once(' ').is_some_and(|(prefix, number)| {
                    prefix.eq_ignore_ascii_case("Profile")
                        && !number.is_empty()
                        && number.bytes().all(|byte| byte.is_ascii_digit())
                })
        });
        if self.prefix_depth + depth >= 2
            && known_profile
            && relative(1).is_some_and(|name| name.eq_ignore_ascii_case("Cache"))
        {
            Category::Cache
        } else if is_file
            && layout == BrowserLayout::ChromeStable
            && self.prefix_depth + depth == 1
            && relative(0).is_some_and(|name| name.eq_ignore_ascii_case("chrome_debug.log"))
        {
            Category::Logs
        } else {
            Category::Data
        }
    }
}
