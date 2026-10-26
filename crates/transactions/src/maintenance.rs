//! Opt-in, stop-the-world namespace collection. Durable non-expiring pins prefer
//! a leaked pin after a crash to deletion of live data. Maintenance is explicit
//! and remains fenced after cancellation or an ambiguous mutating error.
use crate::{
    Commit, ContentId, Error, FORMAT, Head, Repository, Result, Snapshot, canonical_cbor,
    codec::decode,
    tree::{Node, Tree},
};
use futures::{StreamExt, TryStreamExt, stream::FuturesUnordered};
use object_store::{PutMode, UpdateVersion};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default, Serialize, Deserialize)]
struct Control {
    epoch: u64,
    exclusive: Option<String>,
    pins: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
pub struct MaintenanceStatus {
    pub epoch: u64,
    pub exclusive: Option<String>,
    pub pins: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct Pin {
    repo: Repository,
    id: Option<String>,
    held: AtomicBool,
}
impl Pin {
    pub(crate) fn hold(&self) {
        self.held.store(true, Ordering::SeqCst);
    }
    pub(crate) fn settle(&self) {
        self.held.store(false, Ordering::SeqCst);
    }
    async fn close(&mut self) -> Result<()> {
        if self.held.load(Ordering::SeqCst) {
            return Err(Error::UnsettledPin {
                id: self.id.clone().unwrap_or_default(),
            });
        }
        if let Some(id) = &self.id {
            self.repo
                .control(|c| {
                    c.pins.remove(id);
                    Ok(())
                })
                .await?;
            self.id = None;
        }
        Ok(())
    }
}
impl Drop for Pin {
    fn drop(&mut self) {
        // A cancelled/timed-out head PUT may still publish remotely.
        if self.held.load(Ordering::SeqCst) {
            return;
        }
        if let Some(id) = self.id.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let repo = self.repo.clone();
            runtime.spawn(async move {
                let _ = repo
                    .control(|c| {
                        c.pins.remove(&id);
                        Ok(())
                    })
                    .await;
            });
        }
        // If the runtime is gone or releasing fails, the durable pin leaks safely.
    }
}

impl Repository {
    /// All clients in this namespace must use this mode before enabling collection.
    /// Each public snapshot/transaction pins the namespace until its final close.
    /// Registration is a coordination CAS; it is intentionally opt-in.
    pub fn with_coordinated_gc(mut self) -> Self {
        self.managed = true;
        self
    }

