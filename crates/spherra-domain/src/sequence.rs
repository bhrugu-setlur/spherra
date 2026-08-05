use crate::DomainError;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PutSeq(u64);

impl PutSeq {
    pub const MAX_INDEX: u64 = (1_u64 << 48) - 1;

    pub fn new(epoch: u16, index: u64) -> Result<Self, DomainError> {
        if index > Self::MAX_INDEX {
            return Err(DomainError::SequenceOverflow);
        }
        Ok(Self((u64::from(epoch) << 48) | index))
    }

    pub const fn epoch(self) -> u16 {
        (self.0 >> 48) as u16
    }

    pub const fn index(self) -> u64 {
        self.0 & Self::MAX_INDEX
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}
