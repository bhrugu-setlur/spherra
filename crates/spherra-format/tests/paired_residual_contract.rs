mod common;
use spherra_format::{PairedSegmentReaders, PrimaryFileReader, ResidualFileReader, SegmentSource};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
struct Source {
    bytes: Vec<u8>,
    closed: Arc<AtomicBool>,
}
impl Drop for Source {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}
impl SegmentSource for Source {
    fn len(&self) -> std::io::Result<u64> {
        Ok(self.bytes.len() as u64)
    }
    fn read_exact_at(&self, buffer: &mut [u8], offset: u64) -> std::io::Result<()> {
        buffer.copy_from_slice(&self.bytes[offset as usize..offset as usize + buffer.len()]);
        Ok(())
    }
}
#[test]
fn consuming_pair_closes_primary_and_preserves_residual_capability() {
    let closed = Arc::new(AtomicBool::new(false));
    let primary = PrimaryFileReader::open(
        Box::new(Source {
            bytes: common::primary_bytes(),
            closed: closed.clone(),
        }),
        &common::expectations(),
    )
    .unwrap();
    let residual =
        ResidualFileReader::open_bytes(common::residual_bytes(), &common::expectations()).unwrap();
    let paired = PairedSegmentReaders::open(primary, residual).unwrap();
    let rows: Vec<_> = (0..2).map(|r| paired.residual_code(r).unwrap()).collect();
    let residual = paired.into_residual();
    assert!(closed.load(Ordering::SeqCst));
    for (r, expected) in rows.iter().enumerate() {
        assert_eq!(&residual.residual_code(r as u32).unwrap(), expected);
    }
}
