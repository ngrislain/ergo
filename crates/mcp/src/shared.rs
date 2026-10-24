//! Production workspaces are caches of an S3-backed assembly, not private forks.
use crate::publish::{Publisher, RemoteFailure};
use anyhow::{Context, Result, bail, ensure};
use ergo_world::WorldExport;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub origin: String,
    pub world_id: String,
    pub assembly_id: Option<String>,
    pub base_revision: String,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub external_dependencies: bool,
    #[serde(default)]
    pub receipt: Option<Value>,
}
pub fn state_path(path: &Path) -> PathBuf {
    path.with_extension("shared.json")
}
impl Publisher {
    pub async fn common_world(&self) -> Result<Option<String>> {
        let config = self.request(Method::GET, "/api/config", None).await?;
        config["common_world"]
            .as_str()
            .map(|id| {
                uuid::Uuid::parse_str(id)
                    .map(|id| id.to_string())
                    .map_err(Into::into)
            })
            .transpose()
    }
    pub async fn new_binding(&self, world: String) -> Result<Binding> {
        let head = self
            .request(Method::GET, &format!("/api/shared-worlds/{world}"), None)
            .await?;
        Ok(Binding {
            origin: self.url("/")?.to_string(),
            world_id: world,
            assembly_id: None,
            base_revision: head["revision"]
                .as_str()
                .context("Missing common revision")?
                .into(),
            pending: false,
            external_dependencies: false,
            receipt: None,
        })
    }
    pub async fn load_shared(&self, binding: &mut Binding, assembly: &str) -> Result<WorldExport> {
        ensure!(
            assembly.len() == 64 && assembly.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid shared assembly ID"
        );
        let workspace = self
            .request(
                Method::GET,
                &format!(
                    "/api/shared-worlds/{}/assemblies/{assembly}/workspace",
                    binding.world_id
                ),
                None,
            )
            .await?;
        ensure!(
            workspace["world_id"] == binding.world_id && workspace["assembly_id"] == assembly,
            "Wrong shared workspace"
        );
        let scene = serde_json::from_value(workspace["scene"].clone())?;
        binding.base_revision = workspace["base_revision"]
            .as_str()
            .context("Missing shared revision")?
            .into();
        binding.assembly_id = Some(assembly.into());
        binding.external_dependencies =
            workspace["external_dependencies"].as_bool().unwrap_or(true);
        binding.pending = false;
        binding.receipt = None;
        Ok(scene)
    }
    pub async fn locate_shared(&self, position: [f64; 3]) -> Result<Value> {
        ensure!(
            position
                .iter()
                .all(|p| p.is_finite() && p.abs() <= ergo_world::MAX_WORLD_COORD),
            "Invalid position"
        );
        self.request(
            Method::GET,
            &format!(
                "/api/common-world/locate?x={}&y={}&z={}",
                position[0], position[1], position[2]
            ),
            None,
        )
        .await
    }
    pub async fn commit_shared(
        &self,
        binding: &mut Binding,
        scene: WorldExport,
        local_revision: u32,
    ) -> Result<Value> {
        ensure!(
            !binding.pending,
            "Previous shared commit outcome is unresolved; reload/reconcile it before editing"
        );
        binding.pending = true;
        if let Some(assembly) = binding.assembly_id.clone() {
            let response = self
                .request(
                    Method::PUT,
                    &format!(
                        "/api/shared-worlds/{}/assemblies/{assembly}/workspace",
                        binding.world_id
                    ),
                    Some(json!({"base":binding.base_revision,"scene":scene})),
                )
                .await;
            let imported = match response {
                Ok(value) => value,
                Err(error) => {
                    if error
                        .downcast_ref::<RemoteFailure>()
                        .is_some_and(|e| (400..500).contains(&e.status))
                    {
                        binding.pending = false;
                    }
                    return Err(error);
                }
            };
            let accepted = imported["accepted"]["revision"]
                .as_str()
                .context("Missing accepted shared revision")?;
            let read = self
                .request(
                    Method::GET,
                    &format!(
                        "/api/shared-worlds/{}/assemblies/{assembly}/workspace?revision={accepted}",
                        binding.world_id
                    ),
                    None,
                )
                .await?;
            ensure!(
                read["base_revision"] == accepted && read["assembly_id"] == assembly,
                "Shared edit read-back failed"
            );
            binding.base_revision = accepted.into();
            binding.external_dependencies = read["external_dependencies"].as_bool().unwrap_or(true);
            let value = json!({"published":true,"publication_state":"confirmed","world_id":binding.world_id,
                "assembly_id":assembly,"accepted_revision":accepted,"audit":imported["accepted"]["audit"],
                "pieces":imported["pieces"],
                "view_url":self.url(&format!("/app?shared_world={}",binding.world_id))?.as_str()});
            binding.pending = false;
            binding.receipt = Some(value.clone());
            Ok(value)
        } else {
            let result = self
                .publish(
                    scene,
                    local_revision,
                    json!({"world_id":binding.world_id,"expected_local_revision":local_revision}),
                )
                .await;
            let value = result["structuredContent"].clone();
            if let Some(assembly) = value["assembly_id"].as_str() {
                binding.assembly_id = Some(assembly.into());
            }
            if result["isError"] == true {
                binding.pending = value["publication_state"] != "rejected"
                    && value["publication_state"] != "not_attempted";
                bail!("Shared edit was not confirmed: {value}");
            }
            binding.base_revision = value["accepted_revision"]
                .as_str()
                .context("Missing accepted revision")?
                .into();
            binding.pending = false;
            binding.receipt = Some(value.clone());
            Ok(value)
        }
    }
}
