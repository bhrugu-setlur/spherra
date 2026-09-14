mod common;
use spherra_format::{FormatError, PrimaryFileReader, SegmentSource, encode_primary_segment};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
struct Spy {
    bytes: Vec<u8>,
    reads: Arc<AtomicUsize>,
}
impl SegmentSource for Spy {
    fn len(&self) -> io::Result<u64> {
        Ok(self.bytes.len() as u64)
    }
    fn read_exact_at(&self, out: &mut [u8], offset: u64) -> io::Result<()> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        SegmentSource::read_exact_at(&self.bytes, out, offset)
    }
}
#[test]
fn tiles_equal_each_row_code_with_one_read_including_partial_tiles() {
    for count in [1_u32, 31, 32, 33, 65536] {
        let mut segment = common::primary_segment();
        segment.rows = (0..count).map(common::row_entry).collect();
        segment.radius_flags = vec![[0; 4]; count as usize];
        segment.primary_codes = (0..count)
            .map(|row| {
                let mut code = common::primary_code(row);
                // Make every physical row distinct, including between distant tiles.
                code[..4].copy_from_slice(&row.to_le_bytes());
                code
            })
            .collect();
        let bytes = encode_primary_segment(&segment).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let reader = PrimaryFileReader::open(
            Box::new(Spy {
                bytes,
                reads: Arc::clone(&reads),
            }),
            &common::expectations(),
        )
        .unwrap();
        for tile_index in 0..count.div_ceil(32) {
            reads.store(0, Ordering::SeqCst);
            let tile = reader.primary_tile(tile_index).unwrap();
            assert_eq!(tile.len(), 768 * 16);
            assert_eq!(reads.load(Ordering::SeqCst), 1);
            for lane in 0..32_u32 {
                let row = tile_index * 32 + lane;
                let nibble = |c: usize| (tile[c * 16 + lane as usize / 2] >> ((lane % 2) * 4)) & 15;
                let code: [u8; 384] =
                    std::array::from_fn(|i| nibble(2 * i) | (nibble(2 * i + 1) << 4));
                if row < count {
                    assert_eq!(code, segment.primary_codes[row as usize]);
                    assert_eq!(code, reader.primary_code(row).unwrap());
                } else {
                    assert_eq!(code, [0; 384]);
                }
            }
        }
        for tile in [count.div_ceil(32), u32::MAX] {
            reads.store(0, Ordering::SeqCst);
            assert!(matches!(
                reader.primary_tile(tile),
                Err(FormatError::TileOutOfRange { .. })
            ));
            assert_eq!(reads.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn radius_blocks_are_bounded_single_reads_and_preserve_every_byte() {
    let count = 4101_u32;
    let mut segment = common::primary_segment();
    segment.rows = (0..count).map(common::row_entry).collect();
    segment.primary_codes = (0..count).map(common::primary_code).collect();
    segment.radius_flags = (0..count).map(u32::to_le_bytes).collect();
    let reads = Arc::new(AtomicUsize::new(0));
    let reader = PrimaryFileReader::open(
        Box::new(Spy {
            bytes: encode_primary_segment(&segment).unwrap(),
            reads: Arc::clone(&reads),
        }),
        &common::expectations(),
    )
    .unwrap();
    for (first, n) in [(0, 4096), (4096, 5), (2, 31)] {
        reads.store(0, Ordering::SeqCst);
        let data = reader.radius_flags_range(first, n).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            data,
            &segment.radius_flags[first as usize..(first + n) as usize]
        );
    }
    for (first, n) in [(0, 0), (0, 4097), (4101, 1), (4100, 2), (u32::MAX, 2)] {
        reads.store(0, Ordering::SeqCst);
        assert!(reader.radius_flags_range(first, n).is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}
