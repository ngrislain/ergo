//! Immutable CBOR octrees with optimistic, serializable, single-root transactions.
//!
//! Disjoint writes rebase; subtree reads (including empty regions) protect against
//! phantoms and write skew. All dependencies must be read through the transaction.
//! A backend must supply linearizable conditional PUT and strong object reads.
//! See `docs/transactional-storage.md` and the `s3_edits` example in this repository.
//!
//! ```
//! use ergo_transactions::{CommitOptions, OctreePath, Repository};
//! use object_store::memory::InMemory;
//! use std::sync::Arc;
//!
//! # #[tokio::main]
//! # async fn main() -> ergo_transactions::Result<()> {
//! let repo = Repository::new(Arc::new(InMemory::new()), "geometry")?;
//! repo.create("shard-17").await?;
//! let region: OctreePath = "2/6".parse()?;
//! let mut tx = repo.begin("shard-17").await?;
//! tx.put(&region, &vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0]]).await?;
//! let accepted = tx.commit("alice", "edit vertices", CommitOptions::default()).await?;
//! let vertices: Vec<[f32; 3]> = accepted.snapshot.get(&region).await?.unwrap();
//! assert_eq!(vertices.len(), 2);
//! # Ok(())
//! # }
//! ```

mod batch;
mod cache;
mod codec;
mod history;
pub mod local;
mod maintenance;
mod path;
mod tree;

pub use batch::Submission;
pub use cache::CacheStats;
pub use codec::canonical_cbor;
pub use history::{PruneReport, RevisionEntry, RevisionPage};
pub use maintenance::{
    GcOptions, GcReport, Maintenance, MaintenanceStatus, MigrationReport, RecordEdit,
};
pub use path::{MAX_DEPTH, OctreePath};

use bytes::Bytes;
use object_store::{ObjectStore, PutMode, UpdateVersion, path::Path};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tree::{PresenceVersion, RegionVersion, Tree};

pub type Result<T> = std::result::Result<T, Error>;
const FORMAT: u32 = 3;
fn supported_format(format: u32) -> bool {
    format == FORMAT
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] object_store::Error),
    #[error("CBOR encoding/decoding: {0}")]
    Encoding(String),
    #[error("invalid octree path (octants must be 0..7, depth <= {MAX_DEPTH})")]
    InvalidPath,
    #[error("invalid SHA-256 content ID")]
    InvalidContentId,
    #[error("invalid branch or storage prefix")]
    InvalidName,
    #[error("branch already exists: {0}")]
    BranchExists(String),
    #[error("corrupt transaction object: {0}")]
    Corrupt(String),
    #[error("an ancestor of {0} is a geometry leaf; remove it before subdividing")]
    CoveredByLeaf(OctreePath),
    #[error("{0} is a branch; use scan to read its leaves")]
    NotLeaf(OctreePath),
    #[error("transaction dependencies changed at {paths:?}; re-execute against a fresh snapshot")]
    Conflict { paths: Vec<OctreePath> },
    #[error("root contention exhausted {attempts} publication attempts")]
    Contention { attempts: usize },
    #[error("operation limits must be positive and initial_backoff must not exceed max_backoff")]
    InvalidOptions,
    #[error("conditional updates require a backend-provided ETag or version")]
    MissingVersion,
    #[error("revision does not belong to this branch")]
    InvalidBase,
    #[error("revision is absent from retained accepted history")]
    RevisionUnavailable,
    #[error("commit batch members must share backend, namespace, branch, and coordination mode")]
    MixedBatch,
    #[error("group publication storage failure: {0}")]
    BatchStorageFailure(String),
    #[error("conditional failure did not establish a newer head fence")]
    UnfencedConflict,
    #[error("scan limit must be 1..=10000 and cursor must be inside the requested region")]
    InvalidCursor,
    #[error("page exceeds its {max_nodes}-node traversal budget; narrow the region or compact it")]
    TraversalLimit { max_nodes: usize },
    #[error("namespace garbage collection requires coordinated GC mode on all clients")]
    MaintenanceDisabled,
    #[error("namespace is fenced for maintenance")]
    MaintenanceBusy,
    #[error("namespace has {count} active durable reader/writer pins")]
    ActivePins { count: usize },
    #[error(
        "maintenance {token} has an ambiguous outstanding mutation; operator recovery is required"
    )]
    MaintenancePoisoned { token: String },
    #[error("pin {id} protects an unsettled publication and needs operator recovery")]
    UnsettledPin { id: String },
    #[error("object size {size} exceeds the remaining read budget {limit}")]
    ObjectTooLarge { size: u64, limit: usize },
    #[error(
        "compaction region exceeds its {max_nodes}-node working budget; choose a smaller region"
    )]
    CompactionLimit { max_nodes: usize },
    #[error(
        "publication outcome unknown for {candidate} on {branch}; inspect history before retrying: {source}"
    )]
    PublicationUnknown {
        branch: String,
        candidate: ContentId,
        #[source]
        source: Box<Error>,
    },
}

