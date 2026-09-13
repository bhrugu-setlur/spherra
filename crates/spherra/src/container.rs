//! v1 container framing. The trailer hashes preceding bytes; filenames and
//! references hash the complete container, including that trailer.
pub(crate) const HEADER_LEN: usize = 18;
pub(crate) const HASH_LEN: usize = 32;
pub(crate) const MAX_CONTAINER_PAYLOAD: usize = 1 << 20;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContainerError {
    Magic,
    Version(u16),
    Length,
    Hash,
    Crc,
    ModelValues,
    SegmentCount,
}

pub(crate) fn pack(magic: [u8; 8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len() + HASH_LEN);
    bytes.extend_from_slice(&magic);
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(payload);
    let hash = *blake3::hash(&bytes).as_bytes();
    bytes.extend_from_slice(&hash);
    bytes
}
pub(crate) fn unpack<'a>(bytes: &'a [u8], magic: &[u8; 8]) -> Result<&'a [u8], ContainerError> {
    if bytes.len() < HEADER_LEN + HASH_LEN {
        return Err(ContainerError::Length);
    }
    if &bytes[..8] != magic {
        return Err(ContainerError::Magic);
    }
    let version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    if version != 1 {
        return Err(ContainerError::Version(version));
    }
    let size = u64::from_le_bytes(bytes[10..18].try_into().unwrap());
    let size = usize::try_from(size).map_err(|_| ContainerError::Length)?;
    if size > MAX_CONTAINER_PAYLOAD || size.checked_add(HEADER_LEN + HASH_LEN) != Some(bytes.len())
    {
        return Err(ContainerError::Length);
    }
    let end = HEADER_LEN + size;
    if blake3::hash(&bytes[..end]).as_bytes() != &bytes[end..] {
        return Err(ContainerError::Hash);
    }
    Ok(&bytes[HEADER_LEN..end])
}
pub(crate) struct Decoder<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Decoder<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    pub fn take<const N: usize>(&mut self) -> Result<[u8; N], ContainerError> {
        let end = self.position.checked_add(N).ok_or(ContainerError::Length)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(ContainerError::Length)?
            .try_into()
            .unwrap();
        self.position = end;
        Ok(value)
    }
    pub fn u16(&mut self) -> Result<u16, ContainerError> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    pub fn u32(&mut self) -> Result<u32, ContainerError> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    pub fn u64(&mut self) -> Result<u64, ContainerError> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    pub fn f32(&mut self) -> Result<f32, ContainerError> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn f64(&mut self) -> Result<f64, ContainerError> {
        Ok(f64::from_bits(self.u64()?))
    }
    pub fn finish(self) -> Result<(), ContainerError> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(ContainerError::Length)
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Current {
    pub generation: u64,
    pub manifest_hash: [u8; 32],
}
impl Current {
    pub const BYTE_LEN: usize = 94;
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(44);
        payload.extend_from_slice(&self.generation.to_le_bytes());
        payload.extend_from_slice(&self.manifest_hash);
        let mut prefix = b"SPHRCUR1".to_vec();
        prefix.extend_from_slice(&1_u16.to_le_bytes());
        prefix.extend_from_slice(&44_u64.to_le_bytes());
        prefix.extend_from_slice(&payload);
        payload.extend_from_slice(&crc32c::crc32c(&prefix).to_le_bytes());
        pack(*b"SPHRCUR1", &payload)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ContainerError> {
        let payload = unpack(bytes, b"SPHRCUR1")?;
        if payload.len() != 44 {
            return Err(ContainerError::Length);
        }
        let mut d = Decoder::new(payload);
        let generation = d.u64()?;
        let manifest_hash = d.take()?;
        let crc = d.u32()?;
        d.finish()?;
        if crc32c::crc32c(&bytes[..HEADER_LEN + 40]) != crc {
            return Err(ContainerError::Crc);
        }
        Ok(Self {
            generation,
            manifest_hash,
        })
    }
}
