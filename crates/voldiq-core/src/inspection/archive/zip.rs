use super::{Kind, MAX_PATH, Report, invalid, safe_path};
use std::io::{self, Read, Seek, SeekFrom};

pub(super) fn list(
    source: &mut (impl Read + Seek),
    length: u64,
    report: &mut Report,
) -> io::Result<()> {
    let (start, size, count) = directory(source, length)?;
    let end = start.checked_add(size).ok_or_else(invalid)?;
    source.seek(SeekFrom::Start(start))?;
    let mut position = start;
    for _ in 0..count {
        if !report.next() {
            return Ok(());
        }
        if end - position < 46 {
            return Err(invalid());
        }
        let mut header = [0; 46];
        source.read_exact(&mut header)?;
        if &header[..4] != b"PK\x01\x02" {
            return Err(invalid());
        }
        let name_len = le16(&header[28..]) as usize;
        let extra_len = le16(&header[30..]) as usize;
        let comment_len = le16(&header[32..]) as usize;
        position += 46;
        let next = position + (name_len + extra_len + comment_len) as u64;
        if next > end || le16(&header[34..]) != 0 {
            return Err(invalid());
        }
        let flags = le16(&header[8..]);
        let attributes = le32(&header[38..]);
        let mode = if matches!(header[5], 3 | 19) {
            attributes >> 16 & 0o170000
        } else {
            0
        };
        if name_len == 0
            || name_len > MAX_PATH
            || flags & (1 << 13) != 0
            || !matches!(mode, 0 | 0o040000 | 0o100000 | 0o120000)
        {
            report.omitted += 1;
        } else {
            let mut name = [0; MAX_PATH];
            let name = &mut name[..name_len];
            source.read_exact(name)?;
            let path = if flags & (1 << 11) != 0 {
                std::str::from_utf8(name).ok().and_then(safe_path)
            } else {
                safe_path(&cp437(name))
            };
            let kind = if mode == 0o120000 {
                Kind::Link
            } else if mode == 0o040000 || attributes & 0x10 != 0 || name.ends_with(b"/") {
                Kind::Folder
            } else {
                Kind::File
            };
            let size = le32(&header[24..]);
            let size = if size == u32::MAX {
                let mut extra = vec![0; extra_len];
                source.read_exact(&mut extra)?;
                extended_size(&extra)?
            } else {
                u64::from(size)
            };
            report.push(path, kind, size);
        }
        source.seek(SeekFrom::Start(next))?;
        position = next;
    }
    if position != end {
        // A central directory may end with a digital signature record.
        let mut signature = [0; 6];
        if end - position < signature.len() as u64 {
            return Err(invalid());
        }
        source.read_exact(&mut signature)?;
        if &signature[..4] != b"PK\x05\x05"
            || end - position != 6 + u64::from(le16(&signature[4..]))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn directory(source: &mut (impl Read + Seek), length: u64) -> io::Result<(u64, u64, u64)> {
    const FOOTER: usize = 22;
    let tail_len = length.min(FOOTER as u64 + u64::from(u16::MAX)) as usize;
    if tail_len < FOOTER {
        return Err(invalid());
    }
    let tail_start = length - tail_len as u64;
    let mut tail = vec![0; tail_len];
    source.seek(SeekFrom::Start(tail_start))?;
    source.read_exact(&mut tail)?;
    let end_at = (0..=tail_len - FOOTER)
        .rev()
        .find(|&at| {
            &tail[at..at + 4] == b"PK\x05\x06"
                && at + FOOTER + le16(&tail[at + 20..]) as usize == tail_len
        })
        .ok_or_else(invalid)?;
    let footer = &tail[end_at..];
    let footer_at = tail_start + end_at as u64;
    let mut count = u64::from(le16(&footer[10..]));
    let mut size = u64::from(le32(&footer[12..]));
    let mut start = u64::from(le32(&footer[16..]));
    if le32(&footer[4..]) != 0 || u64::from(le16(&footer[8..])) != count {
        return Err(invalid());
    }
    let mut directory_end = footer_at;
    if let Some(locator_at) = footer_at.checked_sub(20) {
        let mut locator = [0; 20];
        source.seek(SeekFrom::Start(locator_at))?;
        source.read_exact(&mut locator)?;
        if &locator[..4] == b"PK\x06\x07" {
            let extended_at = le64(&locator[8..]);
            if le32(&locator[4..]) != 0
                || le32(&locator[16..]) != 1
                || extended_at
                    .checked_add(56)
                    .is_none_or(|end| end > locator_at)
            {
                return Err(invalid());
            }
            let mut extended = [0; 56];
            source.seek(SeekFrom::Start(extended_at))?;
            source.read_exact(&mut extended)?;
            let body = le64(&extended[4..]);
            if &extended[..4] != b"PK\x06\x06"
                || body < 44
                || extended_at
                    .checked_add(12)
                    .and_then(|at| at.checked_add(body))
                    != Some(locator_at)
                || le64(&extended[16..]) != 0
                || le64(&extended[24..]) != le64(&extended[32..])
            {
                return Err(invalid());
            }
            count = le64(&extended[32..]);
            size = le64(&extended[40..]);
            start = le64(&extended[48..]);
            directory_end = extended_at;
        }
    }
    if start.checked_add(size) != Some(directory_end) || count > size / 46 {
        return Err(invalid());
    }
    Ok((start, size, count))
}

fn extended_size(mut extra: &[u8]) -> io::Result<u64> {
    while extra.len() >= 4 {
        let tag = le16(extra);
        let length = le16(&extra[2..]) as usize;
        let data = extra.get(4..4 + length).ok_or_else(invalid)?;
        if tag == 1 && data.len() >= 8 {
            return Ok(le64(data));
        }
        extra = &extra[4 + length..];
    }
    Err(invalid())
}

fn le16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes[..2].try_into().unwrap())
}
fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}
fn le64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}

