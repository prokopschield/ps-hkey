pub mod constants;
pub mod implementations;
pub mod methods;

use std::{
    fmt::{Display, Write},
    future::Future,
    sync::Arc,
};

use futures::future::try_join_all;
use ps_datachunk::{DataChunk, PsDataChunkError};
use ps_hash::Hash;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::{Hkey, PsHkeyError, Range, Store};

use super::LongHkey;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct LongHkeyExpanded {
    depth: u32,
    size: usize,
    parts: Arc<[(Range, Hkey)]>,
}

impl LongHkeyExpanded {
    #[must_use]
    pub const fn new(depth: u32, size: usize, parts: Arc<[(Range, Hkey)]>) -> Self {
        Self { depth, size, parts }
    }

    pub fn resolve<C, E, S>(&self, store: &S) -> Result<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk = C, Error = E> + Sync,
    {
        self.resolve_slice(store, 0..self.size)
    }

    pub fn resolve_slice<C, E, S>(&self, store: &S, range: Range) -> Result<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk = C, Error = E> + Sync,
    {
        // Collect the data chunks in parallel
        let result: Result<Vec<Arc<[u8]>>, E> = self
            .parts
            .par_iter()
            .filter_map(|(part_range, hkey)| {
                if part_range.end <= range.start || part_range.start >= range.end {
                    // Skip parts that are completely outside the requested range
                    None
                } else {
                    // Calculate the overlapping range
                    let overlap_start = range.start.max(part_range.start) - part_range.start;
                    let overlap_end = range.end.min(part_range.end) - part_range.start;
                    let overlap_range = overlap_start..overlap_end;

                    // Fetch the data chunk using the resolver
                    Some(hkey.resolve_slice(store, overlap_range))
                }
            })
            .collect();

        let parts = result?;

        // Combine the results into a single vector
        let mut combined_result = Vec::with_capacity(range.end - range.start);

        for part in parts {
            combined_result.extend_from_slice(&part);
        }

        // Convert the result vector into an Arc<[u8]>
        Ok(combined_result.into())
    }

    pub async fn resolve_async<'lt, C, E, F, Ff>(&self, resolver: &F) -> Result<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = Result<C, E>> + Send + Sync,
    {
        self.resolve_slice_async(resolver, 0..self.size).await
    }

    pub async fn resolve_slice_async<'lt, C, E, F, Ff>(
        &self,
        resolver: &F,
        range: Range,
    ) -> Result<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = Result<C, E>> + Send + Sync,
    {
        let futures = self
            .parts
            .iter()
            .filter_map(|(part_range, hkey)| {
                if part_range.end <= range.start || part_range.start >= range.end {
                    // Skip parts that are completely outside the requested range
                    None
                } else {
                    // Calculate the overlapping range
                    let overlap_start = range.start.max(part_range.start) - part_range.start;
                    let overlap_end = range.end.min(part_range.end) - part_range.start;
                    let overlap_range = overlap_start..overlap_end;

                    // Fetch the data chunk using the resolver
                    Some((hkey, overlap_range))
                }
            })
            .map(|(hkey, overlap_range)| async move {
                let chunk = hkey.resolve_slice_async(resolver, overlap_range).await?;

                Ok::<_, E>(chunk)
            });

        let parts = try_join_all(futures).await?;

        // Combine the results into a single vector
        let mut combined_result = Vec::with_capacity(range.end - range.start);

        for part in parts {
            combined_result.extend_from_slice(&part);
        }

        // Convert the result vector into an Arc<[u8]>
        Ok(combined_result.into())
    }
}

impl Display for LongHkeyExpanded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}{};{};", '{', self.depth, self.size))?;

        for i in 0..self.parts.len() {
            let part = &self.parts[i];

            f.write_fmt(format_args!(
                "{}{}-{}:{}",
                if i == 0 { "" } else { "," },
                part.0.start,
                part.0.end - 1,
                part.1
            ))?;
        }

        f.write_char('}')
    }
}

/// the longer buffer is greater, or compare parts
impl Ord for LongHkeyExpanded {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let cmp = self.size.cmp(&other.size);

        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }

        let cmp = self.parts.len().cmp(&other.parts.len());

        if cmp != std::cmp::Ordering::Equal {
            return cmp;
        }

        for i in 0..self.parts.len() {
            let cmp = self.parts[i].1.cmp(&other.parts[i].1);

            if cmp != std::cmp::Ordering::Equal {
                return cmp;
            }
        }

        std::cmp::Ordering::Equal
    }
}

impl PartialOrd for LongHkeyExpanded {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<LongHkey> for Hkey {
    fn from(lhkey: LongHkey) -> Self {
        Self::LongHkey(Arc::from(lhkey))
    }
}

impl From<Arc<LongHkey>> for Hkey {
    fn from(lhkey: Arc<LongHkey>) -> Self {
        Self::LongHkey(lhkey)
    }
}

impl From<&Arc<LongHkey>> for Hkey {
    fn from(lhkey: &Arc<LongHkey>) -> Self {
        Self::LongHkey(lhkey.clone())
    }
}

impl From<LongHkeyExpanded> for Hkey {
    fn from(lhkey: LongHkeyExpanded) -> Self {
        Self::LongHkeyExpanded(Arc::from(lhkey))
    }
}

impl From<Arc<LongHkeyExpanded>> for Hkey {
    fn from(lhkey: Arc<LongHkeyExpanded>) -> Self {
        Self::LongHkeyExpanded(lhkey)
    }
}

impl From<&Arc<LongHkeyExpanded>> for Hkey {
    fn from(lhkey: &Arc<LongHkeyExpanded>) -> Self {
        Self::LongHkeyExpanded(lhkey.clone())
    }
}
