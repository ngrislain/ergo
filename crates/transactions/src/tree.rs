use crate::{ContentId, Error, OctreePath, Repository, Result, canonical_cbor, codec::decode};
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_DECODED_NODE_BYTES: usize = 8 * 1024 * 1024;

/// Discardable decoded *read* nodes, separate from staged nodes which must stay
/// available until publication. Paging must not accumulate the whole visited trie.
#[derive(Default)]
struct ReadNodes {
    entries: BTreeMap<ContentId, (Node, usize)>,
    order: VecDeque<ContentId>,
    bytes: usize,
}
impl ReadNodes {
    fn get(&self, id: &ContentId) -> Option<&Node> {
        self.entries.get(id).map(|(node, _)| node)
    }

    fn insert(&mut self, id: ContentId, node: Node) {
        if self.entries.contains_key(&id) {
            return;
        }
        // Account for decoded strings, path buffers and index/queue overhead,
        // not just the much smaller encoded CBOR payload.
        let bytes = 256
            + std::mem::size_of::<Node>()
            + match &node {
                Node::Branch {
                    generation,
                    children,
                } => {
                    generation.as_ref().map_or(0, String::capacity)
                        + children
                            .iter()
                            .flatten()
                            .map(|id| id.as_str().len())
                            .sum::<usize>()
                }
                Node::Extension { path, child } => path.capacity() + child.as_str().len(),
                Node::Leaf { generation, value } => generation.capacity() + value.as_str().len(),
                Node::Deleted { generation } => generation.capacity(),
            };
        if bytes > MAX_DECODED_NODE_BYTES {
            return;
        }
        while self.bytes + bytes > MAX_DECODED_NODE_BYTES {
            let oldest = self.order.pop_front().unwrap();
            self.bytes -= self.entries.remove(&oldest).unwrap().1;
        }
        self.order.push_back(id.clone());
        self.entries.insert(id, (node, bytes));
        self.bytes += bytes;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Node {
    Branch {
        generation: Option<String>,
        children: [Option<ContentId>; 8],
    },
    /// Packed unary, unfenced octree levels. Fenced branches remain explicit.
    Extension {
        path: Vec<u8>,
        child: ContentId,
    },
    Leaf {
        generation: String,
        value: ContentId,
    },
    Deleted {
        generation: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegionVersion {
    fences: Vec<(usize, String)>,
    target: RegionTarget,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PresenceVersion {
    fences: Vec<(usize, String)>,
    generation: Option<String>,
    pub present: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum RegionTarget {
    Empty,
    Subtree(ContentId),
    Covered { depth: usize, node: ContentId },
}
enum Frame {
    Branch(Option<String>, [Option<ContentId>; 8], usize),
    Extension(Vec<u8>),
}

pub(crate) struct Tree {
    pub repo: Repository,
    nodes: BTreeMap<ContentId, Node>,
    read_nodes: ReadNodes,
    pending: BTreeMap<ContentId, Bytes>,
    uploaded: BTreeSet<ContentId>,
    node_read_limit: usize,
}
impl Tree {
    pub fn absorb(&mut self, other: Tree) {
        self.nodes.extend(other.nodes);
        for (id, (node, _)) in other.read_nodes.entries {
            self.read_nodes.insert(id, node);
        }
        self.pending.extend(other.pending);
        self.uploaded.extend(other.uploaded);
    }
    pub async fn presence(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
    ) -> Result<PresenceVersion> {
        let region = self.region(root, path).await?;
        let (generation, present) = match region.target {
            RegionTarget::Empty => (None, false),
            RegionTarget::Covered { .. } => return Err(Error::CoveredByLeaf(path.clone())),
            RegionTarget::Subtree(id) => match self.node(&id).await? {
                Node::Branch { generation, .. } => (generation, true),
                Node::Extension { .. } => (None, true),
                Node::Leaf { generation, .. } => (Some(generation), true),
                Node::Deleted { generation } => (Some(generation), false),
            },
        };
        Ok(PresenceVersion {
            fences: region.fences,
            generation,
            present,
        })
    }
    pub fn new(repo: Repository) -> Self {
        Self {
            repo,
            nodes: BTreeMap::new(),
            read_nodes: ReadNodes::default(),
            pending: BTreeMap::new(),
            uploaded: BTreeSet::new(),
            node_read_limit: usize::MAX,
        }
    }
    pub fn bounded_read(repo: Repository) -> Self {
        let mut tree = Self::new(repo);
        tree.node_read_limit = 64 * 1024;
        tree
    }
    async fn prefetch_nodes(&mut self, ids: Vec<ContentId>) {
        // Read only four immutable node levels, never application payloads.
        // Failed/oversized speculative reads are misses, not page failures.
        let mut frontier = ids;
        for _ in 0..4 {
            let repo = &self.repo;
            let mut next = Vec::new();
            for id in &frontier {
                if let Some(node) = self.cached_node(id) {
                    match node {
                        Node::Extension { child, .. } => next.push(child.clone()),
                        Node::Branch { children, .. } => {
                            next.extend(children.iter().flatten().cloned())
                        }
                        _ => {}
                    }
                }
            }
            let loaded: Vec<_> = stream::iter(
                frontier
                    .into_iter()
                    .filter(|id| self.cached_node(id).is_none()),
            )
            .map(|id| async move {
                let bytes = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    repo.object_limited(&id, 64 * 1024),
                )
                .await
                .ok()
                .and_then(Result::ok)?;
                Some((id, decode::<Node>(&bytes).ok()?))
            })
            .buffered(64)
            .collect()
            .await;
            frontier = next;
            for (id, node) in loaded.into_iter().flatten() {
                if Self::validate_node(&node).is_err() {
                    continue;
                }
                match &node {
                    Node::Extension { child, .. } => frontier.push(child.clone()),
                    Node::Branch { children, .. } => {
                        frontier.extend(children.iter().flatten().cloned())
                    }
                    _ => {}
                }
                self.read_nodes.insert(id, node);
            }
            if frontier.is_empty() {
                break;
            }
        }
    }
    pub fn stage<T: Serialize>(&mut self, value: &T) -> Result<ContentId> {
        let bytes = canonical_cbor(value)?;
        let id = ContentId::of(&bytes);
        self.pending.entry(id.clone()).or_insert(bytes.into());
        Ok(id)
    }
    fn intern(&mut self, node: Node) -> Result<ContentId> {
        let id = self.stage(&node)?;
        self.nodes.insert(id.clone(), node);
        Ok(id)
    }
    fn cached_node(&self, id: &ContentId) -> Option<&Node> {
        self.nodes.get(id).or_else(|| self.read_nodes.get(id))
    }
    async fn node(&mut self, id: &ContentId) -> Result<Node> {
        if let Some(n) = self.cached_node(id) {
            return Ok(n.clone());
        }
        let node: Node = decode(&self.repo.object_limited(id, self.node_read_limit).await?)?;
        Self::validate_node(&node)?;
        self.read_nodes.insert(id.clone(), node.clone());
        Ok(node)
    }
    fn validate_node(node: &Node) -> Result<()> {
        match node {
            Node::Branch { children, .. } if children.iter().all(Option::is_none) => {
                return Err(Error::Corrupt(
                    "empty branch requires absence or a tombstone".into(),
                ));
            }
            Node::Extension { path, .. }
                if path.is_empty()
                    || path.len() > crate::MAX_DEPTH
                    || path.iter().any(|q| *q > 7) =>
            {
                return Err(Error::Corrupt("invalid packed octree path".into()));
            }
            _ => {}
        }
        Ok(())
    }
    async fn extension(&mut self, mut path: Vec<u8>, mut child: ContentId) -> Result<ContentId> {
        if path.is_empty() {
            return Ok(child);
        }
        if let Node::Extension {
            path: suffix,
            child: grandchild,
        } = self.node(&child).await?
        {
            path.extend(suffix);
            child = grandchild;
        }
        if path.len() > crate::MAX_DEPTH {
            return Err(Error::Corrupt("packed path exceeds depth limit".into()));
        }
        self.intern(Node::Extension { path, child })
    }
    async fn branch(
        &mut self,
        generation: Option<String>,
        children: [Option<ContentId>; 8],
    ) -> Result<ContentId> {
        if generation.is_none() && children.iter().flatten().count() == 1 {
            let (q, child) = children
                .into_iter()
                .enumerate()
                .find_map(|(q, c)| c.map(|c| (q, c)))
                .unwrap();
            return self.extension(vec![q as u8], child).await;
        }
        self.intern(Node::Branch {
            generation,
            children,
        })
    }
    pub async fn region(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
    ) -> Result<RegionVersion> {
        let mut current = root.cloned();
        let mut depth = 0;
        let mut fences = Vec::new();
        while depth < path.octants().len() {
            let Some(id) = current else {
                return Ok(RegionVersion {
                    fences,
                    target: RegionTarget::Empty,
                });
            };
            match self.node(&id).await? {
                Node::Branch {
                    generation,
                    children,
                } => {
                    if let Some(generation) = generation {
                        fences.push((depth, generation));
                    }
                    current = children[path.octants()[depth] as usize].clone();
                    depth += 1;
                }
                Node::Extension {
                    path: segment,
                    child,
                } => {
                    let remaining = &path.octants()[depth..];
                    if remaining.starts_with(&segment) {
                        depth += segment.len();
                        current = Some(child);
                    } else if segment.starts_with(remaining) {
                        // A dependency can end at a virtual node inside a packed
                        // edge. Its canonical suffix hash is independent of how
                        // ancestors/sibling paths are physically represented.
                        current = Some(self.intern(Node::Extension {
                            path: segment[remaining.len()..].to_vec(),
                            child,
                        })?);
                        depth = path.octants().len();
                    } else {
                        return Ok(RegionVersion {
                            fences,
                            target: RegionTarget::Empty,
                        });
                    }
                }
                Node::Leaf { .. } => {
                    return Ok(RegionVersion {
                        fences,
                        target: RegionTarget::Covered { depth, node: id },
                    });
                }
                Node::Deleted { generation } => {
                    fences.push((depth, generation));
                    return Ok(RegionVersion {
                        fences,
                        target: RegionTarget::Empty,
                    });
                }
            }
        }
        Ok(RegionVersion {
            fences,
            target: current
                .map(RegionTarget::Subtree)
                .unwrap_or(RegionTarget::Empty),
        })
    }
    pub async fn get(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
    ) -> Result<Option<ContentId>> {
        match self.region(root, path).await?.target {
            RegionTarget::Empty => Ok(None),
            RegionTarget::Covered { .. } => Err(Error::CoveredByLeaf(path.clone())),
            RegionTarget::Subtree(id) => match self.node(&id).await? {
                Node::Leaf { value, .. } => Ok(Some(value)),
                Node::Deleted { .. } => Ok(None),
                Node::Branch { .. } | Node::Extension { .. } => Err(Error::NotLeaf(path.clone())),
            },
        }
    }
    pub async fn payload_limited(&self, id: &ContentId, limit: usize) -> Result<Bytes> {
        match self.pending.get(id) {
            Some(bytes) if bytes.len() <= limit => Ok(bytes.clone()),
            Some(bytes) => Err(Error::ObjectTooLarge {
                size: bytes.len() as u64,
                limit,
            }),
            None => self.repo.object_limited(id, limit).await,
        }
    }
    fn descend(path: &OctreePath, suffix: &[u8]) -> Result<OctreePath> {
        let mut p = path.octants().to_vec();
        p.extend(suffix);
        OctreePath::new(p)
    }
    pub async fn scan(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
    ) -> Result<Vec<(OctreePath, ContentId)>> {
        let mut result = Vec::new();
        let mut after = None;
        loop {
            let page = self.scan_page(root, path, after.as_ref(), 10_000).await?;
            result.extend(page.leaves);
            after = page.next;
            if after.is_none() {
                return Ok(result);
            }
        }
    }
    pub async fn scan_page(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<crate::ScanPage> {
        self.scan_page_inner(root, path, after, limit, false).await
    }
    pub async fn scan_page_prefetched(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<crate::ScanPage> {
        let enabled = self.repo.cache_stats().capacity_bytes > 0;
        self.scan_page_inner(root, path, after, limit, enabled)
            .await
    }
    async fn scan_page_inner(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
        after: Option<&OctreePath>,
        limit: usize,
        read_prefetch: bool,
    ) -> Result<crate::ScanPage> {
        if !(1..=10_000).contains(&limit) || after.is_some_and(|a| !path.is_prefix_of(a)) {
            return Err(Error::InvalidCursor);
        }
        let mut stack = match self.region(root, path).await?.target {
            RegionTarget::Empty => {
                return Ok(crate::ScanPage {
                    leaves: Vec::new(),
                    next: None,
                });
            }
            RegionTarget::Covered { .. } => return Err(Error::CoveredByLeaf(path.clone())),
            RegionTarget::Subtree(id) => vec![(path.clone(), id)],
        };
        let mut leaves = Vec::with_capacity(limit + 1);
        let mut visited = 0;
        while let Some((path, id)) = stack.pop() {
            visited += 1;
            if visited > 100_000 {
                return Err(Error::TraversalLimit { max_nodes: 100_000 });
            }
            if after.is_some_and(|a| path <= *a && !path.is_prefix_of(a)) {
                continue;
            }
            match self.node(&id).await? {
                Node::Deleted { .. } => {}
                Node::Leaf { value, .. } => {
                    if after.is_none_or(|a| path > *a) {
                        leaves.push((path, value));
                    }
                    if leaves.len() > limit {
                        break;
                    }
                }
                Node::Extension {
                    path: suffix,
                    child,
                } => stack.push((Self::descend(&path, &suffix)?, child)),
                Node::Branch { children, .. } => {
                    if read_prefetch {
                        self.prefetch_nodes(children.iter().flatten().cloned().collect())
                            .await;
                    }
                    for (q, child) in children.into_iter().enumerate().rev() {
                        if let Some(child) = child {
                            stack.push((path.child(q as u8)?, child));
                        }
                    }
                }
            }
        }
        let next = if leaves.len() > limit {
            leaves.pop();
            leaves.last().map(|(p, _)| p.clone())
        } else {
            None
        };
        Ok(crate::ScanPage { leaves, next })
    }
    /// Sparse directory paging: stop at a logical depth instead of walking every
    /// descendant leaf. Returned CIDs identify nodes, not application payloads.
    pub async fn scan_level(
        &mut self,
        root: Option<&ContentId>,
        region: &OctreePath,
        depth: usize,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<crate::ScanPage> {
        self.scan_level_inner(root, region, depth, after, limit, false)
            .await
    }
    pub async fn scan_level_prefetched(
        &mut self,
        root: Option<&ContentId>,
        region: &OctreePath,
        depth: usize,
        after: Option<&OctreePath>,
        limit: usize,
    ) -> Result<crate::ScanPage> {
        let enabled = self.repo.cache_stats().capacity_bytes > 0;
        self.scan_level_inner(root, region, depth, after, limit, enabled)
            .await
    }
    async fn scan_level_inner(
        &mut self,
        root: Option<&ContentId>,
        region: &OctreePath,
        depth: usize,
        after: Option<&OctreePath>,
        limit: usize,
        read_prefetch: bool,
    ) -> Result<crate::ScanPage> {
        if !(1..=10_000).contains(&limit)
            || depth < region.octants().len()
            || depth > crate::MAX_DEPTH
            || after.is_some_and(|a| a.octants().len() != depth || !region.is_prefix_of(a))
        {
            return Err(Error::InvalidCursor);
        }
        let mut stack = match self.region(root, region).await?.target {
            RegionTarget::Empty => Vec::new(),
            RegionTarget::Covered { .. } => return Err(Error::CoveredByLeaf(region.clone())),
            RegionTarget::Subtree(id) => vec![(region.clone(), id)],
        };
        let mut leaves = Vec::new();
        let mut visited = 0;
        while let Some((path, id)) = stack.pop() {
            visited += 1;
            if visited > 100_000 {
                return Err(Error::TraversalLimit { max_nodes: 100_000 });
            }
            if after.is_some_and(|a| path <= *a && !path.is_prefix_of(a)) {
                continue;
            }
            let node = self.node(&id).await?;
            if matches!(node, Node::Deleted { .. }) {
                continue;
            }
            if path.octants().len() == depth {
                if after.is_none_or(|a| path > *a) {
                    leaves.push((path, id));
                    if leaves.len() > limit {
                        break;
                    }
                }
                continue;
            }
            match node {
                Node::Branch { children, .. } => {
                    if read_prefetch {
                        self.prefetch_nodes(children.iter().flatten().cloned().collect())
                            .await;
                    }
                    for (q, c) in children.into_iter().enumerate().rev() {
                        if let Some(c) = c {
                            stack.push((path.child(q as u8)?, c));
                        }
                    }
                }
                Node::Extension {
                    path: suffix,
                    child,
                } => {
                    let take = (depth - path.octants().len()).min(suffix.len());
                    let prefix = Self::descend(&path, &suffix[..take])?;
                    let node = if take == suffix.len() {
                        child
                    } else {
                        self.intern(Node::Extension {
                            path: suffix[take..].to_vec(),
                            child,
                        })?
                    };
                    stack.push((prefix, node));
                }
                _ => {}
            }
        }
        let next = if leaves.len() > limit {
            leaves.pop();
            leaves.last().map(|(p, _)| p.clone())
        } else {
            None
        };
        Ok(crate::ScanPage { leaves, next })
    }
    pub async fn replace(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
        value: Option<&ContentId>,
        generation: &str,
    ) -> Result<Option<ContentId>> {
        let replacement = self.intern(match value {
            Some(value) => Node::Leaf {
                generation: generation.into(),
                value: value.clone(),
            },
            None => Node::Deleted {
                generation: generation.into(),
            },
        })?;
        self.rewrite_node(root, path, Some(replacement)).await
    }

    async fn rewrite_node(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
        mut replacement: Option<ContentId>,
    ) -> Result<Option<ContentId>> {
        let mut frames = Vec::new();
        let mut current = root.cloned();
        let mut depth = 0;
        while depth < path.octants().len() {
            let Some(id) = current else {
                frames.push(Frame::Extension(path.octants()[depth..].to_vec()));
                break;
            };
            match self.node(&id).await? {
                Node::Leaf { .. } => return Err(Error::CoveredByLeaf(path.clone())),
                Node::Deleted { generation } => {
                    frames.push(Frame::Branch(
                        Some(generation),
                        std::array::from_fn(|_| None),
                        path.octants()[depth] as usize,
                    ));
                    depth += 1;
                    current = None;
                }
                Node::Branch {
                    generation,
                    children,
                } => {
                    let q = path.octants()[depth] as usize;
                    current = children[q].clone();
                    frames.push(Frame::Branch(generation, children, q));
                    depth += 1;
                }
                Node::Extension {
                    path: segment,
                    child,
                } => {
                    let remaining = &path.octants()[depth..];
                    let common = remaining
                        .iter()
                        .zip(&segment)
                        .take_while(|(a, b)| a == b)
                        .count();
                    if common == segment.len() {
                        depth += common;
                        current = Some(child);
                        frames.push(Frame::Extension(segment));
                    } else if common == remaining.len() {
                        if common > 0 {
                            frames.push(Frame::Extension(segment[..common].to_vec()));
                        }
                        break;
                    } else {
                        if common > 0 {
                            frames.push(Frame::Extension(segment[..common].to_vec()));
                        }
                        let mut children = std::array::from_fn(|_| None);
                        children[segment[common] as usize] = Some(
                            self.extension(segment[common + 1..].to_vec(), child)
                                .await?,
                        );
                        frames.push(Frame::Branch(None, children, remaining[common] as usize));
                        depth += common + 1;
                        current = None;
                    }
                }
            }
        }
        for frame in frames.into_iter().rev() {
            replacement = match frame {
                Frame::Extension(path) => match replacement {
                    Some(child) => Some(self.extension(path, child).await?),
                    None => None,
                },
                Frame::Branch(generation, mut children, q) => {
                    children[q] = replacement;
                    if children.iter().all(Option::is_none) {
                        None
                    } else {
                        Some(self.branch(generation, children).await?)
                    }
                }
            };
        }
        Ok(replacement)
    }
    pub(crate) async fn erase(
        &mut self,
        root: Option<&ContentId>,
        path: &OctreePath,
    ) -> Result<Option<ContentId>> {
        self.rewrite_node(root, path, None).await
    }
    pub async fn compact_region(
        &mut self,
        root: Option<&ContentId>,
        region: &OctreePath,
        max_nodes: usize,
    ) -> Result<Option<ContentId>> {
        let target = match self.region(root, region).await?.target {
            RegionTarget::Empty => return Ok(root.cloned()),
            RegionTarget::Covered { .. } => return Err(Error::CoveredByLeaf(region.clone())),
            RegionTarget::Subtree(id) => id,
        };
        let replacement = self.compact(Some(&target), max_nodes).await?;
        self.rewrite_node(root, region, replacement).await
    }
    async fn compact(
        &mut self,
        root: Option<&ContentId>,
        max_nodes: usize,
    ) -> Result<Option<ContentId>> {
        let mut stack: Vec<_> = root.cloned().map(|id| (id, false)).into_iter().collect();
        let mut replacements = BTreeMap::new();
        let mut visited_nodes = BTreeSet::new();
        while let Some((id, visited)) = stack.pop() {
            if replacements.contains_key(&id) {
                continue;
            }
            if visited_nodes.insert(id.clone()) && visited_nodes.len() > max_nodes {
                return Err(Error::CompactionLimit { max_nodes });
            }
            match self.node(&id).await? {
                Node::Deleted { .. } => {
                    replacements.insert(id, None);
                }
                Node::Leaf { .. } => {
                    replacements.insert(id.clone(), Some(id));
                }
                Node::Extension { child, .. } if !visited => {
                    stack.push((id, true));
                    stack.push((child, false));
                }
                Node::Extension { path, child } => {
                    let new = match replacements[&child].clone() {
                        Some(child) => Some(self.extension(path, child).await?),
                        None => None,
                    };
                    replacements.insert(id, new);
                }
                Node::Branch { children, .. } if !visited => {
                    stack.push((id, true));
                    stack.extend(children.into_iter().flatten().map(|c| (c, false)));
                }
                Node::Branch { children, .. } => {
                    let children = children.map(|c| c.and_then(|id| replacements[&id].clone()));
                    let new = if children.iter().all(Option::is_none) {
                        None
                    } else {
                        Some(self.branch(None, children).await?)
                    };
                    replacements.insert(id, new);
                }
            }
        }
        Ok(root.and_then(|id| replacements[id].clone()))
    }
    pub async fn flush(
        &mut self,
        root: Option<&ContentId>,
        history_payloads: &[ContentId],
        concurrency: usize,
    ) -> Result<()> {
        let mut stack: Vec<_> = root
            .cloned()
            .into_iter()
            .chain(history_payloads.iter().cloned())
            .collect();
        let mut objects = BTreeMap::new();
        while let Some(id) = stack.pop() {
            if self.uploaded.contains(&id) || objects.contains_key(&id) {
                continue;
            }
            let Some(bytes) = self.pending.get(&id) else {
                continue;
            };
            objects.insert(id.clone(), bytes.clone());
            match self.nodes.get(&id) {
                Some(Node::Branch { children, .. }) => {
                    stack.extend(children.iter().flatten().cloned())
                }
                Some(Node::Extension { child, .. }) => stack.push(child.clone()),
                Some(Node::Leaf { value, .. }) => stack.push(value.clone()),
                Some(Node::Deleted { .. }) | None => {}
            }
        }
        let uploaded: Vec<_> = stream::iter(objects)
            .map(|(id, bytes)| {
                let repo = self.repo.clone();
                async move {
                    repo.put_object(&id, bytes).await?;
                    Ok::<_, Error>(id)
                }
            })
            .buffer_unordered(concurrency)
            .try_collect()
            .await?;
        self.uploaded.extend(uploaded);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn churn(cache: &mut ReadNodes) {
        for i in 0..4096 {
            let node = Node::Deleted {
                generation: format!("{i}:{}", "x".repeat(4096)),
            };
            let id = ContentId::of(&canonical_cbor(&node).unwrap());
            cache.insert(id, node);
            assert!(cache.bytes <= MAX_DECODED_NODE_BYTES);
        }
    }

    #[tokio::test]
    async fn paging_evicts_decoded_reads_but_retains_staged_nodes_and_reloads_immutable_nodes() {
        let repo = Repository::new(
            std::sync::Arc::new(object_store::memory::InMemory::new()),
            "bounded",
        )
        .unwrap();
        let mut writer = Tree::new(repo.clone());
        let value = writer.stage(&42u32).unwrap();
        let root = writer
            .intern(Node::Leaf {
                generation: "initial".into(),
                value: value.clone(),
            })
            .unwrap();
        writer.flush(Some(&root), &[], 4).await.unwrap();

        let mut reader = Tree::new(repo.clone());
        let before = reader.node(&root).await.unwrap();
        assert!(
            reader.nodes.is_empty(),
            "Reads must not become permanently staged nodes"
        );
        churn(&mut reader.read_nodes);
        assert!(reader.read_nodes.get(&root).is_none());
        assert_eq!(
            canonical_cbor(&reader.node(&root).await.unwrap()).unwrap(),
            canonical_cbor(&before).unwrap()
        );

        // In-flight replacements must survive cache churn/absorption and still
        // flush all reachable nodes and payloads, including pending children.
        let next_value = reader.stage(&99u32).unwrap();
        let leaf = reader
            .intern(Node::Leaf {
                generation: "pending".into(),
                value: next_value.clone(),
            })
            .unwrap();
        let next_root = reader
            .intern(Node::Extension {
                path: vec![3, 1],
                child: leaf.clone(),
            })
            .unwrap();
        churn(&mut reader.read_nodes);
        let mut combined = Tree::new(repo.clone());
        combined.absorb(reader);
        assert!(combined.nodes.contains_key(&next_root));
        assert!(combined.nodes.contains_key(&leaf));
        assert!(combined.read_nodes.bytes <= MAX_DECODED_NODE_BYTES);
        combined.flush(Some(&next_root), &[], 4).await.unwrap();
        let path = OctreePath::new(vec![3, 1]).unwrap();
        assert_eq!(
            Tree::new(repo).get(Some(&next_root), &path).await.unwrap(),
            Some(next_value)
        );
    }
}
