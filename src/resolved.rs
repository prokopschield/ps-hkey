use std::sync::Arc;

use ps_datachunk::{Bytes, DataChunk, OwnedDataChunk, PsDataChunkError, SerializedDataChunk};

pub enum Resolved<C: DataChunk> {
    Custom(C),
    Data(Arc<[u8]>),
    Owned(OwnedDataChunk),
    Serialized(SerializedDataChunk),
}

impl<C: DataChunk> Resolved<C> {
    pub fn data_ref(&self) -> &[u8] {
        match self {
            Self::Custom(custom) => custom.data_ref(),
            Self::Data(data) => data,
            Self::Owned(owned) => owned.data_ref(),
            Self::Serialized(serialized) => serialized.data_ref(),
        }
    }

    pub fn into_bytes(self) -> Bytes {
        match self {
            Self::Custom(custom) => custom.into_bytes(),
            Self::Data(bytes) => Bytes::from_owner(bytes),
            Self::Owned(owned) => owned.into_bytes(),
            Self::Serialized(serialized) => serialized.into_bytes(),
        }
    }

    pub fn try_into_owned(self) -> Result<OwnedDataChunk, PsDataChunkError> {
        match self {
            Self::Custom(custom) => Ok(custom.into_owned()),
            Self::Data(data) => OwnedDataChunk::from_data(data),
            Self::Owned(owned) => Ok(owned),
            Self::Serialized(serialized) => Ok(serialized.into_owned()),
        }
    }
}

impl<C: DataChunk> From<Arc<[u8]>> for Resolved<C> {
    fn from(value: Arc<[u8]>) -> Self {
        Self::Data(value)
    }
}

impl<C: DataChunk> From<OwnedDataChunk> for Resolved<C> {
    fn from(value: OwnedDataChunk) -> Self {
        Self::Owned(value)
    }
}

impl<C: DataChunk> From<SerializedDataChunk> for Resolved<C> {
    fn from(value: SerializedDataChunk) -> Self {
        Self::Serialized(value)
    }
}
