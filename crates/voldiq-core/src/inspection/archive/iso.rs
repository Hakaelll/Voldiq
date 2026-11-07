use super::{Kind, MAX_DEPTH, Report, invalid, safe_path};
use std::{
    collections::{HashSet, VecDeque},
    io::{self, Read, Seek, SeekFrom},
};

const SECTOR: u64 = 2048;

#[derive(Clone, Copy)]
struct Volume {
    end: u64,
    block: u64,
    root: Extent,
    joliet: bool,
}

#[derive(Clone, Copy)]
struct Extent {
    start: u64,
    length: u64,
}

struct Record<'a> {
    extent: Extent,
    kind: Kind,
    name: &'a [u8],
    supported: bool,
    continued: bool,
}

pub(super) fn list(
    source: &mut (impl Read + Seek),
    length: u64,
    report: &mut Report,
) -> io::Result<()> {
    let volume = volume(source, length)?;
    let mut pending = VecDeque::from([(volume.root, String::new(), 1)]);
    let mut seen = HashSet::from([volume.root.start]);
    let mut buffer = [0; SECTOR as usize];
    while let Some((directory, prefix, depth)) = pending.pop_front() {
        let mut position = directory.start;
        let end = position + directory.length;
        let mut continuation: Option<Vec<u8>> = None;
        while position < end {
            let size = (SECTOR - position % SECTOR).min(end - position) as usize;
            source.seek(SeekFrom::Start(position))?;
            source.read_exact(&mut buffer[..size])?;
            position += size as u64;
            let mut sector = &buffer[..size];
            while let Some(&record_size) = sector.first().filter(|&&byte| byte != 0) {
                let data = sector.get(..usize::from(record_size)).ok_or_else(invalid)?;
                sector = &sector[usize::from(record_size)..];
                let record = record(data, volume.block, volume.end)?;
                if matches!(record.name, [0] | [1]) {
                    continue;
                }
                if !report.next() {
                    return Ok(());
                }
                let split = continuation.take();
                if split
                    .as_ref()
                    .is_some_and(|expected| expected != record.name)
                {
                    return Err(invalid());
                }
                continuation = record.continued.then(|| record.name.to_vec());
                let path = if record.supported && split.is_none() {
                    name(&record, volume.joliet).and_then(|name| {
                        if prefix.is_empty() {
                            safe_path(&name)
                        } else {
                            safe_path(&format!("{prefix}/{name}"))
                        }
                    })
                } else {
                    None
                };
                if let Some(path) = &path
                    && record.kind == Kind::Folder
                {
                    if depth < MAX_DEPTH && seen.insert(record.extent.start) {
                        pending.push_back((record.extent, path.clone(), depth + 1));
                    } else if record.extent.length != 0 {
                        report.omitted += 1;
                    }
                }
                report.push(path, record.kind, record.extent.length);
            }
        }
        if continuation.is_some() {
            return Err(invalid());
        }
    }
    Ok(())
}

fn volume(source: &mut (impl Read + Seek), length: u64) -> io::Result<Volume> {
    let mut primary = None;
    let mut joliet = None;
    let mut descriptor = [0; SECTOR as usize];
    source.seek(SeekFrom::Start(16 * SECTOR))?;
    for _ in 0..64 {
        source.read_exact(&mut descriptor)?;
        if &descriptor[1..6] != b"CD001" || descriptor[6] != 1 {
            return Err(invalid());
        }
        match descriptor[0] {
            1 if primary.is_none() => primary = Some(describe(&descriptor, length, false)?),
            2 if joliet.is_none() && matches!(&descriptor[88..91], b"%/@" | b"%/C" | b"%/E") => {
                joliet = Some(describe(&descriptor, length, true)?);
            }
            255 if primary.is_some() => return joliet.or(primary).ok_or_else(invalid),
            255 => break,
            _ => (),
        }
    }
    Err(invalid())
}

fn describe(descriptor: &[u8], length: u64, joliet: bool) -> io::Result<Volume> {
    let block = u64::from(both16(&descriptor[128..])?);
    let end = block * u64::from(both32(&descriptor[80..])?);
    if !matches!(block, 512 | 1024 | 2048)
        || end > length
        || end < 17 * SECTOR
        || both16(&descriptor[120..])? != 1
        || both16(&descriptor[124..])? != 1
    {
        return Err(invalid());
    }
    let root = record(&descriptor[156..190], block, end)?;
    if root.kind != Kind::Folder || root.name != [0] || !root.supported {
        return Err(invalid());
    }
    Ok(Volume {
        end,
        block,
        root: root.extent,
        joliet,
    })
}

fn record(data: &[u8], block: u64, end: u64) -> io::Result<Record<'_>> {
    if data.len() < 34 || usize::from(data[0]) != data.len() {
        return Err(invalid());
    }
    let start = (u64::from(both32(&data[2..])?) + u64::from(data[1])) * block;
    let length = u64::from(both32(&data[10..])?);
    if start + length > end {
        return Err(invalid());
    }
    let name_len = usize::from(data[32]);
    let name = data.get(33..33 + name_len).ok_or_else(invalid)?;
    let system = data
        .get(33 + name_len + usize::from(name_len.is_multiple_of(2))..)
        .ok_or_else(invalid)?;
    let flags = data[25];
    let mut supported =
        name_len != 0 && flags & 0x84 == 0 && data[26..28] == [0, 0] && both16(&data[28..])? == 1;
    let mut kind = if flags & 2 != 0 {
        Kind::Folder
    } else {
        Kind::File
    };
    // Rock Ridge links and relocated directories are never traversed.
    let mut system = system;
    while system.len() >= 4 {
        let size = usize::from(system[2]);
        if size < 4 || size > system.len() {
            break;
        }
        match &system[..2] {
            b"SL" if system[3] == 1 => kind = Kind::Link,
            b"CL" | b"RE" if system[3] == 1 => supported = false,
            b"ST" => break,
            _ => (),
        }
        system = &system[size..];
    }
    Ok(Record {
        extent: Extent { start, length },
        kind,
        name,
        supported,
        continued: flags & 0x80 != 0,
    })
}

fn name(record: &Record<'_>, joliet: bool) -> Option<String> {
    let value = if joliet {
        if !record.name.len().is_multiple_of(2) {
            return None;
        }
        String::from_utf16(
            &record
                .name
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )
        .ok()?
    } else {
        std::str::from_utf8(record.name).ok()?.to_owned()
    };
    if value.contains('/') || matches!(value.trim_end(), "." | "..") {
        return None;
    }
    if record.kind != Kind::Folder
        && let Some((name, version)) = value.rsplit_once(';')
        && !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Some(name.trim_end_matches('.').to_owned());
    }
    Some(value)
}

fn both16(bytes: &[u8]) -> io::Result<u16> {
    let little = u16::from_le_bytes([bytes[0], bytes[1]]);
    if little != u16::from_be_bytes([bytes[2], bytes[3]]) {
        return Err(invalid());
    }
    Ok(little)
}

fn both32(bytes: &[u8]) -> io::Result<u32> {
    let little = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    if little != u32::from_be_bytes(bytes[4..8].try_into().unwrap()) {
        return Err(invalid());
    }
    Ok(little)
}