    async fn control<T>(&self, mut change: impl FnMut(&mut Control) -> Result<T>) -> Result<T> {
        let path = self.path("maintenance.cbor");
        for _ in 0..256 {
            let (mut control, mode) = match self.backend.get(&path).await {
                Ok(result) => {
                    let version = UpdateVersion {
                        e_tag: result.meta.e_tag.clone(),
                        version: result.meta.version.clone(),
                    };
                    if version.e_tag.is_none() && version.version.is_none() {
                        return Err(Error::MissingVersion);
                    }
                    (decode(&result.bytes().await?)?, PutMode::Update(version))
                }
                Err(object_store::Error::NotFound { .. }) => (Control::default(), PutMode::Create),
                Err(e) => return Err(e.into()),
            };
            let value = change(&mut control)?;
            match self
                .backend
                .put_opts(&path, canonical_cbor(&control)?.into(), mode.into())
                .await
            {
                Ok(_) => return Ok(value),
                Err(
                    object_store::Error::Precondition { .. }
                    | object_store::Error::AlreadyExists { .. },
                ) => tokio::task::yield_now().await,
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Contention { attempts: 256 })
    }

    pub(crate) async fn enter(&self) -> Result<Option<Arc<Pin>>> {
        if !self.managed {
            return Ok(None);
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.control(|c| {
            if c.exclusive.is_some() {
                return Err(Error::MaintenanceBusy);
            }
            c.pins.insert(id.clone());
            Ok(())
        })
        .await?;
        Ok(Some(Arc::new(Pin {
            repo: self.clone(),
            id: Some(id),
            held: AtomicBool::new(false),
        })))
    }

    pub async fn active_pins(&self) -> Result<Vec<String>> {
        Ok(self.maintenance_status().await?.pins)
    }

    pub async fn maintenance_status(&self) -> Result<MaintenanceStatus> {
        match self.backend.get(&self.path("maintenance.cbor")).await {
            Ok(result) => {
                let control: Control = decode(&result.bytes().await?)?;
                Ok(MaintenanceStatus {
                    epoch: control.epoch,
                    exclusive: control.exclusive,
                    pins: control.pins.into_iter().collect(),
                })
            }
            Err(object_store::Error::NotFound { .. }) => Ok(MaintenanceStatus {
                epoch: 0,
                exclusive: None,
                pins: Vec::new(),
            }),
            Err(e) => Err(e.into()),
        }
    }

    /// Operator recovery only: the former holder and its in-flight requests must
    /// be permanently stopped/settled before removing a crash-leaked pin.
    pub async fn recover_abandoned_pin(&self, id: &str) -> Result<()> {
        self.control(|c| {
            c.pins.remove(id);
            Ok(())
        })
        .await
    }

    pub async fn maintenance(&self) -> Result<Maintenance> {
        if !self.managed {
            return Err(Error::MaintenanceDisabled);
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.control(|c| {
            if c.exclusive.is_some() {
                return Err(Error::MaintenanceBusy);
            }
            if !c.pins.is_empty() {
                return Err(Error::ActivePins {
                    count: c.pins.len(),
                });
            }
            c.epoch = c
                .epoch
                .checked_add(1)
                .ok_or_else(|| Error::Corrupt("maintenance epoch overflow".into()))?;
            c.exclusive = Some(token.clone());
            Ok(())
        })
        .await?;
        Ok(Maintenance {
            repo: self.clone(),
            token,
            poisoned: false,
        })
    }

    /// Explicit operator recovery after ALL interrupted mutation requests have
    /// settled. Never automatically expire a maintenance barrier by wall time.
    pub async fn recover_maintenance(&self, token: &str) -> Result<()> {
        self.control(|c| {
            if c.exclusive.as_deref() != Some(token) {
                return Err(Error::MaintenanceBusy);
            }
            c.exclusive = None;
            Ok(())
        })
        .await
    }
}

impl Snapshot {
    /// Close this handle. The final clone awaits durable unpinning; other clones
    /// retain the pin. Drop is best-effort, so explicit close is useful before GC.
    pub async fn close(self) -> Result<()> {
        if let Some(pin) = self.pin
            && let Ok(mut pin) = Arc::try_unwrap(pin)
        {
            pin.close().await?;
        }
        Ok(())
    }
    pub fn pin_id(&self) -> Option<&str> {
        self.pin.as_ref().and_then(|p| p.id.as_deref())
    }
}

#[derive(Debug, Default, Serialize)]
pub struct GcReport {
    pub retained_objects: usize,
    pub deleted_objects: usize,
    pub deleted_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct GcOptions {
    /// Scratch metadata/frontier live on disk, never as a world-sized RAM set.
    pub scratch_directory: Option<std::path::PathBuf>,
    pub max_object_bytes: usize,
    pub delete_concurrency: usize,
}
impl Default for GcOptions {
    fn default() -> Self {
        Self {
            scratch_directory: None,
            max_object_bytes: 128 * 1024 * 1024,
            delete_concurrency: 32,
        }
    }
}

struct DiskMarks {
    dir: tempfile::TempDir,
    reader: std::io::BufReader<std::fs::File>,
    writer: std::fs::File,
    count: usize,
}
fn scratch_error(error: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::Store(object_store::Error::Generic {
        store: "gc-scratch",
        source: Box::new(error),
    })
}
impl DiskMarks {
    fn new(parent: Option<&std::path::Path>) -> Result<Self> {
        let dir = match parent {
            Some(p) => tempfile::tempdir_in(p),
            None => tempfile::tempdir(),
        }
        .map_err(scratch_error)?;
        let queue = dir.path().join("frontier");
        let writer = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&queue)
            .map_err(scratch_error)?;
        let reader = std::io::BufReader::new(std::fs::File::open(queue).map_err(scratch_error)?);
        Ok(Self {
            dir,
            reader,
            writer,
            count: 0,
        })
    }
    fn marker(&self, prefix: &str, id: &ContentId) -> std::path::PathBuf {
        self.dir
            .path()
            .join(prefix)
            .join(&id.as_str()[..2])
            .join(&id.as_str()[2..])
    }
    fn create(path: &std::path::Path) -> Result<bool> {
        std::fs::create_dir_all(path.parent().unwrap()).map_err(scratch_error)?;
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
        {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(scratch_error(e)),
        }
    }
    fn enqueue(&mut self, id: ContentId, kind: u8) -> Result<()> {
        use std::io::Write;
        if !Self::create(&self.marker(&format!("seen-{kind}"), &id))? {
            return Ok(());
        }
        if Self::create(&self.marker("marked", &id))? {
            self.count += 1;
        }
        let bytes = hex::decode(id.as_str()).map_err(scratch_error)?;
        self.writer
            .write_all(&[kind])
            .and_then(|_| self.writer.write_all(&bytes))
            .map_err(scratch_error)
    }
    fn next(&mut self) -> Result<Option<(ContentId, u8)>> {
        use std::io::Read;
        let mut kind = [0];
        if self.reader.read(&mut kind).map_err(scratch_error)? == 0 {
            return Ok(None);
        }
        let mut bytes = [0u8; 32];
        self.reader.read_exact(&mut bytes).map_err(scratch_error)?;
        Ok(Some((hex::encode(bytes).parse()?, kind[0])))
    }
    fn marked(&self, id: &ContentId) -> Result<bool> {
        match std::fs::metadata(self.marker("marked", id)) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(scratch_error(e)),
        }
    }
}

async fn streaming_collect(
    repo: Repository,
    options: GcOptions,
    links: impl Fn(&[u8]) -> Result<Vec<ContentId>>,
) -> Result<GcReport> {
    let mut marks = DiskMarks::new(options.scratch_directory.as_deref())?;
    let mut heads = repo.backend.list(Some(&repo.path("refs")));
    while let Some(head) = heads.try_next().await? {
        let h: Head = decode(&repo.backend.get(&head.location).await?.bytes().await?)?;
        if !crate::supported_format(h.format) {
            return Err(Error::Corrupt("unsupported head during collection".into()));
        }
        if let Some(index) = h.history {
            marks.enqueue(h.commit, 5)?;
            if let Some(root) = index.root {
                marks.enqueue(root, 3)?;
            }
        } else {
            return Err(Error::Corrupt(
                "Current heads require an acceptance index".into(),
            ));
        }
    }
    while let Some((id, kind)) = marks.next()? {
        let bytes = repo.download(&id, options.max_object_bytes).await?;
        match kind {
            5 => {
                let c: Commit = decode(&bytes)?;
                if !crate::supported_format(c.format) {
                    return Err(Error::Corrupt(
                        "unsupported commit during collection".into(),
                    ));
                }
                if let Some(id) = c.root {
                    marks.enqueue(id, 1)?;
                }
                for id in c
                    .edits
                    .into_iter()
                    .filter_map(|e| e.value)
                    .chain(c.metadata)
                    .chain(c.blobs)
                {
                    marks.enqueue(id, 2)?;
                }
            }
            1 | 3 => match decode::<Node>(&bytes)? {
                Node::Branch { children, .. } => {
                    for id in children.into_iter().flatten() {
                        marks.enqueue(id, kind)?;
                    }
                }
                Node::Extension { child, .. } => marks.enqueue(child, kind)?,
                Node::Leaf { value, .. } => marks.enqueue(value, if kind == 3 { 4 } else { 2 })?,
                Node::Deleted { .. } => {}
            },
            4 => {
                let entry: crate::RevisionEntry = decode(&bytes)?;
                marks.enqueue(entry.revision, 5)?;
            }
            _ => {
                for id in links(&bytes)? {
                    marks.enqueue(id, 2)?;
                }
            }
        }
    }
    let mut report = GcReport {
        retained_objects: marks.count,
        ..Default::default()
    };
    let prefix = format!("{}/", repo.path("objects"));
    let mut objects = repo.backend.list(Some(&repo.path("objects")));
    let mut pending = FuturesUnordered::new();
    while let Some(object) = objects.try_next().await? {
        let id: ContentId = object
            .location
            .as_ref()
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(".cbor"))
            .ok_or_else(|| Error::Corrupt("non-CBOR object in managed namespace".into()))?
            .parse()?;
        if !marks.marked(&id)? {
            let backend = repo.backend.clone();
            pending.push(async move {
                backend.delete(&object.location).await?;
                Ok::<_, Error>(object.size)
            });
            if pending.len() >= options.delete_concurrency {
                report.deleted_bytes += pending.next().await.unwrap()?;
                report.deleted_objects += 1;
            }
        }
    }
    while let Some(result) = pending.next().await {
        report.deleted_bytes += result?;
        report.deleted_objects += 1;
    }
    repo.cache.lock().unwrap().clear();
    Ok(report)
}

pub struct Maintenance {
    pub(crate) repo: Repository,
    pub(crate) token: String,
    pub(crate) poisoned: bool,
}

#[derive(Debug, Serialize)]
pub struct MigrationReport {
    pub source_prefix: String,
    pub target_prefix: String,
    pub objects: usize,
    pub bytes: u64,
    pub branches: usize,
}

pub struct RecordEdit {
    pub path: crate::OctreePath,
    pub value: Option<ciborium::value::Value>,
}
impl RecordEdit {
    pub fn put<T: Serialize>(path: crate::OctreePath, value: &T) -> Result<Self> {
        Ok(Self {
            path,
            value: Some(
                ciborium::value::Value::serialized(value)
                    .map_err(|e| Error::Encoding(e.to_string()))?,
            ),
        })
    }
    pub fn remove(path: crate::OctreePath) -> Self {
        Self { path, value: None }
    }
}
impl Maintenance {
    pub async fn branch_revision(&self, branch: &str) -> Result<ContentId> {
        self.assert_owner().await?;
        Ok(self.repo.head(branch).await?.0.id)
    }
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Copy a fenced namespace to an empty, non-overlapping prefix on the same
    /// backend. Verify immutable hashes and publish heads last. The target is
    /// opened only after the complete copy; the source stays fenced until the
    /// operator switches configuration and explicitly releases/retires it.
    pub async fn migrate_to(
        &mut self,
        target: &Repository,
        max_object_bytes: usize,
    ) -> Result<MigrationReport> {
        self.assert_owner().await?;
        if max_object_bytes == 0
            || !Arc::ptr_eq(&self.repo.backend, &target.backend)
            || target.prefix.is_empty()
            || target.prefix == self.repo.prefix
            || target.prefix.starts_with(&format!("{}/", self.repo.prefix))
            || self.repo.prefix.starts_with(&format!("{}/", target.prefix))
        {
            return Err(Error::InvalidOptions);
        }
        let mut destination = target.maintenance().await?;
        if target
            .backend
            .list(Some(&target.path("refs")))
            .try_next()
            .await?
            .is_some()
        {
            destination.finish().await?;
            return Err(Error::BranchExists(
                "migration target already has heads".into(),
            ));
        }
        self.poisoned = true;
        destination.poisoned = true;
        let mut report = MigrationReport {
            source_prefix: self.repo.prefix.clone(),
            target_prefix: target.prefix.clone(),
            objects: 0,
            bytes: 0,
            branches: 0,
        };
        let mut objects = self.repo.backend.list(Some(&self.repo.path("objects")));
        while let Some(object) = objects.try_next().await? {
            let name = object.location.filename().ok_or(Error::InvalidName)?;
            let id: ContentId = name
                .strip_suffix(".cbor")
                .ok_or(Error::InvalidName)?
                .parse()?;
            let bytes = self.repo.download(&id, max_object_bytes).await?;
            report.bytes += bytes.len() as u64;
            target.put_object(&id, bytes).await?;
            report.objects += 1;
        }
        let mut heads = self.repo.backend.list(Some(&self.repo.path("refs")));
        while let Some(object) = heads.try_next().await? {
            let bytes = self
                .repo
                .backend
                .get(&object.location)
                .await?
                .bytes()
                .await?;
            let head: Head = decode(&bytes)?;
            if !crate::supported_format(head.format) {
                return Err(Error::Corrupt("unsupported migration head".into()));
            }
            target.download(&head.commit, max_object_bytes).await?;
            let name = object.location.filename().ok_or(Error::InvalidName)?;
            target
                .backend
                .put_opts(
                    &target.path(&format!("refs/{name}")),
                    bytes.into(),
                    PutMode::Create.into(),
                )
                .await?;
            report.branches += 1;
        }
        destination.poisoned = false;
        destination.finish().await?;
        self.poisoned = false;
        Ok(report)
    }

