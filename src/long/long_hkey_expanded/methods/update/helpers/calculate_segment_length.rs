use crate::long::long_hkey_expanded::constants::{
    LHKEY_PART_COUNT_LOG2, LHKEY_SEGMENT_MAX_LENGTH_LOG2,
};

#[inline(always)]
pub const fn calculate_segment_length_log2(depth: u32) -> u32 {
    let log2 = LHKEY_SEGMENT_MAX_LENGTH_LOG2 + depth * LHKEY_PART_COUNT_LOG2;

    if log2 >= usize::BITS {
        usize::BITS - 1
    } else {
        log2
    }
}

#[inline(always)]
pub const fn calculate_segment_length(depth: u32) -> usize {
    1 << calculate_segment_length_log2(depth)
}

#[cfg(test)]
mod tests {
    use crate::long::long_hkey_expanded::methods::update::helpers::calculate_segment_length;

    #[test]
    fn powers() {
        assert_eq!(calculate_segment_length(0), 0x1000);
        assert_eq!(calculate_segment_length(1), 0x10000);
        assert_eq!(calculate_segment_length(2), 0x100000);
        assert_eq!(calculate_segment_length(3), 0x1000000);
        assert_eq!(calculate_segment_length(4), 0x10000000);

        // disable on 32-bit platforms
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(calculate_segment_length(5), 0x100000000);
            assert_eq!(calculate_segment_length(6), 0x1000000000);
            assert_eq!(calculate_segment_length(7), 0x10000000000);
            assert_eq!(calculate_segment_length(8), 0x100000000000);
            assert_eq!(calculate_segment_length(9), 0x1000000000000);
            assert_eq!(calculate_segment_length(10), 0x10000000000000);
            assert_eq!(calculate_segment_length(11), 0x100000000000000);
            assert_eq!(calculate_segment_length(12), 0x1000000000000000);
        }

        // as of writing this comment, longer buffers than 2^64-1 bytes are not supported
    }
}
