use std::sync::Arc;

use rayon::{
    iter::{IndexedParallelIterator, ParallelIterator},
    slice::ParallelSlice,
};

use crate::{
    long::{
        long_hkey_expanded::{
            constants::{LHKEY_LEVEL_MAX_LENGTH, LHKEY_SEGMENT_MAX_LENGTH},
            methods::update::helpers::{calculate_depth, calculate_segment_length},
        },
        LongHkeyExpanded,
    },
    Hkey, PsHkeyError, Range,
};

impl LongHkeyExpanded {
    pub fn from_blob<E, Es, S>(store: &S, data: &[u8]) -> Result<Self, E>
    where
        E: From<Es> + From<PsHkeyError> + Send,
        Es: Into<E> + Send,
        S: Fn(&[u8]) -> Result<Hkey, Es> + Sync,
    {
        let depth = calculate_depth(0, data.len());

        let parts: Result<Vec<(Range, Hkey)>, E> = if data.len() > LHKEY_LEVEL_MAX_LENGTH {
            let segment_length = calculate_segment_length(depth);

            let chunks = data.par_chunks(segment_length);

            chunks
                .enumerate()
                .map(|(index, chunk)| {
                    let start = index * segment_length;
                    let end = start + chunk.len();
                    let hkey = LongHkeyExpanded::from_blob(store, chunk)?.shrink(store)?;

                    Ok((start..end, hkey))
                })
                .collect()
        } else {
            let chunks = data.par_chunks(LHKEY_SEGMENT_MAX_LENGTH);

            chunks
                .enumerate()
                .map(|(index, chunk)| {
                    let start = index * LHKEY_SEGMENT_MAX_LENGTH;
                    let end = start + chunk.len();
                    let hkey = store(chunk)?;

                    Ok((start..end, hkey))
                })
                .collect()
        };

        let parts = Arc::from(parts?.into_boxed_slice());
        let lhkey = LongHkeyExpanded::new(depth, data.len(), parts);

        Ok(lhkey)
    }
}
