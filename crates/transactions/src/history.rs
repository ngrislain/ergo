//! The acceptance index is published in the same head CAS as the commit. It is
//! outside Commit (which cannot contain its own content ID without a hash cycle).
use crate::{
    ContentId, Error, FORMAT, Head, Maintenance, OctreePath, Repository, Result, Snapshot,
    canonical_cbor, codec::decode, tree::Tree,
};
use object_store::PutMode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct HistoryIndex {
    pub root: Option<ContentId>,
    pub retained_from: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevisionEntry {
    pub revision: ContentId,
    pub sequence: u64,
}
#[derive(Debug, Serialize)]
pub struct RevisionPage {
    pub revisions: Vec<ContentId>,
    pub next: Option<ContentId>,
    pub retained_from: u64,
}
#[derive(Debug, Serialize)]
pub struct PruneReport {
    pub removed_revisions: usize,
    pub retained_from: u64,
}

pub(crate) fn id_path(id: &ContentId) -> OctreePath {
    let bytes = hex::decode(id.as_str()).expect("validated CID");
    let bits: Vec<_> = [0u8, 0]
        .into_iter()
        .chain(
            bytes
                .into_iter()
                .flat_map(|b| (0..8).rev().map(move |i| (b >> i) & 1)),
        )
        .collect();
    let mut p = vec![0];
    p.extend(bits.chunks(3).map(|b| b[0] * 4 + b[1] * 2 + b[2]));
    OctreePath::new(p).unwrap()
}
pub(crate) fn seq_path(sequence: u64) -> OctreePath {
    let mut p = vec![1];
    p.extend(
        (0..22)
            .rev()
            .map(|q| ((u128::from(sequence) >> (q * 3)) & 7) as u8),
    );
    OctreePath::new(p).unwrap()
}
pub(crate) async fn entry(
    repo: &Repository,
    index: &HistoryIndex,
    path: &OctreePath,
) -> Result<Option<RevisionEntry>> {
    let mut tree = Tree::new(repo.clone());
    let Some(id) = tree.get(index.root.as_ref(), path).await? else {
        return Ok(None);
    };
    let e: RevisionEntry = decode(&tree.payload_limited(&id, 4096).await?)?;
    if !((path.octants().first() == Some(&0) && id_path(&e.revision) == *path)
        || (path.octants().first() == Some(&1) && seq_path(e.sequence) == *path))
    {
        return Err(Error::Corrupt("acceptance index key/value mismatch".into()));
    }
    Ok((e.sequence >= index.retained_from).then_some(e))
}
pub(crate) async fn append(
    repo: &Repository,
    index: Option<&HistoryIndex>,
    revision: &ContentId,
    sequence: u64,
) -> Result<Option<HistoryIndex>> {
    append_many(
        repo,
        index,
        &[RevisionEntry {
            revision: revision.clone(),
            sequence,
        }],
    )
    .await
}
pub(crate) async fn append_many(
    repo: &Repository,
    index: Option<&HistoryIndex>,
    entries: &[RevisionEntry],
) -> Result<Option<HistoryIndex>> {
    let index =
        index.ok_or_else(|| Error::Corrupt("Current heads require an acceptance index".into()))?;
    let mut tree = Tree::new(repo.clone());
    let mut root = index.root.clone();
    for entry in entries {
        let value = tree.stage(entry)?;
        let generation = format!("history/{}", entry.revision);
        root = tree
            .replace(
                root.as_ref(),
                &id_path(&entry.revision),
                Some(&value),
                &generation,
            )
            .await?;
        root = tree
            .replace(
                root.as_ref(),
                &seq_path(entry.sequence),
                Some(&value),
                &generation,
            )
            .await?;
    }
    tree.flush(root.as_ref(), &[], 32).await?;
    Ok(Some(HistoryIndex {
        root,
        retained_from: index.retained_from,
    }))
}

impl Repository {
    pub async fn accepted_snapshot(&self, branch: &str, id: &ContentId) -> Result<Snapshot> {
        let pin = self.enter().await?;
        let (tip, _) = self.head(branch).await?;
        if !self.contains(tip, id, 0).await? {
            return Err(Error::RevisionUnavailable);
        }
        let mut snapshot = self.load_snapshot(id).await?;
        snapshot.pin = pin;
        Ok(snapshot)
    }
    pub async fn revision_entry(
        &self,
        branch: &str,
        id: &ContentId,
    ) -> Result<Option<RevisionEntry>> {
        let _pin = self.enter().await?;
        let (tip, _) = self.head(branch).await?;
        let index = tip
            .history
            .as_ref()
            .ok_or_else(|| Error::Corrupt("Missing acceptance index".into()))?;
        entry(self, index, &id_path(id)).await
    }
    /// Most-recent-first, inclusive cursor. Pruned prefixes are never traversed.
    pub async fn revisions(
        &self,
        branch: &str,
        cursor: Option<&ContentId>,
        limit: usize,
    ) -> Result<RevisionPage> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::InvalidOptions);
        }
        let _pin = self.enter().await?;
        let (tip, _) = self.head(branch).await?;
        let index = tip
            .history
            .as_ref()
            .ok_or_else(|| Error::Corrupt("Missing acceptance index".into()))?;
        let mut sequence = match cursor {
            Some(id) => {
                entry(self, index, &id_path(id))
                    .await?
                    .ok_or(Error::RevisionUnavailable)?
                    .sequence
            }
            None => tip.commit.sequence,
        };
        let mut revisions = Vec::new();
        loop {
            let e = entry(self, index, &seq_path(sequence))
                .await?
                .ok_or_else(|| Error::Corrupt("acceptance sequence gap".into()))?;
            revisions.push(e.revision);
            if sequence == index.retained_from {
                return Ok(RevisionPage {
                    revisions,
                    next: None,
                    retained_from: index.retained_from,
                });
            }
            sequence -= 1;
            if revisions.len() == limit {
                break;
            }
        }
        let next = entry(self, index, &seq_path(sequence))
            .await?
            .map(|e| e.revision);
        Ok(RevisionPage {
            revisions,
            next,
            retained_from: index.retained_from,
        })
    }
}

