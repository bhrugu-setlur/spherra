//! Whole-file BLAKE3 identity.
//!
//! The identity covers every durable byte — header, directory, section
//! payloads, alignment padding, and CRC tables — with the identity field itself
//! read as thirty-two zero bytes so the value can be recomputed and compared.

use crate::header::{IDENTITY_LEN, OFFSET_WHOLE_FILE_BLAKE3};

const IDENTITY_END: usize = OFFSET_WHOLE_FILE_BLAKE3 + IDENTITY_LEN;

pub fn whole_file_blake3(bytes: &[u8]) -> [u8; IDENTITY_LEN] {
    let mut hasher = Hasher::new();
    hasher.update(bytes, 0);
    hasher.finalize()
}

/// Incremental form for readers that stream a file in positional chunks.
pub struct Hasher(blake3::Hasher);

impl Hasher {
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    /// Absorbs `chunk`, which begins at `offset` in the file, masking any
    /// overlap with the identity field to zero.
    pub fn update(&mut self, chunk: &[u8], offset: u64) {
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let end = start.saturating_add(chunk.len());
        if end <= OFFSET_WHOLE_FILE_BLAKE3 || start >= IDENTITY_END {
            self.0.update(chunk);
            return;
        }

        let masked_start = OFFSET_WHOLE_FILE_BLAKE3.max(start);
        let masked_end = IDENTITY_END.min(end);
        self.0.update(&chunk[..masked_start - start]);
        self.0.update(&vec![0; masked_end - masked_start]);
        self.0.update(&chunk[masked_end - start..]);
    }

    pub fn finalize(self) -> [u8; IDENTITY_LEN] {
        *self.0.finalize().as_bytes()
    }
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}