fn cp437(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte < 128 {
                char::from(byte)
            } else {
                CP437[usize::from(byte - 128)]
            }
        })
        .collect()
}

const CP437: [char; 128] = [
    '\u{c7}', '\u{fc}', '\u{e9}', '\u{e2}', '\u{e4}', '\u{e0}', '\u{e5}', '\u{e7}', '\u{ea}',
    '\u{eb}', '\u{e8}', '\u{ef}', '\u{ee}', '\u{ec}', '\u{c4}', '\u{c5}', '\u{c9}', '\u{e6}',
    '\u{c6}', '\u{f4}', '\u{f6}', '\u{f2}', '\u{fb}', '\u{f9}', '\u{ff}', '\u{d6}', '\u{dc}',
    '\u{a2}', '\u{a3}', '\u{a5}', '\u{20a7}', '\u{192}', '\u{e1}', '\u{ed}', '\u{f3}', '\u{fa}',
    '\u{f1}', '\u{d1}', '\u{aa}', '\u{ba}', '\u{bf}', '\u{2310}', '\u{ac}', '\u{bd}', '\u{bc}',
    '\u{a1}', '\u{ab}', '\u{bb}', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}',
    '\u{2561}', '\u{2562}', '\u{2556}', '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255d}',
    '\u{255c}', '\u{255b}', '\u{2510}', '\u{2514}', '\u{2534}', '\u{252c}', '\u{251c}', '\u{2500}',
    '\u{253c}', '\u{255e}', '\u{255f}', '\u{255a}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}',
    '\u{2550}', '\u{256c}', '\u{2567}', '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}',
    '\u{2552}', '\u{2553}', '\u{256b}', '\u{256a}', '\u{2518}', '\u{250c}', '\u{2588}', '\u{2584}',
    '\u{258c}', '\u{2590}', '\u{2580}', '\u{3b1}', '\u{df}', '\u{393}', '\u{3c0}', '\u{3a3}',
    '\u{3c3}', '\u{b5}', '\u{3c4}', '\u{3a6}', '\u{398}', '\u{3a9}', '\u{3b4}', '\u{221e}',
    '\u{3c6}', '\u{3b5}', '\u{2229}', '\u{2261}', '\u{b1}', '\u{2265}', '\u{2264}', '\u{2320}',
    '\u{2321}', '\u{f7}', '\u{2248}', '\u{b0}', '\u{2219}', '\u{b7}', '\u{221a}', '\u{207f}',
    '\u{b2}', '\u{25a0}', '\u{a0}',
];
