#![allow(clippy::expect_used)]

use super::*;
use crate::{InMemoryStore, InMemoryStoreError};

#[allow(clippy::cast_possible_truncation)]
fn sequential_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Builds a [`LongHkeyExpanded`] of the given depth whose parts cover `original` in
/// segments of at most [`LHKEY_SEGMENT_MAX_LENGTH`] bytes.
fn lhkey_of_depth(store: &InMemoryStore, original: &[u8], depth: u32) -> LongHkeyExpanded {
    let seg = LHKEY_SEGMENT_MAX_LENGTH;

    let parts: Vec<(Range, Hkey)> = (0..original.len().div_ceil(seg))
        .map(|index| {
            let start = index * seg;
            let end = (start + seg).min(original.len());
            let hkey = store
                .put(&original[start..end])
                .expect("Failed to store segment");

            (start..end, hkey)
        })
        .collect();

    LongHkeyExpanded::new(depth, original.len(), parts.into())
}

fn assert_invalid_range(result: Result<LongHkeyExpanded, InMemoryStoreError>, expected: &Range) {
    match result {
        Err(InMemoryStoreError::Hkey(HkeyError::InvalidRange(range))) => {
            assert_eq!(&range, expected, "The rejected range was misreported");
        }
        other => panic!("Expected HkeyError::InvalidRange({expected:?}), got {other:?}"),
    }
}

#[test]
#[allow(clippy::reversed_empty_ranges)]
fn update_flat_inverted_range_errors() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let result = lhkey.update_flat(&store, &[0x66; 10], &(20..10));

    assert_invalid_range(result, &(20..10));
}

/// At depth 0, `update` delegates to `update_flat`, whose own guard would reject the range.
/// The fixture is therefore built at depth 1, so that rejection can only come from `update`.
#[test]
#[allow(clippy::reversed_empty_ranges)]
fn update_inverted_range_errors() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 1);

    let result = lhkey.update(&store, &[0x66; 10], 20..10);

    assert_invalid_range(result, &(20..10));
}

/// An empty range is well-formed; only `start > end` is rejected.
#[test]
fn update_flat_empty_range_is_accepted() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    lhkey
        .update_flat(&store, &[0x66; 10], &(20..20))
        .expect("An empty range should be accepted");
}

/// An empty range is well-formed; only `start > end` is rejected.
#[test]
fn update_empty_range_is_accepted() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    lhkey
        .update(&store, &[0x66; 10], 20..20)
        .expect("An empty range should be accepted");
}

/// At depth >= 1 an empty range must be short-circuited by the early return in `update`: the segments
/// `normalize_segment` hands back keep the same depth and size as their parent, so the recursion
/// has no shrinking measure and would overflow the stack. Also reachable as
/// `update(store, &[], 20..30)`, which clamps to `20..20`.
#[test]
fn update_empty_range_at_depth_one_is_accepted() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 1);

    lhkey
        .update(&store, &[0x66; 10], 20..20)
        .expect("An empty range should be accepted");
}

/// Empty data clamps a non-empty range to an empty one, reaching the same recursion as
/// [`update_empty_range_at_depth_one_is_accepted`] without an empty range in the signature.
#[test]
fn update_empty_data_at_depth_one_is_accepted() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 1);

    lhkey
        .update(&store, &[], 20..30)
        .expect("Empty data should be accepted");
}
