//! Canonical commit envelopes with file-backed, fallible record lists. Accepted
//! bytes/IDs do not change; no history-sized decoded arrays are kept in snapshots.
use crate::{ContentId, Edit, Error, Result, canonical_cbor, codec::header_bytes};
use serde::{Serialize, de::DeserializeOwned, ser::SerializeSeq};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    marker::PhantomData,
    path::PathBuf,
    sync::Arc,
};

const BUFFER: usize = 64 * 1024;
const MAX_DEPTH: usize = 64;
const CACHE_HEADERS: usize = 32;
const CACHE_ENTRIES: usize = 1024;
const CACHE_HEADER_BYTES: usize = 1024 * 1024;
const CACHE_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug)]
struct CachedCommit {
    weak: std::sync::Weak<Commit>,
    retained: Option<Arc<Commit>>,
    disk: u64,
    ram: usize,
    used: u64,
}
/// This is a file cache, not an encoded-byte or decoded-history cache. Expired
/// weak entries aid concurrent readers when ordinary caching is disabled.
#[derive(Debug, Default)]
pub(crate) struct CommitCache {
    entries: BTreeMap<String, CachedCommit>,
    disk: u64,
    ram: usize,
    retained: usize,
    clock: u64,
    capacity: u64,
}
impl CommitCache {
    pub(crate) fn new(bytes: usize) -> Self {
        Self {
            capacity: (bytes as u64).min(CACHE_FILE_BYTES),
            ..Default::default()
        }
    }
    pub(crate) fn get(&mut self, key: &str) -> Option<Arc<Commit>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(key)?;
        entry.used = self.clock;
        entry.weak.upgrade()
    }
    pub(crate) fn insert(&mut self, key: String, commit: &Arc<Commit>) {
        if key.len() > 1024 {
            return;
        }
        if let Some(old) = self.entries.remove(&key)
            && old.retained.is_some()
        {
            self.disk -= old.disk;
            self.ram -= old.ram;
            self.retained -= 1;
        }
        if self.entries.len() >= CACHE_ENTRIES {
            self.entries
                .retain(|_, entry| entry.weak.strong_count() > 0);
        }
        if self.entries.len() >= CACHE_ENTRIES {
            return;
        }
        let disk = commit.encoded_len();
        let ram = 1024
            + key.len()
            + commit.transaction_id.capacity()
            + commit.actor.capacity()
            + commit.message.capacity()
            + 4 * 64;
        let keep = disk <= self.capacity && ram <= CACHE_HEADER_BYTES;
        if keep {
            while self.retained >= CACHE_HEADERS
                || self.disk + disk > self.capacity
                || self.ram + ram > CACHE_HEADER_BYTES
            {
                let oldest = self
                    .entries
                    .iter()
                    .filter(|(_, entry)| entry.retained.is_some())
                    .min_by_key(|(_, entry)| entry.used)
                    .map(|(key, _)| key.clone())
                    .expect("cache accounting has a retained entry");
                let entry = self.entries.get_mut(&oldest).unwrap();
                entry.retained = None;
                self.disk -= entry.disk;
                self.ram -= entry.ram;
                self.retained -= 1;
            }
        }
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            key,
            CachedCommit {
                weak: Arc::downgrade(commit),
                retained: keep.then(|| commit.clone()),
                disk,
                ram,
                used: self.clock,
            },
        );
        if keep {
            self.disk += disk;
            self.ram += ram;
            self.retained += 1;
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct CommitLimits {
    pub bytes: usize,
    pub item_bytes: usize,
}
impl Default for CommitLimits {
    fn default() -> Self {
        Self {
            bytes: 128 * 1024 * 1024,
            item_bytes: 1024 * 1024,
        }
    }
}
impl CommitLimits {
    pub(crate) fn validate(self) -> Result<()> {
        if self.bytes == 0
            || self.bytes > 512 * 1024 * 1024
            || self.item_bytes == 0
            || self.item_bytes > self.bytes
        {
            return Err(Error::InvalidOptions);
        }
        Ok(())
    }
}
fn scratch(error: impl std::fmt::Display) -> Error {
    Error::Scratch(error.to_string())
}
fn corrupt(error: impl std::fmt::Display) -> Error {
    Error::Corrupt(format!("commit: {error}"))
}

#[derive(Clone, Debug)]
pub struct CommitHeader {
    pub format: u32,
    pub transaction_id: String,
    pub sequence: u64,
    pub parent: Option<ContentId>,
    pub base: Option<ContentId>,
    pub root: Option<ContentId>,
    pub actor: String,
    pub message: String,
    pub metadata: Option<ContentId>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Commit {
    pub format: u32,
    pub transaction_id: String,
    pub sequence: u64,
    pub parent: Option<ContentId>,
    pub base: Option<ContentId>,
    pub root: Option<ContentId>,
    pub edits: CommitList<Edit>,
    pub actor: String,
    pub message: String,
    pub metadata: Option<ContentId>,
    pub blobs: CommitList<ContentId>,
    #[serde(skip)]
    file: Arc<CommitFile>,
}

#[derive(Debug)]
pub(crate) struct CommitFile {
    _directory: tempfile::TempDir,
    path: PathBuf,
    pub(crate) len: u64,
    pub(crate) id: ContentId,
    limits: CommitLimits,
}
impl CommitFile {
    pub(crate) fn create(limits: CommitLimits) -> Result<(Self, File)> {
        limits.validate()?;
        let directory = tempfile::Builder::new()
            .prefix("ergo-commit-")
            .tempdir()
            .map_err(scratch)?;
        let path = directory.path().join("commit.cbor");
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(scratch)?;
        Ok((
            Self {
                _directory: directory,
                path,
                len: 0,
                id: ContentId::of(&[]),
                limits,
            },
            file,
        ))
    }
    pub(crate) fn open(&self) -> Result<BufReader<File>> {
        Ok(BufReader::with_capacity(
            BUFFER,
            File::open(&self.path).map_err(scratch)?,
        ))
    }
    fn verified_reader(&self) -> Result<BufReader<File>> {
        let mut reader = self.open()?;
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0; BUFFER];
        loop {
            let n = reader.read(&mut buffer).map_err(scratch)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            if count > self.len {
                return Err(scratch("sealed commit file grew"));
            }
            digest.update(&buffer[..n]);
        }
        if count != self.len || ContentId::of_digest(digest.finalize().into()) != self.id {
            return Err(scratch("sealed commit file checksum changed"));
        }
        reader.seek(SeekFrom::Start(0)).map_err(scratch)?;
        Ok(reader)
    }
    pub(crate) fn upload_bytes(&self) -> Result<bytes::Bytes> {
        let mut bytes = Vec::with_capacity(usize::try_from(self.len).map_err(scratch)?);
        self.open()?
            .take(self.limits.bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(scratch)?;
        if bytes.len() as u64 != self.len || ContentId::of(&bytes) != self.id {
            return Err(scratch("sealed commit file changed"));
        }
        Ok(bytes.into())
    }
    pub(crate) fn seal(&mut self, len: u64, id: ContentId) -> Result<()> {
        if len > self.limits.bytes as u64 {
            return Err(scratch("commit exceeds its encoded-byte budget"));
        }
        self.len = len;
        self.id = id;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Range {
    offset: u64,
    bytes: u64,
    count: usize,
    digest: [u8; 32],
}
impl Default for Range {
    fn default() -> Self {
        Self {
            offset: 0,
            bytes: 0,
            count: 0,
            digest: Sha256::digest([]).into(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct CommitList<T> {
    file: Arc<CommitFile>,
    range: Range,
    kind: PhantomData<fn() -> T>,
}
impl<T: DeserializeOwned> CommitList<T> {
    pub fn len(&self) -> usize {
        self.range.count
    }
    pub fn is_empty(&self) -> bool {
        self.range.count == 0
    }
    pub fn iter(&self) -> Result<CommitIter<T>> {
        let mut reader = self.file.open()?;
        reader
            .seek(SeekFrom::Start(self.range.offset))
            .map_err(scratch)?;
        Ok(CommitIter {
            file: self.file.clone(),
            reader,
            remaining: self.range.count,
            end: self.range.offset + self.range.bytes,
            digest: Sha256::new(),
            expected: self.range.digest,
            done: false,
            kind: PhantomData,
        })
    }
}
impl<T: Serialize + DeserializeOwned> Serialize for CommitList<T> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let mut sequence = serializer.serialize_seq(Some(self.len()))?;
        for item in self.iter().map_err(S::Error::custom)? {
            sequence.serialize_element(&item.map_err(S::Error::custom)?)?;
        }
        sequence.end()
    }
}
pub struct CommitIter<T> {
    file: Arc<CommitFile>,
    reader: BufReader<File>,
    remaining: usize,
    end: u64,
    digest: Sha256,
    expected: [u8; 32],
    done: bool,
    kind: PhantomData<fn() -> T>,
}
impl<T: DeserializeOwned> Iterator for CommitIter<T> {
    type Item = Result<T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if self.remaining == 0 {
            self.done = true;
            return match self.reader.stream_position() {
                Ok(end)
                    if end == self.end
                        && <[u8; 32]>::from(self.digest.clone().finalize()) == self.expected =>
                {
                    None
                }
                _ => Some(Err(scratch("commit list length/checksum changed"))),
            };
        }
        let result = read_value(
            &mut self.reader,
            self.end,
            self.file.limits.item_bytes,
            &mut self.digest,
        );
        if result.is_err() {
            self.done = true;
        } else {
            self.remaining -= 1;
        }
        Some(result)
    }
}

struct Output {
    writer: BufWriter<File>,
    digest: Sha256,
    len: u64,
    limit: usize,
}
impl Output {
    fn bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let next = self
            .len
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| scratch("commit size overflow"))?;
        if next > self.limit as u64 {
            return Err(scratch("commit exceeds its encoded-byte budget"));
        }
        self.writer.write_all(bytes).map_err(scratch)?;
        self.digest.update(bytes);
        self.len = next;
        Ok(())
    }
    fn header(&mut self, major: u8, n: usize) -> Result<()> {
        let (bytes, size) = header_bytes(major, n as u64);
        self.bytes(&bytes[..size])
    }
    fn value<T: Serialize>(&mut self, value: &T, limit: usize) -> Result<()> {
        let bytes = canonical_cbor(value)?;
        if bytes.len() > limit {
            return Err(scratch("commit item exceeds its byte budget"));
        }
        self.bytes(&bytes)
    }
    fn array<T: Serialize>(
        &mut self,
        count: usize,
        items: impl IntoIterator<Item = Result<T>>,
        limit: usize,
    ) -> Result<Range> {
        self.header(4, count)?;
        let offset = self.len;
        let mut actual = 0;
        let mut digest = Sha256::new();
        for item in items {
            let item = item?;
            if actual >= count {
                return Err(corrupt("record iterator exceeds declared length"));
            }
            let bytes = canonical_cbor(&item)?;
            if bytes.len() > limit {
                return Err(scratch("commit item exceeds its byte budget"));
            }
            digest.update(&bytes);
            self.bytes(&bytes)?;
            actual += 1;
        }
        if actual != count {
            return Err(corrupt("record iterator ended early"));
        }
        Ok(Range {
            offset,
            bytes: self.len - offset,
            count,
            digest: digest.finalize().into(),
        })
    }
}
impl Commit {
    /// Seal current-format history in a private file; iterators are consumed once
    /// and their exact lengths checked. No publication is performed here.
    pub fn from_records(
        header: CommitHeader,
        edit_count: usize,
        edits: impl IntoIterator<Item = Result<Edit>>,
        blob_count: usize,
        blobs: impl IntoIterator<Item = Result<ContentId>>,
        limits: CommitLimits,
    ) -> Result<Self> {
        if !crate::supported_format(header.format) {
            return Err(corrupt("unsupported format"));
        }
        // Check user strings before encoding a potentially oversized scalar.
        if [
            header.actor.len(),
            header.message.len(),
            header.transaction_id.len(),
        ]
        .into_iter()
        .any(|n| n > limits.item_bytes)
        {
            return Err(scratch("commit header exceeds item budget"));
        }
        let (mut file, writer) = CommitFile::create(limits)?;
        let mut out = Output {
            writer: BufWriter::with_capacity(BUFFER, writer),
            digest: Sha256::new(),
            len: 0,
            limit: limits.bytes,
        };
        // Same RFC 8949 key order as the ordinary deterministic encoder. Scalar
        // fields and individual records use that encoder; there is no second
        // scalar/map codec or alternate persisted history format.
        let names = [
            "base",
            "root",
            "actor",
            "blobs",
            "edits",
            "format",
            "parent",
            "message",
            "metadata",
            "sequence",
            "transaction_id",
        ];
        out.header(5, names.len())?;
        let mut edits = Some(edits);
        let mut blobs = Some(blobs);
        let mut er = Range::default();
        let mut br = Range::default();
        for name in names {
            out.value(&name, limits.item_bytes)?;
            match name {
                "base" => out.value(&header.base, limits.item_bytes)?,
                "root" => out.value(&header.root, limits.item_bytes)?,
                "actor" => out.value(&header.actor, limits.item_bytes)?,
                "blobs" => br = out.array(blob_count, blobs.take().unwrap(), limits.item_bytes)?,
                "edits" => er = out.array(edit_count, edits.take().unwrap(), limits.item_bytes)?,
                "format" => out.value(&header.format, limits.item_bytes)?,
                "parent" => out.value(&header.parent, limits.item_bytes)?,
                "message" => out.value(&header.message, limits.item_bytes)?,
                "metadata" => out.value(&header.metadata, limits.item_bytes)?,
                "sequence" => out.value(&header.sequence, limits.item_bytes)?,
                "transaction_id" => out.value(&header.transaction_id, limits.item_bytes)?,
                _ => unreachable!(),
            }
        }
        out.writer.flush().map_err(scratch)?;
        file.seal(out.len, ContentId::of_digest(out.digest.finalize().into()))?;
        Ok(Self::assemble(header, Arc::new(file), er, br))
    }
    fn assemble(header: CommitHeader, file: Arc<CommitFile>, edits: Range, blobs: Range) -> Self {
        Self {
            format: header.format,
            transaction_id: header.transaction_id,
            sequence: header.sequence,
            parent: header.parent,
            base: header.base,
            root: header.root,
            edits: CommitList {
                file: file.clone(),
                range: edits,
                kind: PhantomData,
            },
            actor: header.actor,
            message: header.message,
            metadata: header.metadata,
            blobs: CommitList {
                file: file.clone(),
                range: blobs,
                kind: PhantomData,
            },
            file,
        }
    }
    /// Size and ID of the original sealed record, not a publication receipt.
    pub fn encoded_len(&self) -> u64 {
        self.file.len
    }
    pub fn content_id(&self) -> &ContentId {
        &self.file.id
    }
    pub(crate) fn file(&self) -> Arc<CommitFile> {
        self.file.clone()
    }
    pub(crate) fn parse(file: CommitFile) -> Result<Self> {
        let mut reader = file.verified_reader()?;
        let fields = container(&mut reader, 5, file.len)?;
        if fields > 64 {
            return Err(scratch("commit header field budget exceeded"));
        }
        let (
            mut format,
            mut transaction_id,
            mut sequence,
            mut parent,
            mut base,
            mut root,
            mut actor,
            mut message,
            mut metadata,
        ) = (None, None, None, None, None, None, None, None, None);
        let (mut edits, mut blobs) = (None, None);
        let mut seen = BTreeSet::new();
        for _ in 0..fields {
            let key: String = read_value(&mut reader, file.len, 64, &mut Sha256::new())?;
            if !seen.insert(key.clone()) {
                return Err(corrupt("duplicate header field"));
            }
            macro_rules! value {
                () => {
                    read_value(
                        &mut reader,
                        file.len,
                        file.limits.item_bytes,
                        &mut Sha256::new(),
                    )?
                };
            }
            match key.as_str() {
                "format" => format = Some(value!()),
                "transaction_id" => transaction_id = Some(value!()),
                "sequence" => sequence = Some(value!()),
                "parent" => parent = Some(value!()),
                "base" => base = Some(value!()),
                "root" => root = Some(value!()),
                "actor" => actor = Some(value!()),
                "message" => message = Some(value!()),
                "metadata" => metadata = Some(value!()),
                "edits" => edits = Some(read_array::<Edit>(&mut reader, &file)?),
                "blobs" => blobs = Some(read_array::<ContentId>(&mut reader, &file)?),
                _ => {
                    scan(&mut reader, file.len, 0)?;
                }
            }
        }
        if reader.stream_position().map_err(scratch)? != file.len {
            return Err(corrupt("trailing bytes"));
        }
        macro_rules! required {
            ($v:ident) => {
                $v.ok_or_else(|| corrupt(concat!("missing ", stringify!($v))))?
            };
        }
        let header = CommitHeader {
            format: required!(format),
            transaction_id: required!(transaction_id),
            sequence: required!(sequence),
            parent: parent.unwrap_or(None),
            base: base.unwrap_or(None),
            root: root.unwrap_or(None),
            actor: required!(actor),
            message: required!(message),
            metadata: metadata.unwrap_or(None),
        };
        if !crate::supported_format(header.format) {
            return Err(corrupt("unsupported format"));
        }
        Ok(Self::assemble(
            header,
            Arc::new(file),
            required!(edits),
            blobs.unwrap_or_default(),
        ))
    }
}

fn read_array<T: DeserializeOwned>(
    reader: &mut BufReader<File>,
    file: &CommitFile,
) -> Result<Range> {
    let count = usize::try_from(container(reader, 4, file.len)?).map_err(corrupt)?;
    let offset = reader.stream_position().map_err(scratch)?;
    if count as u64 > file.len - offset {
        return Err(corrupt("truncated record array"));
    }
    let mut digest = Sha256::new();
    for _ in 0..count {
        let _: T = read_value(reader, file.len, file.limits.item_bytes, &mut digest)?;
    }
    Ok(Range {
        offset,
        bytes: reader.stream_position().map_err(scratch)? - offset,
        count,
        digest: digest.finalize().into(),
    })
}
struct Hashed<'a, R> {
    reader: R,
    digest: &'a mut Sha256,
}
impl<R: Read> Read for Hashed<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(bytes)?;
        self.digest.update(&bytes[..n]);
        Ok(n)
    }
}
fn read_value<T: DeserializeOwned>(
    reader: &mut BufReader<File>,
    end: u64,
    limit: usize,
    digest: &mut Sha256,
) -> Result<T> {
    let start = reader.stream_position().map_err(scratch)?;
    let bound = end.min(start.saturating_add(limit as u64));
    scan(reader, bound, 0)?;
    let stop = reader.stream_position().map_err(scratch)?;
    // Rewind within the existing 64 KiB read buffer for ordinary small records;
    // an absolute seek would discard it on every field/entry.
    reader
        .seek_relative(-i64::try_from(stop - start).map_err(corrupt)?)
        .map_err(scratch)?;
    let take = reader.take(stop - start);
    let mut hashed = Hashed {
        reader: take,
        digest,
    };
    let value = ciborium::from_reader(&mut hashed).map_err(corrupt)?;
    if hashed.reader.limit() != 0 {
        return Err(corrupt("record decoder did not consume its value"));
    }
    Ok(value)
}
fn initial(reader: &mut BufReader<File>, end: u64) -> Result<(u8, u8, u64)> {
    let start = reader.stream_position().map_err(scratch)?;
    if start >= end {
        return Err(scratch("truncated/over-budget commit value"));
    }
    let mut byte = [0];
    reader.read_exact(&mut byte).map_err(corrupt)?;
    let major = byte[0] >> 5;
    let ai = byte[0] & 31;
    let n = match ai {
        0..=23 => return Ok((major, ai, u64::from(ai))),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return Err(corrupt("indefinite/reserved CBOR is not canonical")),
    };
    if start + 1 + n as u64 > end {
        return Err(scratch("truncated/over-budget commit header"));
    }
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes[8 - n..]).map_err(corrupt)?;
    Ok((major, ai, u64::from_be_bytes(bytes)))
}
fn container(reader: &mut BufReader<File>, major: u8, end: u64) -> Result<u64> {
    let (kind, _, count) = initial(reader, end)?;
    if kind != major {
        return Err(corrupt("invalid container type"));
    }
    Ok(count)
}
/// Bound encoded size/depth before typed serde decoding can reserve from an
/// untrusted length hint. Payload strings/bytes are skipped with a checked seek.
fn scan(reader: &mut BufReader<File>, end: u64, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(scratch("commit nesting budget exceeded"));
    }
    let (kind, ai, value) = initial(reader, end)?;
    let position = reader.stream_position().map_err(scratch)?;
    match kind {
        0 | 1 => {}
        2 | 3 => {
            let next = position
                .checked_add(value)
                .ok_or_else(|| corrupt("length overflow"))?;
            if next > end {
                return Err(scratch("commit value exceeds item/record budget"));
            }
            reader
                .seek_relative(i64::try_from(value).map_err(corrupt)?)
                .map_err(scratch)?;
        }
        4 | 5 => {
            let count = if kind == 5 {
                value
                    .checked_mul(2)
                    .ok_or_else(|| corrupt("map size overflow"))?
            } else {
                value
            };
            if count > end - position {
                return Err(scratch("commit container exceeds item/record budget"));
            }
            for _ in 0..count {
                scan(reader, end, depth + 1)?;
            }
        }
        6 => scan(reader, end, depth + 1)?,
        7 => {
            let finite = match ai {
                25 => (value & 0x7c00) != 0x7c00,
                26 => f32::from_bits(value as u32).is_finite(),
                27 => f64::from_bits(value).is_finite(),
                _ => true,
            };
            if !finite {
                return Err(corrupt("nonfinite value"));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

impl crate::Repository {
    /// Verified metadata, with bounded fallible list iteration. This is not an
    /// acceptance proof; use snapshots/history to retain or authorize revisions.
    pub async fn read_commit(&self, id: &ContentId) -> Result<Arc<Commit>> {
        let _pin = self.enter().await?;
        self.load_commit(id, true, self.commit_limits).await
    }
    pub(crate) async fn load_commit(
        &self,
        id: &ContentId,
        cached: bool,
        limits: CommitLimits,
    ) -> Result<Arc<Commit>> {
        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;
        limits.validate()?;
        let location = self.path(&format!("objects/{id}.cbor"));
        let key = location.to_string();
        if cached && let Some(commit) = self.commits.lock().unwrap().get(&key) {
            if commit.encoded_len() > limits.bytes as u64 {
                return Err(Error::ObjectTooLarge {
                    size: commit.encoded_len(),
                    limit: limits.bytes,
                });
            }
            if commit.file.limits.item_bytes <= limits.item_bytes {
                return Ok(commit);
            }
        }
        let gate = self.download_gate(&location);
        let _guard = gate.lock().await;
        if cached && let Some(commit) = self.commits.lock().unwrap().get(&key) {
            if commit.encoded_len() > limits.bytes as u64 {
                return Err(Error::ObjectTooLarge {
                    size: commit.encoded_len(),
                    limit: limits.bytes,
                });
            }
            if commit.file.limits.item_bytes <= limits.item_bytes {
                return Ok(commit);
            }
        }
        let (mut file, writer) = CommitFile::create(limits)?;
        let result = self.backend.get(&location).await?;
        if result.meta.size > limits.bytes as u64 {
            return Err(Error::ObjectTooLarge {
                size: result.meta.size,
                limit: limits.bytes,
            });
        }
        let expected = result.meta.size;
        let mut stream = result.into_stream();
        let mut writer = tokio::fs::File::from_std(writer);
        let mut digest = Sha256::new();
        let mut len = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            len = len
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| scratch("commit size overflow"))?;
            if len > limits.bytes as u64 {
                return Err(Error::ObjectTooLarge {
                    size: len,
                    limit: limits.bytes,
                });
            }
            writer.write_all(&chunk).await.map_err(scratch)?;
            digest.update(&chunk);
        }
        writer.flush().await.map_err(scratch)?;
        drop(writer);
        if len != expected || ContentId::of_digest(digest.finalize().into()) != *id {
            return Err(corrupt(format!("hash/size mismatch for {id}")));
        }
        file.seal(len, id.clone())?;
        let commit = Arc::new(
            tokio::task::spawn_blocking(move || Commit::parse(file))
                .await
                .map_err(scratch)??,
        );
        if cached {
            self.remember_commit(&commit);
        }
        Ok(commit)
    }
    /// Uncached remote existence/content proof. Do not allocate or cache the
    /// whole payload merely to reconcile an immutable upload acknowledgement.
    pub(crate) async fn verify_object(&self, id: &ContentId) -> Result<()> {
        use futures::StreamExt;
        let result = self
            .backend
            .get(&self.path(&format!("objects/{id}.cbor")))
            .await?;
        let expected = result.meta.size;
        let mut stream = result.into_stream();
        let mut digest = Sha256::new();
        let mut len = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            len = len
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| corrupt("remote object size overflow"))?;
            digest.update(&chunk);
        }
        if len != expected || ContentId::of_digest(digest.finalize().into()) != *id {
            return Err(Error::Corrupt(format!("hash mismatch for {id}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header() -> CommitHeader {
        CommitHeader {
            format: crate::FORMAT,
            transaction_id: "fixed-transaction".into(),
            sequence: 9,
            parent: Some(ContentId::of(b"parent")),
            base: Some(ContentId::of(b"base")),
            root: Some(ContentId::of(b"root")),
            actor: "alice".into(),
            message: "A canonical commit".into(),
            metadata: Some(ContentId::of(b"audit")),
        }
    }
    fn edit(i: u64) -> Edit {
        Edit {
            path: crate::OctreePath::new(vec![(i % 8) as u8]).unwrap(),
            generation: format!("g-{i}"),
            value: Some(ContentId::of(&i.to_be_bytes())),
        }
    }
    fn parse_bytes(bytes: &[u8], limits: CommitLimits) -> Result<Commit> {
        let (mut file, mut writer) = CommitFile::create(limits)?;
        writer.write_all(bytes).map_err(scratch)?;
        drop(writer);
        file.seal(bytes.len() as u64, ContentId::of(bytes))?;
        Commit::parse(file)
    }

    #[test]
    fn file_encoding_preserves_the_existing_commit_bytes_and_ids() {
        #[derive(Serialize)]
        struct Reference<'a> {
            format: u32,
            transaction_id: &'a str,
            sequence: u64,
            parent: &'a Option<ContentId>,
            base: &'a Option<ContentId>,
            root: &'a Option<ContentId>,
            edits: &'a [Edit],
            actor: &'a str,
            message: &'a str,
            metadata: &'a Option<ContentId>,
            blobs: &'a [ContentId],
        }
        let edits = vec![edit(0), edit(1)];
        let blobs = vec![ContentId::of(b"mesh")];
        let h = header();
        let expected = canonical_cbor(&Reference {
            format: h.format,
            transaction_id: &h.transaction_id,
            sequence: h.sequence,
            parent: &h.parent,
            base: &h.base,
            root: &h.root,
            edits: &edits,
            actor: &h.actor,
            message: &h.message,
            metadata: &h.metadata,
            blobs: &blobs,
        })
        .unwrap();
        let commit = Commit::from_records(
            h,
            edits.len(),
            edits.into_iter().map(Ok),
            blobs.len(),
            blobs.into_iter().map(Ok),
            CommitLimits::default(),
        )
        .unwrap();
        assert_eq!(commit.file.upload_bytes().unwrap().as_ref(), expected);
        assert_eq!(commit.content_id(), &ContentId::of(&expected));
        let loaded = parse_bytes(&expected, CommitLimits::default()).unwrap();
        assert_eq!(canonical_cbor(&loaded).unwrap(), expected);
        assert_eq!(
            loaded
                .edits
                .iter()
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .len(),
            2
        );
        // Current-format optional CID/list fields had Serde defaults before the
        // streaming reader; omitting them must not require a different reader.
        let mut optional = serde_json::to_value(&loaded).unwrap();
        for name in ["parent", "base", "root", "metadata", "blobs"] {
            optional.as_object_mut().unwrap().remove(name);
        }
        let optional =
            parse_bytes(&canonical_cbor(&optional).unwrap(), CommitLimits::default()).unwrap();
        assert!(
            optional.parent.is_none()
                && optional.base.is_none()
                && optional.root.is_none()
                && optional.metadata.is_none()
                && optional.blobs.is_empty()
        );
    }
    #[test]
    fn large_lists_are_file_backed_and_iterators_have_independent_cursors() {
        let commit = Commit::from_records(
            header(),
            15000,
            (0..15000).map(|i| Ok(edit(i))),
            0,
            [],
            CommitLimits::default(),
        )
        .unwrap();
        assert!(commit.encoded_len() > 1024 * 1024);
        assert!(std::mem::size_of::<Commit>() < 2048);
        let mut first = commit.edits.iter().unwrap();
        let mut second = commit.edits.iter().unwrap();
        assert_eq!(first.next().unwrap().unwrap().generation, "g-0");
        assert_eq!(first.next().unwrap().unwrap().generation, "g-1");
        assert_eq!(second.next().unwrap().unwrap().generation, "g-0");
        assert_eq!(
            first
                .try_fold(0, |count, item| item.map(|_| count + 1))
                .unwrap(),
            14998
        );
    }
    #[test]
    fn malformed_or_oversized_lengths_are_bounded_before_typed_allocation() {
        let commit =
            Commit::from_records(header(), 1, [Ok(edit(0))], 0, [], CommitLimits::default())
                .unwrap();
        let bytes = commit.file.upload_bytes().unwrap();
        let start = commit.edits.range.offset as usize;
        let entry = canonical_cbor(&edit(0)).unwrap();
        let mut bad = bytes.to_vec();
        let path = canonical_cbor(&"path").unwrap();
        let at = entry.windows(path.len()).position(|p| p == path).unwrap() + path.len();
        // Replace the tiny path array with a declaration of 2^64-1 elements.
        bad.splice(
            start + at..start + at + 1,
            [0x9b, 255, 255, 255, 255, 255, 255, 255, 255],
        );
        assert!(parse_bytes(&bad, CommitLimits::default()).is_err());
        let limits = CommitLimits {
            bytes: 1024 * 1024,
            item_bytes: 32,
        };
        assert!(parse_bytes(&bytes, limits).is_err());
        assert!(
            Commit::from_records(header(), 2, [Ok(edit(0))], 0, [], CommitLimits::default())
                .is_err()
        );
        assert!(matches!(
            Commit::from_records(
                header(),
                0,
                [Err(Error::Scratch("journal failed".into()))],
                0,
                [],
                CommitLimits::default()
            ),
            Err(Error::Scratch(_))
        ));
    }
    #[test]
    fn changed_local_list_bytes_fail_closed_instead_of_ending_early() {
        let commit =
            Commit::from_records(header(), 1, [Ok(edit(0))], 0, [], CommitLimits::default())
                .unwrap();
        let mut file = File::options().write(true).open(&commit.file.path).unwrap();
        let entry = canonical_cbor(&edit(0)).unwrap();
        let at = entry.windows(3).position(|v| v == b"g-0").unwrap();
        file.seek(SeekFrom::Start(commit.edits.range.offset + at as u64))
            .unwrap();
        file.write_all(b"z").unwrap();
        drop(file);
        assert!(
            commit
                .edits
                .iter()
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .is_err()
        );
        assert!(commit.file.upload_bytes().is_err());
    }
    #[tokio::test]
    async fn verified_commit_files_are_shared_only_within_the_namespace_and_read_budget() {
        let repo = crate::Repository::new(
            Arc::new(object_store::memory::InMemory::new()),
            "commit-read",
        )
        .unwrap();
        let snapshot = repo.create("main").await.unwrap();
        let first = repo.read_commit(snapshot.id()).await.unwrap();
        let second = repo.read_commit(snapshot.id()).await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(std::ptr::eq(first.as_ref(), snapshot.commit()));
        let strict = repo
            .clone()
            .with_commit_limits(CommitLimits {
                bytes: 64,
                item_bytes: 64,
            })
            .unwrap();
        assert!(matches!(
            strict.read_commit(snapshot.id()).await,
            Err(Error::ObjectTooLarge { .. })
        ));
        let other = repo.child("other").unwrap();
        assert!(other.read_commit(snapshot.id()).await.is_err());
        let strict = repo
            .with_commit_limits(CommitLimits {
                bytes: 1024 * 1024,
                item_bytes: 32,
            })
            .unwrap();
        assert!(
            matches!(
                strict.read_commit(snapshot.id()).await,
                Err(Error::Scratch(_))
            ),
            "a weak cache hit cannot bypass a tighter item budget"
        );
    }

    #[test]
    fn file_cache_reuses_sequential_reads_but_bounds_files_headers_and_weak_entries() {
        let mut cache = CommitCache::new(1024);
        let make = |sequence| {
            let mut h = header();
            h.sequence = sequence;
            h.message = "x".repeat(256);
            Arc::new(Commit::from_records(h, 0, [], 0, [], CommitLimits::default()).unwrap())
        };
        let first = make(1);
        let weak = Arc::downgrade(&first);
        cache.insert("first".into(), &first);
        drop(first);
        assert!(
            cache.get("first").is_some(),
            "sequential page requests must reuse a verified file"
        );
        let second = make(2);
        cache.insert("second".into(), &second);
        drop(second);
        assert!(weak.upgrade().is_none());
        assert!(cache.disk <= 1024);
        assert!(cache.ram <= CACHE_HEADER_BYTES);
        let mut cache = CommitCache::new(CACHE_FILE_BYTES as usize);
        for i in 0..40 {
            let c = make(i);
            cache.insert(format!("record-{i}"), &c);
        }
        assert!(cache.retained <= CACHE_HEADERS);
        assert!(cache.disk <= CACHE_FILE_BYTES);
        assert!(cache.ram <= CACHE_HEADER_BYTES);
        let mut weak_only = CommitCache::default();
        let live = make(99);
        for i in 0..1100 {
            weak_only.insert(format!("key-{i}"), &live);
        }
        assert_eq!(weak_only.retained, 0);
        assert_eq!(weak_only.entries.len(), CACHE_ENTRIES);
    }

    #[tokio::test]
    async fn cached_commit_files_do_not_keep_remote_maintenance_pins_alive() {
        let repo = crate::Repository::new(
            Arc::new(object_store::memory::InMemory::new()),
            "cached-commit-gc",
        )
        .unwrap()
        .with_cache_capacity(4 * 1024 * 1024)
        .with_coordinated_gc();
        let snapshot = repo.create("main").await.unwrap();
        let id = snapshot.id().clone();
        snapshot.close().await.unwrap();
        assert!(
            repo.commits
                .lock()
                .unwrap()
                .get(&repo.path(&format!("objects/{id}.cbor")).to_string())
                .is_some()
        );
        let mut maintenance = repo.maintenance().await.unwrap();
        maintenance.collect(|_| Ok(vec![])).await.unwrap();
        maintenance.finish().await.unwrap();
        repo.checkout("main").await.unwrap().close().await.unwrap();
    }
}