    /// Stream a derived-index migration in bounded batches, publishing only the
    /// final root. The source snapshot is valid only inside the maintenance job.
    pub async fn rewrite<F, S>(&mut self, branch: &str, build: F) -> Result<ContentId>
    where
        F: FnOnce(Snapshot) -> S,
        S: futures::Stream<Item = Result<RecordEdit>>,
    {
        self.assert_owner().await?;
        let (parent, version) = self.repo.head(branch).await?;
        let edits = build(parent.clone());
        futures::pin_mut!(edits);
        let mut root = parent.commit.root.clone();
        let mut batch = 0usize;
        let mut ordinal = 0u64;
        let mut tree = Tree::new(self.repo.clone());
        while let Some(edit) = edits.try_next().await? {
            let value = edit.value.as_ref().map(|v| tree.stage(v)).transpose()?;
            root = tree
                .replace(
                    root.as_ref(),
                    &edit.path,
                    value.as_ref(),
                    &format!("rewrite/{}/{ordinal}", self.token),
                )
                .await?;
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| Error::Corrupt("migration counter overflow".into()))?;
            batch += 1;
            if batch == 128 {
                tree.flush(root.as_ref(), &[], 32).await?;
                tree = Tree::new(self.repo.clone());
                batch = 0;
            }
        }
        tree.flush(root.as_ref(), &[], 32).await?;
        let commit = Commit {
            format: FORMAT,
            transaction_id: uuid::Uuid::new_v4().to_string(),
            sequence: parent
                .commit
                .sequence
                .checked_add(1)
                .ok_or_else(|| Error::Corrupt("sequence overflow".into()))?,
            parent: Some(parent.id.clone()),
            base: Some(parent.id.clone()),
            root,
            edits: Vec::new(),
            actor: "maintenance".into(),
            message: "Streamed derived-index migration".into(),
            metadata: None,
            blobs: Vec::new(),
        };
        let id = self.repo.store_commit(&commit).await?;
        let history =
            crate::history::append(&self.repo, parent.history.as_ref(), &id, commit.sequence)
                .await?;
        self.poisoned = true;
        self.repo
            .backend
            .put_opts(
                &self.repo.head_path(branch)?,
                canonical_cbor(&Head {
                    format: FORMAT,
                    commit: id.clone(),
                    history,
                })?
                .into(),
                PutMode::Update(version).into(),
            )
            .await
            .map_err(|e| Repository::unknown(branch, &id, e.into()))?;
        self.poisoned = false;
        Ok(id)
    }

