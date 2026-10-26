//! Per-actor, persistent token bucket. Quota is independent of world size and
//! authorization; paid entitlement is server-owned metadata, not a client claim.
use super::{SharedError, SharedResult};
use crate::store::Store;
use axum::http::StatusCode;
use object_store::{PutMode, UpdateVersion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
pub struct Budget {
    pub bricks: u64,
    pub seconds: u64,
}
#[derive(Clone)]
pub struct Pacing {
    store: Store,
    budget: Option<Budget>,
}
#[derive(Serialize, Deserialize)]
struct Reservation {
    bricks: u64,
    at: u64,
}
#[derive(Serialize, Deserialize)]
struct Ledger {
    credits: u64,
    updated: u64,
    bricks: u64,
    seconds: u64,
    reservations: BTreeMap<String, Reservation>,
}
#[derive(Default, Serialize, Deserialize)]
struct Plan {
    unlimited: bool,
}
pub(super) struct Ticket {
    actor: String,
    id: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn account(actor: &str) -> String {
    hex::encode(Sha256::digest(actor.as_bytes()))
}
impl Ledger {
    fn new(budget: Budget, now: u64) -> Self {
        Self {
            credits: budget.bricks * budget.seconds,
            updated: now,
            bricks: budget.bricks,
            seconds: budget.seconds,
            reservations: BTreeMap::new(),
        }
    }
    fn refill(&mut self, budget: Budget, now: u64) {
        self.credits = self
            .credits
            .saturating_add(now.saturating_sub(self.updated).saturating_mul(self.bricks))
            .min(self.bricks * self.seconds);
        if self.bricks != budget.bricks || self.seconds != budget.seconds {
            self.credits = (self.credits / self.seconds).min(budget.bricks) * budget.seconds;
            self.bricks = budget.bricks;
            self.seconds = budget.seconds;
        }
        self.updated = self.updated.max(now);
        self.reservations
            .retain(|_, r| now.saturating_sub(r.at) < budget.seconds);
    }
}
impl Pacing {
    #[cfg(test)]
    pub(crate) fn test_budget(store: Store, bricks: u64, seconds: u64) -> Self {
        Self {
            store,
            budget: Some(Budget { bricks, seconds }),
        }
    }
    pub fn disabled(store: Store) -> Self {
        Self {
            store,
            budget: None,
        }
    }
    pub fn from_env(store: Store) -> anyhow::Result<Self> {
        let bricks = std::env::var("ERGO_EDIT_BRICKS_PER_PERIOD")
            .unwrap_or_else(|_| "5000".into())
            .parse()?;
        let seconds = std::env::var("ERGO_EDIT_PERIOD_SECONDS")
            .unwrap_or_else(|_| "86400".into())
            .parse()?;
        anyhow::ensure!(
            (1..=1_000_000).contains(&bricks) && (1..=31_536_000).contains(&seconds),
            "Invalid edit pacing budget"
        );
        Ok(Self {
            store,
            budget: Some(Budget { bricks, seconds }),
        })
    }
    pub async fn set_unlimited(&self, actor: &str, unlimited: bool) -> anyhow::Result<()> {
        anyhow::ensure!(!actor.is_empty() && actor.len() <= 256, "Invalid actor ID");
        self.store
            .put(
                &format!("account-plans/{}.json", account(actor)),
                serde_json::to_vec(&Plan { unlimited })?,
            )
            .await
    }
    pub(super) async fn unlimited(&self, actor: &str) -> SharedResult<bool> {
        match self
            .store
            .backend
            .get(
                &self
                    .store
                    .path(&format!("account-plans/{}.json", account(actor))),
            )
            .await
        {
            Ok(value) => Ok(serde_json::from_slice::<Plan>(
                &value.bytes().await.map_err(anyhow::Error::from)?,
            )
            .map_err(anyhow::Error::from)?
            .unlimited),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(error) => Err(anyhow::Error::from(error).into()),
        }
    }
    async fn read(&self, actor: &str, budget: Budget, now: u64) -> SharedResult<(Ledger, PutMode)> {
        let path = self
            .store
            .path(&format!("edit-pacing/{}.json", account(actor)));
        match self.store.backend.get(&path).await {
            Ok(value) => {
                let version = UpdateVersion {
                    e_tag: value.meta.e_tag.clone(),
                    version: value.meta.version.clone(),
                };
                if version.e_tag.is_none() && version.version.is_none() {
                    return Err(SharedError::status(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Pacing store does not support conditional updates",
                    ));
                }
                let ledger =
                    serde_json::from_slice(&value.bytes().await.map_err(anyhow::Error::from)?)
                        .map_err(anyhow::Error::from)?;
                Ok((ledger, PutMode::Update(version)))
            }
            Err(object_store::Error::NotFound { .. }) => {
                Ok((Ledger::new(budget, now), PutMode::Create))
            }
            Err(error) => Err(anyhow::Error::from(error).into()),
        }
    }
    pub async fn status(&self, actor: &str) -> SharedResult<serde_json::Value> {
        let god_mode = self.unlimited(actor).await?;
        if self.budget.is_none() || god_mode {
            return Ok(serde_json::json!({"unlimited":true,"god_mode":god_mode}));
        }
        let budget = self.budget.unwrap();
        let (mut ledger, _) = self.read(actor, budget, now()).await?;
        ledger.refill(budget, now());
        Ok(
            serde_json::json!({"unlimited":false,"god_mode":false,"remaining_brick_changes":ledger.credits/budget.seconds,"capacity":budget.bricks,"refill_period_seconds":budget.seconds}),
        )
    }
    pub(super) async fn reserve(&self, actor: &str, bricks: usize) -> SharedResult<Option<Ticket>> {
        self.reserve_at(actor, bricks, now()).await
    }
    async fn reserve_at(
        &self,
        actor: &str,
        bricks: usize,
        now: u64,
    ) -> SharedResult<Option<Ticket>> {
        if bricks == 0 || self.budget.is_none() || self.unlimited(actor).await? {
            return Ok(None);
        }
        let budget = self.budget.unwrap();
        let bricks = bricks as u64;
        if bricks > budget.bricks {
            return Err(SharedError::status(
                StatusCode::TOO_MANY_REQUESTS,
                "Edit exceeds the free pacing burst; split it into smaller edits or use an unlimited plan",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let path = self
            .store
            .path(&format!("edit-pacing/{}.json", account(actor)));
        for _ in 0..16 {
            let (mut ledger, mode) = self.read(actor, budget, now).await?;
            ledger.refill(budget, now);
            // A storage client can surface a precondition error after a hidden
            // successful write. Recognize our receipt before charging again.
            if ledger.reservations.contains_key(&id) {
                return Ok(Some(Ticket {
                    actor: actor.into(),
                    id,
                }));
            }
            let cost = bricks * budget.seconds;
            if ledger.credits < cost {
                let retry = (cost - ledger.credits).div_ceil(budget.bricks);
                let mut error = SharedError::status(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Free edit allowance exhausted; wait for refill or use an unlimited plan",
                );
                error.retry_after = Some(retry);
                return Err(error);
            }
            ledger.credits -= cost;
            ledger
                .reservations
                .insert(id.clone(), Reservation { bricks, at: now });
            let bytes = serde_json::to_vec(&ledger).map_err(anyhow::Error::from)?;
            match self
                .store
                .backend
                .put_opts(&path, bytes.into(), mode.into())
                .await
            {
                Ok(_) => {
                    return Ok(Some(Ticket {
                        actor: actor.into(),
                        id,
                    }));
                }
                Err(
                    object_store::Error::Precondition { .. }
                    | object_store::Error::AlreadyExists { .. },
                ) => tokio::task::yield_now().await,
                Err(error) => {
                    if self
                        .read(actor, budget, now)
                        .await
                        .is_ok_and(|(l, _)| l.reservations.contains_key(&id))
                    {
                        return Ok(Some(Ticket {
                            actor: actor.into(),
                            id,
                        }));
                    }
                    return Err(SharedError::status(
                        StatusCode::SERVICE_UNAVAILABLE,
                        &format!("Edit allowance reservation could not be confirmed: {error}"),
                    ));
                }
            }
        }
        Err(SharedError::status(
            StatusCode::SERVICE_UNAVAILABLE,
            "Edit allowance contention; try again",
        ))
    }
    pub(super) async fn refund(&self, ticket: Ticket) -> SharedResult<()> {
        let Some(budget) = self.budget else {
            return Ok(());
        };
        let path = self
            .store
            .path(&format!("edit-pacing/{}.json", account(&ticket.actor)));
        for _ in 0..16 {
            let (mut ledger, mode) = self.read(&ticket.actor, budget, now()).await?;
            ledger.refill(budget, now());
            let Some(reservation) = ledger.reservations.remove(&ticket.id) else {
                return Ok(());
            };
            ledger.credits = ledger
                .credits
                .saturating_add(reservation.bricks * budget.seconds)
                .min(budget.bricks * budget.seconds);
            match self
                .store
                .backend
                .put_opts(
                    &path,
                    serde_json::to_vec(&ledger)
                        .map_err(anyhow::Error::from)?
                        .into(),
                    mode.into(),
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(
                    object_store::Error::Precondition { .. }
                    | object_store::Error::AlreadyExists { .. },
                ) => {}
                Err(error) => return Err(anyhow::Error::from(error).into()),
            }
        }
        Err(SharedError::status(
            StatusCode::SERVICE_UNAVAILABLE,
            "Edit allowance refund contention",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn pacing() -> Pacing {
        Pacing::test_budget(
            Store {
                backend: Arc::new(object_store::memory::InMemory::new()),
                prefix: String::new(),
                kind: "Test".into(),
                measurements: Arc::new(Default::default()),
                static_tolerances: Default::default(),
            },
            5,
            100,
        )
    }
    #[tokio::test]
    async fn budget_is_persistent_per_actor_and_refills_over_time() {
        let pacing = pacing();
        assert!(pacing.reserve_at("alice", 5, 1000).await.unwrap().is_some());
        let error = pacing
            .clone()
            .reserve_at("alice", 1, 1000)
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.retry_after, Some(20));
        assert!(pacing.reserve_at("bob", 5, 1000).await.is_ok());
        assert!(pacing.reserve_at("alice", 1, 1020).await.is_ok());
        assert!(pacing.reserve_at("alice", 1, 1020).await.is_err());
        assert!(pacing.reserve_at("alice", 5, 1120).await.is_ok());
    }
    #[tokio::test]
    async fn concurrent_reservations_cannot_overspend() {
        let pacing = pacing();
        let results =
            futures::future::join_all((0..24).map(|_| pacing.reserve_at("alice", 1, 1000))).await;
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 5);
        assert!(pacing.reserve_at("alice", 1, 1000).await.is_err());
    }
    #[tokio::test]
    async fn paid_entitlement_bypasses_pacing_and_refunds_are_idempotent() {
        let pacing = pacing();
        let ticket = pacing.reserve("alice", 3).await.unwrap().unwrap();
        let duplicate = Ticket {
            actor: ticket.actor.clone(),
            id: ticket.id.clone(),
        };
        pacing.refund(ticket).await.unwrap();
        pacing.refund(duplicate).await.unwrap();
        assert_eq!(
            pacing.status("alice").await.unwrap()["remaining_brick_changes"],
            5
        );
        pacing.set_unlimited("alice", true).await.unwrap();
        assert!(pacing.reserve("alice", 100_000).await.unwrap().is_none());
        assert_eq!(pacing.status("alice").await.unwrap()["unlimited"], true);
        assert_eq!(pacing.status("bob").await.unwrap()["unlimited"], false);
        pacing.set_unlimited("alice", false).await.unwrap();
        assert_eq!(pacing.status("alice").await.unwrap()["unlimited"], false);
    }
}
