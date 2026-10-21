use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod volume;
#[cfg(windows)]
pub use volume::VolumeReader;
#[cfg(windows)]
mod indexing;
#[cfg(windows)]
pub use indexing::populate;
#[cfg(windows)]
pub(crate) use indexing::try_scan;
#[cfg(windows)]
mod replay;
#[cfg(windows)]
pub use replay::{refresh, replay};

#[derive(Debug)]
pub struct Record {
    pub reference: u64,
    pub parent: u64,
    pub usn: i64,
    pub reason: u32,
    pub attributes: u32,
    pub name: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub volume: u64,
    pub journal: u64,
    pub next: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct Journal {
    pub id: u64,
    pub first: i64,
    pub next: i64,
    pub lowest: i64,
}

impl Journal {
    pub fn can_resume(&self, volume: u64, checkpoint: Checkpoint) -> bool {
        self.first >= 0
            && self.lowest >= 0
            && self.next >= self.first.max(self.lowest)
            && volume == checkpoint.volume
            && self.id == checkpoint.journal
            && checkpoint.next >= self.first.max(self.lowest).max(0)
            && checkpoint.next <= self.next
    }
}

pub struct Records {
    pub items: Vec<Record>,
    pub state: ReadState,
}

#[derive(Debug)]
pub enum ReadState {
    Complete(Checkpoint),
    Limited,
    Cancelled,
}

pub fn parse_buffer(
    buffer: &[u8],
    max_name_units: u32,
    mut consume: impl FnMut(Record) -> Result<()>,
) -> Result<u64> {
    let next = u64::from_le_bytes(read(buffer, 0)?);
    let mut position = 8;
    while position < buffer.len() {
        let remaining = &buffer[position..];
        let length = u32::from_le_bytes(read(remaining, 0)?) as usize;
        if length < 60 || !length.is_multiple_of(8) {
            return Err(Error::Invalid("Invalid USN record length.".into()));
        }
        let record = remaining
            .get(..length)
            .ok_or_else(|| Error::Invalid("Truncated USN record.".into()))?;
        if u16::from_le_bytes(read(record, 4)?) != 2 {
            return Err(Error::Invalid("Unsupported USN record version.".into()));
        }
        let name_length = usize::from(u16::from_le_bytes(read(record, 56)?));
        let name_offset = usize::from(u16::from_le_bytes(read(record, 58)?));
        if name_offset < 60
            || !name_offset.is_multiple_of(2)
            || name_length == 0
            || !name_length.is_multiple_of(2)
            || name_length / 2 > max_name_units as usize
        {
            return Err(Error::Invalid("Invalid USN filename bounds.".into()));
        }
        let name = record
            .get(
                name_offset
                    ..name_offset
                        .checked_add(name_length)
                        .ok_or_else(|| Error::Invalid("USN filename overflow.".into()))?,
            )
            .ok_or_else(|| Error::Invalid("USN filename exceeds its record.".into()))?;
        consume(Record {
            reference: u64::from_le_bytes(read(record, 8)?),
            parent: u64::from_le_bytes(read(record, 16)?),
            usn: i64::from_le_bytes(read(record, 24)?),
            reason: u32::from_le_bytes(read(record, 40)?),
            attributes: u32::from_le_bytes(read(record, 52)?),
            name: name
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect(),
        })?;
        position += length;
    }
    Ok(next)
}

pub(super) fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    data.get(
        offset
            ..offset
                .checked_add(N)
                .ok_or_else(|| Error::Invalid("USN field overflow.".into()))?,
    )
    .and_then(|bytes| bytes.try_into().ok())
    .ok_or_else(|| Error::Invalid("Truncated USN field.".into()))
}
