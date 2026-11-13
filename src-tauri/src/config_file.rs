use std::{fs::OpenOptions, io::Read, path::Path};

const MAX_CONFIG_BYTES: u64 = 65_536;

pub(crate) fn read_marker(path: &Path) -> Result<bool, ()> {
    match read(path)? {
        None => Ok(false),
        Some(marker) if marker.is_empty() => Ok(true),
        Some(_) => Err(()),
    }
}

pub(crate) fn set_marker(path: &Path, enabled: bool) -> Result<bool, ()> {
    if read_marker(path)? == enabled {
        return Ok(enabled);
    }
    let result = if enabled {
        std::fs::File::create_new(path).map(drop)
    } else {
        std::fs::remove_file(path)
    };
    // A second Voldiq window may already have applied the same choice.
    if result.is_ok() || read_marker(path) == Ok(enabled) {
        Ok(enabled)
    } else {
        Err(())
    }
}

pub(crate) fn read(path: &Path) -> Result<Option<String>, ()> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000 | 0x0010_0000); // No reparse traversal or cloud recall on this leaf.
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let metadata = file.metadata().map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return Err(());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & (0x400 | 0x1000 | 0x40000 | 0x400000) != 0 {
            return Err(());
        }
    }
    let mut text = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| ())?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err(());
    }
    Ok(Some(text))
}
