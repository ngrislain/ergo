//! Minimal MCP client runtime: no Leptos/UI framework, no native simulation or
//! software renderer. The local REST bridge handles I/O; this WASM client owns
//! editing, full Rapier tests, and on-demand WebGL2 image generation.
use crate::renderer::Renderer;
use anyhow::{Context, Result, ensure};
use ergo_world::{
    JointKind, JointSpec, WorldExport, constraints, physical::ConnectionMeasurements,
};
use glam::{DMat3, DVec3};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

#[wasm_bindgen]
pub struct LocalClientRuntime {
    scene: WorldExport,
    measurements: ConnectionMeasurements,
    undo: Option<WorldExport>,
    revision: u32,
    dirty: bool,
    canvas: HtmlCanvasElement,
    renderer: Option<Renderer>,
}

#[wasm_bindgen]
impl LocalClientRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new(
        canvas: HtmlCanvasElement,
        scene: &str,
        measurements: &str,
        revision: u32,
    ) -> Result<LocalClientRuntime, JsValue> {
        console_error_panic_hook::set_once();
        Ok(Self {
            scene: serde_json::from_str(scene).map_err(js_error)?,
            measurements: serde_json::from_str(measurements).map_err(js_error)?,
            undo: None,
            revision,
            dirty: false,
            canvas,
            renderer: None,
        })
    }
    pub fn needs_part(&self, part: &str) -> bool {
        let p = canonical(part);
        self.scene
            .meshes
            .get(&p)
            .is_none_or(|m| m.quality != "source")
    }
    pub fn insert_part(&mut self, asset: &str) -> Result<(), JsValue> {
        let value: Value = serde_json::from_str(asset).map_err(js_error)?;
        let mut mesh: ergo_world::geometry::Mesh =
            serde_json::from_value(value.get("mesh").cloned().unwrap_or(value))
                .map_err(js_error)?;
        ensure_js(
            mesh.schema == ergo_world::SCHEMA_VERSION,
            "Wrong part schema",
        )?;
        mesh.physical = self
            .measurements
            .parts
            .iter()
            .find(|p| p.part == mesh.part)
            .cloned();
        if let Some(record) = &mesh.physical {
            mesh.body_bounds = record.body_bounds;
        }
        self.scene.meshes.insert(mesh.part.clone(), mesh);
        Ok(())
    }
    pub fn execute(&mut self, name: &str, arguments: &str) -> String {
        let result = serde_json::from_str(arguments)
            .map_err(anyhow::Error::from)
            .and_then(|args| self.call(name, args));
        if let Some(renderer) = &mut self.renderer {
            renderer.clear();
        }
        match result {
            Ok(value) => value.to_string(),
            Err(error) => {
                json!({"content":[{"type":"text","text":format!("{error:#}")}],"isError":true})
                    .to_string()
            }
        }
    }
    pub fn pending_snapshot(&self) -> String {
        if self.dirty {
            serde_json::to_string(&self.scene).unwrap_or_default()
        } else {
            String::new()
        }
    }
    pub fn acknowledge(&mut self) {
        self.dirty = false;
    }
    pub fn restore(&mut self, scene: &str, revision: u32) -> Result<(), JsValue> {
        self.scene = serde_json::from_str(scene).map_err(js_error)?;
        self.revision = revision;
        self.dirty = false;
        self.undo = None;
        Ok(())
    }
}

fn js_error(e: impl ToString) -> JsValue {
    JsValue::from_str(&e.to_string())
}
fn ensure_js(ok: bool, message: &str) -> Result<(), JsValue> {
    if ok { Ok(()) } else { Err(message.into()) }
}
fn canonical(part: &str) -> String {
    ergo_world::catalog::canonical_part(&if part.ends_with(".dat") {
        part.into()
    } else {
        format!("{part}.dat")
    })
}
fn vector(args: &Value, key: &str) -> Result<DVec3> {
    let array: [f64; 3] = serde_json::from_value(
        args.get(key)
            .with_context(|| format!("Missing {key}"))?
            .clone(),
    )?;
    let p = DVec3::from(array);
    ensure!(p.is_finite(), "Invalid vector");
    Ok(ergo_world::coordinates::internal(p))
}
fn id(args: &Value, key: &str) -> Result<u32> {
    Ok(u32::try_from(
        args.get(key)
            .and_then(Value::as_u64)
            .with_context(|| format!("Missing {key}"))?,
    )?)
}
fn text(value: Value) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false})
}