/// SHA-256 of the canonical, uncompressed CBOR bytes, encoded as lowercase hex.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentId(String);

impl ContentId {
    pub fn of(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ContentId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        if value.len() == 64
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            Ok(Self(value))
        } else {
            Err(Error::InvalidContentId)
        }
    }
}

impl FromStr for ContentId {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        value.to_owned().try_into()
    }
}

impl From<ContentId> for String {
    fn from(value: ContentId) -> Self {
        value.0
    }
}

/// An exact subtree substitution, not an application-level semantic command.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub path: OctreePath,
    /// Stable replacement fence, retained across rebases and logical deletions.
    pub generation: String,
    /// Geometry payload ID; null deletes the entire region.
    pub value: Option<ContentId>,
}

/// Immutable history record. Actor/message are caller-supplied metadata, not authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub format: u32,
    pub transaction_id: String,
    pub sequence: u64,
    pub parent: Option<ContentId>,
    /// Snapshot on which execution began; parent may be newer after rebase.
    pub base: Option<ContentId>,
    pub root: Option<ContentId>,
    pub edits: Vec<Edit>,
    pub actor: String,
    pub message: String,
    /// Optional immutable application audit/certificate payload.
    #[serde(default)]
    pub metadata: Option<ContentId>,
    #[serde(default)]
    pub blobs: Vec<ContentId>,
}

#[derive(Serialize, Deserialize)]
struct Head {
    format: u32,
    commit: ContentId,
    #[serde(default)]
    history: Option<history::HistoryIndex>,
}

#[derive(Clone, Debug)]
pub struct Repository {
    backend: Arc<dyn ObjectStore>,
    prefix: String,
    cache: Arc<Mutex<cache::Cache>>,
    downloads:
        Arc<Mutex<std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>>,
    managed: bool,
}

