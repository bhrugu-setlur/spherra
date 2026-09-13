//! Checked positional segment readers.
//!
//! `SegmentReaderCore` is private. It validates the complete header and section
//! directory, every block CRC32C, every section BLAKE3, and the whole-file
//! BLAKE3 identity *before* any data accessor exists. Only then does the open
//! path construct one of the two public capability types, chosen by
//! `file_kind`.
//!
//! M1 reads positionally and never maps memory. A later milestone may add a
//! read-only mapping inside this core alone, behind an audited unsafe module
//! with reader-lifetime, unlink, truncation, and SIGBUS tests. Never map for
//! writing.

use std::fs::File;
use std::io;
use std::path::Path;

use spherra_domain::{ChunkId, DocumentId, PutSeq};

use crate::error::FormatError;
use crate::header::{
    FileKind, HEADER_LEN, IDENTITY_LEN, SegmentExpectations, SegmentHeader, SegmentIdentity,
};
use crate::identity;
use crate::section::{
    BLOCK_SIZE, DIRECT_CODE_BYTE_LEN, DIRECTORY_ENTRY_LEN, ERROR_CERTIFICATE_BYTE_LEN,
    MAX_SECTION_ALIGNMENT, MAX_SECTIONS, NO_CRC_TABLE, PQ_CODEBOOK_VALUE_LEN, PQ96_CODE_BYTE_LEN,
    QUANTIZER_TABLE_VALUE_LEN, ROW_ENTRY_BYTE_LEN, SectionEntry, SectionKind, TILE_ROWS,
};
use crate::writer::{RowEntry, StoredErrorCertificate};

/// Bytes read per positional call while verifying checksums and identities.
const VERIFY_CHUNK: usize = 1 << 16;

/// A sealed, read-only byte source addressed positionally.
pub trait SegmentSource: Send + Sync {
    fn len(&self) -> io::Result<u64>;
    fn read_exact_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()>;

    fn is_empty(&self) -> io::Result<bool> {
        self.len().map(|len| len == 0)
    }
}

impl SegmentSource for Vec<u8> {
    fn len(&self) -> io::Result<u64> {
        Ok(self.as_slice().len() as u64)
    }

    fn read_exact_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| unexpected_eof())?;
        let end = start.checked_add(buffer.len()).ok_or_else(unexpected_eof)?;
        let bytes = self.get(start..end).ok_or_else(unexpected_eof)?;
        buffer.copy_from_slice(bytes);
        Ok(())
    }
}

impl SegmentSource for File {
    fn len(&self) -> io::Result<u64> {
        self.metadata().map(|metadata| metadata.len())
    }

    #[cfg(unix)]
    fn read_exact_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(self, buffer, offset)
    }

    #[cfg(windows)]
    fn read_exact_at(&self, buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
        let mut written = 0;
        while written < buffer.len() {
            let read =
                std::os::windows::fs::FileExt::seek_read(self, &mut buffer[written..], offset)?;
            if read == 0 {
                return Err(unexpected_eof());
            }
            written += read;
            offset += read as u64;
        }
        Ok(())
    }
}

fn unexpected_eof() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "read past the end of a segment",
    )
}

/// The private validated core shared by both capability readers.
struct SegmentReaderCore {
    source: Box<dyn SegmentSource>,
    header: SegmentHeader,
    entries: Vec<SectionEntry>,
}

impl SegmentReaderCore {
    /// Validates a complete file and returns a core whose accessors can rely on
    /// every structural and cryptographic invariant already holding.
    fn open(
        source: Box<dyn SegmentSource>,
        expected: &SegmentExpectations,
        expected_kind: FileKind,
    ) -> Result<Self, FormatError> {
        let file_len = source.len()?;
        if file_len < HEADER_LEN as u64 {
            return Err(FormatError::ShortFile { actual: file_len });
        }

        let mut header_bytes = [0; HEADER_LEN];
        source.read_exact_at(&mut header_bytes, 0)?;
        let header = SegmentHeader::decode(&header_bytes)?;
        if header.file_kind != expected_kind {
            return Err(FormatError::UnexpectedFileKind {
                expected: expected_kind,
                actual: header.file_kind,
            });
        }
        header.check_expectations(expected)?;

        let declared_len = (HEADER_LEN as u64)
            .checked_add(header.payload_len)
            .ok_or(FormatError::LengthOverflow)?;
        if declared_len != file_len {
            return Err(FormatError::PayloadLengthMismatch {
                declared: declared_len,
                actual: file_len,
            });
        }

        let entries = read_directory(source.as_ref(), &header, file_len)?;
        let core = Self {
            source,
            header,
            entries,
        };
        core.check_sections(file_len)?;
        core.check_block_checksums()?;
        core.check_section_identities()?;
        core.check_file_identity(file_len)?;
        Ok(core)
    }