impl LocalClientRuntime {
    fn prepared(&self) -> (WorldExport, Option<String>) {
        let mut scene = self.scene.clone();
        let part_issue = self
            .measurements
            .normalize_scene(&mut scene)
            .err()
            .map(|e| e.to_string());
        let pairs: HashSet<_> = scene
            .joints
            .iter()
            .map(|j| (j.a.min(j.b), j.a.max(j.b)))
            .collect();
        scene.joints.extend(
            constraints::inferred_joints(&scene.bricks)
                .into_iter()
                .filter(|j| !pairs.contains(&(j.a.min(j.b), j.a.max(j.b)))),
        );
        let issue = self
            .measurements
            .apply(&scene.bricks, &mut scene.joints)
            .err()
            .map(|e| e.to_string());
        (scene, part_issue.or(issue))
    }
    fn feedback(&self) -> Value {
        if self.scene.bricks.is_empty() {
            return json!({"revision":self.revision,"brick_count":0,"publication_ready":false,"message":"Empty local working copy"});
        }
        let (mut scene, issue) = self.prepared();
        let validation = ergo_world::validation::validate_static_with_measurements(
            &mut scene,
            Default::default(),
            &self.measurements,
        );
        let valid = validation.is_ok();
        let connector_issue = if validation
            .as_ref()
            .is_ok_and(|report| report.joint_strength_required == Some(false))
        {
            issue.clone()
        } else {
            None
        };
        // Successful contact-only validation has already checked sourced masses
        // and nominal geometry; an unused connector capacity is not required.
        let issue = if validation
            .as_ref()
            .is_ok_and(|report| report.joint_strength_required == Some(false))
        {
            None
        } else {
            issue
        };
        json!({"revision":self.revision,"brick_count":scene.bricks.len(),"overlap_free":overlap_free(&scene).is_ok(),"static_under_current_joint_model":valid,"publication_ready":valid&&issue.is_none(),"stability_report":validation.as_ref().ok(),"failure":validation.err().map(|e|e.to_string()),"calibration_issue":issue,"connector_calibration_issue":connector_issue,"global_commit_available":false,"execution":"local-wasm-client","next_action":if valid{"Inspect visual feedback and connector calibration before requesting an authorized commit."}else{"Add support or fix the reported constraint/contact. A client gravity test can provide additional diagnostics but cannot override the server certificate."}})
    }
    fn update(&mut self, edit: impl FnOnce(&mut WorldExport) -> Result<()>) -> Result<Value> {
        let mut next = self.scene.clone();
        edit(&mut next)?;
        ergo_world::catalog::validate(&next.bricks)?;
        constraints::validate_joints(&next.bricks, &next.joints, &next.fixed_bricks)?;
        ergo_world::groups::validate(&next.bricks, &next.groups)?;
        overlap_free(&next)?;
        if !next.bricks.is_empty() {
            ergo_world::partition(&next.bricks)?;
        }
        self.undo = Some(std::mem::replace(&mut self.scene, next));
        self.revision += 1;
        self.dirty = true;
        Ok(self.feedback())
    }
    fn update_metadata(
        &mut self,
        edit: impl FnOnce(&mut WorldExport) -> Result<()>,
    ) -> Result<Value> {
        let mut next = self.scene.clone();
        edit(&mut next)?;
        ergo_world::groups::validate(&next.bricks, &next.groups)?;
        self.undo = Some(std::mem::replace(&mut self.scene, next));
        self.revision += 1;
        self.dirty = true;
        Ok(self.feedback())
    }
    fn capture(&mut self, scene: &WorldExport, view: &str, args: &Value) -> Result<Value> {
        let width = args.get("width").and_then(Value::as_u64).unwrap_or(640) as u32;
        let height = args.get("height").and_then(Value::as_u64).unwrap_or(480) as u32;
        if self.renderer.is_none() {
            self.renderer = Some(
                Renderer::new_capture(self.canvas.clone())
                    .map_err(|e| anyhow::anyhow!("{:?}", e))?,
            );
        }
        let renderer = self.renderer.as_mut().unwrap();
        renderer.studio_lighting = args["lighting"].as_str() == Some("studio");
        renderer.clear();
        renderer.geo = scene.geo;
        renderer.set_meshes(scene.meshes.clone());
        renderer.set_bricks(scene.bricks.clone());
        for block in &scene.ground {
            renderer.set_ground(block.clone());
        }
        if let Some(camera) = args.get("camera") {
            let position = vector(camera, "position_world")?;
            let target = vector(camera, "target_world")?;
            let fov = camera
                .get("vertical_fov_degrees")
                .and_then(Value::as_f64)
                .unwrap_or(45.0);
            renderer
                .set_camera(position, target, fov)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        } else {
            renderer
                .preset(view)
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        }
        let data = renderer
            .png(width, height)
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        Ok(
            json!({"type":"image","data":data.strip_prefix("data:image/png;base64,").context("Canvas did not return PNG")?,"mimeType":"image/png"}),
        )
    }
    fn quake(
        &mut self,
        pga: f64,
        frequency: f64,
        duration: f64,
        images: bool,
        preset: &str,
    ) -> Result<Value> {
        let (scene, issue) = self.prepared();
        let mut reports = Vec::new();
        let mut pictures = Vec::new();
        for (axis, direction) in [("X", DVec3::X), ("Y", DVec3::NEG_Z)] {
            let case = ergo_world::solidity::LoadCase::Earthquake(
                ergo_world::solidity::earthquake_with_acceleration_bound(
                    pga, frequency, duration, direction,
                )?,
            );
            let (bricks, _, report) = ergo_world::solidity::test(&scene, case)?;
            if images {
                let mut result = scene.clone();
                result.bricks = bricks;
                pictures.push(json!({"type":"text","text":format!("Post-test view: {axis}")}));
                pictures.push(self.capture(&result, "isometric", &json!({}))?);
            }
            reports.push(json!({"axis":axis,"report":report}));
        }
        let survived = reports
            .iter()
            .all(|r| r["report"]["survived_declared_case"] == true);
        let calibrated = issue.is_none()
            && reports
                .iter()
                .all(|r| r["report"]["connector_capacities_complete"] == true);
        let summary = json!({"preset":preset,"acceleration_upper_bound_g":pga,"frequency_hz":frequency,"shake_duration_s":duration,"settle_duration_s":2,"tested_axes":["X","Y"],"survived_both_declared_cases":survived,"calibrated_stability_verified_for_preset":survived&&calibrated,"calibration_issue":issue,"reports":reports,"working_copy_changed":false,"execution":"local-wasm-client","interpretation":"PGA is local shaking intensity, not Richter/moment magnitude or a seismic-code rating."});
        let mut content = vec![json!({"type":"text","text":summary.to_string()})];
        content.extend(pictures);
        Ok(json!({"content":content,"structuredContent":summary,"isError":false}))
    }
    fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        match name {
            "get_catalog" => {
                let catalog = if args["include_extended"].as_bool().unwrap_or(false) {
                    ergo_world::catalog::catalog()
                } else {
                    ergo_world::catalog::preferred_catalog()
                };
                let part = args["part"].as_str().map(canonical);
                let asset = part.as_ref().and_then(|p|self.scene.meshes.get(p)).map(|m|json!({"part":m.part,"quality":m.quality,"bounds":m.bounds,"mass_kg":self.measurements.mass_kg(&m.part).ok()}));
                Ok(text(
                    json!({"catalog":catalog,"requested_part":asset,"building_guidance":ergo_world::mcp::BUILDING_GUIDANCE,"units":{"position":"X east, Y north, Z up; X/Y 8 mm, Z 9.6 mm","force":"N","torque":"N m","mass":"kg"},"execution":"local-wasm-client"}),
                ))
            }
            "list_bricks" => {
                let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(50)
                    .min(200) as usize;
                Ok(text(
                    json!({"revision":self.revision,"total":self.scene.bricks.len(),"bricks":self.scene.bricks.iter().skip(offset).take(limit).map(|b|json!({"id":b.id,"part":b.part,"color":b.color,"position":ergo_world::coordinates::public(b.position),"bounds":b.bounds,"mass_kg":self.measurements.mass_kg(&b.part).ok()})).collect::<Vec<_>>()}),
                ))
            }
            "inspect_brick" => {
                let id = id(&args, "id")?;
                let brick = self
                    .scene
                    .bricks
                    .iter()
                    .find(|b| b.id == id)
                    .context("Brick is absent")?;
                let contacts: Vec<_> = self
                    .scene
                    .bricks
                    .iter()
                    .filter(|b| b.id != id)
                    .filter_map(|b| ergo_world::spatial::relation(brick, b))
                    .collect();
                Ok(text(
                    json!({"brick":brick,"mass_kg":self.measurements.mass_kg(&brick.part).ok(),"mass_source":self.measurements.parts.iter().find(|p|p.part==brick.part).map(|p|p.source.clone()).or_else(||ergo_world::physical::mass_kg(&brick.part).ok().map(|_|ergo_world::physical::mass_source(&brick.part))),"body_dimensions_m":ergo_world::coordinates::dimensions(ergo_world::physical::to_meters(brick.bounds.size())),"contacts":contacts,"constraints":self.scene.joints.iter().filter(|j|j.a==id||j.b==id).collect::<Vec<_>>()}),
                ))
            }
            "place_brick" => {
                #[derive(Deserialize)]
                struct Args {
                    part: String,
                    color: u32,
                    min_corner: [f64; 3],
                    #[serde(default)]
                    quarter_turns: i32,
                }
                let a: Args = serde_json::from_value(args)?;
                let part = canonical(&a.part);
                let feedback = self.update(|scene| {
                    let mesh = scene
                        .meshes
                        .entry(part.clone())
                        .or_insert_with(|| ergo_world::geometry::fallback_mesh(&part));
                    let rotation = DMat3::from_rotation_y(
                        (a.quarter_turns.rem_euclid(4) as f64) * std::f64::consts::FRAC_PI_2,
                    );
                    let bounds = mesh.body_bounds.transformed(rotation, DVec3::ZERO);
                    let p = ergo_world::coordinates::internal(DVec3::from(a.min_corner))
                        - DVec3::new(bounds.min.x, bounds.min.y, bounds.max.z);
                    let c = ergo_world::ldraw::conversion();
                    let reference = ergo_world::ldraw::Reference {
                        name: part,
                        color: a.color,
                        position: c.inverse() * p,
                        transform: c.inverse() * rotation * c,
                    };
                    let id = scene
                        .bricks
                        .iter()
                        .map(|b| b.id)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                        .context("ID overflow")?;
                    scene.bricks.push(ergo_world::ldraw::brick_from_reference(
                        id,
                        &reference,
                        mesh,
                        &BTreeMap::new(),
                    ));
                    Ok(())
                })?;
                Ok(text(feedback))
            }
            "place_submodel" => {
                ensure!(
                    args.get("model").is_some() != args.get("document").is_some(),
                    "Supply exactly one native model or imported document"
                );
                let document = if let Some(source) = args.get("document") {
                    let source = source.as_str().context("Expected text model document")?;
                    ensure!(
                        source.len() <= 128 * 1024,
                        "Imported document exceeds 128 KiB"
                    );
                    Some(ergo_world::ldraw::Document::parse(source, "model.ldr")?)
                } else {
                    None
                };
                let model = if let Some(value) = args.get("model") {
                    ensure!(
                        serde_json::to_vec(value)?.len() <= 768 * 1024,
                        "Native model exceeds 768 KiB"
                    );
                    serde_json::from_value::<ergo_world::assembly::Model>(value.clone())?
                } else {
                    ergo_world::assembly::Model::from_document(document.as_ref().unwrap())?
                };
                let colors = document
                    .as_ref()
                    .map(|d| d.colors.clone())
                    .unwrap_or_default();
                let matrix = if let Some(value) = args.get("transform") {
                    ergo_world::assembly::Transform::from_world(serde_json::from_value(
                        value.clone(),
                    )?)?
                } else {
                    Default::default()
                };
                let color = args
                    .get("color")
                    .map(|v| serde_json::from_value::<u32>(v.clone()))
                    .transpose()?
                    .unwrap_or(71);
                Ok(text(self.update(|scene| {
                    let mut combined = ergo_world::assembly::Model::for_scene(scene);
                    combined.append(&model, matrix, color)?;
                    let references = combined.references()?;
                    let mut next_id = scene.bricks.iter().map(|b| b.id).max().unwrap_or(0);
                    for r in references.iter().skip(scene.bricks.len()) {
                        if !scene.meshes.contains_key(&r.name) {
                            let mut mesh = ergo_world::ldraw::bake_mesh(
                                &r.name,
                                &|name| document.as_ref().and_then(|d| d.files.get(name)).cloned(),
                                &colors,
                            )
                            .or_else(|error| {
                                ensure!(
                                    ergo_world::geometry::part_spec(&r.name).is_some(),
                                    "Resolve geometry for {} first: {error}",
                                    r.name
                                );
                                Ok(ergo_world::geometry::fallback_mesh(&r.name))
                            })?;
                            if ergo_world::geometry::part_spec(&r.name).is_some() {
                                mesh.body_bounds =
                                    ergo_world::geometry::fallback_mesh(&r.name).body_bounds;
                            }
                            scene.meshes.insert(r.name.clone(), mesh);
                        }
                        next_id = next_id.checked_add(1).context("ID overflow")?;
                        let brick = ergo_world::ldraw::brick_from_reference(
                            next_id,
                            r,
                            &scene.meshes[&r.name],
                            &colors,
                        );
                        ergo_world::physical::validate_pose(&brick)?;
                        scene.bricks.push(brick);
                    }
                    scene.model = Some(combined);
                    Ok(())
                })?))
            }
            "move_brick" => {
                let selected = id(&args, "id")?;
                let delta = vector(&args, "delta")?;
                Ok(text(self.update(|scene| {
                    ensure!(
                        scene.bricks.iter().any(|b| b.id == selected),
                        "Brick is absent"
                    );
                    let mut graph = scene.joints.clone();
                    graph.extend(constraints::inferred_joints(&scene.bricks));
                    let group = constraints::connected_group(selected, &graph);
                    ensure!(
                        !scene.fixed_bricks.iter().any(|id| group.contains(id)),
                        "The group is anchored"
                    );
                    ensure!(
                        !graph.iter().any(|j| !matches!(j.kind, JointKind::Fixed)
                            && (group.contains(&j.a) || group.contains(&j.b))),
                        "Use simulation to manipulate an articulated group"
                    );
                    constraints::can_translate(&scene.bricks, &group, delta)?;
                    for b in &mut scene.bricks {
                        if group.contains(&b.id) {
                            b.translated(delta);
                        }
                    }
                    for j in &mut scene.joints {
                        if group.contains(&j.a) && group.contains(&j.b) {
                            j.anchor += delta;
                        }
                    }
                    Ok(())
                })?))
            }
            "remove_bricks" => {
                let ids: HashSet<u32> =
                    serde_json::from_value(args.get("ids").context("Missing ids")?.clone())?;
                Ok(text(self.update(|scene| {
                    ensure!(
                        ids.iter()
                            .all(|id| scene.bricks.iter().any(|b| b.id == *id)),
                        "A brick is absent"
                    );
                    scene.bricks.retain(|b| !ids.contains(&b.id));
                    scene
                        .joints
                        .retain(|j| !ids.contains(&j.a) && !ids.contains(&j.b));
                    scene.fixed_bricks.retain(|id| !ids.contains(id));
                    ergo_world::groups::remove_piece_references(&mut scene.groups, &ids);
                    Ok(())
                })?))
            }
            "connect_bricks" => {
                let a = id(&args, "a")?;
                let b = id(&args, "b")?;
                let anchor = vector(&args, "anchor")?;
                let kind = match args.get("kind").and_then(Value::as_str).unwrap_or("fixed") {
                    "fixed" => JointKind::Fixed,
                    "revolute" => JointKind::Revolute {
                        axis: vector(&args, "axis")?,
                        limits: args
                            .get("limits")
                            .map(|v| serde_json::from_value(v.clone()))
                            .transpose()?,
                    },
                    _ => anyhow::bail!("Unknown constraint"),
                };
                Ok(text(self.update(|scene| {
                    scene
                        .joints
                        .retain(|j| !((j.a == a && j.b == b) || (j.a == b && j.b == a)));
                    scene.joints.push(JointSpec {
                        a,
                        b,
                        anchor,
                        kind,
                        break_force: None,
                        break_torque: None,
                        strength_source: None,
                    });
                    Ok(())
                })?))
            }
            "validate_construction" => Ok(text(self.feedback())),
            "list_groups" => Ok(text(
                json!({"groups":self.scene.groups.iter().map(|g|json!({"group":g,"piece_count":ergo_world::groups::descendants(&self.scene.groups,g.id).map(|(p,_)|p.len()).unwrap_or(0),"bounds":ergo_world::groups::bounds(&self.scene,g.id).ok().flatten()})).collect::<Vec<_>>()}),
            )),
            "create_group" => {
                let name = args
                    .get("name")
                    .and_then(Value::as_str)
                    .context("Missing group name")?
                    .to_owned();
                let members = members(&args)?;
                let pivot = if args.get("pivot_world").is_some() {
                    Some(vector(&args, "pivot_world")?)
                } else {
                    None
                };
                let mut created = 0;
                let mut feedback = self.update_metadata(|scene| {
                    created = ergo_world::groups::create(scene, name, members, pivot)?;
                    Ok(())
                })?;
                feedback["group_id"] = json!(created);
                Ok(text(feedback))
            }
            "set_group_members" => {
                let id = id(&args, "group_id")?;
                let members = members(&args)?;
                let reparent = args
                    .get("reparent")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(text(self.update_metadata(|scene| {
                    ergo_world::groups::set_members(scene, id, members, reparent)
                })?))
            }
            "transform_group" => {
                let id = id(&args, "group_id")?;
                let translation = if args.get("translation").is_some() {
                    vector(&args, "translation")?
                } else {
                    DVec3::ZERO
                };
                let axis = if args.get("rotation_axis").is_some() {
                    vector(&args, "rotation_axis")?
                } else {
                    DVec3::Y
                };
                let angle = args
                    .get("rotation_radians")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let pivot = if args.get("pivot_world").is_some() {
                    Some(vector(&args, "pivot_world")?)
                } else {
                    None
                };
                let mut count = 0;
                let mut feedback = self.update(|scene| {
                    count =
                        ergo_world::groups::transform(scene, id, translation, axis, angle, pivot)?;
                    Ok(())
                })?;
                feedback["transformed_piece_count"] = json!(count);
                Ok(text(feedback))
            }
            "ungroup" => {
                let id = id(&args, "group_id")?;
                Ok(text(self.update_metadata(|scene| {
                    ergo_world::groups::ungroup(scene, id)
                })?))
            }
            "test_stability_by_gravity" => {
                let (scene, issue) = self.prepared();
                let settings: ergo_world::physical::GravityProbeSettings =
                    serde_json::from_value(args)?;
                settings.validate()?;
                let mut static_scene = scene.clone();
                let tolerances = ergo_world::validation::StaticTolerances {
                    position_brick_units: settings.position_tolerance,
                    rotation_radians: settings.rotation_tolerance_radians,
                    ..Default::default()
                };
                let certificate = ergo_world::validation::validate_static_with_measurements(
                    &mut static_scene,
                    tolerances,
                    &self.measurements,
                );
                let certified = certificate.is_ok();
                let (_, _, report) = ergo_world::solidity::test(
                    &scene,
                    ergo_world::solidity::LoadCase::Gravity(settings.clone()),
                )?;
                let stable = certified && report.survived_declared_case;
                Ok(text(
                    json!({"settings":settings,"stable_within_tolerance":stable,"static_certificate":certificate.as_ref().ok(),"static_certificate_issue":certificate.err().map(|e|e.to_string()),"unstable_pieces":report.unstable_pieces,"report":report,"calibrated_stability_verified":stable&&report.connector_capacities_complete&&issue.is_none(),"calibration_issue":issue,"working_copy_changed":false,"execution":"local-wasm-client","interpretation":"Short gravity equilibrium probe with explicit numeric tolerances; robustness is assessed separately by disturbance tests."}),
                ))
            }
            "test_small_earthquake_stability" => {
                let (mut scene, mut issue) = self.prepared();
                let contacts_only = args["connection_model"] == "contacts_only";
                let estimated = args["connection_model"] == "estimated";
                let estimate_model = if estimated {
                    let report = ergo_world::estimates::apply_stud_estimates(
                        &scene.bricks,
                        &mut scene.joints,
                    );
                    issue = Some(format!(
                        "Explicit ballpark connector estimates, not measured calibration; {} unsupported joint pairs",
                        report.unsupported_piece_pairs.len()
                    ));
                    Some(report)
                } else {
                    None
                };
                if contacts_only {
                    scene.joints.clear();
                    issue=Some(match issue {
                        Some(reason)=>format!("{reason}; contact-only diagnostic excludes joint constraints and does not certify stud-clutch strength"),
                        None=>"Contact-only diagnostic: joint constraints excluded; stud-clutch strength is not certified".into(),
                    });
                }
                let mut static_scene = scene.clone();
                let certificate = ergo_world::validation::validate_static_with_measurements(
                    &mut static_scene,
                    Default::default(),
                    &self.measurements,
                );
                let settings: ergo_world::solidity::SmallEarthquake =
                    serde_json::from_value(args.clone())?;
                settings.validate()?;
                let (_, _, gravity) = ergo_world::solidity::test(
                    &scene,
                    ergo_world::solidity::LoadCase::Gravity(
                        ergo_world::physical::GravityProbeSettings {
                            steps: 120,
                            position_tolerance: settings.position_tolerance,
                            rotation_tolerance_radians: settings.rotation_tolerance_radians,
                        },
                    ),
                )?;
                let (bricks, _, shake) = ergo_world::solidity::test(
                    &scene,
                    ergo_world::solidity::LoadCase::SmallEarthquake(settings.clone()),
                )?;
                let passed = certificate.is_ok()
                    && gravity.survived_declared_case
                    && shake.survived_declared_case;
                let calibrated = issue.is_none()
                    && gravity.connector_capacities_complete
                    && shake.connector_capacities_complete;
                let estimated_complete = estimate_model
                    .as_ref()
                    .is_some_and(|e| e.unsupported_piece_pairs.is_empty())
                    && gravity.omitted_inferred_connections == 0
                    && gravity.unmeasured_connections == 0;
                let summary = json!({"settings":settings,"connection_model":if estimated{"estimated"}else if contacts_only{"contacts_only"}else{"measured"},"estimate_model":estimate_model,"gravity":gravity,"shake":shake,"static_certificate_issue":certificate.err().map(|error|error.to_string()),"stable_under_declared_model":passed,"stable_under_declared_estimates":estimated&&estimated_complete&&passed,
                    "calibrated_stability_verified":passed&&calibrated,"calibration_issue":issue,"working_copy_changed":false,
                    "execution":"local-wasm-client","next_action":if estimated{"Treat survival as conditional on the disclosed ballpark assumptions, not measured strength; inspect unsupported pairs and moving IDs, and check solver convergence."}else if !calibrated{"Resolve missing measurements/omitted connections and check solver convergence before classifying a physical failure; inspect unstable_pieces for the diagnostic motion."}else if passed{"Inspect stated model and numerical limits before claiming stability."}else{"Inspect unstable_pieces IDs, parts and nearby_piece_ids; check solver convergence, repair confirmed defects and rerun the same seed."}});
                let mut content = vec![json!({"type":"text","text":summary.to_string()})];
                if args["include_images"].as_bool().unwrap_or(false) {
                    let moving: HashSet<_> = shake
                        .unstable_pieces
                        .iter()
                        .map(|p| p.motion.id)
                        .chain(gravity.unstable_pieces.iter().map(|p| p.motion.id))
                        .collect();
                    let mut image = scene;
                    image.bricks = bricks;
                    for brick in &mut image.bricks {
                        if moving.contains(&brick.id) {
                            brick.rgb = 0xff3b30;
                        }
                    }
                    content.push(json!({"type":"text","text":"Post-test geometry: unstable piece IDs are highlighted red."}));
                    content.push(self.capture(&image, "isometric", &json!({}))?);
                }
                Ok(json!({"content":content,"structuredContent":summary,"isError":false}))
            }
            "test_stability_by_earthquake" => self.quake(
                args.get("peak_ground_acceleration_g")
                    .and_then(Value::as_f64)
                    .context(
                        "PGA in g is required; source earthquake magnitude alone is insufficient",
                    )?,
                args.get("frequency_hz")
                    .and_then(Value::as_f64)
                    .unwrap_or(4.0),
                args.get("duration_s")
                    .and_then(Value::as_f64)
                    .unwrap_or(3.0),
                args.get("include_images")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                "custom-pga-bound",
            ),
            "test_moderate_earthquake_stability" => self.quake(
                0.15,
                4.0,
                3.0,
                args.get("include_images")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                "ergo-moderate-shake-v2",
            ),
            "test_solidity" => {
                let (scene, issue) = self.prepared();
                let case = serde_json::from_value(
                    args.get("load_case").context("Missing load case")?.clone(),
                )?;
                let (bricks, _, report) = ergo_world::solidity::test(&scene, case)?;
                let mut content = vec![
                    json!({"type":"text","text":json!({"report":report,"calibration_issue":issue,"working_copy_changed":false,"execution":"local-wasm-client"}).to_string()}),
                ];
                if args
                    .get("include_image")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                {
                    let mut result = scene;
                    result.bricks = bricks;
                    content.push(self.capture(&result, "isometric", &json!({}))?);
                }
                Ok(json!({"content":content,"isError":false}))
            }
            "simulate_gravity" => {
                let (scene, issue) = self.prepared();
                let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(2.0);
                ensure!((0.0..=4.0).contains(&seconds), "Duration must be 0–4 s");
                let mut sim = ergo_world::physics::Simulation::new(
                    &scene.bricks,
                    &scene.meshes,
                    &scene.joints,
                    &scene.fixed_bricks,
                    false,
                )?;
                for _ in 0..(seconds / ergo_world::physics::TIMESTEP).ceil() as usize {
                    sim.step();
                }
                let mut next = scene;
                next.bricks = sim.bricks();
                next.joints = sim.current_joints();
                self.undo = Some(std::mem::replace(&mut self.scene, next));
                self.revision += 1;
                self.dirty = true;
                let mut feedback = self.feedback();
                feedback["simulation_stats"] = json!(sim.stats());
                feedback["calibration_issue"] = json!(issue);
                Ok(text(feedback))
            }
            "test_visual_appearance" | "view_construction" => {
                ensure!(
                    args.get("camera").is_none() || args.get("views").is_none(),
                    "Choose a camera or preset views"
                );
                let views: Vec<String> = args
                    .get("views")
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()?
                    .unwrap_or_else(|| vec!["isometric".into()]);
                ensure!(!views.is_empty() && views.len() <= 4, "Choose 1–4 views");
                let scene = self.scene.clone();
                let mut content = vec![
                    json!({"type":"text","text":json!({"feedback":self.feedback(),"execution":"local-wasm-webgl2-client","camera":args.get("camera")}).to_string()}),
                ];
                for view in views {
                    content.push(json!({"type":"text","text":format!("View: {view}")}));
                    content.push(self.capture(&scene, &view, &args)?);
                }
                Ok(json!({"content":content,"isError":false}))
            }
            "undo" => {
                self.scene = self.undo.take().context("No previous edit")?;
                self.revision += 1;
                self.dirty = true;
                Ok(text(self.feedback()))
            }
            "export_construction" => Ok(text(
                json!({"revision":self.revision,"kind":"local working-copy JSON, not a commit","feedback":self.feedback()}),
            )),
            _ => anyhow::bail!("Unknown tool: {name}"),
        }
    }
}
fn overlap_free(scene: &WorldExport) -> Result<()> {
    let tree = ergo_world::spatial::Octree::new(scene.bricks.iter().map(|b| b.bounds).collect());
    for (i, a) in scene.bricks.iter().enumerate() {
        for j in tree.query(a.bounds) {
            if j <= i {
                continue;
            }
            let b = &scene.bricks[j];
            let overlap = a.bounds.max.min(b.bounds.max) - a.bounds.min.max(b.bounds.min);
            ensure!(
                overlap.min_element() <= ergo_world::validation::OVERLAP_TOLERANCE,
                "Edit rejected: bricks {} and {} overlap; working copy unchanged",
                a.id,
                b.id
            );
        }
    }
    Ok(())
}
fn members(args: &Value) -> Result<Vec<ergo_world::groups::Member>> {
    let pieces: Vec<u32> = args
        .get("piece_ids")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();
    let groups: Vec<u32> = args
        .get("group_ids")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();
    Ok(pieces
        .into_iter()
        .map(|id| ergo_world::groups::Member::Piece { id })
        .chain(
            groups
                .into_iter()
                .map(|id| ergo_world::groups::Member::Group { id }),
        )
        .collect())
}
