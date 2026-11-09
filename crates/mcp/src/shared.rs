//! Production workspaces are caches of an S3-backed assembly, not private forks.
use crate::publish::{Publisher, RemoteFailure};
use anyhow::{Context, Result, bail, ensure};
use ergo_world::WorldExport;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub format: String,
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
    #[serde(default)]
    pub versions: BTreeMap<u32, String>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub total_pieces: Option<usize>,
    #[serde(default = "first_id")]
    pub next_local_id: u64,
    #[serde(default = "first_id")]
    pub next_group_id: u64,
    #[serde(default)]
    pub blocked_transform_groups: std::collections::BTreeSet<u32>,
}
fn first_id() -> u64 {
    1
}
pub fn state_path(path: &Path) -> PathBuf {
    path.with_extension("shared.json")
}
pub fn load_binding(path: &Path) -> Result<Binding> {
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        value["format"] == "ergo-workspace-binding-2",
        "Retired shared cache: stop the MCP client and run scripts/migrate-authoring-cache.py WORKSPACE --token-file TOKEN --apply against the upgraded service. Pending outcomes must be reconciled first."
    );
    Ok(serde_json::from_value(value)?)
}
impl Publisher {
    pub async fn common_world(&self) -> Result<Option<String>> {
        let config = self.request(Method::GET, "/api/config", None).await?;
        if config["common_world"].is_string() {
            ensure!(
                config["authoring_format"] == "ergo-workspace-2"
                    && config["edit_format"] == ergo_world::authoring::FORMAT,
                "Server/client authoring protocols differ; deploy matching paged-authoring server/native/WASM before attaching this client"
            );
        }
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
            format: "ergo-workspace-binding-2".into(),
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
            versions: BTreeMap::new(),
            next: None,
            total_pieces: None,
            next_local_id: 1,
            next_group_id: 1,
            blocked_transform_groups: Default::default(),
        })
    }
    pub async fn load_shared_file(
        &self,
        binding: &mut Binding,
        assembly: &str,
        _path: Option<&Path>,
    ) -> Result<WorldExport> {
        ensure!(
            binding.format == "ergo-workspace-binding-2",
            "Retired shared cache; run scripts/migrate-authoring-cache.py on this workspace before opening it"
        );
        ensure!(
            assembly.len() == 64 && assembly.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid shared assembly ID"
        );
        let endpoint = format!(
            "/api/shared-worlds/{}/assemblies/{assembly}/workspace",
            binding.world_id
        );
        let workspace = self.request(Method::GET, &endpoint, None).await?;
        ensure!(
            workspace["world_id"] == binding.world_id && workspace["assembly_id"] == assembly,
            "Wrong shared workspace"
        );
        ensure!(
            workspace["format"] == "ergo-workspace-2",
            "Server requires the paged authoring protocol"
        );
        let scene = WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: workspace["name"]
                .as_str()
                .context("Missing assembly name")?
                .into(),
            geo: serde_json::from_value(workspace["geo"].clone())?,
            bricks: vec![],
            meshes: Default::default(),
            joints: vec![],
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
            model: None,
        };
        binding.base_revision = workspace["base_revision"]
            .as_str()
            .context("Missing shared revision")?
            .into();
        binding.assembly_id = Some(assembly.into());
        binding.external_dependencies =
            workspace["external_dependencies"].as_bool().unwrap_or(true);
        binding.pending = false;
        binding.receipt = None;
        binding.versions.clear();
        binding.next = None;
        binding.total_pieces = workspace["piece_count"].as_u64().map(|n| n as usize);
        binding.next_local_id = workspace["next_local_id"]
            .as_u64()
            .context("Missing ID allocation floor")?;
        binding.next_group_id = workspace["next_group_id"]
            .as_u64()
            .context("Missing group ID floor")?;
        binding.blocked_transform_groups.clear();
        Ok(scene)
    }
    pub async fn load_page(
        &self,
        binding: &mut Binding,
        args: &Value,
    ) -> Result<(WorldExport, Value)> {
        let assembly = binding
            .assembly_id
            .as_ref()
            .context("Open a shared construction first")?;
        let mut url = self.url(&format!(
            "/api/shared-worlds/{}/assemblies/{assembly}/workspace/pages",
            binding.world_id
        ))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("revision", &binding.base_revision);
            if let Some(cursor) = args["cursor"].as_str() {
                q.append_pair("cursor", cursor);
            }
            if let Some(limit) = args["limit"].as_u64() {
                q.append_pair("limit", &limit.to_string());
            }
            if let Some(bounds) = args.get("bounds") {
                q.append_pair("bounds", &bounds.to_string());
            }
            if let Some(ids) = args.get("ids") {
                q.append_pair("ids", &ids.to_string());
            }
        }
        let endpoint = format!("{}?{}", url.path(), url.query().unwrap_or_default());
        let mut page = self.request(Method::GET, &endpoint, None).await?;
        ensure!(
            page["format"] == "ergo-authoring-page-1"
                && page["world_id"] == binding.world_id
                && page["assembly_id"] == *assembly
                && page["base_revision"] == binding.base_revision,
            "Wrong authoring page"
        );
        let mut meshes = BTreeMap::new();
        let mut mesh_bytes = 0usize;
        for (part, cid) in page["mesh_ids"]
            .as_object()
            .context("Missing mesh manifest")?
        {
            let cid = cid.as_str().context("Invalid mesh ID")?;
            let local = page["bricks"]
                .as_array()
                .context("Missing piece page")?
                .iter()
                .find(|b| b["part"].as_str() == Some(part))
                .and_then(|b| b["id"].as_u64())
                .context("Mesh has no page member")?;
            let mesh=self.request(Method::GET,&format!("/api/shared-worlds/{}/assemblies/{assembly}/workspace/meshes/{cid}?revision={}&piece={local}",binding.world_id,binding.base_revision),None).await?;
            mesh_bytes = mesh_bytes
                .checked_add(serde_json::to_vec(&mesh)?.len())
                .context("Geometry budget overflow")?;
            ensure!(
                mesh_bytes <= 32 * 1024 * 1024,
                "Page geometry exceeds the 32 MiB resident budget; request a smaller page"
            );
            meshes.insert(part.clone(), serde_json::from_value(mesh)?);
        }
        let bricks = serde_json::from_value(page["bricks"].take())?;
        let scene = WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: page["name"].as_str().context("Missing name")?.into(),
            geo: serde_json::from_value(page["geo"].clone())?,
            bricks,
            meshes,
            joints: vec![],
            fixed_bricks: vec![],
            ground: vec![],
            groups: vec![],
            model: None,
        };
        binding.versions = serde_json::from_value(page["versions"].clone())?;
        binding.blocked_transform_groups.clear();
        binding.next = page["next"].as_str().map(str::to_owned);
        let details = json!({"world_id":binding.world_id,"assembly_id":assembly,"base_revision":binding.base_revision,"loaded_pieces":scene.bricks.len(),"total_pieces":binding.total_pieces,"next":binding.next,"partial":true,"external_dependencies":true,"scope":"loaded_page_only"});
        Ok((scene, details))
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
    pub async fn list_group_headers(&self, binding: &Binding, args: &Value) -> Result<Value> {
        let assembly = binding
            .assembly_id
            .as_ref()
            .context("Open a shared construction first")?;
        let mut url = self.url(&format!(
            "/api/shared-worlds/{}/assemblies/{assembly}/workspace/groups",
            binding.world_id
        ))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("revision", &binding.base_revision);
            if let Some(cursor) = args["cursor"].as_str() {
                q.append_pair("cursor", cursor);
            }
            if let Some(limit) = args["limit"].as_u64() {
                q.append_pair("limit", &limit.to_string());
            }
        }
        let value = self
            .request(
                Method::GET,
                &format!("{}?{}", url.path(), url.query().unwrap_or_default()),
                None,
            )
            .await?;
        ensure!(
            value["world_id"] == binding.world_id
                && value["assembly_id"] == *assembly
                && value["base_revision"] == binding.base_revision,
            "Wrong group header page"
        );
        Ok(value)
    }

    pub async fn load_group(
        &self,
        binding: &mut Binding,
        group: u32,
    ) -> Result<(WorldExport, Value)> {
        let assembly = binding
            .assembly_id
            .as_ref()
            .context("Open a shared construction first")?
            .clone();
        let value=self.request(Method::GET,&format!("/api/shared-worlds/{}/assemblies/{assembly}/workspace/groups/{group}?revision={}",binding.world_id,binding.base_revision),None).await?;
        ensure!(
            value["assembly_id"] == assembly
                && value["world_id"] == binding.world_id
                && value["base_revision"] == binding.base_revision
                && value["root_group_id"] == group
                && value["group_complete"] == true,
            "Wrong group closure"
        );
        let ids: Vec<u32> = serde_json::from_value(value["piece_ids"].clone())?;
        ensure!(
            ids.len() <= ergo_world::authoring::RESIDENT_PIECES,
            "Group exceeds resident piece limit"
        );
        let mut scene = WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: value["name"]
                .as_str()
                .context("Missing group assembly name")?
                .into(),
            geo: serde_json::from_value(value["geo"].clone())?,
            bricks: Vec::new(),
            meshes: Default::default(),
            joints: serde_json::from_value(value["joints"].clone())?,
            fixed_bricks: serde_json::from_value(value["fixed_bricks"].clone())?,
            groups: serde_json::from_value(value["groups"].clone())?,
            model: None,
            ground: Vec::new(),
        };
        let mut versions = BTreeMap::new();
        for chunk in ids.chunks(128) {
            let mut page_binding = binding.clone();
            let (part, _) = self
                .load_page(&mut page_binding, &json!({"ids":chunk,"limit":128}))
                .await?;
            ensure!(
                page_binding.next.is_none() && part.bricks.len() == chunk.len(),
                "Incomplete group piece page"
            );
            scene.bricks.extend(part.bricks);
            versions.extend(page_binding.versions);
            for (name, mesh) in part.meshes {
                if let Some(old) = scene.meshes.get(&name) {
                    ensure!(
                        ergo_world::authoring::version(old)?
                            == ergo_world::authoring::version(&mesh)?,
                        "Group contains incompatible part meshes"
                    );
                }
                scene.meshes.insert(name, mesh);
            }
            ensure!(
                serde_json::to_vec(&scene)?.len() <= 32 * 1024 * 1024,
                "Group exceeds resident geometry budget"
            );
        }
        ergo_world::groups::validate(&scene.bricks, &scene.groups)?;
        ergo_world::constraints::validate_joints(
            &scene.bricks,
            &scene.joints,
            &scene.fixed_bricks,
        )?;
        binding.versions = versions;
        binding.next = None;
        binding.blocked_transform_groups =
            serde_json::from_value(value["blocked_transform_groups"].clone())?;
        Ok((
            scene,
            json!({"assembly_id":assembly,"base_revision":binding.base_revision,"root_group_id":group,"loaded_pieces":ids.len(),"loaded_groups":value["groups"].as_array().map(Vec::len),"group_complete":true,"physical_domain_complete":false,"blocked_transform_groups":binding.blocked_transform_groups,"partial":true}),
        ))
    }
    pub async fn commit_shared(
        &self,
        binding: &mut Binding,
        before: &WorldExport,
        scene: WorldExport,
        local_revision: u32,
    ) -> Result<Value> {
        ensure!(
            !binding.pending,
            "Previous shared commit outcome is unresolved; reload/reconcile it before editing"
        );
        let delta = if binding.assembly_id.is_some() {
            Some(ergo_world::authoring::Delta::between(
                before,
                &scene,
                &binding.versions,
            )?)
        } else {
            None
        };
        binding.pending = true;
        if let Some(assembly) = binding.assembly_id.clone() {
            let response = self
                .request(
                    Method::PATCH,
                    &format!(
                        "/api/shared-worlds/{}/assemblies/{assembly}/workspace",
                        binding.world_id
                    ),
                    Some(json!({"base":binding.base_revision,"delta":delta})),
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
            binding.next_group_id = read["next_group_id"]
                .as_u64()
                .context("Missing group ID floor")?;
            binding.total_pieces = read["piece_count"].as_u64().map(|n| n as usize);
            binding.external_dependencies = read["external_dependencies"].as_bool().unwrap_or(true);
            let value = json!({"published":true,"publication_state":"confirmed","world_id":binding.world_id,
                "assembly_id":assembly,"accepted_revision":accepted,"audit":imported["accepted"]["audit"],
                "pieces":imported["pieces"],
                "view_url":self.url(&format!("/app?shared_world={}",binding.world_id))?.as_str()});
            self.refresh_versions(binding, &scene, &imported["pieces"])
                .await?;
            binding.pending = false;
            binding.next = None;
            binding.receipt = Some(value.clone());
            Ok(value)
        } else {
            let result = self
                .publish(
                    scene.clone(),
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
            self.refresh_versions(binding, &scene, &value["pieces"])
                .await?;
            binding.total_pieces = Some(scene.bricks.len());
            binding.next_group_id = scene
                .groups
                .iter()
                .map(|g| u64::from(g.id) + 1)
                .max()
                .unwrap_or(1);
            binding.external_dependencies = true;
            binding.pending = false;
            binding.receipt = Some(value.clone());
            Ok(value)
        }
    }
    async fn refresh_versions(
        &self,
        binding: &mut Binding,
        scene: &WorldExport,
        pieces: &Value,
    ) -> Result<()> {
        let present: std::collections::BTreeSet<_> = scene.bricks.iter().map(|b| b.id).collect();
        binding.versions.retain(|id, _| present.contains(id));
        for (local, global) in pieces
            .as_object()
            .context("Missing accepted piece identities")?
        {
            let local = local.parse::<u32>()?;
            if !present.contains(&local) {
                continue;
            }
            let global = global.as_str().context("Invalid piece identity")?;
            let record = self
                .request(
                    Method::GET,
                    &format!(
                        "/api/shared-worlds/{}/pieces/{global}?revision={}",
                        binding.world_id, binding.base_revision
                    ),
                    None,
                )
                .await?;
            ensure!(
                record["id"].as_str() == Some(global)
                    && record["assembly"].as_str() == binding.assembly_id.as_deref(),
                "Accepted piece read-back identity mismatch"
            );
            binding
                .versions
                .insert(local, ergo_world::authoring::version(&record)?);
        }
        binding.next_local_id = binding.next_local_id.max(
            scene
                .bricks
                .iter()
                .map(|b| u64::from(b.id) + 1)
                .max()
                .unwrap_or(1),
        );
        Ok(())
    }
}
