use std::{
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) struct LimitedOutput<'a, W> {
    pub writer: &'a mut W,
    pub written: usize,
    pub limit: usize,
    pub cancelled: &'a AtomicBool,
}
impl<W: Write> Write for LimitedOutput<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other("Export cancelled."));
        }
        if bytes.len() > self.limit.saturating_sub(self.written) {
            return Err(io::Error::other("Export exceeds its output byte limit."));
        }
        let count = self.writer.write(bytes)?;
        self.written += count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