    /// Structural rules that hold before any byte of a payload is trusted.
    ///
    /// The section *set* is settled first — no forbidden kind, no duplicate,
    /// nothing missing — so a file that describes the wrong kind of segment is
    /// refused for that reason rather than for whatever entry happens to look
    /// malformed first.
    fn check_sections(&self, file_len: u64) -> Result<(), FormatError> {
        self.check_section_set()?;

        let directory_end = directory_end(&self.header)?;
        let mut crc_owners = vec![0_u16; self.entries.len()];

        for (index, entry) in self.entries.iter().enumerate() {
            let index = index as u16;

            if !entry.alignment.is_power_of_two() || entry.alignment > MAX_SECTION_ALIGNMENT {
                return Err(FormatError::InvalidAlignment {
                    index,
                    alignment: entry.alignment,
                });
            }
            if !entry.offset.is_multiple_of(u64::from(entry.alignment)) {
                return Err(FormatError::MisalignedSection {
                    index,
                    offset: entry.offset,
                    alignment: entry.alignment,
                });
            }

            let end = entry.end()?;
            if entry.offset < directory_end || end > file_len {
                return Err(FormatError::SectionOutsideFile { index });
            }
            if index > 0 {
                let previous = &self.entries[index as usize - 1];
                if entry.offset < previous.offset {
                    return Err(FormatError::NonMonotonicDirectory { index });
                }
                if entry.offset < previous.end()? {
                    return Err(FormatError::OverlappingSections { index });
                }
            }

            let expected_rows = if entry.kind.is_row_addressed() {
                self.header.row_count
            } else {
                0
            };
            if entry.logical_row_count != expected_rows {
                return Err(FormatError::SectionRowCountMismatch {
                    index,
                    expected: expected_rows,
                    actual: entry.logical_row_count,
                });
            }
            if let Some(expected) = entry.kind.expected_len(self.header.row_count)?
                && entry.length != expected
            {
                return Err(FormatError::SectionLengthMismatch {
                    index,
                    kind: entry.kind,
                    expected,
                    actual: entry.length,
                });
            }

            self.check_crc_pairing(index, entry, &mut crc_owners)?;
        }

        for (index, owners) in crc_owners.iter().enumerate() {
            if self.entries[index].kind == SectionKind::CrcTable && *owners != 1 {
                return Err(FormatError::CrcTableAliased {
                    index: index as u16,
                });
            }
        }
        Ok(())
    }

    /// The directory must name exactly the section kinds this file kind
    /// requires: no residual section inside a primary file, no primary section
    /// inside a residual file, no repeats, and nothing missing.
    fn check_section_set(&self) -> Result<(), FormatError> {
        let mut seen = Vec::with_capacity(self.entries.len());
        for (index, entry) in self.entries.iter().enumerate() {
            if !entry.kind.is_allowed_in(self.header.file_kind) {
                return Err(FormatError::ForbiddenSection {
                    index: index as u16,
                    kind: entry.kind,
                });
            }
            if entry.kind != SectionKind::CrcTable {
                if seen.contains(&entry.kind) {
                    return Err(FormatError::DuplicateSection { kind: entry.kind });
                }
                seen.push(entry.kind);
            }
        }

        for kind in self.required_kinds() {
            if !seen.contains(kind) {
                return Err(FormatError::MissingSection { kind: *kind });
            }
        }
        Ok(())
    }

