use crate::{
    Error, Result,
    agents::pin_directory,
    windows::{self, cleanup_io},
};
use parley::{FontContext, fontique::Blob};
use skrifa::{
    MetadataProvider,
    raw::{FileRef, TableProvider},
};
use std::{
    collections::BTreeSet,
    io::Read,
    os::windows::fs::MetadataExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    FILE_ATTRIBUTE_REPARSE_POINT,
};

const FILE_LIMIT: u64 = 32 * 1024 * 1024;
const RETAINED_LIMIT: usize = 64 * 1024 * 1024;
const READ_LIMIT: usize = 256 * 1024 * 1024;

pub(super) struct Fonts {
    pub context: FontContext,
    pub families: Vec<String>,
}

/// Read installed OS font resources only when a PDF is requested. Fontique system discovery is off.
pub(super) fn load(text: &str, cancelled: &AtomicBool) -> Result<Option<Fonts>> {
    let mut missing: BTreeSet<char> = text.chars().filter(|c| {
        !c.is_control() && !matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
    }).collect();
    let directory = pin_directory(&windows::fonts_directory()?)?;
    let parent = windows::open_metadata(directory.path())?;
    if windows::file_identity(&parent)?.key != directory.identity() {
        return Err(Error::Invalid("Windows font directory changed.".into()));
    }
    let mut names = Vec::new();
    for (position, entry) in std::fs::read_dir(directory.path())?.enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if position >= 4096 {
            return Err(Error::Invalid(
                "Windows font directory exceeds the PDF resource limit.".into(),
            ));
        }
        let entry = entry?;
        if entry
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "ttf" | "ttc" | "otf"))
        {
            names.push(entry.file_name());
        }
    }
    names.sort_by_key(|name| {
        let name = name.to_string_lossy().to_ascii_lowercase();
        (
            match name.as_str() {
                "segoeui.ttf" => 0,
                "arial.ttf" => 1,
                "seguisym.ttf" => 2,
                _ => 3,
            },
            name,
        )
    });
    let mut result = Fonts {
        context: FontContext::default(),
        families: Vec::new(),
    };
    let mut read: usize = 0;
    let mut retained = 0;
    for name in names {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        if missing.is_empty() {
            break;
        }
        let Ok(mut file) = cleanup_io::open_leaf(&parent, &name, cleanup_io::LeafAccess::Retain)
        else {
            continue;
        };
        let metadata = file.metadata()?;
        let forbidden = FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
        if !metadata.is_file()
            || metadata.file_attributes() & forbidden != 0
            || metadata.len() > FILE_LIMIT
        {
            continue;
        }
        if read.saturating_add(metadata.len() as usize) > READ_LIMIT {
            break;
        }
        let before = windows::file_identity(&file)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        Read::by_ref(&mut file)
            .take(FILE_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        read += bytes.len();
        let after = windows::file_identity(&file)?;
        if bytes.len() as u64 != metadata.len()
            || before.key != after.key
            || before.change_time != after.change_time
        {
            continue;
        }
        let Ok(fonts) = FileRef::new(&bytes) else {
            continue;
        };
        // Collections are registered as a whole, so every face must allow editable/installable subsets.
        // Restricted, print-only, no-subsetting, bitmap-only and unrecognized permissions are excluded.
        if !fonts.fonts().all(|font| {
            font.ok()
                .and_then(|font| font.os2().ok().map(|os2| matches!(os2.fs_type(), 0 | 8)))
                .unwrap_or(false)
        }) {
            continue;
        }
        let mut covered = BTreeSet::new();
        for font in fonts.fonts().flatten() {
            let map = font.charmap();
            covered.extend(
                missing
                    .iter()
                    .copied()
                    .filter(|c| map.map(*c).is_some_and(|id| id.to_u32() != 0)),
            );
        }
        if covered.is_empty() {
            continue;
        }
        if retained + bytes.len() > RETAINED_LIMIT {
            break;
        }
        retained += bytes.len();
        let registered = result
            .context
            .collection
            .register_fonts(Blob::new(Arc::new(bytes)), None);
        if registered.is_empty() {
            continue;
        }
        for (id, _) in registered {
            if let Some(name) = result.context.collection.family_name(id)
                && !result.families.iter().any(|existing| existing == name)
            {
                result.families.push(name.to_owned());
            }
        }
        missing.retain(|c| !covered.contains(c));
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    if let Some(c) = missing.first() {
        return Err(Error::Invalid(format!(
            "No eligible installed font covers U+{:04X} within PDF resource limits. Use CSV or XLSX for this selection.",
            *c as u32
        )));
    }
    Ok(Some(result))
}
