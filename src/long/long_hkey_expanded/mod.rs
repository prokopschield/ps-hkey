use std::{
    fmt::{Display, Write},
    future::Future,
    pin::Pin,
    sync::Arc,
};

use futures::future::try_join_all;
use ps_datachunk::{DataChunk, PsDataChunkError};
use ps_hash::Hash;
use ps_util::ToResult;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::{Hkey, PsHkeyError, Range};

use super::LongHkey;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct LongHkeyExpanded {
    depth: usize,
    size: usize,
    parts: Arc<[(Range, Arc<Hkey>)]>,
}

impl LongHkeyExpanded {
    pub fn new(depth: usize, size: usize, parts: Arc<[(Range, Arc<Hkey>)]>) -> Self {
        Self { depth, size, parts }
    }

    pub fn store<'lt, E, F>(&self, store: &F) -> Result<LongHkey, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> Result<Hkey, E> + Sync,
    {
        match store(format!("{}", self).as_bytes())? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => PsHkeyError::StorageError.err()?,
        }
        .ok()
    }

    pub fn resolve<'lt, E, F>(&self, resolver: &F) -> Result<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Result<DataChunk<'lt>, E> + Sync,
    {
        self.resolve_slice(resolver, 0..self.size)
    }

    pub fn resolve_slice<'lt, E, F>(&self, resolver: &F, range: Range) -> Result<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Result<DataChunk<'lt>, E> + Sync,
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
                    Some(hkey.resolve_slice(resolver, overlap_range))
                }
            })
            .collect();

        let parts = result?;

        // Combine the results into a single vector
        let mut combined_result = Vec::with_capacity(range.end - range.start);

        for part in parts {
            combined_result.extend_from_slice(&part)
        }

        // Convert the result vector into an Arc<[u8]>
        Ok(combined_result.into())
    }

    pub async fn resolve_async<'lt, E, F>(&self, resolver: &F) -> Result<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = Result<DataChunk<'lt>, E>>>> + Sync,
    {
        self.resolve_slice_async(resolver, 0..self.size).await
    }

    pub async fn resolve_slice_async<'lt, E, F>(
        &self,
        resolver: &F,
        range: Range,
    ) -> Result<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = Result<DataChunk<'lt>, E>>>> + Sync,
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
            combined_result.extend_from_slice(&part)
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
        self.cmp(other).some()
    }
}

impl From<LongHkey> for Hkey {
    fn from(lhkey: LongHkey) -> Self {
        Hkey::LongHkey(Arc::from(lhkey))
    }
}

impl From<Arc<LongHkey>> for Hkey {
    fn from(lhkey: Arc<LongHkey>) -> Self {
        Hkey::LongHkey(lhkey)
    }
}

impl From<&Arc<LongHkey>> for Hkey {
    fn from(lhkey: &Arc<LongHkey>) -> Self {
        Hkey::LongHkey(lhkey.clone())
    }
}

impl From<LongHkeyExpanded> for Hkey {
    fn from(lhkey: LongHkeyExpanded) -> Self {
        Hkey::LongHkeyExpanded(Arc::from(lhkey))
    }
}

impl From<Arc<LongHkeyExpanded>> for Hkey {
    fn from(lhkey: Arc<LongHkeyExpanded>) -> Self {
        Hkey::LongHkeyExpanded(lhkey)
    }
}

impl From<&Arc<LongHkeyExpanded>> for Hkey {
    fn from(lhkey: &Arc<LongHkeyExpanded>) -> Self {
        Hkey::LongHkeyExpanded(lhkey.clone())
    }
}