impl Repository {
    /// Dedicated object-store namespace, without credentials or bucket location.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    /// Use a dedicated namespace. Heads must never be overwritten/deleted outside this API.
    /// LocalFileSystem in object_store 0.12 does not support conditional updates;
    /// InMemory is suitable for tests, and S3 uses conditional ETag PUTs.
    pub fn new(backend: Arc<dyn ObjectStore>, prefix: &str) -> Result<Self> {
        let prefix = prefix.trim_matches('/');
        if !prefix.is_empty() {
            Path::parse(prefix).map_err(|_| Error::InvalidName)?;
        }
        Ok(Self {
            backend,
            prefix: prefix.to_owned(),
            cache: Arc::new(Mutex::new(cache::Cache::default())),
            managed: false,
            downloads: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// A byte-budgeted verified-object LRU, shared by clones; mutable heads are never cached.
    pub fn with_cache_capacity(mut self, bytes: usize) -> Self {
        self.cache = Arc::new(Mutex::new(cache::Cache::new(bytes)));
        self
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.lock().unwrap().stats
    }

    /// Isolated heads/objects under a child namespace, sharing the byte budget.
    pub fn child(&self, suffix: &str) -> Result<Self> {
        Path::parse(suffix).map_err(|_| Error::InvalidName)?;
        if suffix.is_empty() {
            return Err(Error::InvalidName);
        }
        let mut child = self.clone();
        child.prefix = self.path(suffix).to_string();
        Ok(child)
    }

    fn path(&self, suffix: &str) -> Path {
        Path::from(if self.prefix.is_empty() {
            suffix.to_owned()
        } else {
            format!("{}/{suffix}", self.prefix)
        })
    }

    fn head_path(&self, branch: &str) -> Result<Path> {
        if branch.is_empty()
            || branch.len() > 128
            || !branch
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(Error::InvalidName);
        }
        Ok(self.path(&format!("refs/{branch}.cbor")))
    }

    async fn object(&self, id: &ContentId) -> Result<Bytes> {
        self.object_limited(id, usize::MAX).await
    }

    async fn object_limited(&self, id: &ContentId, limit: usize) -> Result<Bytes> {
        let location = self.path(&format!("objects/{id}.cbor"));
        if let Some(bytes) = self.cache.lock().unwrap().get(location.as_ref()) {
            if bytes.len() > limit {
                return Err(Error::ObjectTooLarge {
                    size: bytes.len() as u64,
                    limit,
                });
            }
            return Ok(bytes);
        }
        let gate = {
            let mut downloads = self.downloads.lock().unwrap();
            if downloads.len() >= 1024 {
                downloads.retain(|_, gate| gate.strong_count() > 0);
            }
            if let Some(gate) = downloads
                .get(location.as_ref())
                .and_then(std::sync::Weak::upgrade)
            {
                gate
            } else {
                let gate = Arc::new(tokio::sync::Mutex::new(()));
                downloads.insert(location.to_string(), Arc::downgrade(&gate));
                gate
            }
        };
        let _guard = gate.lock().await;
        // Parallel tree traversals share verified immutable downloads. Heads
        // and explicit uncached verification never pass through this gate.
        if let Some(bytes) = self.cache.lock().unwrap().get(location.as_ref()) {
            if bytes.len() > limit {
                return Err(Error::ObjectTooLarge {
                    size: bytes.len() as u64,
                    limit,
                });
            }
            return Ok(bytes);
        }
        self.download(id, limit).await
    }

    async fn uncached_object(&self, id: &ContentId) -> Result<Bytes> {
        self.download(id, usize::MAX).await
    }

    async fn download(&self, id: &ContentId, limit: usize) -> Result<Bytes> {
        let location = self.path(&format!("objects/{id}.cbor"));
        let result = self.backend.get(&location).await?;
        if result.meta.size > limit as u64 {
            return Err(Error::ObjectTooLarge {
                size: result.meta.size,
                limit,
            });
        }
        let bytes = result.bytes().await?;
        if ContentId::of(&bytes) != *id {
            return Err(Error::Corrupt(format!("hash mismatch for {id}")));
        }
        self.cache
            .lock()
            .unwrap()
            .insert(location.to_string(), bytes.clone());
        Ok(bytes)
    }

    async fn put_object(&self, id: &ContentId, bytes: Bytes) -> Result<()> {
        let location = self.path(&format!("objects/{id}.cbor"));
        match self
            .backend
            .put_opts(&location, bytes.clone().into(), PutMode::Create.into())
            .await
        {
            Ok(_) => {
                // Only an acknowledged immutable upload may populate the LRU.
                // Never elide the upload based on a cache hit: GC may have
                // collected an unreferenced object since it was downloaded.
                self.cache
                    .lock()
                    .unwrap()
                    .insert(location.to_string(), bytes);
                Ok(())
            }
            // A 409 may represent a concurrent conditional request, not an existing
            // object. Verify the object before treating either status as deduplication.
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. },
            ) => {
                self.uncached_object(id).await?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn store_commit(&self, commit: &Commit) -> Result<ContentId> {
        let bytes = canonical_cbor(commit)?;
        let id = ContentId::of(&bytes);
        self.put_object(&id, bytes.into()).await?;
        Ok(id)
    }

    /// Upload shared geometry/mesh data before referencing its ID from a block.
    /// Such references are application-defined; retention must trace them too.
    pub async fn put_blob<T: Serialize>(&self, value: &T) -> Result<ContentId> {
        let _pin = self.enter().await?;
        let bytes = canonical_cbor(value)?;
        let id = ContentId::of(&bytes);
        self.put_object(&id, bytes.into()).await?;
        Ok(id)
    }

    /// Read an immutable object by ID with hash verification. Discover mutable
    /// dependencies through Transaction::get/scan/watch, not a separate checkout.
    pub async fn blob<T: DeserializeOwned>(&self, id: &ContentId) -> Result<T> {
        let _pin = self.enter().await?;
        codec::decode(&self.object(id).await?)
    }

    fn unknown(branch: &str, candidate: &ContentId, source: Error) -> Error {
        Error::PublicationUnknown {
            branch: branch.into(),
            candidate: candidate.clone(),
            source: Box::new(source),
        }
    }

    async fn contains(
        &self,
        current: Snapshot,
        candidate: &ContentId,
        _sequence: u64,
    ) -> Result<bool> {
        let index = current
            .history
            .as_ref()
            .ok_or_else(|| Error::Corrupt("Current heads require an acceptance index".into()))?;
        Ok(history::entry(self, index, &history::id_path(candidate))
            .await?
            .is_some_and(|e| e.revision == *candidate && e.sequence <= current.commit.sequence))
    }

    /// Create an empty independent serialization domain (branch or shard).
    pub async fn create(&self, branch: &str) -> Result<Snapshot> {
        let pin = self.enter().await?;
        let path = self.head_path(branch)?;
        let commit = Commit {
            format: FORMAT,
            transaction_id: uuid::Uuid::new_v4().to_string(),
            sequence: 0,
            parent: None,
            base: None,
            root: None,
            edits: Vec::new(),
            actor: String::new(),
            message: "Initialize branch".into(),
            metadata: None,
            blobs: Vec::new(),
        };
        let id = self.store_commit(&commit).await?;
        let history =
            history::append(self, Some(&history::HistoryIndex::default()), &id, 0).await?;
        let head = canonical_cbor(&Head {
            format: FORMAT,
            commit: id.clone(),
            history: history.clone(),
        })?;
        if let Some(pin) = &pin {
            pin.hold();
        }
        match self
            .backend
            .put_opts(&path, head.into(), PutMode::Create.into())
            .await
        {
            Ok(_) => {
                if let Some(pin) = &pin {
                    pin.settle();
                }
                Ok(Snapshot {
                    repo: self.clone(),
                    id,
                    commit,
                    pin,
                    history,
                })
            }
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. },
            ) => {
                // A retried request may return 412 after its first attempt succeeded.
                let current = self
                    .head(branch)
                    .await
                    .map_err(|e| Self::unknown(branch, &id, e))?
                    .0;
                if self
                    .contains(current, &id, 0)
                    .await
                    .map_err(|e| Self::unknown(branch, &id, e))?
                {
                    if let Some(pin) = &pin {
                        pin.settle();
                    }
                    Ok(Snapshot {
                        repo: self.clone(),
                        id,
                        commit,
                        pin,
                        history,
                    })
                } else {
                    if let Some(pin) = &pin {
                        pin.settle();
                    }
                    Err(Error::BranchExists(branch.into()))
                }
            }
            Err(source) => Err(Self::unknown(branch, &id, source.into())),
        }
    }

    async fn head(&self, branch: &str) -> Result<(Snapshot, UpdateVersion)> {
        let result = self.backend.get(&self.head_path(branch)?).await?;
        let version = UpdateVersion {
            e_tag: result.meta.e_tag.clone(),
            version: result.meta.version.clone(),
        };
        let head: Head = codec::decode(&result.bytes().await?)?;
        if head.history.is_none() {
            return Err(Error::Corrupt(
                "Current heads require an acceptance index".into(),
            ));
        }
        if !supported_format(head.format) {
            return Err(Error::Corrupt("unsupported head format".into()));
        }
        let mut snapshot = self.load_snapshot(&head.commit).await?;
        snapshot.history = head.history;
        Ok((snapshot, version))
    }

    pub async fn checkout(&self, branch: &str) -> Result<Snapshot> {
        let pin = self.enter().await?;
        let mut snapshot = self.head(branch).await?.0;
        snapshot.pin = pin;
        Ok(snapshot)
    }

    /// Open a historical snapshot without replay. Caller is responsible for retaining its objects.
    pub async fn snapshot(&self, id: &ContentId) -> Result<Snapshot> {
        let pin = self.enter().await?;
        let mut snapshot = self.load_snapshot(id).await?;
        snapshot.pin = pin;
        Ok(snapshot)
    }

    async fn load_snapshot(&self, id: &ContentId) -> Result<Snapshot> {
        let commit: Commit = codec::decode(&self.object(id).await?)?;
        if !supported_format(commit.format) {
            return Err(Error::Corrupt("unsupported commit format".into()));
        }
        Ok(Snapshot {
            repo: self.clone(),
            id: id.clone(),
            commit,
            pin: None,
            history: None,
        })
    }

    pub async fn begin(&self, branch: &str) -> Result<Transaction> {
        let pin = self.enter().await?;
        let (mut base, version) = self.head(branch).await?;
        base.pin = pin;
        Ok(self.transaction(branch, base.clone(), base, version))
    }

    /// Execute against an accepted older revision, preserving its preconditions.
    pub async fn begin_at(&self, branch: &str, base: &ContentId) -> Result<Transaction> {
        let pin = self.enter().await?;
        let (mut tip, version) = self.head(branch).await?;
        if !self.contains(tip.clone(), base, 0).await? {
            return Err(Error::RevisionUnavailable);
        }
        let mut base = self.load_snapshot(base).await?;
        tip.pin = pin.clone();
        base.pin = pin;
        if !self
            .contains(tip.clone(), base.id(), base.commit.sequence)
            .await?
        {
            return Err(Error::InvalidBase);
        }
        Ok(self.transaction(branch, base, tip, version))
    }

    fn transaction(
        &self,
        branch: &str,
        base: Snapshot,
        tip: Snapshot,
        version: UpdateVersion,
    ) -> Transaction {
        Transaction {
            tree: Tree::new(self.clone()),
            branch: branch.into(),
            working_root: base.commit.root.clone(),
            read_snapshot: base.clone(),
            base,
            tip,
            version,
            observed: BTreeMap::new(),
            directories: BTreeMap::new(),
            edits: Vec::new(),
            transaction_id: uuid::Uuid::new_v4(),
            metadata: None,
            refreshed: false,
            attachments: BTreeSet::new(),
        }
    }

    /// Resolve a possibly lost publication acknowledgement, including when later
    /// commits have already advanced the head. Never use object existence as proof.
    /// A false result only describes the observed history: an in-flight timed-out
    /// request could still publish later. It is not permission to re-execute blindly.
    pub async fn was_committed(&self, branch: &str, candidate: &ContentId) -> Result<bool> {
        let _pin = self.enter().await?;
        self.contains(self.head(branch).await?.0, candidate, 0)
            .await
    }
}

/// A stable, materialized checkpoint. Cloning it performs no storage I/O.
#[derive(Clone, Debug)]
pub struct Snapshot {
    repo: Repository,
    id: ContentId,
    commit: Commit,
    pin: Option<Arc<maintenance::Pin>>,
    history: Option<history::HistoryIndex>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanPage {
    pub leaves: Vec<(OctreePath, ContentId)>,
    /// Resume strictly after this path, on the same immutable snapshot and region.
    pub next: Option<OctreePath>,
}

impl Snapshot {
    pub async fn scan_level(
        &self,
        region: &OctreePath,
        depth: usize,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<ScanPage> {
        Tree::new(self.repo.clone())
            .scan_level(self.commit.root.as_ref(), region, depth, after, limit)
            .await
    }
    pub fn id(&self) -> &ContentId {
        &self.id
    }
    pub fn commit(&self) -> &Commit {
        &self.commit
    }
    pub fn retained_from(&self) -> u64 {
        self.history.as_ref().map(|h| h.retained_from).unwrap_or(0)
    }

    pub async fn value_id(&self, path: &OctreePath) -> Result<Option<ContentId>> {
        Tree::new(self.repo.clone())
            .get(self.commit.root.as_ref(), path)
            .await
    }

    /// Read a payload discovered through this pinned snapshot without another admission CAS.
    pub async fn blob<T: DeserializeOwned>(&self, id: &ContentId) -> Result<T> {
        codec::decode(&self.repo.object(id).await?)
    }

    pub async fn blob_limited<T: DeserializeOwned>(
        &self,
        id: &ContentId,
        limit: usize,
    ) -> Result<T> {
        codec::decode(&self.repo.object_limited(id, limit).await?)
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &OctreePath) -> Result<Option<T>> {
        self.get_limited(path, usize::MAX).await
    }

    pub async fn get_limited<T: DeserializeOwned>(
        &self,
        path: &OctreePath,
        limit: usize,
    ) -> Result<Option<T>> {
        let mut tree = Tree::new(self.repo.clone());
        let Some(id) = tree.get(self.commit.root.as_ref(), path).await? else {
            return Ok(None);
        };
        Ok(Some(codec::decode(
            &tree.payload_limited(&id, limit).await?,
        )?))
    }

    /// Enumerate geometry payload IDs, in octant order, within a region.
    pub async fn scan(&self, path: &OctreePath) -> Result<Vec<(OctreePath, ContentId)>> {
        Tree::new(self.repo.clone())
            .scan(self.commit.root.as_ref(), path)
            .await
    }

    pub async fn scan_page(
        &self,
        path: &OctreePath,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<ScanPage> {
        Tree::new(self.repo.clone())
            .scan_page(self.commit.root.as_ref(), path, after, limit)
            .await
    }
    /// Ordered pinned-snapshot paging with bounded immutable-node lookahead.
    /// Values are not prefetched; mutable heads and transaction predicates are unchanged.
    pub async fn scan_page_prefetched(
        &self,
        path: &OctreePath,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<ScanPage> {
        Tree::new(self.repo.clone())
            .with_read_prefetch()
            .scan_page(self.commit.root.as_ref(), path, after, limit)
            .await
    }
}

pub struct Transaction {
    tree: Tree,
    branch: String,
    base: Snapshot,
    tip: Snapshot,
    read_snapshot: Snapshot,
    version: UpdateVersion,
    working_root: Option<ContentId>,
    observed: BTreeMap<OctreePath, RegionVersion>,
    directories: BTreeMap<OctreePath, PresenceVersion>,
    edits: Vec<Edit>,
    transaction_id: uuid::Uuid,
    metadata: Option<ContentId>,
    refreshed: bool,
    attachments: BTreeSet<ContentId>,
}

impl Transaction {
    /// Track structural directory presence, not every descendant version. A true
    /// directory can contain tombstones; inspect its actual rows separately.
    pub async fn directory_exists(&mut self, path: &OctreePath) -> Result<bool> {
        if !self.directories.contains_key(path) {
            let v = self
                .tree
                .presence(self.read_snapshot.commit.root.as_ref(), path)
                .await?;
            self.directories.insert(path.clone(), v);
        }
        Ok(self
            .tree
            .presence(self.working_root.as_ref(), path)
            .await?
            .present)
    }
    pub async fn scan_level(
        &mut self,
        region: &OctreePath,
        depth: usize,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<ScanPage> {
        self.observe(region).await?;
        self.tree
            .scan_level(self.working_root.as_ref(), region, depth, after, limit)
            .await
    }
    pub async fn value_id(&mut self, path: &OctreePath) -> Result<Option<ContentId>> {
        self.observe(path).await?;
        self.tree.get(self.working_root.as_ref(), path).await
    }
    async fn observe(&mut self, path: &OctreePath) -> Result<()> {
        if !self.observed.contains_key(path) {
            let version = self
                .tree
                .region(self.read_snapshot.commit.root.as_ref(), path)
                .await?;
            self.observed.insert(path.clone(), version);
        }
        Ok(())
    }

    pub fn base_id(&self) -> &ContentId {
        &self.base.id
    }

    pub fn pin_id(&self) -> Option<&str> {
        self.base.pin_id()
    }

    /// Stage an immutable mesh referenced inside an application payload. The
    /// commit records this attachment explicitly for retention.
    pub fn stage_blob<T: Serialize>(&mut self, value: &T) -> Result<ContentId> {
        let id = self.tree.stage(value)?;
        self.attachments.insert(id.clone());
        Ok(id)
    }

    /// Reusing an older standalone blob must verify remote existence while the
    /// writer pin is held; a stale cache entry cannot resurrect swept garbage.
    pub async fn retain_blob(&mut self, id: &ContentId) -> Result<()> {
        self.tree.repo.uncached_object(id).await?;
        self.attachments.insert(id.clone());
        Ok(())
    }

    pub fn set_metadata<T: Serialize>(&mut self, value: &T) -> Result<ContentId> {
        let id = self.tree.stage(value)?;
        self.metadata = Some(id.clone());
        Ok(id)
    }

    async fn validate_and_replay(&mut self, tip: Snapshot, version: UpdateVersion) -> Result<()> {
        let mut conflicts = Vec::new();
        for (path, expected) in &self.observed {
            if self.tree.region(tip.commit.root.as_ref(), path).await? != *expected {
                conflicts.push(path.clone());
            }
        }
        for (path, expected) in &self.directories {
            if self.tree.presence(tip.commit.root.as_ref(), path).await? != *expected {
                conflicts.push(path.clone());
            }
        }
        if !conflicts.is_empty() {
            return Err(Error::Conflict { paths: conflicts });
        }
        self.working_root = tip.commit.root.clone();
        for edit in &self.edits {
            self.working_root = self
                .tree
                .replace(
                    self.working_root.as_ref(),
                    &edit.path,
                    edit.value.as_ref(),
                    &edit.generation,
                )
                .await?;
        }
        self.refreshed |= self.read_snapshot.id != tip.id;
        self.read_snapshot = tip.clone();
        self.tip = tip;
        self.version = version;
        Ok(())
    }

    /// Explicitly rebase before application validation. Later reads depend on the
    /// refreshed immutable tip; prior read/write preconditions must still hold.
    /// The returned snapshot is the tip before this transaction's pending edits.
    pub async fn refresh(&mut self) -> Result<Snapshot> {
        let (mut tip, version) = self.tree.repo.head(&self.branch).await?;
        tip.pin = self.base.pin.clone();
        self.validate_and_replay(tip.clone(), version).await?;
        Ok(tip)
    }

    pub async fn scan_page(
        &mut self,
        path: &OctreePath,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<ScanPage> {
        self.observe(path).await?;
        self.tree
            .scan_page(self.working_root.as_ref(), path, after, limit)
            .await
    }

    /// Protect an entire subtree, including its absence, without downloading payloads.
    /// Use this for swept collision regions, support neighborhoods, and predicates.
    pub async fn watch(&mut self, path: &OctreePath) -> Result<()> {
        self.observe(path).await
    }

    pub async fn get<T: DeserializeOwned>(&mut self, path: &OctreePath) -> Result<Option<T>> {
        self.get_limited(path, usize::MAX).await
    }

    pub async fn blob_limited<T: DeserializeOwned>(
        &self,
        id: &ContentId,
        limit: usize,
    ) -> Result<T> {
        codec::decode(&self.tree.payload_limited(id, limit).await?)
    }

    pub async fn get_limited<T: DeserializeOwned>(
        &mut self,
        path: &OctreePath,
        limit: usize,
    ) -> Result<Option<T>> {
        self.observe(path).await?;
        let Some(id) = self.tree.get(self.working_root.as_ref(), path).await? else {
            return Ok(None);
        };
        Ok(Some(codec::decode(
            &self.tree.payload_limited(&id, limit).await?,
        )?))
    }

    /// A predicate read: changes anywhere under this region invalidate the transaction.
    pub async fn scan(&mut self, path: &OctreePath) -> Result<Vec<(OctreePath, ContentId)>> {
        self.observe(path).await?;
        self.tree.scan(self.working_root.as_ref(), path).await
    }

    /// Replace this whole subtree with one immutable geometry block. This is not
    /// a fieldwise merge. All prior descendants are removed from the new snapshot.
    pub async fn put<T: Serialize>(&mut self, path: &OctreePath, value: &T) -> Result<ContentId> {
        self.observe(path).await?;
        let id = self.tree.stage(value)?;
        let generation = format!("{}/{}", self.transaction_id, self.edits.len());
        self.working_root = self
            .tree
            .replace(self.working_root.as_ref(), path, Some(&id), &generation)
            .await?;
        self.edits.push(Edit {
            path: path.clone(),
            generation,
            value: Some(id.clone()),
        });
        Ok(id)
    }

    /// Logical deletion only. Older snapshots and physical objects remain intact.
    pub async fn remove(&mut self, path: &OctreePath) -> Result<()> {
        self.observe(path).await?;
        let generation = format!("{}/{}", self.transaction_id, self.edits.len());
        self.working_root = self
            .tree
            .replace(self.working_root.as_ref(), path, None, &generation)
            .await?;
        self.edits.push(Edit {
            path: path.clone(),
            generation,
            value: None,
        });
        Ok(())
    }

    /// Atomically publish or report changed dependencies. Independent edits are
    /// replayed on a newer root without re-running caller code or side effects.
    pub async fn commit(
        mut self,
        actor: &str,
        message: &str,
        options: CommitOptions,
    ) -> Result<CommitOutcome> {
        options.validate()?;
        if self.edits.is_empty() && self.metadata.is_none() && self.attachments.is_empty() {
            return Ok(CommitOutcome {
                snapshot: self.read_snapshot,
                attempts: 0,
                rebases: 0,
            });
        }
        // begin_at may have staged edits against an older base. Validate these
        // before ever attempting a CAS against the newer tip's version.
        if self.read_snapshot.id != self.tip.id {
            self.validate_and_replay(self.tip.clone(), self.version.clone())
                .await?;
        }
        let mut parent = self.tip.clone();
        let mut version = self.version.clone();
        for attempt in 0..options.max_attempts {
            if version.e_tag.is_none() && version.version.is_none() {
                return Err(Error::MissingVersion);
            }
            let history_payloads: Vec<_> = self
                .edits
                .iter()
                .filter_map(|e| e.value.clone())
                .chain(self.metadata.clone())
                .chain(self.attachments.iter().cloned())
                .collect();
            self.tree
                .flush(
                    self.working_root.as_ref(),
                    &history_payloads,
                    options.upload_concurrency,
                )
                .await?;
            let commit = Commit {
                format: FORMAT,
                transaction_id: self.transaction_id.to_string(),
                sequence: parent
                    .commit
                    .sequence
                    .checked_add(1)
                    .ok_or_else(|| Error::Corrupt("sequence overflow".into()))?,
                parent: Some(parent.id.clone()),
                base: Some(self.base.id.clone()),
                root: self.working_root.clone(),
                edits: self.edits.clone(),
                actor: actor.into(),
                message: message.into(),
                metadata: self.metadata.clone(),
                blobs: self.attachments.iter().cloned().collect(),
            };
            let id = self.tree.repo.store_commit(&commit).await?;
            let history = history::append(
                &self.tree.repo,
                parent.history.as_ref(),
                &id,
                commit.sequence,
            )
            .await?;
            let bytes = canonical_cbor(&Head {
                format: FORMAT,
                commit: id.clone(),
                history: history.clone(),
            })?;
            let path = self.tree.repo.head_path(&self.branch)?;
            let expected_version = version.clone();
            if let Some(pin) = &self.base.pin {
                pin.hold();
            }
            match self
                .tree
                .repo
                .backend
                .put_opts(&path, bytes.into(), PutMode::Update(version).into())
                .await
            {
                Ok(_) => {
                    if let Some(pin) = &self.base.pin {
                        pin.settle();
                    }
                    return Ok(CommitOutcome {
                        snapshot: Snapshot {
                            repo: self.tree.repo.clone(),
                            id,
                            commit,
                            pin: self.base.pin.clone(),
                            history,
                        },
                        attempts: attempt + 1,
                        rebases: attempt + usize::from(self.refreshed),
                    });
                }
                Err(
                    object_store::Error::Precondition { .. }
                    | object_store::Error::AlreadyExists { .. },
                ) => {}
                Err(source) => return Err(Repository::unknown(&self.branch, &id, source.into())),
            }
            if attempt + 1 < options.max_attempts {
                options.backoff(self.transaction_id, attempt).await;
            }
            // Resolve hidden success before rebasing: an SDK can retry a request
            // after losing its success response and surface a 412 instead.
            (parent, version) = self
                .tree
                .repo
                .head(&self.branch)
                .await
                .map_err(|e| Repository::unknown(&self.branch, &id, e))?;
            if self
                .tree
                .repo
                .contains(parent.clone(), &id, commit.sequence)
                .await
                .map_err(|e| Repository::unknown(&self.branch, &id, e))?
            {
                if let Some(pin) = &self.base.pin {
                    pin.settle();
                }
                return Ok(CommitOutcome {
                    snapshot: Snapshot {
                        repo: self.tree.repo.clone(),
                        id,
                        commit,
                        pin: self.base.pin.clone(),
                        history,
                    },
                    attempts: attempt + 1,
                    rebases: attempt + usize::from(self.refreshed),
                });
            }
            // A newer head fences every request carrying the old version.
            if version != expected_version
                && let Some(pin) = &self.base.pin
            {
                pin.settle();
            }
            if version == expected_version {
                return Err(Repository::unknown(
                    &self.branch,
                    &id,
                    Error::UnfencedConflict,
                ));
            }
            let mut conflicts = Vec::new();
            for (path, expected) in &self.observed {
                if self.tree.region(parent.commit.root.as_ref(), path).await? != *expected {
                    conflicts.push(path.clone());
                }
            }
            for (path, expected) in &self.directories {
                if self
                    .tree
                    .presence(parent.commit.root.as_ref(), path)
                    .await?
                    != *expected
                {
                    conflicts.push(path.clone());
                }
            }
            if !conflicts.is_empty() {
                return Err(Error::Conflict { paths: conflicts });
            }
            if attempt + 1 == options.max_attempts {
                break;
            }
            self.working_root = parent.commit.root.clone();
            for edit in &self.edits {
                self.working_root = self
                    .tree
                    .replace(
                        self.working_root.as_ref(),
                        &edit.path,
                        edit.value.as_ref(),
                        &edit.generation,
                    )
                    .await?;
            }
        }
        Err(Error::Contention {
            attempts: options.max_attempts,
        })
    }
}

#[derive(Clone, Debug)]
pub struct CommitOptions {
    pub max_attempts: usize,
    pub upload_concurrency: usize,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for CommitOptions {
    fn default() -> Self {
        Self {
            max_attempts: 32,
            upload_concurrency: 32,
            initial_backoff: Duration::from_millis(2),
            max_backoff: Duration::from_millis(100),
        }
    }
}

impl CommitOptions {
    fn validate(&self) -> Result<()> {
        if self.max_attempts == 0
            || self.upload_concurrency == 0
            || self.initial_backoff > self.max_backoff
        {
            return Err(Error::InvalidOptions);
        }
        Ok(())
    }

    async fn backoff(&self, transaction: uuid::Uuid, attempt: usize) {
        let ceiling = self
            .initial_backoff
            .saturating_mul(1u32 << attempt.min(16))
            .min(self.max_backoff);
        // Per-transaction/attempt jitter avoids synchronizing losing publishers.
        let seed = transaction
            .as_u128()
            .wrapping_mul(0x9e3779b97f4a7c15)
            .rotate_left((attempt % 128) as u32);
        let delay = ceiling.mul_f64(0.5 + (seed as u64 % 1001) as f64 / 2000.0);
        tokio::time::sleep(delay).await;
    }
}

#[derive(Debug)]
pub struct CommitOutcome {
    pub snapshot: Snapshot,
    pub attempts: usize,
    pub rebases: usize,
}
