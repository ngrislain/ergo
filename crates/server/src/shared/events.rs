//! One cross-process S3 poll per actively watched world per process. Slow clients
//! coalesce notifications instead of retaining an unbounded queue or snapshot pin.
use super::*;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::{
    convert::Infallible,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::watch;

#[derive(Clone)]
enum Observed {
    Revision {
        id: ContentId,
        sequence: u64,
        floor: u64,
        policy: Arc<Policy>,
    },
    Unavailable,
}
struct Hub {
    sender: watch::Sender<Observed>,
}
#[derive(Default)]
pub struct Hubs {
    active: Arc<Mutex<BTreeMap<String, Weak<Hub>>>>,
}
impl Hubs {
    fn subscribe(
        &self,
        id: &str,
        repo: Repository,
        initial: Observed,
        period: Duration,
    ) -> watch::Receiver<Observed> {
        let mut active = self.active.lock().unwrap();
        if let Some(hub) = active
            .get(id)
            .and_then(Weak::upgrade)
            .filter(|h| h.sender.receiver_count() > 0)
        {
            let newer = {
                let current = hub.sender.borrow();
                match (&*current, &initial) {
                    (
                        Observed::Revision {
                            sequence: old,
                            floor: old_floor,
                            ..
                        },
                        Observed::Revision {
                            sequence: new,
                            floor: new_floor,
                            ..
                        },
                    ) => (new, new_floor) > (old, old_floor),
                    _ => true,
                }
            };
            if newer {
                hub.sender.send_replace(initial.clone());
            }
            return hub.sender.subscribe();
        }
        let (sender, receiver) = watch::channel(initial);
        let hub = Arc::new(Hub { sender });
        active.insert(id.into(), Arc::downgrade(&hub));
        let registry = self.active.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            loop {
                tokio::select! {_ = hub.sender.closed()=>break,_=tokio::time::sleep(period)=>{}}
                let observed = async {
                    let snapshot = repo.checkout("main").await?;
                    let policy: Policy = snapshot.get(&root(0)).await?.ok_or_else(|| {
                        ergo_transactions::Error::Corrupt("missing policy".into())
                    })?;
                    let value = Observed::Revision {
                        id: snapshot.id().clone(),
                        sequence: snapshot.commit().sequence,
                        floor: snapshot.retained_from(),
                        policy: Arc::new(policy),
                    };
                    snapshot.close().await?;
                    Ok::<_, ergo_transactions::Error>(value)
                }
                .await
                .unwrap_or(Observed::Unavailable);
                hub.sender.send_if_modified(|old| {
                    let changed = match (&*old, &observed) {
                        (
                            Observed::Revision {
                                id: a, floor: af, ..
                            },
                            Observed::Revision {
                                id: b, floor: bf, ..
                            },
                        ) => a != b || af != bf,
                        (Observed::Unavailable, Observed::Unavailable) => false,
                        _ => true,
                    };
                    if changed {
                        *old = observed;
                    }
                    changed
                });
            }
            let mut active = registry.lock().unwrap();
            if active
                .get(&id)
                .and_then(Weak::upgrade)
                .is_some_and(|h| Arc::ptr_eq(&h, &hub))
            {
                active.remove(&id);
            }
        });
        receiver
    }
}
#[derive(Deserialize)]
pub(super) struct EventQuery {
    after: Option<ContentId>,
}
pub(super) async fn subscribe(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EventQuery>,
) -> SharedResult<Response> {
    let principal = reader(&s, &headers)?;
    let actor = principal.id.clone().unwrap();
    let (repo, snapshot, policy) = s.shared.authorized(&id, &principal).await?;
    let cursor = q.after.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    });
    let last_sequence = match &cursor {
        Some(id) => repo.revision_entry("main", id).await?.map(|e| e.sequence),
        None => None,
    };
    let reset = cursor.is_some() && last_sequence.is_none();
    let initial = Observed::Revision {
        id: snapshot.id().clone(),
        sequence: snapshot.commit().sequence,
        floor: snapshot.retained_from(),
        policy: Arc::new(policy),
    };
    snapshot.close().await?;
    let mut receiver = s
        .shared
        .hubs
        .subscribe(&id, repo, initial, s.shared.event_period);
    let auth = s.auth.clone();
    let stream = async_stream::stream! {
        let mut previous=cursor;let mut sequence=last_sequence;let mut reset=reset;
        loop{
            if auth.shared_principal(&headers).ok().and_then(|p|p.id).as_deref()!=Some(&actor){yield Ok::<_,Infallible>(Event::default().event("revoked").data("session expired"));break;}
            let observed=receiver.borrow_and_update().clone();
            match observed{
                Observed::Revision{id,sequence:now,floor,policy}=>{
                    if !policy.can_read(&actor){yield Ok(Event::default().event("revoked").data("permission revoked"));break;}
                    if previous.as_ref()!=Some(&id)||reset{
                        let gap=sequence.is_some_and(|n|now>n.saturating_add(1));let expired=sequence.is_some_and(|n|n.saturating_add(1)<floor);
                        let event=Event::default().event(if reset||expired{"reset"}else{"revision"}).id(id.as_str()).data(serde_json::json!({"revision":id,"sequence":now,"retained_from":floor,"coalesced":gap}).to_string());
                        previous=Some(id);sequence=Some(now);reset=false;yield Ok(event);
                    }
                }
                Observed::Unavailable=>yield Ok(Event::default().event("unavailable").data("storage or maintenance temporarily unavailable")),
            }
            tokio::select!{changed=receiver.changed()=>{if changed.is_err(){break;}},_=tokio::time::sleep(Duration::from_secs(15))=>{}}
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default().interval(Duration::from_secs(15)))
        .into_response())
}