    /// Every independently read data section owns exactly one CRC table sized
    /// to one canonical little-endian u32 per logical block.
    fn check_crc_pairing(
        &self,
        index: u16,
        entry: &SectionEntry,
        crc_owners: &mut [u16],
    ) -> Result<(), FormatError> {
        if entry.kind == SectionKind::CrcTable {
            if entry.crc_table_index != NO_CRC_TABLE || entry.block_size != 0 {
                return Err(FormatError::InvalidCrcTableReference { index });
            }
            return Ok(());
        }

        if entry.crc_table_index == NO_CRC_TABLE {
            return Err(FormatError::MissingCrcTable { index });
        }
        let table = self
            .entries
            .get(entry.crc_table_index as usize)
            .ok_or(FormatError::InvalidCrcTableReference { index })?;
        if table.kind != SectionKind::CrcTable {
            return Err(FormatError::InvalidCrcTableReference { index });
        }
        // v1 defines exactly one block size. Accepting a declared size would
        // let a file dictate a multi-gigabyte verification buffer while still
        // sizing its CRC table consistently.
        if entry.block_size != BLOCK_SIZE {
            return Err(FormatError::InvalidBlockSize { index });
        }

        let expected = entry
            .block_count(index)?
            .checked_mul(4)
            .ok_or(FormatError::LengthOverflow)?;
        if table.length != expected {
            return Err(FormatError::SectionLengthMismatch {
                index: entry.crc_table_index,
                kind: SectionKind::CrcTable,
                expected,
                actual: table.length,
            });
        }

        crc_owners[entry.crc_table_index as usize] += 1;
        Ok(())
    }

