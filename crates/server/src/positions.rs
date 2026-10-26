//! Authenticated last-position context. Actor identity comes only from auth.
use super::*;
use ergo_world::user_position::{PositionUpdate, UserPosition};
use object_store::{PutMode, UpdateVersion};
use sha2::{Digest, Sha256};

fn key(actor: &str) -> String {
    format!(
        "user-positions/{}.json",
        hex::encode(Sha256::digest(actor.as_bytes()))
    )
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn context(position: Option<UserPosition>) -> serde_json::Value {
    let ground = position.as_ref().map(UserPosition::ground_position);
    let transform = ground.map(|p| {
        [
            [1., 0., 0., p[0]],
            [0., 1., 0., p[1]],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ]
    });
    serde_json::json!({"position":position,"ground_position_world":ground,"model_transform":transform})
}
pub async fn get(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let actor = match s.auth.shared_principal(&headers).and_then(|p| p.required()) {
        Ok(actor) => actor,
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error":"Sign in to read your position"})),
            )
                .into_response());
        }
    };
    let position = match s.store.backend.get(&s.store.path(&key(&actor))).await {
        Ok(value) => {
            let position: UserPosition = serde_json::from_slice(&value.bytes().await?)?;
            position.validate()?;
            Some(position)
        }
        Err(object_store::Error::NotFound { .. }) => None,
        Err(error) => return Err(error.into()),
    };
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(context(position)),
    )
        .into_response())
}
pub async fn put(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(update): Json<PositionUpdate>,
) -> Result<Response, ApiError> {
    let actor = match s.auth.check_shared_edit(&headers) {
        Ok(actor) => actor,
        Err(_) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error":"Sign in to save your position"})),
            )
                .into_response());
        }
    };
    update.validate()?;
    if s.shared.common_world.as_deref() != Some(update.world_id.as_str()) {
        return Err(anyhow::anyhow!("Position must refer to the shared Sandbox").into());
    }
    let path = s.store.path(&key(&actor));
    for _ in 0..16 {
        let mode = match s.store.backend.get(&path).await {
            Ok(value) => {
                let version = UpdateVersion {
                    e_tag: value.meta.e_tag.clone(),
                    version: value.meta.version.clone(),
                };
                if version.e_tag.is_none() && version.version.is_none() {
                    return Err(
                        anyhow::anyhow!("Position storage requires conditional writes").into(),
                    );
                }
                let previous: UserPosition = serde_json::from_slice(&value.bytes().await?)?;
                previous.validate()?;
                if previous.context.client_session == update.client_session
                    && previous.context.sequence >= update.sequence
                {
                    return Ok((
                        [(header::CACHE_CONTROL, "private, no-store")],
                        Json(context(Some(previous))),
                    )
                        .into_response());
                }
                PutMode::Update(version)
            }
            Err(object_store::Error::NotFound { .. }) => PutMode::Create,
            Err(error) => return Err(error.into()),
        };
        let position = UserPosition::new(update.clone(), now());
        match s
            .store
            .backend
            .put_opts(&path, serde_json::to_vec(&position)?.into(), mode.into())
            .await
        {
            Ok(_) => {
                return Ok((
                    [(header::CACHE_CONTROL, "private, no-store")],
                    Json(context(Some(position))),
                )
                    .into_response());
            }
            Err(
                object_store::Error::Precondition { .. }
                | object_store::Error::AlreadyExists { .. },
            ) => tokio::task::yield_now().await,
            Err(error) => return Err(error.into()),
        }
    }
    Ok((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"error":"Position changed concurrently; retry later"})),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    async fn value(response: Response) -> serde_json::Value {
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .unwrap(),
        )
        .unwrap()
    }
    #[tokio::test]
    async fn private_positions_reject_out_of_order_updates_and_cross_account_reads() {
        let store = Store {
            backend: Arc::new(object_store::memory::InMemory::new()),
            prefix: String::new(),
            kind: "Memory".into(),
            measurements: Arc::new(Default::default()),
            static_tolerances: Default::default(),
        };
        let (auth, alice) = auth::Auth::test_session("alice");
        let (_, bob) = auth::Auth::test_session("bob");
        let mut shared = shared::SharedWorlds::from_env(store.clone()).unwrap();
        shared.common_world = Some("world".into());
        let state = AppState {
            parts: store.part_cache().unwrap(),
            ground: store.ground_cache().unwrap(),
            store,
            geo: Default::default(),
            library: None,
            auth,
            shared,
        };
        assert_eq!(
            get(State(state.clone()), HeaderMap::new())
                .await
                .map_err(|e| e.0)
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let update = PositionUpdate {
            world_id: "world".into(),
            position_world: [12., 34., 5.],
            view_target_world: [14., 35., 0.],
            navigation: "walk".into(),
            client_session: "browser".into(),
            sequence: 2,
        };
        assert_eq!(
            put(State(state.clone()), alice.clone(), Json(update.clone()))
                .await
                .map_err(|e| e.0)
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let mut older = update.clone();
        older.sequence = 1;
        older.position_world[0] = 1.;
        put(State(state.clone()), alice.clone(), Json(older))
            .await
            .map_err(|e| e.0)
            .unwrap();
        let own = value(
            get(State(state.clone()), alice.clone())
                .await
                .map_err(|e| e.0)
                .unwrap(),
        )
        .await;
        assert_eq!(
            own["ground_position_world"],
            serde_json::json!([12., 34., 0.])
        );
        assert_eq!(own["position"]["context"]["sequence"], 2);
        let other = value(
            get(State(state.clone()), bob)
                .await
                .map_err(|e| e.0)
                .unwrap(),
        )
        .await;
        assert!(other["position"].is_null());
        let mut invalid = update;
        invalid.world_id = "other-world".into();
        assert!(put(State(state), alice, Json(invalid)).await.is_err());
    }
}
