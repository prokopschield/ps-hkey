use ps_hash::Hash;

use crate::{Hkey, HkeyFromCompactError, DOUBLE_HASH_SIZE_COMPACT, HASH_SIZE_COMPACT};

impl Hkey {
    pub fn from_compact(bytes: &[u8]) -> Result<Self, HkeyFromCompactError> {
        match bytes.len() {
            HASH_SIZE_COMPACT => Ok(Self::Direct(Hash::validate(bytes)?)),

            DOUBLE_HASH_SIZE_COMPACT => {
                let hash = Hash::validate(&bytes[..HASH_SIZE_COMPACT])?;
                let key = Hash::validate(&bytes[HASH_SIZE_COMPACT..])?;

                if bytes[0] & 1 == 0 {
                    Ok(Self::Encrypted(hash, key))
                } else {
                    Ok(Self::ListRef(hash, key))
                }
            }

            _ => Self::from_raw(bytes).map_err(Into::into),
        }
    }
}
