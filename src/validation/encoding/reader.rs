//! Bounded sequential reader over an encoded Data Set: the file after its
//! File Meta Information, or the inflated stream of a Deflated Data Set.
//!
//! Values the rules do not inspect are skipped (seeked over in a file), so
//! Pixel Data is never buffered whole.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Take};
use std::path::Path;

use flate2::bufread::DeflateDecoder;

/// Read-ahead window; also the largest `peek`.
const WINDOW: usize = 64 * 1024;
/// What a file refill reads beyond the bytes asked for: room for many
/// element headers, little next to the Values that are then skipped.
const READ_AHEAD: usize = 4 * 1024;

/// Why the stream could not supply the bytes asked of it.
#[derive(Debug)]
pub(super) enum Failure {
    /// The Data Set ended `available` bytes into a field of `needed` bytes.
    Truncated {
        needed: u64,
        available: u64,
    },
    /// The inflated Data Set exceeds this many bytes.
    Limit(u64),
    Io(io::Error),
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

enum Input {
    /// `unread` bytes of the Data Set remain in the file after the window.
    File {
        file: File,
        unread: u64,
    },
    Inflate(Box<DeflateDecoder<BufReader<Take<File>>>>),
}

pub(super) struct Source {
    input: Input,
    window: Vec<u8>,
    start: usize,
    end: usize,
    /// Data Set bytes consumed.
    position: u64,
    /// Data Set bytes admitted: the rest of the file, or the inflation bound.
    limit: u64,
}

impl Source {
    /// The Data Set of `path` from byte `offset`, within the first
    /// `max_input_bytes` of the file.
    pub(super) fn file(path: &Path, offset: u64, max_input_bytes: u64) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len().min(max_input_bytes);
        let limit = length.saturating_sub(offset);
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self::new(
            Input::File {
                file,
                unread: limit,
            },
            limit,
        ))
    }

    /// The Deflated Data Set of `path` from byte `offset`, inflated to at
    /// most `max_inflated_bytes`.
    pub(super) fn deflated(
        path: &Path,
        offset: u64,
        max_input_bytes: u64,
        max_inflated_bytes: u64,
    ) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len().min(max_input_bytes);
        file.seek(SeekFrom::Start(offset))?;
        let compressed = BufReader::new(file.take(length.saturating_sub(offset)));
        Ok(Self::new(
            Input::Inflate(Box::new(DeflateDecoder::new(compressed))),
            max_inflated_bytes,
        ))
    }

    fn new(input: Input, limit: u64) -> Self {
        Self {
            input,
            window: vec![0; WINDOW],
            start: 0,
            end: 0,
            position: 0,
            limit,
        }
    }

    pub(super) fn position(&self) -> u64 {
        self.position
    }

    /// Data Set bytes not yet consumed, when the stream length is known.
    pub(super) fn remaining(&self) -> Option<u64> {
        match self.input {
            Input::File { .. } => Some(self.limit - self.position),
            Input::Inflate(_) => None,
        }
    }

    /// Compressed bytes the inflater consumed, for a Deflated Data Set.
    pub(super) fn deflated_length(&self) -> Option<u64> {
        match &self.input {
            Input::File { .. } => None,
            Input::Inflate(decoder) => Some(decoder.total_in()),
        }
    }

    /// Buffers up to `wanted` (at most the window) bytes; returns how many
    /// are buffered, fewer only at the end of the stream.
    fn fill(&mut self, wanted: usize) -> Result<usize, Failure> {
        let wanted = wanted.min(WINDOW);
        if self.end - self.start >= wanted {
            return Ok(self.end - self.start);
        }
        self.window.copy_within(self.start..self.end, 0);
        self.end -= self.start;
        self.start = 0;
        let target = wanted.clamp(READ_AHEAD, WINDOW);
        while self.end < wanted {
            let read = match &mut self.input {
                Input::File { file, unread } => {
                    let room = (target - self.end).min(usize::try_from(*unread).unwrap_or(WINDOW));
                    if room == 0 {
                        break;
                    }
                    let read = file.read(&mut self.window[self.end..self.end + room])?;
                    *unread -= read as u64;
                    read
                }
                Input::Inflate(decoder) => decoder.read(&mut self.window[self.end..])?,
            };
            if read == 0 {
                break;
            }
            self.end += read;
            if self.position + (self.end as u64) > self.limit {
                return Err(Failure::Limit(self.limit));
            }
        }
        Ok(self.end - self.start)
    }

    /// Up to `count` (at most the window) next bytes, without consuming them.
    pub(super) fn peek(&mut self, count: usize) -> Result<&[u8], Failure> {
        let available = self.fill(count)?.min(count);
        Ok(&self.window[self.start..self.start + available])
    }

    pub(super) fn at_end(&mut self) -> Result<bool, Failure> {
        Ok(self.fill(1)? == 0)
    }

    pub(super) fn read_array<const N: usize>(&mut self) -> Result<[u8; N], Failure> {
        let available = self.fill(N)?;
        if available < N {
            return Err(Failure::Truncated {
                needed: N as u64,
                available: available as u64,
            });
        }
        let mut bytes = [0; N];
        bytes.copy_from_slice(&self.window[self.start..self.start + N]);
        self.consume(N);
        Ok(bytes)
    }

    fn consume(&mut self, count: usize) {
        self.start += count;
        self.position += count as u64;
    }

    /// Passes the next `length` bytes to `visit` in window-sized pieces.
    pub(super) fn chunks(
        &mut self,
        length: u64,
        mut visit: impl FnMut(&[u8]),
    ) -> Result<(), Failure> {
        let mut left = length;
        while left > 0 {
            let available = self.fill(usize::try_from(left).unwrap_or(WINDOW))?;
            if available == 0 {
                return Err(Failure::Truncated {
                    needed: length,
                    available: length - left,
                });
            }
            let take = available.min(usize::try_from(left).unwrap_or(available));
            visit(&self.window[self.start..self.start + take]);
            self.consume(take);
            left -= take as u64;
        }
        Ok(())
    }

    /// Consumes `length` bytes without buffering them.
    pub(super) fn skip(&mut self, length: u64) -> Result<(), Failure> {
        let buffered = (self.end - self.start) as u64;
        if length <= buffered {
            self.consume(length as usize);
            return Ok(());
        }
        if let Input::File { file, unread } = &mut self.input {
            let beyond = length - buffered;
            if beyond > *unread {
                return Err(Failure::Truncated {
                    needed: length,
                    available: buffered + *unread,
                });
            }
            file.seek(SeekFrom::Current(i64::try_from(beyond).unwrap_or(i64::MAX)))?;
            *unread -= beyond;
            self.start = self.end;
            self.position += length;
            return Ok(());
        }
        self.chunks(length, |_| {})
    }
}