    fn required_kinds(&self) -> &'static [SectionKind] {
        match self.header.file_kind {
            FileKind::Primary => &SectionKind::PRIMARY_SECTIONS,
            FileKind::Residual => &SectionKind::RESIDUAL_SECTIONS,
        }
    }

    /// Verifies one CRC32C per logical block; the final partial block is
    /// checksummed over only its logical bytes.
    fn check_block_checksums(&self) -> Result<(), FormatError> {
        // Every data section was validated to use `BLOCK_SIZE`, so one buffer
        // of that fixed size serves the whole file.
        let mut block_bytes = vec![0; BLOCK_SIZE as usize];

        for (index, entry) in self.entries.iter().enumerate() {
            let index = index as u16;
            if entry.kind == SectionKind::CrcTable {
                continue;
            }

            let table = &self.entries[entry.crc_table_index as usize];
            let block_size = u64::from(entry.block_size);
            let mut recorded = [0; 4];

            for block in 0..entry.block_count(index)? {
                let start = block * block_size;
                let len = block_size.min(entry.length - start) as usize;
                let block_bytes = &mut block_bytes[..len];
                self.read_at(block_bytes, entry.offset + start)?;
                self.read_at(&mut recorded, table.offset + block * 4)?;

                if crc32c::crc32c(block_bytes) != u32::from_le_bytes(recorded) {
                    return Err(FormatError::BlockChecksumMismatch { index, block });
                }
            }
        }
        Ok(())
    }

    fn check_section_identities(&self) -> Result<(), FormatError> {
        let mut chunk = vec![0; VERIFY_CHUNK];
        for (index, entry) in self.entries.iter().enumerate() {
            let mut hasher = blake3::Hasher::new();
            let mut read = 0;
            while read < entry.length {
                let len = (VERIFY_CHUNK as u64).min(entry.length - read) as usize;
                self.read_at(&mut chunk[..len], entry.offset + read)?;
                hasher.update(&chunk[..len]);
                read += len as u64;
            }
            if *hasher.finalize().as_bytes() != entry.identity {
                return Err(FormatError::SectionIdentityMismatch {
                    index: index as u16,
                });
            }
        }
        Ok(())
    }

    fn check_file_identity(&self, file_len: u64) -> Result<(), FormatError> {
        let mut hasher = identity::Hasher::new();
        let mut chunk = vec![0; VERIFY_CHUNK];
        let mut offset = 0;
        while offset < file_len {
            let len = (VERIFY_CHUNK as u64).min(file_len - offset) as usize;
            self.read_at(&mut chunk[..len], offset)?;
            hasher.update(&chunk[..len], offset);
            offset += len as u64;
        }

        if hasher.finalize() != self.header.whole_file_blake3 {
            return Err(FormatError::FileIdentityMismatch);
        }
        Ok(())
    }

    fn read_at(&self, buffer: &mut [u8], offset: u64) -> Result<(), FormatError> {
        self.source.read_exact_at(buffer, offset)?;
        Ok(())
    }

    fn section(&self, kind: SectionKind) -> Result<&SectionEntry, FormatError> {
        self.entries
            .iter()
            .find(|entry| entry.kind == kind)
            .ok_or(FormatError::MissingSection { kind })
    }

    /// Reads `N` bytes at `row * N` inside a row-addressed section.
    fn read_row<const N: usize>(
        &self,
        kind: SectionKind,
        row: u32,
    ) -> Result<[u8; N], FormatError> {
        self.check_row(row)?;
        let entry = self.section(kind)?;
        let mut bytes = [0; N];
        self.read_at(&mut bytes, entry.offset + u64::from(row) * N as u64)?;
        Ok(bytes)
    }

    fn check_row(&self, row: u32) -> Result<(), FormatError> {
        if row >= self.header.row_count {
            return Err(FormatError::RowOutOfRange {
                row,
                row_count: self.header.row_count,
            });
        }
        Ok(())
    }

    fn read_section(&self, kind: SectionKind) -> Result<Vec<u8>, FormatError> {
        let entry = self.section(kind)?;
        // Section lengths were validated against their kind, so this allocation
        // is bounded by the format rather than by the file's own claim.
        let mut bytes =
            vec![0; usize::try_from(entry.length).map_err(|_| FormatError::TooManyRows)?];
        self.read_at(&mut bytes, entry.offset)?;
        Ok(bytes)
    }

    fn read_f32_table(&self, kind: SectionKind, values: usize) -> Result<Vec<f32>, FormatError> {
        let bytes = self.read_section(kind)?;
        let table: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|value| {
                let mut raw = [0; 4];
                raw.copy_from_slice(value);
                f32::from_le_bytes(raw)
            })
            .collect();

        if table.len() != values || table.iter().any(|value| !value.is_finite()) {
            return Err(FormatError::InvalidStoredValue {
                value: "FP32 table entry",
            });
        }
        Ok(table)
    }

    fn certificate(&self, kind: SectionKind) -> Result<StoredErrorCertificate, FormatError> {
        let bytes = self.read_section(kind)?;
        if bytes.len() != ERROR_CERTIFICATE_BYTE_LEN {
            return Err(FormatError::InvalidStoredValue {
                value: "certificate length",
            });
        }
        StoredErrorCertificate::decode(&bytes)
    }
}

fn directory_end(header: &SegmentHeader) -> Result<u64, FormatError> {
    let directory_len = u64::from(header.section_count)
        .checked_mul(DIRECTORY_ENTRY_LEN as u64)
        .ok_or(FormatError::LengthOverflow)?;
    header
        .section_directory_offset
        .checked_add(directory_len)
        .ok_or(FormatError::LengthOverflow)
}

fn read_directory(
    source: &dyn SegmentSource,
    header: &SegmentHeader,
    file_len: u64,
) -> Result<Vec<SectionEntry>, FormatError> {
    if header.section_count == 0 || header.section_count > MAX_SECTIONS {
        return Err(FormatError::SectionCountOutOfRange {
            actual: header.section_count,
        });
    }

    let end = directory_end(header)?;
    if header.section_directory_offset < HEADER_LEN as u64 || end > file_len {
        return Err(FormatError::DirectoryOutOfFile);
    }

    let mut entry_bytes = [0; DIRECTORY_ENTRY_LEN];
    let mut entries = Vec::with_capacity(header.section_count as usize);
    for index in 0..header.section_count {
        let offset =
            header.section_directory_offset + u64::from(index) * DIRECTORY_ENTRY_LEN as u64;
        source.read_exact_at(&mut entry_bytes, offset)?;
        entries.push(SectionEntry::decode(index, &entry_bytes)?);
    }
    Ok(entries)
}

