//! Opt-in bounded per-world coalescing; each member keeps its own commit/audit.
use super::*;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
struct Request {
    submission: ergo_transactions::Submission,
    reply: oneshot::Sender<SharedResult<ergo_transactions::CommitOutcome>>,
}
type ActiveQueues = BTreeMap<String, (String, mpsc::Sender<Request>)>;
#[derive(Default)]
pub struct Queues {
    active: Arc<Mutex<ActiveQueues>>,
}
impl Queues {
    async fn submit(
        &self,
        id: &str,
        repo: Repository,
        submission: ergo_transactions::Submission,
    ) -> SharedResult<ergo_transactions::CommitOutcome> {
        let (reply, receive) = oneshot::channel();
        let mut request = Request { submission, reply };
        loop {
            let sender = {
                let mut active = self.active.lock().unwrap();
                if let Some((_, sender)) = active.get(id) {
                    sender.clone()
                } else {
                    let (sender, mut receiver) = mpsc::channel::<Request>(32);
                    let token = uuid::Uuid::new_v4().to_string();
                    active.insert(id.into(), (token.clone(), sender.clone()));
                    let registry = self.active.clone();
                    let world = id.to_owned();
                    let repo = repo.clone();
                    tokio::spawn(async move {
                        loop {
                            let first = match tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                receiver.recv(),
                            )
                            .await
                            {
                                Ok(Some(v)) => v,
                                _ => break,
                            };
                            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                            let mut requests = vec![first];
                            while requests.len() < 16 {
                                match receiver.try_recv() {
                                    Ok(r) => requests.push(r),
                                    Err(_) => break,
                                }
                            }
                            let (mut submissions, mut replies) = (Vec::new(), Vec::new());
                            for r in requests {
                                submissions.push(r.submission);
                                replies.push(r.reply);
                            }
                            match repo
                                .commit_batch(submissions, CommitOptions::default())
                                .await
                            {
                                Ok(results) => {
                                    for (reply, result) in replies.into_iter().zip(results) {
                                        let _ = reply.send(result.map_err(SharedError::from));
                                    }
                                }
                                Err(error) => {
                                    let error = SharedError::from(error);
                                    for reply in replies {
                                        let _ = reply.send(Err(error.clone()));
                                    }
                                }
                            }
                        }
                        let mut active = registry.lock().unwrap();
                        if active.get(&world).is_some_and(|(t, _)| t == &token) {
                            active.remove(&world);
                        }
                    });
                    sender
                }
            };
            match sender.try_send(request) {
                Ok(()) => break,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    return Err(SharedError::status(
                        StatusCode::TOO_MANY_REQUESTS,
                        "World commit queue is full",
                    ));
                }
                Err(mpsc::error::TrySendError::Closed(r)) => {
                    request = r;
                    let mut active = self.active.lock().unwrap();
                    if active.get(id).is_some_and(|(_, s)| s.same_channel(&sender)) {
                        active.remove(id);
                    }
                }
            }
        }
        receive.await.map_err(|_| {
            SharedError::status(
                StatusCode::SERVICE_UNAVAILABLE,
                "Commit worker stopped; publication may require reconciliation",
            )
        })?
    }
}
impl Prepared {
    pub(super) async fn publish_with(self, service: &SharedWorlds) -> SharedResult<Accepted> {
        if !service.group_commit {
            return self.publish().await;
        }
        let result = service
            .queues
            .submit(
                &self.id,
                service.repo(&self.id)?,
                ergo_transactions::Submission {
                    transaction: self.tx,
                    actor: self.actor,
                    message: self.message,
                },
            )
            .await?;
        Ok(Accepted {
            id: self.id,
            revision: result.snapshot.id().clone(),
            attempts: result.attempts,
            rebases: result.rebases,
            audit: self.audit,
            joint_ids: self.joint_ids,
        })
    }
}
