pub use ps_hash::HASH_SIZE_COMPACT;
pub const DOUBLE_HASH_SIZE_COMPACT: usize = 2 * HASH_SIZE_COMPACT;

pub const HASH_SIZE: usize = ps_hash::HASH_SIZE_BASE64;
pub const DOUBLE_HASH_SIZE: usize = HASH_SIZE * 2;

pub const HASH_SIZE_PREFIXED: usize = HASH_SIZE + 1;
pub const DOUBLE_HASH_SIZE_PREFIXED: usize = DOUBLE_HASH_SIZE + 1;

pub const BUF_SIZE_RAW: usize = HASH_SIZE_COMPACT - 1;
pub const BUF_SIZE_BASE64: usize = BUF_SIZE_RAW * 4 / 3;

pub const MAX_SIZE_RAW: usize = BUF_SIZE_BASE64 * 3 / 4;
pub const MAX_SIZE_BASE64: usize = MAX_SIZE_RAW * 4 / 3;

pub const MAX_DECRYPTED_SIZE: usize = 4096;
/// Upper bound on the size of an encrypted chunk holding [`MAX_DECRYPTED_SIZE`]
/// bytes, derived from the documented worst case of each layer rather than
/// from measured output:
///
/// - zstd bounds the compressed size of 4096 bytes by 4174, using
///   `ZSTD_COMPRESSBOUND`: `n + (n >> 8) + ((128 KiB - n) >> 11)`;
/// - ChaCha20-Poly1305 appends a 16-byte tag, giving 4190;
/// - long ECC with 12 parity bytes stores 207 new bytes per segment after
///   the first 231, so 4190 bytes need 21 segments at 24 parity bytes each,
///   plus a 32-byte header, giving 4726;
/// - ps-cypher appends a 4-byte tag, giving 4730.
pub const MAX_ENCRYPTED_SIZE: usize = 4730;