    pub(crate) async fn assert_owner(&self) -> Result<()> {
        let control: Control = decode(
            &self
                .repo
                .backend
                .get(&self.repo.path("maintenance.cbor"))
                .await?
                .bytes()
                .await?,
        )?;
        if control.exclusive.as_deref() != Some(&self.token) {
            return Err(Error::MaintenanceBusy);
        }
        Ok(())
    }

    /// Prune current tombstones/replacement fences only while no older reader or
    /// writer is admitted. Accepted historical checkpoints remain intact.
    pub async fn compact(&mut self, branch: &str) -> Result<ContentId> {
        self.compact_region(branch, &crate::OctreePath::root(), 10_000)
            .await
    }

    pub async fn compact_region(
        &mut self,
        branch: &str,
        region: &crate::OctreePath,
        max_nodes: usize,
    ) -> Result<ContentId> {
        if max_nodes == 0 {
            return Err(Error::InvalidOptions);
        }
        self.assert_owner().await?;
        let (parent, version) = self.repo.head(branch).await?;
        let mut tree = Tree::new(self.repo.clone());
        let root = tree
            .compact_region(parent.commit.root.as_ref(), region, max_nodes)
            .await?;
        if root == parent.commit.root {
            return Ok(parent.id);
        }
        tree.flush(root.as_ref(), &[], 32).await?;
        let commit = Commit {
            format: FORMAT,
            transaction_id: uuid::Uuid::new_v4().to_string(),
            sequence: parent
                .commit
                .sequence
                .checked_add(1)
                .ok_or_else(|| Error::Corrupt("sequence overflow".into()))?,
            parent: Some(parent.id.clone()),
            base: Some(parent.id),
            root,
            edits: Vec::new(),
            actor: "maintenance".into(),
            message: "Compact current tombstones and replacement fences".into(),
            metadata: None,
            blobs: Vec::new(),
        };
        let id = self.repo.store_commit(&commit).await?;
        let history =
            crate::history::append(&self.repo, parent.history.as_ref(), &id, commit.sequence)
                .await?;
        self.poisoned = true;
        self.repo
            .backend
            .put_opts(
                &self.repo.head_path(branch)?,
                canonical_cbor(&Head {
                    format: FORMAT,
                    commit: id.clone(),
                    history,
                })?
                .into(),
                PutMode::Update(version).into(),
            )
            .await
            .map_err(|e| Repository::unknown(branch, &id, e.into()))?;
        self.poisoned = false;
        Ok(id)
    }

