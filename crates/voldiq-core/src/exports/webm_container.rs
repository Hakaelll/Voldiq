use crate::{Error, Result};
use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};

const SEGMENT: u32 = 0x18538067;
const CLUSTER: u32 = 0x1f43b675;
const INFO: u32 = 0x1549a966;
const TRACKS: u32 = 0x1654ae6b;

fn invalid() -> Error {
    Error::Invalid("Unsupported or invalid recorder WebM container.".into())
}

struct Budget<'a> {
    count: usize,
    cancel: &'a AtomicBool,
}
impl Budget<'_> {
    fn check(&mut self) -> Result<()> {
        self.count += 1;
        if self.count > 100_000 || self.cancel.load(Ordering::Relaxed) {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Header {
    id: u32,
    start: usize,
    end: Option<usize>,
}

fn vint(data: &[u8], pos: &mut usize, id: bool) -> Result<(u64, bool)> {
    let first = *data.get(*pos).ok_or_else(invalid)?;
    let width = first.leading_zeros() as usize + 1;
    if width > if id { 4 } else { 8 } {
        return Err(invalid());
    }
    let bytes = data.get(*pos..*pos + width).ok_or_else(invalid)?;
    let mut value = u64::from(if id {
        first
    } else {
        first & (0xff_u16 >> width) as u8
    });
    for byte in &bytes[1..] {
        value = (value << 8) | u64::from(*byte);
    }
    *pos += width;
    Ok((value, !id && value == (1_u64 << (7 * width)) - 1))
}
fn header(data: &[u8], pos: &mut usize, budget: &mut Budget) -> Result<Header> {
    budget.check()?;
    let (id, _) = vint(data, pos, true)?;
    let (size, unknown) = vint(data, pos, false)?;
    let start = *pos;
    let end = if unknown {
        None
    } else {
        Some(
            start
                .checked_add(usize::try_from(size).map_err(|_| invalid())?)
                .filter(|end| *end <= data.len())
                .ok_or_else(invalid)?,
        )
    };
    Ok(Header {
        id: u32::try_from(id).map_err(|_| invalid())?,
        start,
        end,
    })
}
fn children<'a>(data: &'a [u8], budget: &mut Budget) -> Result<Vec<(u32, &'a [u8])>> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < data.len() {
        let h = header(data, &mut pos, budget)?;
        let end = h.end.ok_or_else(invalid)?;
        out.push((h.id, &data[h.start..end]));
        pos = end;
    }
    Ok(out)
}
fn uint(data: &[u8]) -> Result<u64> {
    if data.is_empty() || data.len() > 8 {
        return Err(invalid());
    }
    Ok(data
        .iter()
        .fold(0, |value, byte| (value << 8) | u64::from(*byte)))
}
fn float(data: &[u8]) -> Result<f64> {
    let value = match data.len() {
        4 => f64::from(f32::from_be_bytes(data.try_into().map_err(|_| invalid())?)),
        8 => f64::from_be_bytes(data.try_into().map_err(|_| invalid())?),
        _ => return Err(invalid()),
    };
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid());
    }
    Ok(value)
}
fn one<'a>(parts: &[(u32, &'a [u8])], id: u32) -> Result<Option<&'a [u8]>> {
    let mut values = parts
        .iter()
        .filter(|(key, _)| *key == id)
        .map(|(_, value)| *value);
    let first = values.next();
    if values.next().is_some() {
        return Err(invalid());
    }
    Ok(first)
}
fn level_one(id: u32) -> bool {
    matches!(
        id,
        CLUSTER | INFO | TRACKS | 0x114d9b74 | 0x1c53bb6b | 0x1254c367 | 0x1941a469 | 0x1043a770
    )
}

struct Block<'a> {
    parts: Vec<(u32, &'a [u8])>,
    group: bool,
}
struct Cluster<'a> {
    time: u64,
    blocks: Vec<Block<'a>>,
}
pub(super) struct Container<'a> {
    scale: u64,
    duration: Option<f64>,
    track: Vec<u8>,
    clusters: Vec<Cluster<'a>>,
    pub codec: &'static str,
    pub blocks: usize,
    pub first_timestamp_ns: u128,
    pub last_timestamp_ns: u128,
}

