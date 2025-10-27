pub use ps_hash::HASH_SIZE_COMPACT;
pub const DOUBLE_HASH_SIZE_COMPACT: usize = 2 * HASH_SIZE_COMPACT;

pub use ps_hash::HASH_SIZE;
pub const DOUBLE_HASH_SIZE: usize = HASH_SIZE * 2;

pub const HASH_SIZE_PREFIXED: usize = HASH_SIZE + 1;
pub const DOUBLE_HASH_SIZE_PREFIXED: usize = DOUBLE_HASH_SIZE + 1;

pub const MAX_SIZE_RAW: usize = HASH_SIZE_COMPACT - 1;
pub const MAX_SIZE_BASE64: usize = MAX_SIZE_RAW / 3 * 4;

pub const MAX_DECRYPTED_SIZE: usize = 4096;
pub const MAX_ENCRYPTED_SIZE: usize = 4629;