/// The capability a resident primary scan receives.
///
/// No handle reachable from this type exposes residual codes.
pub struct PrimaryFileReader(SegmentReaderCore);

/// The capability a residual file grants.
///
/// Its PQ96x8 row accessor is crate-private: only a validated
/// [`PairedSegmentReaders`] can expose one publicly.
pub struct ResidualFileReader(SegmentReaderCore);

macro_rules! capability_reader {
    ($reader:ident, $kind:expr) => {
        impl $reader {
            pub fn open(
                source: Box<dyn SegmentSource>,
                expected: &SegmentExpectations,
            ) -> Result<Self, FormatError> {
                SegmentReaderCore::open(source, expected, $kind).map(Self)
            }

            pub fn open_bytes(
                bytes: Vec<u8>,
                expected: &SegmentExpectations,
            ) -> Result<Self, FormatError> {
                Self::open(Box::new(bytes), expected)
            }

            pub fn open_path(
                path: &Path,
                expected: &SegmentExpectations,
            ) -> Result<Self, FormatError> {
                Self::open(Box::new(File::open(path)?), expected)
            }

            pub const fn file_kind(&self) -> FileKind {
                self.0.header.file_kind
            }

            pub const fn identity(&self) -> &SegmentIdentity {
                &self.0.header.identity
            }

            pub const fn row_count(&self) -> u32 {
                self.0.header.row_count
            }

            pub const fn file_identity(&self) -> [u8; IDENTITY_LEN] {
                self.0.header.whole_file_blake3
            }
        }
    };
}

capability_reader!(PrimaryFileReader, FileKind::Primary);
capability_reader!(ResidualFileReader, FileKind::Residual);

impl PrimaryFileReader {
    pub fn row(&self, row: u32) -> Result<RowEntry, FormatError> {
        let bytes = self
            .0
            .read_row::<ROW_ENTRY_BYTE_LEN>(SectionKind::IdsSequences, row)?;
        let raw_seq = u64_at(&bytes, 32);
        Ok(RowEntry {
            chunk_id: ChunkId::from_u128(u128_at(&bytes, 0)),
            document_id: DocumentId::from_u128(u128_at(&bytes, 16)),
            // The 16-bit epoch and 48-bit index partition the raw word, so this
            // split can never exceed the sequence's index range.
            put_seq: PutSeq::new((raw_seq >> 48) as u16, raw_seq & PutSeq::MAX_INDEX).map_err(
                |_| FormatError::InvalidStoredValue {
                    value: "put sequence",
                },
            )?,
        })
    }

    pub fn radius_flags(&self, row: u32) -> Result<[u8; 4], FormatError> {
        self.0.read_row::<4>(SectionKind::RadiusFlags, row)
    }

    /// Reads one physical TILED_SOA_32 tile in a single positional call. The
    /// ordinal is a tile index, not a row index. A final partial tile retains
    /// its padding lanes; the caller uses `row_count` to select logical rows.
    pub fn primary_tile(&self, tile: u32) -> Result<[u8; 768 * 16], FormatError> {
        let tile_count = self.row_count().div_ceil(TILE_ROWS);
        if tile >= tile_count {
            return Err(FormatError::TileOutOfRange { tile, tile_count });
        }
        let entry = self.0.section(SectionKind::PrimaryDirectInt4)?;
        let mut bytes = [0; 768 * 16];
        let offset = entry
            .offset
            .checked_add(u64::from(tile) * bytes.len() as u64)
            .ok_or(FormatError::LengthOverflow)?;
        self.0.read_at(&mut bytes, offset)?;
        Ok(bytes)
    }

