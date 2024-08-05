pub const LHKEY_SEGMENT_MAX_LENGTH_LOG2: u32 = 12;
pub const LHKEY_SEGMENT_MAX_LENGTH: usize = 1 << LHKEY_SEGMENT_MAX_LENGTH_LOG2;

pub const LHKEY_PART_COUNT_LOG2: u32 = 4;
#[allow(dead_code)] // not used as of its definition
pub const LHKEY_PART_COUNT: usize = 1 << LHKEY_PART_COUNT_LOG2;

pub const LHKEY_LEVEL_MAX_LENGTH_LOG2: u32 = LHKEY_SEGMENT_MAX_LENGTH_LOG2 + LHKEY_PART_COUNT_LOG2;
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
