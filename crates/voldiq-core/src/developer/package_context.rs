use crate::{Entry, Kind, NodeId, cloud::CloudState, index::Index, metadata::path_key};
use serde::Serialize;
use std::collections::HashMap;

const OLD_INSTALL_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    ProjectAssociated,
    SharedStore,
    PossibleRemnant,
    Protected,
    Unknown,
}

#[derive(Serialize)]
pub struct Context {
    pub status: Status,
    pub evidence: &'static str,
    pub marker_modified: Option<u64>,
    pub rule_version: u8,
}

impl Context {
    fn new(status: Status, evidence: &'static str, marker_modified: Option<u64>) -> Self {
        Self {
            status,
            evidence,
            marker_modified,
            rule_version: 1,
        }
    }

    pub(super) fn shared() -> Self {
        Self::new(
            Status::SharedStore,
            "Falls within the recorded configuration for NuGet global packages.",
            None,
        )
    }

    pub(super) fn unverified(&mut self) {
        self.status = Status::Unknown;
        self.evidence = "Recorded install layout found. Current folder identity or protection could not be confirmed.";
    }

    pub(super) fn protected(&mut self) {
        self.status = Status::Protected;
        self.evidence = "The package directory or its parent is protected from cleanup.";
    }
}

#[derive(Default)]
struct Markers {
    node_project: bool,
    node_lock: bool,
    npm_install: Option<NodeId>,
    python_project: bool,
    python_lock: bool,
    venv_configuration: Option<NodeId>,
    venv_interpreter: bool,
    venv_packages: bool,
}

#[derive(Default)]
pub(super) struct Signals {
    by_parent: HashMap<NodeId, Markers>,
}

impl Signals {
    pub(super) fn observe(&mut self, index: &Index, entry: &Entry) {
        let Some(parent) = entry.parent else {
            return;
        };
        if entry.incomplete || entry.cloud != CloudState::NotPlaceholder {
            return;
        }
        if entry.kind == Kind::File {
            match entry.name.as_str() {
                "package.json" | "pnpm-workspace.yaml" => {
                    self.by_parent.entry(parent).or_default().node_project = true;
                }
                "package-lock.json" => {
                    self.by_parent.entry(parent).or_default().node_lock = true;
                }
                ".package-lock.json" => {
                    self.by_parent.entry(parent).or_default().npm_install = Some(entry.id);
                }
                "pyproject.toml" | "Pipfile" | "requirements.txt" => {
                    self.by_parent.entry(parent).or_default().python_project = true;
                }
                "Pipfile.lock" | "poetry.lock" => {
                    self.by_parent.entry(parent).or_default().python_lock = true;
                }
                "pyvenv.cfg" => {
                    self.by_parent.entry(parent).or_default().venv_configuration = Some(entry.id);
                }
                _ => (),
            }
        }
        if entry.kind == Kind::File && entry.name.eq_ignore_ascii_case("python.exe") {
            if let Some(root) = layout_root(index, parent, "Scripts") {
                self.by_parent.entry(root).or_default().venv_interpreter = true;
            }
        } else if entry.kind == Kind::Directory
            && entry.name.eq_ignore_ascii_case("site-packages")
            && let Some(root) = layout_root(index, parent, "Lib")
        {
            self.by_parent.entry(root).or_default().venv_packages = true;
        }
    }

    pub(super) fn classify(
        &self,
        index: &Index,
        entry: &Entry,
        scope: NodeId,
    ) -> Option<(&'static str, Context)> {
        let markers = self.by_parent.get(&entry.id);
        let parent = entry
            .parent
            .filter(|&id| index.in_scope(id, scope))
            .and_then(|id| index.entry(id).ok());
        let project = parent.and_then(|parent| self.by_parent.get(&parent.id));
        let npm_install = markers.and_then(|markers| markers.npm_install);
        if entry.name.eq_ignore_ascii_case("node_modules")
            && (npm_install.is_some() || project.is_some_and(|files| files.node_project))
        {
            let modified = marker_date(index, npm_install);
            let context = if project.is_some_and(|files| files.node_project) {
                Context::new(
                    Status::ProjectAssociated,
                    "An indexed JavaScript project or workspace marker shares this directory's parent.",
                    modified,
                )
            } else if project.is_some_and(|files| files.node_lock)
                && possible(index, entry, parent, modified)
            {
                Context::new(
                    Status::PossibleRemnant,
                    "An npm hidden lockfile at least 30 days old and a sibling lockfile were indexed. The parent has no indexed project manifest.",
                    modified,
                )
            } else {
                Context::new(
                    Status::Unknown,
                    "The indexed npm hidden lockfile identifies an install layout. Project association is unconfirmed.",
                    modified,
                )
            };
            return Some(("Node.js", context));
        }
        let markers = markers.filter(|files| files.venv_interpreter && files.venv_packages)?;
        let configuration = markers.venv_configuration?;
        let modified = marker_date(index, Some(configuration));
        let context = if project.is_some_and(|files| files.python_project) {
            Context::new(
                Status::ProjectAssociated,
                "The indexed pyvenv.cfg, Scripts/python.exe and Lib/site-packages match a Windows virtual environment beside a Python project marker.",
                modified,
            )
        } else if project.is_some_and(|files| files.python_lock)
            && possible(index, entry, parent, modified)
        {
            Context::new(
                Status::PossibleRemnant,
                "The Windows environment has a pyvenv.cfg at least 30 days old. A sibling lockfile remains and the parent has no indexed project manifest.",
                modified,
            )
        } else {
            Context::new(
                Status::Unknown,
                "The indexed pyvenv.cfg, interpreter and package layout identify a Windows virtual environment. Project association is unconfirmed.",
                modified,
            )
        };
        Some(("Python", context))
    }
}

fn layout_root(index: &Index, parent: NodeId, name: &str) -> Option<NodeId> {
    let folder = index.entry(parent).ok()?;
    if folder.kind == Kind::Directory
        && folder.name.eq_ignore_ascii_case(name)
        && !folder.incomplete
    {
        folder.parent
    } else {
        None
    }
}

fn marker_date(index: &Index, marker: Option<NodeId>) -> Option<u64> {
    index.entry(marker?).ok()?.modified
}

fn possible(index: &Index, folder: &Entry, parent: Option<&Entry>, modified: Option<u64>) -> bool {
    if folder.incomplete || index.snapshot_id.is_some() {
        return false;
    }
    let Some(observation) = &index.observation else {
        return false;
    };
    if !modified.is_some_and(|date| {
        observation
            .completed_at
            .checked_sub(date)
            .is_some_and(|age| age >= OLD_INSTALL_SECONDS)
    }) {
        return false;
    }
    let Some(parent) = parent.filter(|parent| !parent.incomplete) else {
        return false;
    };
    index.path(parent.id).is_ok_and(|path| {
        let path = path_key(&path);
        index.exclusions.iter().all(|excluded| {
            let excluded = path_key(excluded);
            !excluded.starts_with(&path) && !path.starts_with(excluded)
        })
    })
}
