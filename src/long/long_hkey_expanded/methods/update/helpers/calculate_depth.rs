use std::ops::Sub;

use crate::long::long_hkey_expanded::constants::{
    LHKEY_LEVEL_MAX_LENGTH, LHKEY_LEVEL_MAX_LENGTH_LOG2, LHKEY_PART_COUNT_LOG2,
};

pub fn calculate_depth(min: u32, end: usize) -> u32 {
    if end <= LHKEY_LEVEL_MAX_LENGTH {
        return min;
    }

    let log2 = (end - 1).ilog2() + 1;

    let derived = log2
        .sub(LHKEY_LEVEL_MAX_LENGTH_LOG2)
        .div_ceil(LHKEY_PART_COUNT_LOG2);

    min.max(derived)
}

#[cfg(test)]
mod tests {
    use crate::long::long_hkey_expanded::{
        constants::{LHKEY_LEVEL_MAX_LENGTH, LHKEY_PART_COUNT_LOG2},
        methods::update::helpers::calculate_depth,
    };

    #[test]
    fn border_values() {
        let max_depth = usize::MAX.ilog2() / 4 - 3;

        for depth in 0..max_depth {
            let cutoff = LHKEY_LEVEL_MAX_LENGTH << (depth * LHKEY_PART_COUNT_LOG2);

            for test in (cutoff - 8)..cutoff {
                assert_eq!(calculate_depth(0, test), depth, "failed under={:x}", test);
            }

            for test in (cutoff + 1)..(cutoff + 8) {
                assert_eq!(
                    calculate_depth(0, test),
                    depth + 1,
                    "failed over={:x}",
                    test
                );
            }
        }
    }

    #[test]
    fn powers() {
        assert_eq!(calculate_depth(0, 0x1), 0);
        assert_eq!(calculate_depth(0, 0x10), 0);
        assert_eq!(calculate_depth(0, 0x100), 0);
        assert_eq!(calculate_depth(0, 0x1000), 0);
        assert_eq!(calculate_depth(0, 0x10000), 0);
        assert_eq!(calculate_depth(0, 0x100000), 1);
        assert_eq!(calculate_depth(0, 0x1000000), 2);
        assert_eq!(calculate_depth(0, 0x10000000), 3);
        assert_eq!(calculate_depth(0, 0xFFFFFFFF), 4);

        // disable on 32-bit platforms
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(calculate_depth(0, 0x100000000), 4);
            assert_eq!(calculate_depth(0, 0x1000000000), 5);
            assert_eq!(calculate_depth(0, 0x10000000000), 6);
            assert_eq!(calculate_depth(0, 0x100000000000), 7);
            assert_eq!(calculate_depth(0, 0x1000000000000), 8);
            assert_eq!(calculate_depth(0, 0x10000000000000), 9);
            assert_eq!(calculate_depth(0, 0x100000000000000), 10);
            assert_eq!(calculate_depth(0, 0x1000000000000000), 11);
            assert_eq!(calculate_depth(0, 0xFFFFFFFFFFFFFFFF), 12);
        }

        // as of writing this comment, longer buffers than 2^64-1 bytes are not supported
    }
}
