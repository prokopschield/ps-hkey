#![allow(clippy::expect_used)]

use super::*;
use crate::long::long_hkey_expanded::constants::LHKEY_LEVEL_MAX_LENGTH;
use crate::{InMemoryStore, InMemoryStoreError};

#[allow(clippy::cast_possible_truncation)]
fn sequential_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Builds a [`LongHkeyExpanded`] of the given depth whose parts cover `original` in
/// segments of at most [`LHKEY_SEGMENT_MAX_LENGTH`] bytes.
///
/// Depth 0 builds a valid node. A nonzero depth stamps a deliberately invalid label onto
/// direct-data parts, modelling legacy or adversarial wire input.
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

/// Writes a patch of `range.len()` bytes at `range` over `original_len` sequential bytes, and
/// asserts that the result is the original grown to `range.end`, carrying the patch at `range`,
/// and zeros between the end of the original and `range.start`.
///
/// Requires `original_len <= range.end`; a longer original keeps the bytes past `range.end`, which
/// the expectation built here does not model.
fn assert_write_past_the_end(original_len: usize, range: Range) {
    let store = InMemoryStore::default();

    let original = sequential_bytes(original_len);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let patch = vec![0xBB; range.end - range.start];

    let updated = lhkey
        .update(&store, &patch, range.clone())
        .expect("Failed to update");

    let mut expected = original;

    expected.resize(range.start, 0);
    expected.extend_from_slice(&patch);

    let resolved = updated
        .resolve_slice(&store, 0..range.end)
        .expect("Failed to resolve");

    let mismatch = resolved
        .iter()
        .zip(expected.iter())
        .position(|(got, want)| got != want);

    assert_eq!(updated.size(), range.end, "Reported size");
    assert_eq!(resolved.len(), expected.len(), "Resolved length");
    assert_eq!(mismatch, None, "Index of the first mismatched byte");
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
/// The fixture is therefore a genuine depth-1 tree built by `from_blob`, so that rejection can
/// only come from `update` itself.
#[test]
#[allow(clippy::reversed_empty_ranges)]
fn update_inverted_range_errors() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_LEVEL_MAX_LENGTH);
    let lhkey = LongHkeyExpanded::from_blob(&store, &original).expect("Failed to build");

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

/// At depth >= 1 an empty range is short-circuited by the early return in `update`. The write is
/// the identity either way, but the early return avoids renormalizing and re-storing every
/// segment of the receiver. Also reachable as `update(store, &[], 20..30)`, which clamps to
/// `20..20`.
#[test]
fn update_empty_range_at_depth_one_is_accepted() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_LEVEL_MAX_LENGTH);
    let lhkey = LongHkeyExpanded::from_blob(&store, &original).expect("Failed to build");

    lhkey
        .update(&store, &[0x66; 10], 20..20)
        .expect("An empty range should be accepted");
}

/// Empty data clamps a non-empty range to an empty one, reaching the same early return as
/// [`update_empty_range_at_depth_one_is_accepted`] without an empty range in the signature.
#[test]
fn update_empty_data_at_depth_one_is_accepted() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_LEVEL_MAX_LENGTH);
    let lhkey = LongHkeyExpanded::from_blob(&store, &original).expect("Failed to build");

    lhkey
        .update(&store, &[], 20..30)
        .expect("Empty data should be accepted");
}

/// Parsing accepts legacy nodes whose direct-data parts carry a depth-1 label, the shape the old
/// `normalize_segment` stored. A non-empty `update` on such a node must terminate: its segments
/// are renormalized to depth 0, so the recursion shrinks on `depth`. Before depth unification
/// this call recursed unboundedly.
#[test]
fn update_on_mislabelled_depth_one_node_terminates() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 1);

    let updated = lhkey
        .update(&store, &[0xAA; 10], 100..110)
        .expect("Failed to update");

    let resolved = updated
        .resolve_slice(&store, 0..original.len())
        .expect("Failed to resolve");

    let mut expected = original;

    expected[100..110].fill(0xAA);

    assert_eq!(&resolved[..], &expected[..]);
}

