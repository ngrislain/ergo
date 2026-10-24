//! A bounded editable assembly in the common world. Success means server
//! acceptance/read-back; local buffers are never described as S3 commits.
use super::*;
use serde::Deserialize;
use serde_json::json;
use std::hash::{Hash, Hasher};

#[derive(Clone, Deserialize)]
pub(super) struct Binding {
    pub world_id: String,
    pub assembly_id: String,
    pub base_revision: String,
    #[serde(default)]
    pub external_dependencies: bool,
}
#[derive(Deserialize)]
struct Workspace {
    #[serde(flatten)]
    binding: Binding,
    scene: WorldExport,
}
#[derive(Clone, Deserialize)]
pub(super) struct Assembly {
    pub id: String,
    pub name: String,
}
#[derive(Deserialize)]
struct Page {
    items: Vec<Assembly>,
    next: Option<String>,
}

pub(super) fn refresh(handle: Handle) {
    let Some(id) = with_client(handle, |c| c.ui.common_world.get_untracked()) else {
        return;
    };
    spawn_local(async move {
        match json::<Page>(&format!("/api/shared-worlds/{id}/assemblies?limit=32")).await {
            Ok(page) => with_client(handle, |c| {
                c.ui.common_assemblies.set(page.items);
                c.ui.common_more.set(page.next);
            }),
            Err(error) => with_client(handle, |c| c.ui.fail(error)),
        }
    });
}
pub(super) fn more(handle: Handle) {
    let Some((id, cursor)) = with_client(handle, |c| {
        Some((
            c.ui.common_world.get_untracked()?,
            c.ui.common_more.get_untracked()?,
        ))
    }) else {
        return;
    };
    spawn_local(async move {
        let cursor = js_sys::encode_uri_component(&cursor)
            .as_string()
            .unwrap_or_default();
        match json::<Page>(&format!(
            "/api/shared-worlds/{id}/assemblies?limit=32&after={cursor}"
        ))
        .await
        {
            Ok(page) => with_client(handle, |c| {
                c.ui.common_assemblies
                    .update(|items| items.extend(page.items));
                c.ui.common_more.set(page.next);
            }),
            Err(error) => with_client(handle, |c| c.ui.fail(error)),
        }
    });
}
pub(super) fn open(handle: Handle, assembly: String) {
    let Some(id) = with_client(handle, |c| c.ui.common_world.get_untracked()) else {
        return;
    };
    if with_client(handle, |c| c.ui.busy.get_untracked()) {
        return;
    }
    with_client(handle, |c| c.ui.busy.set(true));
    spawn_local(async move {
        match json::<Workspace>(&format!(
            "/api/shared-worlds/{id}/assemblies/{assembly}/workspace"
        ))
        .await
        {
            Ok(workspace) => {
                if let Err(error) = open_draft(handle, workspace.scene) {
                    with_client(handle, |c| c.ui.fail(error));
                    return;
                }
                with_client(handle, |c| {
                    c.common_binding = Some(workspace.binding);
                    c.common_attempt = None;
                    c.ui.dirty.set(false);
                    c.ui.editing_shared.set(true);
                    c.ui.message
                        .set("Editing shared construction · accepted edits commit to S3".into());
                });
            }
            Err(error) => with_client(handle, |c| {
                c.ui.busy.set(false);
                c.ui.fail(error);
            }),
        }
    });
}

pub(super) fn pump(handle: Handle) {
    let request = with_client(handle, |c| {
        let world = c.ui.common_world.get_untracked()?;
        if !c.ui.signed_in.get_untracked()
            || c.shared.is_some()
            || c.playing
            || c.ui.busy.get_untracked()
            || c.common_fenced
            || !c.ui.dirty.get_untracked()
            || !c.complete()
        {
            return None;
        }
        let mut scene = c.scene().ok()?;
        scene.ground.clear();
        let payload = serde_json::to_string(&scene).ok()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        payload.hash(&mut hasher);
        let signature = hasher.finish();
        if c.common_attempt == Some(signature) {
            return None;
        }
        c.common_attempt = Some(signature);
        c.ui.busy.set(true);
        c.ui.message
            .set("Committing edit to the common world…".into());
        Some((world, c.common_binding.clone(), scene, c.generation))
    });
    let Some((world, binding, scene, generation)) = request else {
        return;
    };
    spawn_local(async move {
        let mut uncertain = false;
        let result: anyhow::Result<Binding> = async {
            #[derive(Deserialize)]
            struct Head {
                revision: String,
            }
            let base = if let Some(b) = &binding {
                b.base_revision.clone()
            } else {
                json::<Head>(&format!("/api/shared-worlds/{world}"))
                    .await?
                    .revision
            };
            let url = if let Some(b) = &binding {
                format!(
                    "/api/shared-worlds/{world}/assemblies/{}/workspace",
                    b.assembly_id
                )
            } else {
                format!("/api/shared-worlds/{world}/assemblies")
            };
            let request = if binding.is_some() {
                Request::put(&url)
            } else {
                Request::post(&url)
            };
            uncertain = true;
            let response = request
                .header("X-Ergo-Request", "1")
                .json(&json!({"base":base,"scene":scene}))?
                .send()
                .await?;
            if !response.ok() {
                uncertain = response.status() >= 500;
                anyhow::bail!(response.text().await?);
            }
            let accepted: serde_json::Value = response.json().await?;
            let assembly = accepted["assembly"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing shared assembly receipt"))?;
            let revision = accepted["accepted"]["revision"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing accepted revision"))?;
            let read = json::<Workspace>(&format!(
                "/api/shared-worlds/{world}/assemblies/{assembly}/workspace?revision={revision}"
            ))
            .await?;
            anyhow::ensure!(
                read.binding.base_revision == revision
                    && read.binding.world_id == world
                    && read.binding.assembly_id == assembly,
                "Shared edit could not be read back"
            );
            uncertain = false;
            Ok(read.binding)
        }
        .await;
        with_client(handle, |c| {
            if c.generation != generation {
                return;
            }
            c.ui.busy.set(false);
            match result {
                Ok(binding) => {
                    c.common_binding = Some(binding);
                    c.ui.dirty.set(false);
                    c.ui.editing_shared.set(true);
                    c.ui.message.set(
                        "Committed to the common world in S3 · author attribution recorded".into(),
                    );
                }
                Err(error) => {
                    c.common_fenced = uncertain;
                    c.ui.message
                        .set(format!("Preview only — not confirmed in S3: {error}"));
                    if uncertain {
                        c.ui.fail("Shared outcome is uncertain. Reopen/reconcile the construction before further commits; no automatic retry was sent.");
                    }
                }
            }
        });
        refresh(handle);
    });
}

pub(super) fn agent_handoff(handle: Handle) {
    let (position, assembly) = with_client(handle, |c| {
        let position = c
            .renderer
            .as_ref()
            .map(|r| ergo_world::coordinates::public(r.target))
            .unwrap_or_default();
        let assembly = c
            .common_binding
            .as_ref()
            .map(|b| b.assembly_id.clone())
            .or_else(|| {
                let selected = c.ui.selected.get_untracked()?.id;
                c.shared.as_ref()?.assemblies.get(&selected).cloned()
            });
        (position, assembly)
    });
    let mut url = format!("/login?x={}&y={}&z={}", position.x, position.y, position.z);
    if let Some(id) = assembly {
        url.push_str(&format!("&assembly={id}"));
    }
    url.push_str("#connect");
    if let Some(window) = web_sys::window() {
        let _ = window.location().set_href(&url);
    }
}
