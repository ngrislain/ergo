use anyhow::{Context, Result, ensure};
use ergo_world::{BlockInfo, SCHEMA_VERSION, WorldExport, WorldManifest, bounds, partition};
use futures::TryStreamExt;
use object_store::{
    ObjectStore,
    aws::{AmazonS3Builder, S3ConditionalPut},
    path::Path,
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, sync::Arc};

#[derive(Clone)]
pub struct Store {
    pub backend: Arc<dyn ObjectStore>,
    pub(crate) prefix: String,
    pub kind: String,
    pub(crate) measurements: Arc<ergo_world::physical::ConnectionMeasurements>,
    pub(crate) static_tolerances: ergo_world::validation::StaticTolerances,
}

impl Store {
    pub fn part_cache(&self) -> Result<ergo_assets::PartCache> {
        ergo_assets::PartCache::new(self.backend.clone(), self.prefix.clone())
    }
    pub fn ground_cache(&self) -> Result<ergo_assets::ground::GroundCache> {
        ergo_assets::ground::GroundCache::new(self.backend.clone(), self.prefix.clone())
    }
    pub fn from_env() -> Result<Self> {
        let defaults = ergo_world::validation::StaticTolerances::default();
        let number = |key: &str, default: f64| -> Result<f64> {
            Ok(env::var(key)
                .ok()
                .map(|s| s.parse::<f64>())
                .transpose()?
                .unwrap_or(default))
        };
        let static_tolerances = ergo_world::validation::StaticTolerances {
            position_brick_units: number(
                "ERGO_STATIC_POSITION_EPSILON",
                defaults.position_brick_units,
            )?,
            rotation_radians: number("ERGO_STATIC_ROTATION_EPSILON", defaults.rotation_radians)?,
            force_newtons: number("ERGO_STATIC_FORCE_EPSILON_N", defaults.force_newtons)?,
            torque_newton_meters: number(
                "ERGO_STATIC_TORQUE_EPSILON_NM",
                defaults.torque_newton_meters,
            )?,
        };
        static_tolerances.validate()?;
        let measurements: ergo_world::physical::ConnectionMeasurements =
            if let Ok(path) = env::var("ERGO_CONNECTION_MEASUREMENTS") {
                serde_json::from_slice(&std::fs::read(path)?)?
            } else {
                Default::default()
            };
        measurements.validate()?;
        let measurements = Arc::new(measurements);
        if let Ok(bucket) = env::var("S3_BUCKET") {
            let backend = AmazonS3Builder::from_env()
                .with_bucket_name(bucket)
                .with_region(env::var("S3_REGION").unwrap_or_else(|_| "fr-par".into()))
                .with_endpoint(
                    env::var("S3_ENDPOINT")
                        .unwrap_or_else(|_| "https://s3.fr-par.scw.cloud".into()),
                )
                .with_virtual_hosted_style_request(false)
                .with_conditional_put(S3ConditionalPut::ETagMatch)
                .build()
                .context("Invalid S3 configuration")?;
            let prefix = env::var("S3_PREFIX")
                .unwrap_or_else(|_| "ergo".into())
                .trim_matches('/')
                .to_owned();
            Ok(Self {
                backend: Arc::new(backend),
                prefix,
                kind: "Scaleway S3".into(),
                measurements,
                static_tolerances,
            })
        } else {
            let dir = env::var("ERGO_DATA_DIR").unwrap_or_else(|_| "data".into());
            std::fs::create_dir_all(&dir)?;
            Ok(Self {
                backend: Arc::new(ergo_transactions::local::LocalCasStore::new(dir)?),
                prefix: String::new(),
                kind: "Local object store".into(),
                measurements,
                static_tolerances,
            })
        }
    }

    pub(crate) fn path(&self, key: &str) -> Path {
        Path::from(if self.prefix.is_empty() {
            key.into()
        } else {
            format!("{}/{key}", self.prefix)
        })
    }

    pub fn calibrate(&self, scene: &mut WorldExport) -> Result<()> {
        self.measurements.apply(&scene.bricks, &mut scene.joints)
    }

    pub async fn get(&self, key: &str) -> Result<Vec<u8>> {
        Ok(self
            .backend
            .get(&self.path(key))
            .await?
            .bytes()
            .await?
            .to_vec())
    }

    pub async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<()> {
        self.backend.put(&self.path(key), bytes.into()).await?;
        Ok(())
    }

