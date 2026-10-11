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

#[derive(Debug)]
pub struct Record {
    pub reference: u64,
    pub parent: u64,
    pub usn: i64,
    pub reason: u32,
    pub attributes: u32,
    pub name: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &[u16]) -> Vec<u8> {
        let length = (60 + name.len() * 2).next_multiple_of(8);
        let mut bytes = vec![0; length];
        bytes[..4].copy_from_slice(&(length as u32).to_le_bytes());
        bytes[4..6].copy_from_slice(&2_u16.to_le_bytes());
        bytes[8..16].copy_from_slice(&42_u64.to_le_bytes());
        bytes[16..24].copy_from_slice(&5_u64.to_le_bytes());
        bytes[24..32].copy_from_slice(&123_i64.to_le_bytes());
        bytes[40..44].copy_from_slice(&0x2000_u32.to_le_bytes());
        bytes[52..56].copy_from_slice(&0x20_u32.to_le_bytes());
        bytes[56..58].copy_from_slice(&((name.len() * 2) as u16).to_le_bytes());
        bytes[58..60].copy_from_slice(&60_u16.to_le_bytes());
        for (index, unit) in name.iter().enumerate() {
            bytes[60 + index * 2..62 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn parses_bounded_records_and_preserves_original_utf16() {
        let name: Vec<u16> = "日本語.bin".encode_utf16().collect();
        let mut buffer = 456_u64.to_le_bytes().to_vec();
        buffer.extend(record(&name));
        buffer.extend(record(&[0x61, 0xd800]));
        let mut entries = Vec::new();
        assert_eq!(
            parse_buffer(&buffer, 255, |entry| {
                entries.push(entry);
                Ok(())
            })
            .unwrap(),
            456
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, name);
        assert_eq!(entries[1].name, [0x61, 0xd800]);
        assert_eq!(
            (
                entries[0].reference,
                entries[0].parent,
                entries[0].usn,
                entries[0].reason,
                entries[0].attributes
            ),
            (42, 5, 123, 0x2000, 0x20)
        );
    }

    #[test]
    fn rejects_every_truncated_record_and_malformed_header() {
        let mut buffer = 456_u64.to_le_bytes().to_vec();
        buffer.extend(record(&[0x61, 0x62, 0x63]));
        for length in 0..buffer.len() {
            if length == 8 {
                continue;
            }
            assert!(
                parse_buffer(&buffer[..length], 255, |_| Ok(())).is_err(),
                "length {length}"
            );
        }
        for (offset, bytes) in [
            (8, 0_u32.to_le_bytes().to_vec()),
            (8, 63_u32.to_le_bytes().to_vec()),
            (12, 3_u16.to_le_bytes().to_vec()),
            (64, 3_u16.to_le_bytes().to_vec()),
            (64, 500_u16.to_le_bytes().to_vec()),
            (66, 58_u16.to_le_bytes().to_vec()),
            (66, 61_u16.to_le_bytes().to_vec()),
            (66, u16::MAX.to_le_bytes().to_vec()),
        ] {
            let mut malformed = buffer.clone();
            malformed[offset..offset + bytes.len()].copy_from_slice(&bytes);
            assert!(parse_buffer(&malformed, 255, |_| Ok(())).is_err());
        }
        assert!(parse_buffer(&buffer, 2, |_| Ok(())).is_err());
    }

    #[test]
    fn minor_extensions_use_the_recorded_filename_offset() {
        let mut extended = record(&[0x61]);
        extended[6..8].copy_from_slice(&1_u16.to_le_bytes());
        extended[58..60].copy_from_slice(&62_u16.to_le_bytes());
        extended[60..62].copy_from_slice(&999_u16.to_le_bytes());
        extended[62..64].copy_from_slice(&0x62_u16.to_le_bytes());
        let mut buffer = 123_u64.to_le_bytes().to_vec();
        buffer.extend(extended);
        parse_buffer(&buffer, 255, |entry| {
            assert_eq!(entry.name, [0x62]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn journal_continuity_rejects_volume_reset_gap_and_future_cursor() {
        let journal = Journal {
            id: 42,
            first: 100,
            next: 200,
            lowest: 120,
        };
        let checkpoint = Checkpoint {
            volume: 17,
            journal: 42,
            next: 120,
        };
        assert!(journal.can_resume(17, checkpoint));
        assert!(!journal.can_resume(18, checkpoint));
        assert!(!journal.can_resume(
            17,
            Checkpoint {
                journal: 43,
                ..checkpoint
            }
        ));
        for next in [-1, 99, 119, 201] {
            assert!(!journal.can_resume(17, Checkpoint { next, ..checkpoint }));
        }
        assert!(journal.can_resume(
            17,
            Checkpoint {
                next: 200,
                ..checkpoint
            }
        ));
        assert!(
            !Journal {
                first: -1,
                ..journal
            }
            .can_resume(17, checkpoint)
        );
        assert!(
            !Journal {
                lowest: -1,
                ..journal
            }
            .can_resume(17, checkpoint)
        );
        assert!(
            !Journal {
                next: 99,
                ..journal
            }
            .can_resume(17, checkpoint)
        );
    }
}
