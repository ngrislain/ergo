//! Advisory-lock CAS for a local development object store. All writers to the
//! transaction namespace must use this adapter, on a filesystem supporting locks
//! and atomic replacement. This is not a distributed/network-filesystem backend.
use async_trait::async_trait;
use futures::stream::BoxStream;
use object_store::{
    GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore, PutMode,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, local::LocalFileSystem, path::Path,
};
use std::{
    fmt,
    fs::{File, OpenOptions},
    path::{Path as FilePath, PathBuf},
    sync::Arc,
};

#[derive(Debug)]
pub struct LocalCasStore {
    inner: Arc<LocalFileSystem>,
    lock: PathBuf,
}

fn error(e: impl std::error::Error + Send + Sync + 'static) -> object_store::Error {
    object_store::Error::Generic {
        store: "local-cas",
        source: Box::new(e),
    }
}

impl LocalCasStore {
    pub fn new(root: impl AsRef<FilePath>) -> object_store::Result<Self> {
        let root = root.as_ref().canonicalize().map_err(error)?;
        Ok(Self {
            inner: Arc::new(LocalFileSystem::new_with_prefix(&root)?),
            lock: root.join(".ergo-transaction-cas.lock"),
        })
    }

    async fn guard(&self) -> object_store::Result<File> {
        loop {
            let path = self.lock.clone();
            let guard = tokio::task::spawn_blocking(move || {
                let file = OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(path)?;
                match file.try_lock() {
                    Ok(()) => Ok(Some(file)),
                    Err(std::fs::TryLockError::WouldBlock) => Ok(None),
                    Err(std::fs::TryLockError::Error(e)) => Err(e),
                }
            })
            .await
            .map_err(error)?
            .map_err(error)?;
            if let Some(guard) = guard {
                return Ok(guard);
            }
            // Lock waiters must not fill Tokio's blocking pool while its holder
            // needs another worker to finish a filesystem operation.
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }

    async fn mutate<T, F, Fut>(&self, operation: F) -> object_store::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<LocalFileSystem>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = object_store::Result<T>>,
    {
        let guard = self.guard().await?;
        let inner = self.inner.clone();
        let runtime = tokio::runtime::Handle::current();
        // A blocking worker owns the guard until the actual I/O settles. Dropping
        // an awaiting caller cannot unlock an operation still running remotely
        // in LocalFileSystem's own non-cancellable blocking worker.
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            runtime.block_on(operation(inner))
        })
        .await
        .map_err(error)?
    }
}

impl fmt::Display for LocalCasStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "local CAS ({})", self.inner)
    }
}

#[async_trait]
impl ObjectStore for LocalCasStore {
    async fn put_opts(
        &self,
        path: &Path,
        payload: PutPayload,
        mut opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        // Native create-if-absent is already atomic and cannot overwrite a key
        // observed by a conditional updater. Immutable uploads need no global lock.
        if matches!(opts.mode, PutMode::Create) {
            return self.inner.put_opts(path, payload, opts).await;
        }
        let path = path.clone();
        self.mutate(move |inner| async move {
            if let PutMode::Update(expected) = &opts.mode {
                let matches = match inner.head(&path).await {
                    Ok(meta) => {
                        (expected.e_tag.is_some() || expected.version.is_some())
                            && meta.e_tag == expected.e_tag
                            && meta.version == expected.version
                    }
                    Err(object_store::Error::NotFound { .. }) => false,
                    Err(e) => return Err(e),
                };
                if !matches {
                    return Err(object_store::Error::Precondition {
                        path: path.to_string(),
                        source: std::io::Error::other("local object version changed").into(),
                    });
                }
                opts.mode = PutMode::Overwrite;
            }
            inner.put_opts(&path, payload, opts).await
        })
        .await
    }
    async fn put_multipart_opts(
        &self,
        _: &Path,
        _: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        Err(object_store::Error::NotImplemented)
    }
    async fn get_opts(&self, p: &Path, o: GetOptions) -> object_store::Result<GetResult> {
        self.inner.get_opts(p, o).await
    }
    async fn delete(&self, p: &Path) -> object_store::Result<()> {
        let p = p.clone();
        self.mutate(move |inner| async move { inner.delete(&p).await })
            .await
    }
    fn list(&self, p: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.inner.list(p)
    }
    async fn list_with_delimiter(&self, p: Option<&Path>) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(p).await
    }
    async fn copy(&self, a: &Path, b: &Path) -> object_store::Result<()> {
        let a = a.clone();
        let b = b.clone();
        self.mutate(move |inner| async move { inner.copy(&a, &b).await })
            .await
    }
    async fn copy_if_not_exists(&self, a: &Path, b: &Path) -> object_store::Result<()> {
        let a = a.clone();
        let b = b.clone();
        self.mutate(move |inner| async move { inner.copy_if_not_exists(&a, &b).await })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn caller_cancellation_does_not_release_an_in_flight_critical_section() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(LocalCasStore::new(dir.path()).unwrap());
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let first = {
            let store = store.clone();
            let started = started.clone();
            let release = release.clone();
            tokio::spawn(async move {
                store
                    .mutate(move |_| async move {
                        started.notify_one();
                        release.notified().await;
                        Ok(())
                    })
                    .await
            })
        };
        started.notified().await;
        first.abort();
        let _ = first.await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let second = {
            let store = store.clone();
            let entered = entered.clone();
            tokio::spawn(async move {
                store
                    .mutate(move |_| async move {
                        entered.notify_one();
                        Ok(())
                    })
                    .await
            })
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), entered.notified())
                .await
                .is_err()
        );
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), second)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
