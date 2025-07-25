use ps_hash::Hash;

pub const HASH_SIZE: usize = std::mem::size_of::<Hash>();
pub const DOUBLE_HASH_SIZE: usize = HASH_SIZE * 2;

pub const MAX_SIZE_BASE64: usize = HASH_SIZE - 2;
pub const MAX_SIZE_RAW: usize = MAX_SIZE_BASE64 * 3 / 4;

pub const MAX_DECRYPTED_SIZE: usize = 4096;
pub const MAX_ENCRYPTED_SIZE: usize = 4629;
