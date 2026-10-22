use super::*;

/// Trace schema links after old commits stop being retention roots. A hash in a
/// free-form message is not a link. Creation IDs select immutable Birth objects.
pub(super) fn blob_links(bytes: &[u8]) -> ergo_transactions::Result<Vec<ContentId>> {
    use ciborium::value::Value;
    let value: Value = ciborium::from_reader(bytes)
        .map_err(|e| ergo_transactions::Error::Encoding(e.to_string()))?;
    if let Some(items) = value.as_array()
        && items.first().and_then(Value::as_text) == Some("joint-birth-v1")
    {
        let map = items
            .get(3)
            .and_then(Value::as_map)
            .ok_or_else(|| ergo_transactions::Error::Encoding("invalid joint birth".into()))?;
        let mut ids = Vec::new();
        for key in ["a", "b"] {
            let id = map
                .iter()
                .find_map(|(k, v)| (k.as_text() == Some(key)).then_some(v))
                .and_then(Value::as_text)
                .ok_or_else(|| {
                    ergo_transactions::Error::Encoding("invalid joint birth endpoint".into())
                })?;
            ids.push(id.parse()?);
        }
        return Ok(ids);
    }
    let Some(map) = value.as_map() else {
        return Ok(Vec::new());
    };
    let get = |key: &str| {
        map.iter()
            .find_map(|(k, v)| (k.as_text() == Some(key)).then_some(v))
    };
    let mut links = Vec::new();
    let mut add = |v: Option<&Value>| -> ergo_transactions::Result<()> {
        if let Some(v) = v {
            links.push(
                v.as_text()
                    .ok_or_else(|| ergo_transactions::Error::Encoding("invalid schema CID".into()))?
                    .parse()?,
            );
        }
        Ok(())
    };
    if get("geometry").is_some() && get("replicas").is_some() {
        add(get("id"))?;
        add(get("assembly"))?;
        add(get("mesh"))?;
    } else if get("spec").is_some() && get("a").is_some() {
        add(get("id"))?;
        add(get("a"))?;
        add(get("b"))?;
    } else if get("nonce").is_some() && get("initial").is_some() {
        add(get("assembly"))?;
        add(get("mesh"))?;
    } else if get("meshes").is_some() && get("id").is_some() {
        add(get("id"))?;
        if let Some(meshes) = get("meshes").and_then(Value::as_map) {
            for (_, v) in meshes {
                add(Some(v))?;
            }
        }
    } else if get("geo").is_some() && get("model").is_some() {
        add(get("model"))?;
    } else if get("policy_hash").is_some() {
        add(get("policy_hash"))?;
        add(get("model"))?;
        for key in ["changed_pieces", "affected_pieces"] {
            if let Some(ids) = get(key).and_then(Value::as_array) {
                for id in ids {
                    add(Some(id))?;
                }
            }
        }
    }
    Ok(links)
}

#[derive(Deserialize)]
pub(super) struct RetentionRequest {
    base: ContentId,
    keep_last: Option<u64>,
}
pub(super) async fn set(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<RetentionRequest>,
) -> SharedResult<Json<ContentId>> {
    let actor = writer(&s, &headers)?;
    let mut ws = s.shared.working(&id, &actor, &request.base).await?;
    if ws.policy.owner != actor {
        return Err(SharedError::forbidden());
    }
    ws.policy.keep_last = request.keep_last;
    ws.policy.validate()?;
    ws.tx.put(&root(0), &ws.policy).await?;
    Ok(Json(
        s.shared
            .prepared(
                &id,
                &actor,
                request.base,
                "Update opt-in history retention".into(),
                BTreeSet::new(),
                BTreeSet::new(),
                None,
                ws,
            )?
            .publish_with(&s.shared)
            .await?
            .revision,
    ))
}

impl SharedWorlds {
    pub async fn prune(&self, id: &str) -> Result<ergo_transactions::PruneReport> {
        let repo = self.repo(id)?;
        let mut maintenance = repo.maintenance().await?;
        let policy: Policy = maintenance
            .read("main", &root(0))
            .await?
            .context("Missing policy")?;
        let keep = match policy.keep_last {
            Some(n) => n,
            None => {
                maintenance.finish().await?;
                anyhow::bail!(
                    "History retention is disabled; an owner must configure keep_last first"
                );
            }
        };
        tracing::info!(
            world = id,
            token = maintenance.token(),
            "History retention barrier acquired"
        );
        let report = maintenance.prune_history("main", keep).await?;
        maintenance.finish().await?;
        Ok(report)
    }
}