/// A patch that starts inside one segment and ends inside a later one takes the "part begins with
/// new data" branch of `update_flat`, whose untouched tail must be read from `range.end` onward.
#[test]
fn update_across_a_segment_boundary_preserves_the_tail() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let patch = vec![0xAA; LHKEY_SEGMENT_MAX_LENGTH + 804];
    let range = 100..100 + patch.len();

    let mut expected = original;

    expected[range.clone()].copy_from_slice(&patch);

    let updated = lhkey
        .update(&store, &patch, range)
        .expect("Failed to update");

    let resolved = updated
        .resolve_slice(&store, 0..expected.len())
        .expect("Failed to resolve");

    let mismatch = resolved
        .iter()
        .zip(expected.iter())
        .position(|(got, want)| got != want);

    assert_eq!(resolved.len(), expected.len(), "Resolved length");
    assert_eq!(mismatch, None, "Index of the first mismatched byte");
}

/// `update_flat` must report the size its parts cover, not the number of bytes it wrote.
#[test]
fn update_reports_the_full_size_after_a_partial_write() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(2 * LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let updated = lhkey
        .update(&store, &[0xAA; 10], 100..110)
        .expect("Failed to update");

    assert_eq!(updated.size(), original.len());
}

/// A write that begins past the end grows the buffer, zero-filling the gap it leaves behind. The
/// whole write lands in one part, so it is the "range is entirely within part" branch that reads
/// an original shorter than the part it fills.
#[test]
fn update_past_the_end_grows_the_buffer() {
    assert_write_past_the_end(100, 200..300);
}

/// The gap can span whole segments. Those parts lie entirely outside the written range, so their
/// content comes from the receiver alone, which holds nothing at that offset.
#[test]
fn update_past_the_end_skipping_whole_segments() {
    assert_write_past_the_end(100, 8300..8400);
}

/// A write that begins past the end and reaches beyond the first segment takes the "part begins
/// with original data" branch, whose head read also falls short of what the part declares.
#[test]
fn update_past_the_end_across_a_segment_boundary() {
    assert_write_past_the_end(100, 200..5000);
}

/// A write that starts inside the buffer and ends past it extends the buffer, keeping the bytes
/// before `range.start` intact.
#[test]
fn update_extending_past_the_end_keeps_the_head() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(100);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let patch = [0xBBu8; 100];

    let updated = lhkey
        .update(&store, &patch, 50..150)
        .expect("Failed to update");

    let resolved = updated
        .resolve_slice(&store, 0..150)
        .expect("Failed to resolve");

    assert_eq!(updated.size(), 150);
    assert_eq!(&resolved[..50], &original[..50]);
    assert_eq!(&resolved[50..], &patch[..]);
}

/// `normalize_segment` preserves the content of the range it normalizes.
#[test]
fn normalize_segment_preserves_content() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(5000);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let segment = lhkey
        .normalize_segment(&store, 0, 0..original.len())
        .expect("Failed to normalize");

    let resolved = segment
        .resolve_slice(&store, 0..original.len())
        .expect("Failed to resolve");

    assert_eq!(&resolved[..], &original[..]);
}

/// The parts `normalize_segment` emits must not declare more bytes than the segment holds.
#[test]
fn normalize_segment_declares_parts_within_its_size() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(5000);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let segment = lhkey
        .normalize_segment(&store, 0, 0..original.len())
        .expect("Failed to normalize");

    let last = segment.parts.last().expect("No parts").0.clone();

    assert_eq!(last.end, segment.size(), "Last part overruns the segment");
}

/// The recursive branch declares its parts against the same grid, so it must clamp the last one
/// too. A segment of this length is split into one full part and a short tail.
#[test]
fn normalize_segment_declares_parts_within_its_size_at_depth_one() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(LHKEY_LEVEL_MAX_LENGTH + LHKEY_SEGMENT_MAX_LENGTH);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let segment = lhkey
        .normalize_segment(&store, 1, 0..original.len())
        .expect("Failed to normalize");

    let last = segment.parts.last().expect("No parts").0.clone();

    assert_eq!(segment.parts.len(), 2, "Part count");
    assert_eq!(last.end, segment.size(), "Last part overruns the segment");
}

/// A `LongHkeyExpanded` must survive a `Display` and `parse` round trip.
#[test]
fn display_and_parse_round_trip() {
    let store = InMemoryStore::default();

    let original = sequential_bytes(5000);
    let lhkey = lhkey_of_depth(&store, &original, 0);

    let text = lhkey.to_string();

    let parsed = crate::Hkey::try_parse(&text).expect("Failed to parse");

    assert_eq!(parsed.to_string(), text);
}
