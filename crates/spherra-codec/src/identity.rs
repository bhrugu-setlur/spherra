use std::sync::LazyLock;

use spherra_domain::DIMENSION;

/// The provisional M1 representation identity.
///
/// `codec_id` owns representation while `scorer_version` owns comparison scale,
/// so this hash covers exactly the representation choices a stored byte depends
/// on. It is provisional: the direct-int4 tables, the PQ codebook shape, and the
/// layout are all still benchmark-selected, and a change to any of them must
/// change this identity.
pub static CODEC_ID: LazyLock<[u8; 32]> = LazyLock::new(|| {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"spherra.codec.id.v1");
    hasher.update(&(DIMENSION as u32).to_le_bytes());
    hasher.update(b"direct-int4-nibble-low-even-high-odd");
    hasher.update(b"pq96x8-u8");
    hasher.update(b"tiled-soa-32");
    *hasher.finalize().as_bytes()
});