pub(super) fn validate<'a>(
    data: &'a [u8],
    width: u16,
    height: u16,
    cancel: &AtomicBool,
) -> Result<Container<'a>> {
    if data.is_empty() || data.len() > 32 * 1024 * 1024 {
        return Err(invalid());
    }
    let mut budget = Budget { count: 0, cancel };
    let mut pos = 0;
    let h = header(data, &mut pos, &mut budget)?;
    if h.id != 0x1a45dfa3 {
        return Err(invalid());
    }
    let end = h.end.ok_or_else(invalid)?;
    let ebml = children(&data[h.start..end], &mut budget)?;
    if one(&ebml, 0x4282)? != Some(b"webm".as_slice()) {
        return Err(invalid());
    }
    for (id, value) in &ebml {
        let max = match id {
            0x4286 | 0x42f7 => 1,
            0x42f2 => 4,
            0x42f3 => 8,
            0x4287 | 0x4285 => 4,
            0x4282 => continue,
            _ => return Err(invalid()),
        };
        if uint(value)? == 0 || uint(value)? > max {
            return Err(invalid());
        }
    }
    pos = end;
    let segment = header(data, &mut pos, &mut budget)?;
    if segment.id != SEGMENT || segment.end.unwrap_or(data.len()) != data.len() {
        return Err(invalid());
    }
    let body = &data[segment.start..];
    let mut pos = 0;
    let mut info = None;
    let mut tracks = None;
    let mut raw_clusters = Vec::new();
    while pos < body.len() {
        let h = header(body, &mut pos, &mut budget)?;
        let end = if let Some(end) = h.end {
            end
        } else if h.id == CLUSTER {
            let mut end = h.start;
            while end < body.len() {
                let mut probe = end;
                let child = header(body, &mut probe, &mut budget)?;
                if level_one(child.id) {
                    break;
                }
                end = child.end.ok_or_else(invalid)?;
            }
            end
        } else {
            return Err(invalid());
        };
        match h.id {
            INFO => {
                if info.replace(&body[h.start..end]).is_some() {
                    return Err(invalid());
                }
            }
            TRACKS => {
                if tracks.replace(&body[h.start..end]).is_some() {
                    return Err(invalid());
                }
            }
            CLUSTER => {
                if raw_clusters.len() >= 4096 {
                    return Err(invalid());
                }
                raw_clusters.push(&body[h.start..end]);
            }
            0x114d9b74 | 0x1c53bb6b | 0x1254c367 | 0xec | 0xbf => {}
            _ => return Err(invalid()),
        }
        pos = end;
    }
    let parts = children(info.ok_or_else(invalid)?, &mut budget)?;
    let scale = one(&parts, 0x2ad7b1)?
        .map(uint)
        .transpose()?
        .unwrap_or(1_000_000);
    if !(1..=1_000_000_000).contains(&scale) {
        return Err(invalid());
    }
    let duration = one(&parts, 0x4489)?.map(float).transpose()?;
    if duration.is_some_and(|value| value * scale as f64 > 300_000_000_000.0) {
        return Err(invalid());
    }
    let track_parts = children(tracks.ok_or_else(invalid)?, &mut budget)?;
    let entry = one(&track_parts, 0xae)?.ok_or_else(invalid)?;
    if track_parts
        .iter()
        .any(|(id, _)| !matches!(id, 0xae | 0xec | 0xbf))
    {
        return Err(invalid());
    }
    let parts = children(entry, &mut budget)?;
    let number = uint(one(&parts, 0xd7)?.ok_or_else(invalid)?)?;
    if number == 0 || number > 127 || uint(one(&parts, 0x83)?.ok_or_else(invalid)?)? != 1 {
        return Err(invalid());
    }
    let codec = match one(&parts, 0x86)?.ok_or_else(invalid)? {
        b"V_VP8" => "V_VP8",
        b"V_VP9" => "V_VP9",
        _ => return Err(invalid()),
    };
    // Reject linked, encrypted, transformed, audio or auxiliary data rather than exporting it.
    if parts.iter().any(|(id, _)| {
        !matches!(
            id,
            0xd7 | 0x73c5
                | 0x83
                | 0xb9
                | 0x88
                | 0x55aa
                | 0x55ee
                | 0x9c
                | 0x23e383
                | 0x23314f
                | 0x537f
                | 0x536e
                | 0x22b59c
                | 0x22b59d
                | 0x86
                | 0x63a2
                | 0x258688
                | 0xe0
                | 0xec
                | 0xbf
        )
    }) {
        return Err(invalid());
    }
    if one(&parts, 0x55ee)?
        .map(uint)
        .transpose()?
        .is_some_and(|value| value != 0)
    {
        return Err(invalid());
    }
    if one(&parts, 0x23314f)?
        .map(float)
        .transpose()?
        .is_some_and(|v| v != 1.0)
    {
        return Err(invalid());
    }
    if one(&parts, 0x537f)?
        .map(uint)
        .transpose()?
        .is_some_and(|v| v != 0)
    {
        return Err(invalid());
    }
    let video = one(&parts, 0xe0)?.ok_or_else(invalid)?;
    let video_parts = children(video, &mut budget)?;
    if uint(one(&video_parts, 0xb0)?.ok_or_else(invalid)?)? != u64::from(width)
        || uint(one(&video_parts, 0xba)?.ok_or_else(invalid)?)? != u64::from(height)
    {
        return Err(invalid());
    }
    for (id, value) in &video_parts {
        match id {
            0xb0 | 0xba => {}
            0x54b0 | 0x54ba => {
                if uint(value)? == 0 || uint(value)? > 16384 {
                    return Err(invalid());
                }
            }
            0x9a => {
                if ![0, 2].contains(&uint(value)?) {
                    return Err(invalid());
                }
            }
            0x53b8 | 0x53c0 | 0x54aa | 0x54bb | 0x54cc | 0x54dd | 0x54b2 => {
                if uint(value)? != 0 {
                    return Err(invalid());
                }
            }
            0x9d | 0x54b3 => {
                if uint(value)? > 2 {
                    return Err(invalid());
                }
            }
            0x2383e3 => {
                if float(value)? > 240.0 {
                    return Err(invalid());
                }
            }
            0x2fb523 => {
                if float(value)? > 10.0 {
                    return Err(invalid());
                }
            }
            0x55b0 => {
                for (id, value) in children(value, &mut budget)? {
                    if !(0x55b1..=0x55bd).contains(&id) {
                        return Err(invalid());
                    }
                    uint(value)?;
                }
            }
            0xec | 0xbf => {}
            _ => return Err(invalid()),
        }
    }
    let mut track = Vec::new();
    element(&mut track, 0xd7, &uint_bytes(number))?;
    element(&mut track, 0x73c5, &uint_bytes(number))?;
    element(&mut track, 0x83, &[1])?;
    element(&mut track, 0x9c, &[0])?;
    element(&mut track, 0x86, codec.as_bytes())?;
    if let Some(private) = one(&parts, 0x63a2)? {
        if codec == "V_VP8" || private.len() > 12 || !private.len().is_multiple_of(3) {
            return Err(invalid());
        }
        let mut seen = [false; 5];
        for feature in private.chunks_exact(3) {
            let [id, length, value] = [feature[0], feature[1], feature[2]];
            let valid = match id {
                1 | 4 => value <= 3,
                2 => [10, 11, 20, 21, 30, 31, 40, 41, 50, 51, 52, 60, 61, 62].contains(&value),
                3 => [8, 10, 12].contains(&value),
                _ => false,
            };
            if !valid || length != 1 || seen[usize::from(id)] {
                return Err(invalid());
            }
            seen[usize::from(id)] = true;
        }
        element(&mut track, 0x63a2, private)?;
    }
    if let Some(default) = one(&parts, 0x23e383)? {
        let n = uint(default)?;
        if !(1_000_000..=2_000_000_000).contains(&n) {
            return Err(invalid());
        }
        element(&mut track, 0x23e383, &uint_bytes(n))?;
    }
    // Void and CRC fields can carry unrelated input bytes; rebuild the numeric video profile.
    let mut clean_video = Vec::new();
    for (id, value) in video_parts {
        if !matches!(id, 0xec | 0xbf) {
            element(&mut clean_video, id, value)?;
        }
    }
    element(&mut track, 0xe0, &clean_video)?;
    let mut out = Container {
        scale,
        duration,
        track,
        clusters: Vec::new(),
        codec,
        blocks: 0,
        first_timestamp_ns: u128::MAX,
        last_timestamp_ns: 0,
    };
    for raw in raw_clusters {
        let parts = children(raw, &mut budget)?;
        let time = uint(one(&parts, 0xe7)?.ok_or_else(invalid)?)?;
        if u128::from(time) * u128::from(scale) > 300_000_000_000 {
            return Err(invalid());
        }
        let mut cluster = Cluster {
            time,
            blocks: Vec::new(),
        };
        for (id, value) in parts {
            match id {
                0xe7 | 0xa7 | 0xab | 0xec | 0xbf => {}
                0xa3 => {
                    out.block(value, number, time, width, height, true)?;
                    cluster.blocks.push(Block {
                        parts: vec![(id, value)],
                        group: false,
                    });
                }
                0xa0 => {
                    let parts = children(value, &mut budget)?;
                    if parts
                        .iter()
                        .any(|(id, _)| !matches!(id, 0xa1 | 0x9b | 0xfb | 0xfa | 0xec | 0xbf))
                    {
                        return Err(invalid());
                    }
                    let data = one(&parts, 0xa1)?.ok_or_else(invalid)?;
                    out.block(data, number, time, width, height, false)?;
                    for (id, value) in &parts {
                        if matches!(id, 0x9b | 0xfa) {
                            let n = uint(value)?;
                            if *id == 0x9b && u128::from(n) * u128::from(scale) > 300_000_000_000 {
                                return Err(invalid());
                            }
                        } else if *id == 0xfb && (value.is_empty() || value.len() > 8) {
                            return Err(invalid());
                        }
                    }
                    cluster.blocks.push(Block {
                        parts: parts
                            .into_iter()
                            .filter(|(id, _)| !matches!(id, 0xec | 0xbf))
                            .collect(),
                        group: true,
                    });
                }
                _ => return Err(invalid()),
            }
        }
        out.clusters.push(cluster);
    }
    if out.blocks == 0
        || duration.is_some_and(|value| value * (scale as f64) < out.last_timestamp_ns as f64)
    {
        return Err(invalid());
    }
    Ok(out)
}

