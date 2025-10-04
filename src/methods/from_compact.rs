use ps_hash::{Hash, HashValidationError};

use crate::{Hkey, DOUBLE_HASH_SIZE_COMPACT, HASH_SIZE_COMPACT};

impl Hkey {
    pub fn from_compact(bytes: &[u8]) -> Result<Self, HashValidationError> {
        match bytes.len() {
            HASH_SIZE_COMPACT => Ok(Self::Direct(Hash::validate_bin(bytes)?.into())),

            DOUBLE_HASH_SIZE_COMPACT => {
                let hash = Hash::validate_bin(&bytes[..HASH_SIZE_COMPACT])?.into();
                let key = Hash::validate_bin(&bytes[HASH_SIZE_COMPACT..])?.into();

                if bytes[0] & 1 == 0 {
                    Ok(Self::Encrypted(hash, key))
                } else {
                    Ok(Self::ListRef(hash, key))
                }
            }

            _ => Ok(Self::Raw(bytes.into())),
        }
    }
}
