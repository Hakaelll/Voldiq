//! Bounded byte copying and independent readback. These functions open no paths and grant no
//! approval, native ownership, durability or original-handling authority. Native callers must
//! retain authorized source/destination handles, sync before readback and bind durable receipts.
use crate::{Error, Result};
use std::{
    io::{Read, Write},
    sync::atomic::{AtomicBool, Ordering},
};

const BUFFER: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Copying,
    Readback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub phase: Phase,
    pub bytes: u64,
    pub total: u64,
}

/// Byte evidence only, never a capability to publish or modify a source file.
pub struct CopiedBytes {
    bytes: u64,
    digest: blake3::Hash,
}
impl CopiedBytes {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn digest(&self) -> String {
        self.digest.to_hex().to_string()
    }
}

fn check(cancel: &AtomicBool, checkpoint: &mut impl FnMut() -> Result<()>) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Invalid("Transfer byte operation cancelled.".into()));
    }
    checkpoint()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Invalid("Transfer byte operation cancelled.".into()));
    }
    Ok(())
}

/// Copies exactly the expected remaining source bytes to an already-owned sink, then requires EOF.
/// The native caller must use a fresh private sink; error/cancellation prefixes are not complete.
pub fn copy_bytes(
    source: &mut impl Read,
    destination: &mut impl Write,
    expected: u64,
    cancel: &AtomicBool,
    mut checkpoint: impl FnMut() -> Result<()>,
    mut progress: impl FnMut(Progress),
) -> Result<CopiedBytes> {
    let mut buffer = [0_u8; BUFFER];
    let mut hash = blake3::Hasher::new();
    let mut bytes = 0;
    check(cancel, &mut checkpoint)?;
    progress(Progress {
        phase: Phase::Copying,
        bytes,
        total: expected,
    });
    while bytes < expected {
        check(cancel, &mut checkpoint)?;
        let count = usize::try_from((expected - bytes).min(BUFFER as u64)).unwrap_or(BUFFER);
        let read = source.read(&mut buffer[..count])?;
        if read == 0 {
            return Err(Error::Invalid(
                "Source ended before its approved byte length.".into(),
            ));
        }
        check(cancel, &mut checkpoint)?;
        destination.write_all(&buffer[..read])?;
        hash.update(&buffer[..read]);
        bytes += read as u64;
        progress(Progress {
            phase: Phase::Copying,
            bytes,
            total: expected,
        });
    }
    check(cancel, &mut checkpoint)?;
    if source.read(&mut buffer[..1])? != 0 {
        return Err(Error::Invalid(
            "Source exceeds its approved byte length.".into(),
        ));
    }
    destination.flush()?;
    check(cancel, &mut checkpoint)?;
    Ok(CopiedBytes {
        bytes,
        digest: hash.finalize(),
    })
}

/// Reads the actual destination from its caller-selected start position through exact EOF.
/// A native caller must flush/sync and seek its retained destination to zero before calling.
pub fn verify_bytes(
    destination: &mut impl Read,
    copied: &CopiedBytes,
    cancel: &AtomicBool,
    mut checkpoint: impl FnMut() -> Result<()>,
    mut progress: impl FnMut(Progress),
) -> Result<()> {
    let mut buffer = [0_u8; BUFFER];
    let mut hash = blake3::Hasher::new();
    let mut bytes = 0;
    check(cancel, &mut checkpoint)?;
    progress(Progress {
        phase: Phase::Readback,
        bytes,
        total: copied.bytes,
    });
    while bytes < copied.bytes {
        check(cancel, &mut checkpoint)?;
        let count = usize::try_from((copied.bytes - bytes).min(BUFFER as u64)).unwrap_or(BUFFER);
        let read = destination.read(&mut buffer[..count])?;
        if read == 0 {
            return Err(Error::Invalid("Destination readback is truncated.".into()));
        }
        hash.update(&buffer[..read]);
        bytes += read as u64;
        progress(Progress {
            phase: Phase::Readback,
            bytes,
            total: copied.bytes,
        });
    }
    check(cancel, &mut checkpoint)?;
    if destination.read(&mut buffer[..1])? != 0 || hash.finalize() != copied.digest {
        return Err(Error::Invalid(
            "Destination readback length or digest differs from the copied bytes.".into(),
        ));
    }
    check(cancel, &mut checkpoint)
}
