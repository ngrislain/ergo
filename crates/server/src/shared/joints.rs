use super::*;
use ergo_world::{JointKind, JointSpec};

#[derive(Clone, Serialize, Deserialize)]
pub struct JointInput {
    pub a: ContentId,
    pub b: ContentId,
    #[serde(with = "ergo_world::coordinates::vec3")]
    pub anchor: glam::DVec3,
    #[serde(flatten)]
    pub kind: JointKind,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct JointEdit {
    pub id: Option<ContentId>,
    pub value: Option<JointInput>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct GlobalJoint {
    pub id: ContentId,
    pub a: ContentId,
    pub b: ContentId,
    pub spec: JointSpec,
}
pub fn incident(piece: &ContentId) -> OctreePath {
    entity_path(7, piece)
}
fn reference(piece: &ContentId, joint: &ContentId) -> OctreePath {
    paged::append(&incident(piece), &entity_path(0, joint).octants()[1..])
}

impl Working {
    pub async fn global_joint(&mut self, id: &ContentId) -> SharedResult<GlobalJoint> {
        self.read(&entity_path(6, id))
            .await?
            .ok_or_else(SharedError::missing)
    }
    pub async fn external(&mut self, piece: &ContentId) -> SharedResult<BTreeSet<ContentId>> {
        let ids: Vec<ContentId> = paged::rows(self, &incident(piece)).await?;
        let mut peers = BTreeSet::new();
        for id in ids {
            let joint = self.global_joint(&id).await?;
            ensure!(
                joint.a == *piece || joint.b == *piece,
                "Incident joint mismatch"
            );
            peers.insert(if joint.a == *piece {
                joint.b.clone()
            } else {
                joint.a.clone()
            });
            self.add_joint(
                id,
                paged::BoundJoint {
                    a: joint.a,
                    b: joint.b,
                    spec: joint.spec,
                },
            );
        }
        Ok(peers)
    }
    pub async fn change_joint(&mut self, actor: &str, edit: &JointEdit) -> SharedResult<ContentId> {
        let old = if let Some(id) = &edit.id {
            Some(self.global_joint(id).await?)
        } else {
            None
        };
        let id = if let Some(id) = &edit.id {
            id.clone()
        } else {
            let input = edit.value.as_ref().context("New joint needs a value")?;
            self.tx.stage_blob(&(
                "joint-birth-v1",
                uuid::Uuid::new_v4().to_string(),
                actor,
                input,
            ))?
        };
        if let Some(old) = &old {
            for p in [&old.a, &old.b] {
                self.tx.remove(&reference(p, &id)).await?;
            }
        }
        if let Some(input) = &edit.value {
            ensure!(
                input.a != input.b && input.anchor.is_finite(),
                "Invalid global joint endpoints/anchor"
            );
            let spec = JointSpec {
                a: 0,
                b: 0,
                anchor: input.anchor,
                kind: input.kind.clone(),
                break_force: None,
                break_torque: None,
                strength_source: None,
            };
            let joint = GlobalJoint {
                id: id.clone(),
                a: input.a.clone(),
                b: input.b.clone(),
                spec,
            };
            self.tx.put(&entity_path(6, &id), &joint).await?;
            for p in [&input.a, &input.b] {
                self.tx.put(&reference(p, &id), &id).await?;
            }
        } else {
            ensure!(old.is_some(), "Cannot delete a joint without its ID");
            self.tx.remove(&entity_path(6, &id)).await?;
        }
        Ok(id)
    }
    pub async fn no_external(&mut self, piece: &ContentId) -> SharedResult<()> {
        ensure!(
            self.tx
                .scan_page(&incident(piece), None, 1)
                .await?
                .leaves
                .is_empty(),
            "Delete incident global joints before deleting a piece"
        );
        Ok(())
    }
}