impl Maintenance {
    /// Install a semantically verified current-format history under the exclusive
    /// barrier. The application must stage/verify its blob conversion first.
    /// Revisions are oldest-first; actor/audit preservation is the caller's job.
    pub async fn replace_history(
        &mut self,
        branch: &str,
        expected: &ContentId,
        revisions: &[ContentId],
    ) -> Result<ContentId> {
        self.assert_owner().await?;
        let (tip, version) = self.repo.head(branch).await?;
        if tip.id != *expected || revisions.is_empty() {
            return Err(Error::Corrupt(
                "Migration source head changed or history is empty".into(),
            ));
        }
        let mut entries = Vec::with_capacity(revisions.len());
        let mut previous: Option<(ContentId, u64)> = None;
        for id in revisions {
            let snapshot = self.repo.load_snapshot(id).await?;
            if let Some((parent, sequence)) = &previous {
                if snapshot.commit.parent.as_ref() != Some(parent)
                    || sequence.checked_add(1) != Some(snapshot.commit.sequence)
                {
                    return Err(Error::Corrupt(
                        "Migration history is not a consecutive parent chain".into(),
                    ));
                }
            }
            entries.push(RevisionEntry {
                revision: id.clone(),
                sequence: snapshot.commit.sequence,
            });
            previous = Some((id.clone(), snapshot.commit.sequence));
        }
        let seed = HistoryIndex {
            root: None,
            retained_from: entries[0].sequence,
        };
        if entries[0].sequence != tip.retained_from()
            || entries.last().unwrap().sequence != tip.commit.sequence
        {
            return Err(Error::Corrupt(
                "Migration must preserve the complete retained sequence range".into(),
            ));
        }
        let index = append_many(&self.repo, Some(&seed), &entries).await?;
        let commit = revisions.last().unwrap().clone();
        self.poisoned = true;
        self.repo
            .backend
            .put_opts(
                &self.repo.head_path(branch)?,
                canonical_cbor(&Head {
                    format: FORMAT,
                    commit: commit.clone(),
                    history: index,
                })?
                .into(),
                PutMode::Update(version).into(),
            )
            .await?;
        self.poisoned = false;
        Ok(commit)
    }
    pub async fn read<T: serde::de::DeserializeOwned>(
        &self,
        branch: &str,
        path: &OctreePath,
    ) -> Result<Option<T>> {
        self.assert_owner().await?;
        self.repo.head(branch).await?.0.get(path).await
    }
    /// Explicit keep-last-N retention. The namespace barrier excludes old bases.
    /// Application blob links must be supplied to subsequent garbage collection.
    pub async fn prune_history(&mut self, branch: &str, keep_last: u64) -> Result<PruneReport> {
        if keep_last == 0 {
            return Err(Error::InvalidOptions);
        }
        let (tip, version) = self.repo.head(branch).await?;
        let mut index = tip.history.clone().unwrap();
        let floor = tip
            .commit
            .sequence
            .saturating_sub(keep_last - 1)
            .max(index.retained_from);
        let original = index.clone();
        let mut after = None;
        let mut removed = 0;
        loop {
            let mut tree = Tree::new(self.repo.clone());
            let page = tree
                .scan_page(
                    original.root.as_ref(),
                    &OctreePath::new(vec![1])?,
                    after.as_ref(),
                    128,
                )
                .await?;
            let mut finished = page.next.is_none();
            for (_, payload) in page.leaves {
                let e: RevisionEntry = decode(&self.repo.object(&payload).await?)?;
                if e.sequence >= floor {
                    finished = true;
                    break;
                }
                index.root = tree
                    .erase(index.root.as_ref(), &id_path(&e.revision))
                    .await?;
                index.root = tree
                    .erase(index.root.as_ref(), &seq_path(e.sequence))
                    .await?;
                removed += 1;
            }
            tree.flush(index.root.as_ref(), &[], 32).await?;
            after = page.next;
            if finished {
                break;
            }
        }
        index.retained_from = floor;
        self.poisoned = true;
        self.repo
            .backend
            .put_opts(
                &self.repo.head_path(branch)?,
                canonical_cbor(&Head {
                    format: FORMAT,
                    commit: tip.id,
                    history: Some(index),
                })?
                .into(),
                PutMode::Update(version).into(),
            )
            .await?;
        self.poisoned = false;
        Ok(PruneReport {
            removed_revisions: removed,
            retained_from: floor,
        })
    }
}
