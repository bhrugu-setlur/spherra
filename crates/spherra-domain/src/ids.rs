#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChunkId(u128);

impl ChunkId {
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DocumentId(u128);

impl DocumentId {
    pub const fn from_u128(value: u128) -> Self {
        Self(value)
    }
}
