/// log2 of [`LHKEY_SEGMENT_MAX_LENGTH`].
pub const LHKEY_SEGMENT_MAX_LENGTH_LOG2: u32 = 12;
/// Maximum number of bytes a single part of a depth-0 node may span.
pub const LHKEY_SEGMENT_MAX_LENGTH: usize = 1 << LHKEY_SEGMENT_MAX_LENGTH_LOG2;

/// log2 of [`LHKEY_PART_COUNT`].
pub const LHKEY_PART_COUNT_LOG2: u32 = 4;
/// Fan-out per level: each depth level multiplies the maximum span by this factor.
#[allow(dead_code)] // not used as of its definition
pub const LHKEY_PART_COUNT: usize = 1 << LHKEY_PART_COUNT_LOG2;

/// log2 of [`LHKEY_LEVEL_MAX_LENGTH`].
pub const LHKEY_LEVEL_MAX_LENGTH_LOG2: u32 = LHKEY_SEGMENT_MAX_LENGTH_LOG2 + LHKEY_PART_COUNT_LOG2;
/// Maximum number of bytes a depth-0 node may span; a depth-`d` node spans at most
/// `LHKEY_LEVEL_MAX_LENGTH << (LHKEY_PART_COUNT_LOG2 * d)`.
pub const LHKEY_LEVEL_MAX_LENGTH: usize = 1 << LHKEY_LEVEL_MAX_LENGTH_LOG2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(LHKEY_SEGMENT_MAX_LENGTH_LOG2, 12);
        assert_eq!(LHKEY_SEGMENT_MAX_LENGTH, 4096);

        assert_eq!(LHKEY_PART_COUNT_LOG2, 4);
        assert_eq!(LHKEY_PART_COUNT, 16);

        assert_eq!(LHKEY_LEVEL_MAX_LENGTH_LOG2, 16);
        assert_eq!(LHKEY_LEVEL_MAX_LENGTH, 65536);
    }
}