    /// Trace every accepted commit/checkpoint, including superseded edit and
    /// audit payloads. The callback MUST enumerate application-defined blob links.
    /// No accepted history is pruned. Only unreachable objects are deleted.
    pub async fn collect(
        &mut self,
        links: impl Fn(&[u8]) -> Result<Vec<ContentId>> + Send + 'static,
    ) -> Result<GcReport> {
        self.collect_with_options(GcOptions::default(), links).await
    }

    pub async fn collect_with_options(
        &mut self,
        options: GcOptions,
        links: impl Fn(&[u8]) -> Result<Vec<ContentId>> + Send + 'static,
    ) -> Result<GcReport> {
        if options.max_object_bytes == 0 || options.delete_concurrency == 0 {
            return Err(Error::InvalidOptions);
        }
        self.assert_owner().await?;
        self.poisoned = true;
        let repo = self.repo.clone();
        let runtime = tokio::runtime::Handle::current();
        let report = tokio::task::spawn_blocking(move || {
            runtime.block_on(streaming_collect(repo, options, links))
        })
        .await
        .map_err(scratch_error)??;
        self.poisoned = false;
        Ok(report)
    }

    /// Must be explicit. Cancellation, crashes, and uncertain deletes retain the
    /// barrier; a timeout is not evidence that old remote requests have stopped.
    pub async fn finish(self) -> Result<()> {
        if self.poisoned {
            return Err(Error::MaintenancePoisoned { token: self.token });
        }
        self.repo.recover_maintenance(&self.token).await
    }
}
