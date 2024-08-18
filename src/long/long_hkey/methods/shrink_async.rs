use std::future::Future;

use crate::{long::LongHkey, Hkey, PsHkeyError};

impl LongHkey {
    /// transforms this LongHkey into a Hkey::ListRef asynchronously
    pub async fn shrink_async<E, Ef, F, Ff>(&self, store: &F) -> Result<Hkey, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> Ff,
        Ff: Future<Output = Result<Hkey, Ef>> + Sync,
    {
        let hkey = store(self.to_string().as_bytes()).await?;
        let hkey = hkey.encrypted_into_list_ref()?;

        Ok(hkey)
    }
}
