use std::sync::Arc;

use ps_datachunk::{DataChunk, DataChunkError};
use ps_util::ToResult;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{
    long::{long_hkey_expanded::constants::LHKEY_SEGMENT_MAX_LENGTH, LongHkeyExpanded},
    Hkey, HkeyError, Range, Store,
};

use super::update::helpers::{calculate_depth, calculate_segment_length};

impl LongHkeyExpanded {
    /// Normalizes a segment of this `LongHkeyExpanded` within the given range, producing a new
    /// `LongHkeyExpanded` optimized based on depth and size. Uses parallel processing for large
    /// segments and recursive normalization for depth >= 1.
    ///
    /// # Arguments
    /// - `store`: The data store for resolving and storing chunks.
    /// - `depth`: A lower bound on the result's depth label; the effective label is
    ///   `calculate_depth(depth, length)`. Results whose parts are direct data hkeys are
    ///   labelled 0 regardless of part count.
    /// - `range`: The range to normalize (start..end, inclusive start, exclusive end).
    ///
    /// # Returns
    /// A `LongHkeyExpanded` containing the normalized segment, or an error if resolution fails.
    pub fn normalize_segment<'a, C, E, S>(
        &self,
        store: &'a S,
        depth: u32,
        range: Range,
    ) -> Result<Self, E>
    where
        C: DataChunk,
        E: From<HkeyError> + From<DataChunkError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        if range.start > range.end {
            HkeyError::InvalidRange(range.clone()).err()?;
        }

        if range.end == range.start {
            return Ok(Self::default());
        }

        // Check for existing segment
        if let Some(result) = self.parts.iter().find_map(|segment| {
            if segment.0 == range {
                match &segment.1 {
                    Hkey::LongHkeyExpanded(lhkey) => Some(Ok(lhkey.clone())),
                    Hkey::LongHkey(lhkey) => Some(lhkey.expand(store)),
                    _ => None,
                }
            } else {
                None
            }
        }) {
            return result;
        }

        let length = range.end - range.start;
        let depth = calculate_depth(depth, length);

        if depth == 0 && length <= LHKEY_SEGMENT_MAX_LENGTH {
            let data = self.resolve_slice(store, range)?;
            let segment_hkey = store.put(&data)?;
            let segment_parts = Arc::from([(0..length, segment_hkey)]);
            let lhkey = Self::new(0, data.len(), segment_parts);

            return Ok(lhkey);
        }

        if depth == 0 {
            let count = length.div_ceil(LHKEY_SEGMENT_MAX_LENGTH);
            let iterator = (0..count).into_par_iter();

            let parts: Result<Vec<_>, E> = iterator
                .map(|index| {
                    let begin = index * LHKEY_SEGMENT_MAX_LENGTH;
                    let end = length.min(begin + LHKEY_SEGMENT_MAX_LENGTH);

                    let data = self.resolve_slice(store, range.start + begin..range.start + end)?;
                    let hkey = store.put(&data)?;

                    Ok::<_, E>((begin..end, hkey))
                })
                .collect();

            let parts = Arc::from(parts?.into_boxed_slice());

            let lhkey = Self::new(0, length, parts);

            return Ok(lhkey);
        }

        // if depth >= 1, resolve recursively

        let segment_length = calculate_segment_length(depth);

        let iterator = (0..length.div_ceil(segment_length)).into_par_iter();

        let parts: Result<Vec<_>, E> = iterator
            .map(|index| {
                let begin = index * segment_length;
                let end = length.min(begin + segment_length);

                let segment = self.normalize_segment(
                    store,
                    depth - 1,
                    range.start + begin..range.start + end,
                )?;

                let hkey = Hkey::LongHkey(segment.store(store)?);

                Ok::<_, E>((begin..end, hkey))
            })
            .collect();

        let parts = Arc::from(parts?.into_boxed_slice());

        let lhkey = Self::new(depth, length, parts);

        Ok(lhkey)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::long::long_hkey_expanded::constants::LHKEY_LEVEL_MAX_LENGTH;
    use crate::InMemoryStore;

    #[allow(clippy::cast_possible_truncation)]
    fn sequential_bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    /// Builds a valid depth-0 node whose parts cover `original` on the
    /// [`LHKEY_SEGMENT_MAX_LENGTH`] grid.
    fn flat_fixture(store: &InMemoryStore, original: &[u8]) -> LongHkeyExpanded {
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

        LongHkeyExpanded::new(0, original.len(), parts.into())
    }

    /// A result whose parts are direct data hkeys carries the depth-0 label, regardless of part
    /// count.
    #[test]
    fn flat_multi_part_result_is_depth_zero() {
        let store = InMemoryStore::default();

        let original = sequential_bytes(10000);
        let lhkey = flat_fixture(&store, &original);

        let segment = lhkey
            .normalize_segment(&store, 0, 0..original.len())
            .expect("Failed to normalize");

        assert_eq!(segment.depth, 0, "Depth label");
        assert!(
            segment.to_string().starts_with("{0;10000;"),
            "Serialized prefix"
        );
    }

    /// `normalize_segment` and `from_blob` converge on the same node, and therefore the same
    /// serialization and content address, for the same content.
    #[test]
    fn result_matches_from_blob() {
        let store = InMemoryStore::default();

        let original = sequential_bytes(10000);
        let via_blob = LongHkeyExpanded::from_blob(&store, &original).expect("Failed to build");
        let node = flat_fixture(&store, &original);

        let normalized = node
            .normalize_segment(&store, 0, 0..original.len())
            .expect("Failed to normalize");

        assert_eq!(normalized, via_blob, "Node equality");
        assert_eq!(
            normalized.to_string(),
            via_blob.to_string(),
            "Serialization equality"
        );
    }

    /// The recursive branch labels its children exactly one level below the parent. A fixture on
    /// the [`LHKEY_SEGMENT_MAX_LENGTH`] grid keeps the existing-segment shortcut from bypassing
    /// the flat multi-part branch.
    #[test]
    fn recursive_children_are_labelled_one_less() {
        let store = InMemoryStore::default();

        let original = sequential_bytes(LHKEY_LEVEL_MAX_LENGTH + LHKEY_SEGMENT_MAX_LENGTH);
        let lhkey = flat_fixture(&store, &original);

        let segment = lhkey
            .normalize_segment(&store, 1, 0..original.len())
            .expect("Failed to normalize");

        assert_eq!(segment.depth, 1, "Parent depth label");

        for (range, hkey) in segment.parts.iter() {
            match hkey {
                Hkey::LongHkey(child) => {
                    let child = child.expand(&store).expect("Failed to expand");

                    assert_eq!(child.depth, 0, "Child depth label for part {range:?}");
                }
                other => panic!("Expected a LongHkey part, got {other:?}"),
            }
        }
    }
}