    /// Reads one logical direct-int4 code out of the TILED_SOA_32 layout.
    pub fn primary_code(&self, row: u32) -> Result<[u8; DIRECT_CODE_BYTE_LEN], FormatError> {
        self.0.check_row(row)?;
        let tile = self.primary_tile(row / TILE_ROWS)?;

        // A tile stores all 768 coordinates for its 32 lanes: coordinate `c`
        // occupies a 16-byte lane group, and this row is its `lane`-th nibble.
        let lane = (row % TILE_ROWS) as usize;
        let shift = if lane.is_multiple_of(2) { 0 } else { 4 };
        let nibble = |coordinate: usize| {
            (tile[coordinate * (TILE_ROWS as usize / 2) + lane / 2] >> shift) & 0x0f
        };

        let mut code = [0; DIRECT_CODE_BYTE_LEN];
        for (byte, value) in code.iter_mut().enumerate() {
            *value = nibble(byte * 2) | (nibble(byte * 2 + 1) << 4);
        }
        Ok(code)
    }

    pub fn quantizer_table(&self) -> Result<Vec<f32>, FormatError> {
        self.0
            .read_f32_table(SectionKind::Int4QuantizerTable, QUANTIZER_TABLE_VALUE_LEN)
    }

    pub fn primary_certificate(&self) -> Result<StoredErrorCertificate, FormatError> {
        self.0.certificate(SectionKind::PrimaryCertificate)
    }

    pub fn refined_certificate(&self) -> Result<StoredErrorCertificate, FormatError> {
        self.0.certificate(SectionKind::RefinedCertificate)
    }
}

impl ResidualFileReader {
    pub fn pq_codebook(&self) -> Result<Vec<f32>, FormatError> {
        self.0
            .read_f32_table(SectionKind::PqCodebook, PQ_CODEBOOK_VALUE_LEN)
    }

    /// Crate-private: reachable publicly only through [`PairedSegmentReaders`].
    fn residual_code(&self, row: u32) -> Result<[u8; PQ96_CODE_BYTE_LEN], FormatError> {
        self.0
            .read_row::<PQ96_CODE_BYTE_LEN>(SectionKind::Pq96Residual, row)
    }
}

/// A primary and residual file proven to describe the same segment.
///
/// This is the only public route to a residual row. Pairing validates every
/// collection, segment, dimension, row-count, transform, codec, quantizer,
/// codebook, scorer, and layout identity first, so no residual read API is
/// publicly reachable before it succeeds.
pub struct PairedSegmentReaders {
    primary: PrimaryFileReader,
    residual: ResidualFileReader,
}

impl PairedSegmentReaders {
    pub fn open(
        primary: PrimaryFileReader,
        residual: ResidualFileReader,
    ) -> Result<Self, FormatError> {
        let (left, right) = (primary.identity(), residual.identity());
        if left.collection_id != right.collection_id {
            return Err(FormatError::CollectionMismatch);
        }
        if left.segment_id != right.segment_id {
            return Err(FormatError::SegmentMismatch);
        }
        if primary.row_count() != residual.row_count() {
            return Err(FormatError::RowCountMismatch {
                primary: primary.row_count(),
                residual: residual.row_count(),
            });
        }
        // Dimension is validated to the single supported value on open, so
        // equality here follows from both files having opened at all.
        if left.codec_id != right.codec_id
            || left.transform_id != right.transform_id
            || left.quantizer_id != right.quantizer_id
            || left.pq_codebook_id != right.pq_codebook_id
            || left.scorer_version != right.scorer_version
            || left.layout != right.layout
        {
            return Err(FormatError::PairedIdentityMismatch);
        }

        Ok(Self { primary, residual })
    }

    pub const fn primary(&self) -> &PrimaryFileReader {
        &self.primary
    }

    /// The only public candidate-rerank accessor for residual codes.
    pub fn residual_code(&self, row: u32) -> Result<[u8; PQ96_CODE_BYTE_LEN], FormatError> {
        self.residual.residual_code(row)
    }
}

fn u64_at(bytes: &[u8; ROW_ENTRY_BYTE_LEN], offset: usize) -> u64 {
    let mut value = [0; 8];
    value.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(value)
}

fn u128_at(bytes: &[u8; ROW_ENTRY_BYTE_LEN], offset: usize) -> u128 {
    let mut value = [0; 16];
    value.copy_from_slice(&bytes[offset..offset + 16]);
    u128::from_le_bytes(value)
}