impl Container<'_> {
    fn block(
        &mut self,
        data: &[u8],
        number: u64,
        time: u64,
        width: u16,
        height: u16,
        simple: bool,
    ) -> Result<()> {
        self.blocks += 1;
        if self.blocks > 10000 {
            return Err(invalid());
        }
        let mut pos = 0;
        let (track, _) = vint(data, &mut pos, false)?;
        if track != number || data.len() < pos + 4 {
            return Err(invalid());
        }
        let relative = i16::from_be_bytes([data[pos], data[pos + 1]]);
        let flags = data[pos + 2];
        pos += 3;
        if flags & 0x76 != 0 || (!simple && flags & 0x80 != 0) {
            return Err(invalid());
        }
        let timestamp = i128::from(time) + i128::from(relative);
        if timestamp < 0 {
            return Err(invalid());
        }
        let ns = u128::try_from(timestamp)
            .map_err(|_| invalid())?
            .checked_mul(u128::from(self.scale))
            .ok_or_else(invalid)?;
        if ns > 300_000_000_000 {
            return Err(invalid());
        }
        if self.blocks > 1 && ns < self.last_timestamp_ns {
            return Err(invalid());
        }
        self.first_timestamp_ns = self.first_timestamp_ns.min(ns);
        self.last_timestamp_ns = self.last_timestamp_ns.max(ns);
        if self.codec == "V_VP8" {
            let frame = &data[pos..];
            if frame.len() < 3 {
                return Err(invalid());
            }
            let key = frame[0] & 1 == 0;
            if self.blocks == 1 && !key {
                return Err(invalid());
            }
            if key
                && (frame.len() < 10
                    || frame[3..6] != [0x9d, 0x01, 0x2a]
                    || (u16::from_le_bytes([frame[6], frame[7]]) & 0x3fff) != width
                    || (u16::from_le_bytes([frame[8], frame[9]]) & 0x3fff) != height)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn write(&self, writer: &mut impl Write, metadata: &[u8]) -> Result<()> {
        let mut ebml = Vec::new();
        for (id, value) in [
            (0x4286, 1),
            (0x42f7, 1),
            (0x42f2, 4),
            (0x42f3, 8),
            (0x4287, 4),
            (0x4285, 2),
        ] {
            element(&mut ebml, id, &uint_bytes(value))?;
        }
        element(&mut ebml, 0x4282, b"webm")?;
        element(writer, 0x1a45dfa3, &ebml)?;
        writer.write_all(&SEGMENT.to_be_bytes())?;
        writer.write_all(&[1, 255, 255, 255, 255, 255, 255, 255])?;
        let mut info = Vec::new();
        element(&mut info, 0x2ad7b1, &uint_bytes(self.scale))?;
        if let Some(duration) = self.duration {
            element(&mut info, 0x4489, &duration.to_be_bytes())?;
        }
        element(
            &mut info,
            0x4d80,
            concat!("Voldiq ", env!("CARGO_PKG_VERSION")).as_bytes(),
        )?;
        element(
            &mut info,
            0x5741,
            concat!("Voldiq ", env!("CARGO_PKG_VERSION")).as_bytes(),
        )?;
        element(writer, INFO, &info)?;
        let mut tracks = Vec::new();
        element(&mut tracks, 0xae, &self.track)?;
        element(writer, TRACKS, &tracks)?;
        for cluster in &self.clusters {
            let time = uint_bytes(cluster.time);
            let mut size = encoded_len(0xe7, time.len());
            for block in &cluster.blocks {
                let n: usize = block
                    .parts
                    .iter()
                    .map(|(id, data)| encoded_len(*id, data.len()))
                    .sum();
                size += if block.group { encoded_len(0xa0, n) } else { n };
            }
            start(writer, CLUSTER, size)?;
            element(writer, 0xe7, &time)?;
            for block in &cluster.blocks {
                if block.group {
                    start(
                        writer,
                        0xa0,
                        block
                            .parts
                            .iter()
                            .map(|(id, data)| encoded_len(*id, data.len()))
                            .sum(),
                    )?;
                }
                for (id, data) in &block.parts {
                    element(writer, *id, data)?;
                }
            }
        }
        let simple =
            encoded_len(0x45a3, b"VOLDIQ_CONTEXT".len()) + encoded_len(0x4487, metadata.len());
        let tag_size = encoded_len(0x63c0, 0) + encoded_len(0x67c8, simple);
        start(writer, 0x1254c367, encoded_len(0x7373, tag_size))?;
        start(writer, 0x7373, tag_size)?;
        element(writer, 0x63c0, &[])?;
        start(writer, 0x67c8, simple)?;
        element(writer, 0x45a3, b"VOLDIQ_CONTEXT")?;
        element(writer, 0x4487, metadata)
    }
}
fn uint_bytes(value: u64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    bytes[bytes.iter().position(|b| *b != 0).unwrap_or(7)..].to_vec()
}
fn size_width(size: usize) -> usize {
    (1..=8)
        .find(|n| (size as u128) < (1_u128 << (7 * n)) - 1)
        .unwrap_or(8)
}
fn encoded_len(id: u32, size: usize) -> usize {
    ((32 - id.leading_zeros()) as usize).div_ceil(8) + size_width(size) + size
}
fn start(writer: &mut impl Write, id: u32, size: usize) -> Result<()> {
    let id = id.to_be_bytes();
    let first = id.iter().position(|byte| *byte != 0).ok_or_else(invalid)?;
    writer.write_all(&id[first..])?;
    let width = size_width(size);
    let n = u64::try_from(size).map_err(|_| invalid())? | (1_u64 << (7 * width));
    writer.write_all(&n.to_be_bytes()[8 - width..])?;
    Ok(())
}
fn element(writer: &mut impl Write, id: u32, data: &[u8]) -> Result<()> {
    start(writer, id, data.len())?;
    for chunk in data.chunks(64 * 1024) {
        writer.write_all(chunk)?;
    }
    Ok(())
}