    pub async fn manifests(
        &self,
        principal: &crate::auth::Principal,
    ) -> Result<Vec<WorldManifest>> {
        let mut objects = Vec::new();
        for owner in std::iter::once("examples").chain(principal.id.as_deref()) {
            let mut listed: Vec<_> = self
                .backend
                .list(Some(&self.path(&format!("worlds/{owner}/"))))
                .try_collect()
                .await?;
            objects.append(&mut listed);
        }
        let mut worlds = Vec::new();
        for object in objects {
            if object.location.as_ref().ends_with("/manifest.json") {
                let bytes = self.backend.get(&object.location).await?.bytes().await?;
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                if value.get("schema").and_then(serde_json::Value::as_u64)
                    == Some(SCHEMA_VERSION as u64)
                {
                    worlds.push(serde_json::from_value(value)?);
                }
            }
        }
        worlds.sort_by(|a: &WorldManifest, b| a.name.cmp(&b.name));
        Ok(worlds)
    }

    pub async fn manifest(
        &self,
        id: &str,
        principal: &crate::auth::Principal,
    ) -> Result<WorldManifest> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "Invalid world ID");
        for owner in std::iter::once("examples").chain(principal.id.as_deref()) {
            match self
                .backend
                .get(&self.path(&format!("worlds/{owner}/{id}/manifest.json")))
                .await
            {
                Ok(result) => {
                    let value: serde_json::Value = serde_json::from_slice(&result.bytes().await?)?;
                    ensure!(
                        value.get("schema").and_then(serde_json::Value::as_u64)
                            == Some(SCHEMA_VERSION as u64),
                        "Older world coordinate schema; reimport or explicitly migrate it"
                    );
                    let world: WorldManifest = serde_json::from_value(value)?;
                    ensure!(principal.can_read(&world), "Construction is private");
                    return Ok(world);
                }
                Err(object_store::Error::NotFound { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        anyhow::bail!("Construction is absent or private")
    }

    pub async fn user(&self, id: &str) -> Result<Option<crate::auth::User>> {
        ensure!(
            id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Invalid user ID"
        );
        match self
            .backend
            .get(&self.path(&format!("users/{id}.json")))
            .await
        {
            Ok(result) => Ok(Some(serde_json::from_slice(&result.bytes().await?)?)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn put_user(&self, user: &crate::auth::User) -> Result<()> {
        self.put(
            &format!("users/{}.json", user.id),
            serde_json::to_vec(user)?,
        )
        .await
    }

    async fn object<T: Serialize>(&self, value: &T) -> Result<String> {
        let bytes = encode(value)?;
        let hash = hex::encode(Sha256::digest(&bytes));
        self.put(&format!("objects/{hash}.cbor"), bytes).await?;
        Ok(hash)
    }

    pub async fn save(
        &self,
        mut scene: WorldExport,
        source: String,
        notes: Vec<String>,
        owner: String,
    ) -> Result<WorldManifest> {
        let measurements = self.measurements.clone();
        let example = owner == "examples";
        let tolerances = self.static_tolerances;
        let (scene, validation) = tokio::task::spawn_blocking(move || -> Result<_> {
            let pairs:std::collections::HashSet<_>=scene.joints.iter().map(|j|(j.a.min(j.b),j.a.max(j.b))).collect();
            scene.joints.extend(ergo_world::constraints::inferred_joints(&scene.bricks).into_iter().filter(|j|!pairs.contains(&(j.a.min(j.b),j.a.max(j.b)))));
            if !example {measurements.apply(&scene.bricks,&mut scene.joints)?;}
            let mut validation = ergo_world::validation::validate_static_with_tolerances(&mut scene,tolerances)?;
            validation.physical_calibration=if example {"Read-only fixture example: nominal metric dimensions and catalog masses; idealized unbreakable joints"}else{"Nominal metric dimensions; sourced catalog masses; server-supplied measured connector capacities; box inertia and assumed friction"}.into();
            Ok((scene, validation))
        }).await??;
        let id = uuid::Uuid::new_v4().to_string();
        let blocks = partition(&scene.bricks)?;
        let bounds = bounds(&scene.bricks).context("Cannot save an empty world")?;
        let mut geometry = BTreeMap::new();
        let mut proxy_parts = Vec::new();
        for (name, mesh) in &scene.meshes {
            geometry.insert(name.clone(), self.object(mesh).await?);
            if mesh.quality == "proxy" {
                proxy_parts.push(name.clone());
            }
        }
        let mut block_infos = Vec::new();
        for (coord, block) in blocks {
            let key = self.object(&block).await?;
            block_infos.push(BlockInfo {
                coord,
                key,
                brick_count: block.bricks.len(),
            });
        }
        let manifest = WorldManifest {
            schema: SCHEMA_VERSION,
            id,
            name: scene.name,
            source,
            brick_count: scene.bricks.len(),
            bounds,
            geo: scene.geo,
            blocks: block_infos,
            geometry,
            proxy_parts,
            notes,
            joints: scene.joints,
            fixed_bricks: scene.fixed_bricks,
            validation,
            owner,
            ground: scene.ground,
            groups: scene.groups,
        };
        // Publish only after every immutable object exists. Each import is a new world.
        crate::catalog::register(self, &manifest).await?;
        self.put(
            &format!("worlds/{}/{}/manifest.json", manifest.owner, manifest.id),
            serde_json::to_vec_pretty(&manifest)?,
        )
        .await?;
        Ok(manifest)
    }

    pub async fn export(&self, manifest: &WorldManifest) -> Result<WorldExport> {
        let mut bricks = BTreeMap::new();
        for info in &manifest.blocks {
            let block: ergo_world::Block =
                decode(&self.get(&format!("objects/{}.cbor", info.key)).await?)?;
            for brick in block.bricks {
                bricks.insert(brick.id, brick);
            }
        }
        let mut meshes = BTreeMap::new();
        for (part, hash) in &manifest.geometry {
            meshes.insert(
                part.clone(),
                decode(&self.get(&format!("objects/{hash}.cbor")).await?)?,
            );
        }
        Ok(WorldExport {
            schema: SCHEMA_VERSION,
            name: manifest.name.clone(),
            geo: manifest.geo,
            bricks: bricks.into_values().collect(),
            meshes,
            joints: manifest.joints.clone(),
            fixed_bricks: manifest.fixed_bricks.clone(),
            ground: manifest.ground.clone(),
            groups: manifest.groups.clone(),
        })
    }
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes)?;
    Ok(bytes)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    Ok(ciborium::from_reader(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn memory() -> Store {
        Store {
            backend: Arc::new(object_store::memory::InMemory::new()),
            prefix: String::new(),
            kind: "test memory".into(),
            measurements: Arc::new(Default::default()),
            static_tolerances: Default::default(),
        }
    }
    fn scene(stacked: bool) -> WorldExport {
        let mut d = ergo_world::demo::Demo::new();
        d.add("3003", 4, 2.0, 0.0, 2.0);
        if stacked {
            d.add("3003", 1, 2.0, 1.0, 2.0);
        }
        WorldExport {
            schema: SCHEMA_VERSION,
            name: "Test".into(),
            geo: Default::default(),
            bricks: d.bricks,
            meshes: d.meshes,
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            ground: Vec::new(),
            groups: Vec::new(),
        }
    }
    #[tokio::test]
    async fn owner_isolation_and_user_profiles_use_object_storage() {
        let store = memory();
        let alice = crate::auth::Principal {
            id: Some("alice".into()),
        };
        let bob = crate::auth::Principal {
            id: Some("bob".into()),
        };
        let world = store
            .save(scene(false), "test".into(), Vec::new(), "alice".into())
            .await
            .unwrap();
        assert!(store.manifest(&world.id, &alice).await.is_ok());
        assert!(store.manifest(&world.id, &bob).await.is_err());
        assert_eq!(store.manifests(&alice).await.unwrap().len(), 1);
        assert!(store.manifests(&bob).await.unwrap().is_empty());
        let user = crate::auth::User {
            id: "alice".into(),
            name: "Alice".into(),
            email: "alice@example.test".into(),
            picture: None,
            created_at: 1,
            last_login: 2,
        };
        store.put_user(&user).await.unwrap();
        assert_eq!(store.user("alice").await.unwrap().unwrap().name, "Alice");
        assert!(store.user("absent").await.unwrap().is_none());
    }
    #[tokio::test]
    async fn invalid_or_uncalibrated_scene_never_publishes_a_manifest() {
        let store = memory();
        let principal = crate::auth::Principal {
            id: Some("alice".into()),
        };
        let mut overlapping = scene(true);
        overlapping.bricks[1].translated(glam::DVec3::Y * -0.5);
        assert!(
            store
                .save(overlapping, "test".into(), Vec::new(), "alice".into())
                .await
                .is_err()
        );
        let mut untrusted = scene(true);
        untrusted.joints = ergo_world::constraints::inferred_joints(&untrusted.bricks);
        untrusted.joints[0].break_force = Some(1e20);
        untrusted.joints[0].break_torque = Some(1e20);
        untrusted.joints[0].strength_source = Some("untrusted client".into());
        let error = store
            .save(untrusted, "test".into(), Vec::new(), "alice".into())
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Missing measured connector capacity")
        );
        assert!(store.manifests(&principal).await.unwrap().is_empty());
        assert_eq!(
            store
                .backend
                .list(None)
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            0
        );
    }
}
