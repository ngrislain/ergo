//! Separate commits and audits, compatible members, one publication CAS.
use crate::*;
pub struct Submission {
    pub transaction: Transaction,
    pub actor: String,
    pub message: String,
}
fn unknown(
    errors: Vec<Option<Error>>,
    accepted: Vec<(usize, ContentId, Commit)>,
    branch: &str,
    detail: String,
) -> Vec<Result<CommitOutcome>> {
    let mut results: Vec<Option<Result<CommitOutcome>>> =
        errors.into_iter().map(|e| e.map(Err)).collect();
    for (i, id, _) in accepted {
        results[i] = Some(Err(Error::PublicationUnknown {
            branch: branch.into(),
            candidate: id,
            source: Box::new(Error::BatchStorageFailure(detail.clone())),
        }));
    }
    results.into_iter().map(Option::unwrap).collect()
}
impl Repository {
    pub async fn commit_batch(
        &self,
        mut submissions: Vec<Submission>,
        options: CommitOptions,
    ) -> Result<Vec<Result<CommitOutcome>>> {
        options.validate()?;
        if submissions.is_empty() || submissions.len() > 128 {
            return Err(Error::InvalidOptions);
        }
        if submissions.iter().any(|s| {
            let t = &s.transaction;
            t.edits.is_empty() && t.metadata.is_none() && t.attachments.is_empty()
        }) {
            return Err(Error::InvalidOptions);
        }
        let branch = submissions[0].transaction.branch.clone();
        if submissions.iter().any(|s| {
            s.transaction.branch != branch
                || s.transaction.tree.repo.prefix != self.prefix
                || !Arc::ptr_eq(&s.transaction.tree.repo.backend, &self.backend)
                || s.transaction.tree.repo.managed != self.managed
        }) {
            return Err(Error::MixedBatch);
        }
        let mut tree = Tree::new(self.clone());
        for s in &mut submissions {
            tree.absorb(std::mem::replace(
                &mut s.transaction.tree,
                Tree::new(self.clone()),
            ))?;
        }
        for attempt in 0..options.max_attempts {
            let (tip, version) = self.head(&branch).await?;
            if version.e_tag.is_none() && version.version.is_none() {
                return Err(Error::MissingVersion);
            }
            let mut root = tip.commit.root.clone();
            let mut parent = tip.id.clone();
            let mut sequence = tip.commit.sequence;
            let mut errors: Vec<Option<Error>> = (0..submissions.len()).map(|_| None).collect();
            let mut accepted = Vec::new();
            let mut uploads = tree.spool.map::<ContentId, ()>()?;
            let mut commit_bytes = 0u64;
            for (i, s) in submissions.iter().enumerate() {
                let tx = &s.transaction;
                let mut conflicts = Vec::new();
                for row in tx.observed.iter() {
                    let (path, expected) = row?;
                    if tree.region(root.as_ref(), &path).await? != expected {
                        conflicts.push(path.clone());
                    }
                }
                for row in tx.directories.iter() {
                    let (path, expected) = row?;
                    if tree.presence(root.as_ref(), &path).await? != expected {
                        conflicts.push(path.clone());
                    }
                }
                if !conflicts.is_empty() {
                    errors[i] = Some(Error::Conflict { paths: conflicts });
                    continue;
                }
                for row in tx.edits.iter() {
                    let (_, edit) = row?;
                    root = tree
                        .replace(
                            root.as_ref(),
                            &edit.path,
                            edit.value.as_ref(),
                            &edit.generation,
                        )
                        .await?;
                }
                sequence = sequence
                    .checked_add(1)
                    .ok_or_else(|| Error::Corrupt("sequence overflow".into()))?;
                let commit = Commit::from_records(
                    CommitHeader {
                        format: FORMAT,
                        transaction_id: tx.transaction_id.to_string(),
                        sequence,
                        parent: Some(parent.clone()),
                        base: Some(tx.base.id.clone()),
                        root: root.clone(),
                        actor: s.actor.clone(),
                        message: s.message.clone(),
                        metadata: tx.metadata.clone(),
                    },
                    tx.edits.len(),
                    tx.edits.iter().map(|row| row.map(|(_, edit)| edit)),
                    tx.attachments.len(),
                    tx.attachments.iter().map(|row| row.map(|(id, _)| id)),
                    self.commit_limits,
                )?;
                commit_bytes = commit_bytes
                    .checked_add(commit.encoded_len())
                    .ok_or_else(|| Error::Scratch("batch commit size overflow".into()))?;
                if commit_bytes > self.commit_limits.bytes as u64 {
                    return Err(Error::Scratch(
                        "batch commit records exceed encoded-byte budget".into(),
                    ));
                }
                let id = commit.content_id().clone();
                parent = id.clone();
                if let Some(root) = &root {
                    uploads.insert(root.clone(), ())?;
                }
                for row in tx.edits.iter() {
                    if let Some(id) = row?.1.value {
                        uploads.insert(id, ())?;
                    }
                }
                if let Some(id) = &tx.metadata {
                    uploads.insert(id.clone(), ())?;
                }
                for row in tx.attachments.iter() {
                    uploads.insert(row?.0, ())?;
                }
                accepted.push((i, id, commit));
            }
            if accepted.is_empty() {
                return Ok(errors
                    .into_iter()
                    .map(|e| Err(e.unwrap_or(Error::InvalidOptions)))
                    .collect());
            }
            tree.flush(
                None,
                uploads.iter().map(|row| row.map(|(id, _)| id)),
                options.upload_concurrency,
            )
            .await?;
            // The SDK's conditional create consumes one bounded encoded buffer;
            // do not buffer every batch member's commit or bypass PutMode::Create.
            for (_, _, commit) in &accepted {
                self.store_commit(commit).await?;
            }
            let entries: Vec<_> = accepted
                .iter()
                .map(|(_, id, c)| RevisionEntry {
                    revision: id.clone(),
                    sequence: c.sequence,
                })
                .collect();
            let history = history::append_many(self, tip.history.as_ref(), &entries).await?;
            let bytes = canonical_cbor(&Head {
                format: FORMAT,
                commit: parent.clone(),
                history: history.clone(),
            })?;
            for (i, _, _) in &accepted {
                if let Some(pin) = &submissions[*i].transaction.base.pin {
                    pin.hold();
                }
            }
            let result = self
                .backend
                .put_opts(
                    &self.head_path(&branch)?,
                    bytes.into(),
                    PutMode::Update(version.clone()).into(),
                )
                .await;
            let success = match result {
                Ok(_) => true,
                Err(
                    object_store::Error::Precondition { .. }
                    | object_store::Error::AlreadyExists { .. },
                ) => {
                    let (current, new_version) = match self.head(&branch).await {
                        Ok(v) => v,
                        Err(e) => return Ok(unknown(errors, accepted, &branch, e.to_string())),
                    };
                    let committed = match self.contains(current, &parent, sequence).await {
                        Ok(v) => v,
                        Err(e) => return Ok(unknown(errors, accepted, &branch, e.to_string())),
                    };
                    if committed || new_version != version {
                        for s in &submissions {
                            if let Some(pin) = &s.transaction.base.pin {
                                pin.settle();
                            }
                        }
                    } else {
                        return Ok(unknown(
                            errors,
                            accepted,
                            &branch,
                            "conditional conflict did not establish a publication fence".into(),
                        ));
                    }
                    committed
                }
                Err(e) => return Ok(unknown(errors, accepted, &branch, e.to_string())),
            };
            if success {
                for s in &submissions {
                    if let Some(pin) = &s.transaction.base.pin {
                        pin.settle();
                    }
                }
                let mut results: Vec<Option<Result<CommitOutcome>>> =
                    errors.into_iter().map(|e| e.map(Err)).collect();
                for (i, id, commit) in accepted {
                    results[i] = Some(Ok(CommitOutcome {
                        snapshot: Snapshot {
                            repo: self.clone(),
                            id,
                            commit: self.published_commit(commit),
                            pin: submissions[i].transaction.base.pin.clone(),
                            history: history.clone(),
                        },
                        attempts: attempt + 1,
                        rebases: attempt
                            + usize::from(submissions[i].transaction.base.id != tip.id),
                    }));
                }
                return Ok(results.into_iter().map(Option::unwrap).collect());
            }
            if attempt + 1 < options.max_attempts {
                options
                    .backoff(submissions[0].transaction.transaction_id, attempt)
                    .await;
            }
        }
        Err(Error::Contention {
            attempts: options.max_attempts,
        })
    }
}
