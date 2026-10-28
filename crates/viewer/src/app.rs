use crate::location::CameraLocation;
use anyhow::Context;
use ergo_client::{
    navigation::{
        Control, DEFAULT_EYE_HEIGHT, MAX_EYE_HEIGHT, MIN_EYE_HEIGHT, NavigationInput,
        NavigationMode, NavigationStatus,
    },
    renderer::Renderer,
    walking::{JUMP_HEIGHT, STEP_HEIGHT},
};
use ergo_world::{
    Block, BlockCoord, Brick, JointKind, JointSpec, WorldExport, WorldManifest, catalog,
    physics::{self, Simulation, SimulationStats},
    spatial::Relation,
};
use glam::DVec3;
use gloo_net::http::Request;
use leptos::{html, prelude::*};
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    rc::Rc,
};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::spawn_local;

type Handle = StoredValue<Rc<RefCell<Client>>, LocalStorage>;
#[path = "common.rs"]
mod common;
#[path = "presence.rs"]
mod presence;

#[derive(Clone, Deserialize)]
struct SharedMetadata {
    name: String,
    geo: ergo_world::GeoReference,
}
#[derive(Clone, Deserialize)]
struct SharedLocation {
    world: String,
    metadata: SharedMetadata,
    coord: Option<BlockCoord>,
}
#[derive(Deserialize)]
struct SharedPage {
    items: Vec<SharedLocation>,
    next: Option<String>,
}
struct SharedView {
    assemblies: BTreeMap<u32, String>,
    fully_resident_assemblies: HashSet<String>,
    id: String,
    coord: [i64; 3],
    last_check: f64,
    loading: bool,
    revision: String,
}
#[derive(Deserialize)]
struct SharedRegion {
    scene: WorldExport,
    revision: String,
    #[serde(default)]
    assemblies: BTreeMap<u32, String>,
    #[serde(default)]
    fully_resident_assemblies: HashSet<String>,
}

#[derive(Clone, Copy)]
struct Ui {
    common_world: RwSignal<Option<String>>,
    common_assemblies: RwSignal<Vec<common::Assembly>>,
    common_more: RwSignal<Option<String>>,
    common_loading: RwSignal<bool>,
    focused_assembly: RwSignal<Option<String>>,
    editing_shared: RwSignal<bool>,
    worlds: RwSignal<Vec<WorldManifest>>,
    current: RwSignal<Option<WorldManifest>>,
    selected: RwSignal<Option<Brick>>,
    relations: RwSignal<Vec<Relation>>,
    message: RwSignal<String>,
    error: RwSignal<Option<String>>,
    count: RwSignal<usize>,
    blocks: RwSignal<usize>,
    gpu: RwSignal<usize>,
    draws: RwSignal<usize>,
    nodes: RwSignal<usize>,
    fps: RwSignal<u32>,
    playing: RwSignal<bool>,
    stats: RwSignal<SimulationStats>,
    dirty: RwSignal<bool>,
    first: RwSignal<Option<u32>>,
    joint_count: RwSignal<usize>,
    busy: RwSignal<bool>,
    loading: RwSignal<Option<crate::loading::Loading>>,
    user: RwSignal<String>,
    google: RwSignal<bool>,
    signed_in: RwSignal<bool>,
    signing_out: RwSignal<bool>,
    storage: RwSignal<String>,
    ground_status: RwSignal<String>,
    ground_attribution: RwSignal<Option<(String, String)>>,
    fog: RwSignal<f64>,
    render_quality: RwSignal<String>,
    authoring_groups: RwSignal<Vec<ergo_world::groups::Group>>,
    group_selection: RwSignal<Vec<u32>>,
    navigation: RwSignal<NavigationStatus>,
    camera_location: RwSignal<Option<CameraLocation>>,
    north_angle: RwSignal<f64>,
    fullscreen: RwSignal<bool>,
    shared_locations: RwSignal<Vec<SharedLocation>>,
    shared_cursor: RwSignal<Option<String>>,
}

impl Ui {
    fn scene_label(self) -> &'static str {
        if self.common_world.get().is_some() {
            if self.dirty.get() {
                "Pending shared edit"
            } else if self.editing_shared.get() {
                "Saved in Ergo Sandbox"
            } else {
                "Ergo Sandbox"
            }
        } else if self.dirty.get() {
            "Working copy"
        } else if self.current.get().is_some_and(|w| w.owner == "examples") {
            "Fixture example"
        } else if self.current.get().is_some_and(|w| w.owner == "shared") {
            "Shared world view"
        } else {
            "Validated snapshot"
        }
    }
    fn construction_details(self) -> String {
        self.current
            .get()
            .map(|w| {
                if w.id == "draft" {
                    if self.common_world.get().is_some() {
                        if self.dirty.get() {
                            "Preview — awaiting shared acceptance".into()
                        } else {
                            "Shared construction".into()
                        }
                    } else {
                        "Workspace draft".into()
                    }
                } else {
                    format!(
                        "{} bricks in {} spatial block{}",
                        number(w.brick_count),
                        w.blocks.len(),
                        if w.blocks.len() == 1 { "" } else { "s" }
                    )
                }
            })
            .unwrap_or_default()
    }
    fn new() -> Self {
        Self {
            common_world: RwSignal::new(None),
            common_assemblies: RwSignal::new(Vec::new()),
            common_more: RwSignal::new(None),
            common_loading: RwSignal::new(false),
            focused_assembly: RwSignal::new(None),
            editing_shared: RwSignal::new(false),
            worlds: RwSignal::new(Vec::new()),
            current: RwSignal::new(None),
            selected: RwSignal::new(None),
            relations: RwSignal::new(Vec::new()),
            message: RwSignal::new("Opening your workbench…".into()),
            error: RwSignal::new(None),
            count: RwSignal::new(0),
            blocks: RwSignal::new(0),
            gpu: RwSignal::new(0),
            draws: RwSignal::new(0),
            nodes: RwSignal::new(0),
            fps: RwSignal::new(0),
            playing: RwSignal::new(false),
            stats: RwSignal::new(SimulationStats::default()),
            dirty: RwSignal::new(false),
            first: RwSignal::new(None),
            joint_count: RwSignal::new(0),
            busy: RwSignal::new(false),
            loading: RwSignal::new(None),
            user: RwSignal::new("Connecting…".into()),
            google: RwSignal::new(false),
            signed_in: RwSignal::new(false),
            signing_out: RwSignal::new(false),
            storage: RwSignal::new("Connecting".into()),
            ground_status: RwSignal::new("Ground: loading map colors".into()),
            ground_attribution: RwSignal::new(None),
            fog: RwSignal::new(2200.0),
            render_quality: RwSignal::new("auto".into()),
            authoring_groups: RwSignal::new(Vec::new()),
            group_selection: RwSignal::new(Vec::new()),
            navigation: RwSignal::new(NavigationStatus::default()),
            camera_location: RwSignal::new(None),
            north_angle: RwSignal::new(0.),
            fullscreen: RwSignal::new(false),
            shared_locations: RwSignal::new(Vec::new()),
            shared_cursor: RwSignal::new(None),
        }
    }
    fn fail(self, error: impl ToString) {
        self.error.set(Some(error.to_string()));
        self.busy.set(false);
        self.loading.set(None);
    }
}

struct Client {
    presence: presence::Presence,
    common_binding: Option<common::Binding>,
    common_attempt: Option<u64>,
    common_fenced: bool,
    shared: Option<SharedView>,
    renderer: Option<Renderer>,
    world: Option<WorldManifest>,
    blocks: BTreeMap<BlockCoord, Block>,
    pending: HashSet<String>,
    ground_pending: HashSet<(i64, i64)>,
    ground_retry: std::collections::HashMap<(i64, i64), f64>,
    ground_provider: Option<String>,
    navigation_controls: HashSet<Control>,
    known_cells: HashSet<BlockCoord>,
    default_geo: ergo_world::GeoReference,
    generation: u64,
    region_request: u64,
    ready: bool,
    load_all: bool,
    start_when_loaded: bool,
    simulation: Option<Simulation>,
    playing: bool,
    joints: Vec<JointSpec>,
    fixed: Vec<u32>,
    groups: Vec<ergo_world::groups::Group>,
    undo: Option<WorldExport>,
    pointer: Option<(f64, f64, f64, f64, i16, bool)>,
    accumulator: f64,
    ui: Ui,
}

impl Client {
    fn new(ui: Ui) -> Self {
        Self {
            presence: Default::default(),
            common_binding: None,
            common_attempt: None,
            common_fenced: false,
            shared: None,
            renderer: None,
            world: None,
            blocks: BTreeMap::new(),
            pending: HashSet::new(),
            ground_pending: HashSet::new(),
            ground_retry: Default::default(),
            ground_provider: None,
            default_geo: Default::default(),
            navigation_controls: HashSet::new(),
            known_cells: HashSet::new(),
            generation: 0,
            region_request: 0,
            ready: false,
            load_all: false,
            start_when_loaded: false,
            simulation: None,
            playing: false,
            joints: Vec::new(),
            fixed: Vec::new(),
            groups: Vec::new(),
            undo: None,
            pointer: None,
            accumulator: 0.0,
            ui,
        }
    }
    fn scene(&self) -> anyhow::Result<WorldExport> {
        let world = self
            .world
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Open a construction first"))?;
        let renderer = self
            .renderer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Renderer is not ready"))?;
        anyhow::ensure!(
            self.complete(),
            "Load the complete construction before editing, simulating, or saving"
        );
        Ok(WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: world.name.clone(),
            geo: world.geo,
            bricks: renderer.bricks.clone(),
            meshes: renderer.meshes().clone(),
            joints: self.joints.clone(),
            fixed_bricks: self.fixed.clone(),
            ground: renderer.ground.values().cloned().collect(),
            groups: self.groups.clone(),
            model: world.model.clone(),
        })
    }
    fn complete(&self) -> bool {
        self.world.as_ref().is_some_and(|w| {
            self.renderer
                .as_ref()
                .is_some_and(|r| r.bricks.len() == w.brick_count)
                && self.blocks.len() == w.blocks.len()
        })
    }
    fn refresh_selection(&self) {
        let Some(r) = &self.renderer else {
            return;
        };
        self.ui.selected.set(r.selected_brick());
        self.ui
            .relations
            .set(r.selected.map(|id| r.relations(id)).unwrap_or_default());
        self.ui.joint_count.set(self.joints.len());
    }
    fn rebuild(&mut self) {
        let mut bricks = BTreeMap::new();
        for block in self.blocks.values() {
            for b in &block.bricks {
                bricks.entry(b.id).or_insert_with(|| b.clone());
            }
        }
        if let Some(r) = &mut self.renderer {
            r.set_bricks(bricks.into_values().collect());
        }
        self.refresh_selection();
    }
    fn request_all(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.ui.common_world.get_untracked().is_none() || self.ui.signed_in.get_untracked(),
            "Sign in to edit the shared Ergo Sandbox"
        );
        anyhow::ensure!(
            self.shared.is_none(),
            "This is a bounded shared-world navigation view, not a complete editable mechanical domain. Use the MCP/shared-world API for edits."
        );
        let world = self
            .world
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Open a construction first"))?;
        anyhow::ensure!(
            world.brick_count <= physics::MAX_SIMULATION_BRICKS && world.blocks.len() <= 128,
            "The interactive physics/edit workspace is limited to 5,000 bricks and 128 blocks. Larger worlds remain streamable"
        );
        self.load_all = true;
        Ok(())
    }
    fn require_all(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self
                .common_binding
                .as_ref()
                .is_some_and(|b| b.external_dependencies),
            "This shared assembly has external mechanical dependencies; local isolated manipulation/simulation would be incomplete"
        );
        self.request_all()?;
        anyhow::ensure!(
            self.complete(),
            "Loading the complete construction. Try again when the loading indicator finishes"
        );
        Ok(())
    }
    fn remember(&mut self) -> anyhow::Result<()> {
        self.require_all()?;
        if self.undo.is_none() {
            self.undo = Some(self.scene()?);
        }
        self.ui.dirty.set(true);
        Ok(())
    }
    fn begin_physics(&mut self) -> anyhow::Result<()> {
        self.remember()?;
        if self.simulation.is_none() {
            let r = self.renderer.as_ref().unwrap();
            self.simulation = Some(Simulation::new(
                &r.bricks,
                r.meshes(),
                &self.joints,
                &self.fixed,
                false,
            )?);
        }
        self.renderer.as_mut().unwrap().explode = 0.0;
        self.renderer.as_mut().unwrap().changed();
        self.playing = true;
        self.ui.playing.set(true);
        self.ui
            .message
            .set("Gravity simulation · bounding-box collisions".into());
        Ok(())
    }
    fn freeze_simulation(&mut self) {
        self.playing = false;
        self.ui.playing.set(false);
        if let Some(sim) = self.simulation.take() {
            self.joints = sim.current_joints();
            self.renderer.as_mut().unwrap().set_bricks(sim.bricks());
        }
    }
    fn move_selected(&mut self, delta: DVec3, pull: bool) -> anyhow::Result<()> {
        self.remember()?;
        let id = self
            .renderer
            .as_ref()
            .and_then(|r| r.selected)
            .ok_or_else(|| anyhow::anyhow!("Select a brick in the viewport"))?;
        if pull {
            self.begin_physics()?;
            let brick = self.renderer.as_ref().unwrap().selected_brick().unwrap();
            self.simulation
                .as_mut()
                .unwrap()
                .drive(id, brick.position + delta)?;
            self.ui.message.set(
                "Pulling the selected brick; joints and collisions constrain its motion".into(),
            );
            return Ok(());
        }
        self.freeze_simulation();
        let group = physics::connected_group(id, &self.joints);
        anyhow::ensure!(
            !self.fixed.iter().any(|id| group.contains(id)),
            "The group contains an anchored brick"
        );
        anyhow::ensure!(
            !self
                .joints
                .iter()
                .any(|j| !matches!(j.kind, JointKind::Fixed)
                    && (group.contains(&j.a) || group.contains(&j.b))),
            "Use Pull in simulation to move an articulated group"
        );
        let r = self.renderer.as_mut().unwrap();
        let bounds = r
            .bricks
            .iter()
            .filter(|b| group.contains(&b.id))
            .map(|b| b.bounds)
            .reduce(ergo_world::Aabb::union)
            .ok_or_else(|| anyhow::anyhow!("Empty selection"))?;
        let delta = ergo_world::placement::snap_translation(bounds, delta);
        physics::can_translate(&r.bricks, &group, delta)?;
        let mut bricks = r.bricks.clone();
        for brick in &mut bricks {
            if group.contains(&brick.id) {
                brick.translated(delta);
            }
        }
        for joint in &mut self.joints {
            if group.contains(&joint.a) && group.contains(&joint.b) {
                joint.anchor += delta;
            }
        }
        r.set_bricks(bricks);
        self.refresh_selection();
        self.ui.message.set(format!(
            "Moved {} connected bricks; overlap check passed",
            group.len()
        ));
        Ok(())
    }
}

fn with_client<T>(handle: Handle, f: impl FnOnce(&mut Client) -> T) -> T {
    handle.with_value(|c| f(&mut c.borrow_mut()))
}
fn act(handle: Handle, f: impl FnOnce(&mut Client) -> anyhow::Result<()>) {
    with_client(handle, |c| {
        if c.ui.common_world.get_untracked().is_some() && c.ui.busy.get_untracked() {
            return;
        }
        c.ui.error.set(None);
        if let Err(e) = f(c) {
            c.ui.fail(e);
        }
    });
}

async fn json<T: serde::de::DeserializeOwned>(url: &str) -> anyhow::Result<T> {
    let response = Request::get(url).send().await?;
    if !response.ok() {
        anyhow::bail!(response.text().await?);
    }
    Ok(response.json().await?)
}

fn logout(ui: Ui) {
    if ui.signing_out.get_untracked() {
        return;
    }
    ui.signing_out.set(true);
    ui.error.set(None);
    ui.message.set("Signing out…".into());
    spawn_local(async move {
        if let Err(error) = crate::session::sign_out().await {
            ui.signing_out.set(false);
            ui.fail(error);
        }
    });
}

async fn object<T: serde::de::DeserializeOwned>(world: &str, hash: &str) -> anyhow::Result<T> {
    let response = Request::get(&format!("/api/worlds/{world}/objects/{hash}"))
        .send()
        .await?;
    if !response.ok() {
        anyhow::bail!(response.text().await?);
    }
    let bytes = response.binary().await?;
    Ok(ciborium::from_reader(bytes.as_slice())?)
}

fn shared_manifest(
    id: &str,
    scene: &WorldExport,
    coord: [i64; 3],
    blocks: &BTreeMap<BlockCoord, Block>,
) -> WorldManifest {
    let center = BlockCoord::from(coord).origin() + DVec3::new(512., 16., 512.);
    WorldManifest {
        schema: ergo_world::SCHEMA_VERSION,
        id: format!("shared:{id}"),
        name: scene.name.clone(),
        source: "Shared-world spatial view".into(),
        brick_count: scene.bricks.len(),
        bounds: ergo_world::bounds(&scene.bricks).unwrap_or(ergo_world::Aabb::new(
            center - DVec3::splat(16.),
            center + DVec3::splat(16.),
        )),
        geo: scene.geo,
        blocks: blocks
            .values()
            .filter(|b| !b.bricks.is_empty())
            .map(|b| ergo_world::BlockInfo {
                coord: b.coord,
                key: b.coord.key(),
                brick_count: b.bricks.len(),
            })
            .collect(),
        geometry: BTreeMap::new(),
        proxy_parts: Vec::new(),
        notes: vec!["Read-only bounded region; edit through the shared-world API/MCP".into()],
        joints: Vec::new(),
        fixed_bricks: Vec::new(),
        validation: ergo_world::validation::ValidationReport {
            collision_model: "View only".into(),
            method: "Bounded region, not a full mechanical certificate".into(),
            inferred_joints: 0,
            checked_bricks: 0,
            max_displacement: 0.,
            joint_strength_required: None,
            compression_issue: None,
            physical_calibration: "No physical claim for a clipped navigation view".into(),
            tolerances: Default::default(),
        },
        owner: "shared".into(),
        ground: Vec::new(),
        groups: Vec::new(),
        model: None,
    }
}

fn open_shared(handle: Handle, location: SharedLocation) {
    let coord: [i64; 3] = location
        .coord
        .unwrap_or_else(|| BlockCoord::from([0, 0, 0]))
        .into();
    with_client(handle, |c| {
        c.ui.focused_assembly.set(None);
        c.common_binding = None;
        c.common_attempt = None;
        c.common_fenced = false;
        c.ui.editing_shared.set(false);
        c.generation += 1;
        c.ui.loading.set(None);
        c.blocks.clear();
        c.pending.clear();
        c.ground_pending.clear();
        c.ground_retry.clear();
        c.known_cells.clear();
        c.navigation_controls.clear();
        c.simulation = None;
        c.playing = false;
        c.undo = None;
        c.joints.clear();
        c.fixed.clear();
        c.groups.clear();
        c.load_all = false;
        c.start_when_loaded = false;
        c.ready = false;
        let scene = WorldExport {
            schema: ergo_world::SCHEMA_VERSION,
            name: location.metadata.name,
            geo: location.metadata.geo,
            bricks: Vec::new(),
            meshes: BTreeMap::new(),
            joints: Vec::new(),
            fixed_bricks: Vec::new(),
            groups: Vec::new(),
            ground: Vec::new(),
            model: None,
        };
        let manifest = shared_manifest(&location.world, &scene, coord, &c.blocks);
        c.world = Some(manifest.clone());
        if let Some(renderer) = &mut c.renderer {
            renderer.clear();
            renderer.fit(manifest.bounds);
        }
        c.ui.current.set(Some(manifest));
        c.ui.dirty.set(false);
        c.ui.playing.set(false);
        c.ui.selected.set(None);
        c.ui.authoring_groups.set(Vec::new());
        c.ui.group_selection.set(Vec::new());
        c.ui.busy.set(true);
        c.shared = Some(SharedView {
            assemblies: BTreeMap::new(),
            fully_resident_assemblies: HashSet::new(),
            id: location.world.clone(),
            coord,
            last_check: 0.,
            loading: true,
            revision: String::new(),
        });
    });
    request_shared_region(handle, location.world, coord, true);
}

fn request_shared_region(handle: Handle, id: String, coord: [i64; 3], fit_view: bool) {
    let (generation, request_id, previous) = with_client(handle, |c| {
        c.region_request += 1;
        c.ui.loading
            .set(Some(crate::loading::Loading::new(js_sys::Date::now())));
        (
            c.generation,
            c.region_request,
            c.shared.as_ref().map(|shared| shared.coord),
        )
    });
    spawn_local(async move {
        let mut url = format!(
            "/api/shared-worlds/{id}/view?x={}&y={}&z={}&format=cbor",
            coord[0], coord[1], coord[2]
        );
        if let Some(previous) = previous.filter(|previous| *previous != coord) {
            url.push_str(&format!(
                "&from_x={}&from_y={}&from_z={}",
                previous[0], previous[1], previous[2]
            ));
        }
        // Pin the read and its progress polls to one accepted revision. A new
        // head must not make this loader display another request's counters.
        #[derive(Deserialize)]
        struct HeadRevision {
            revision: String,
        }
        if let Ok(head) = json::<HeadRevision>(&format!("/api/shared-worlds/{id}")).await {
            url.push_str(&format!("&revision={}", head.revision));
            poll_region_progress(handle, url.clone(), head.revision, generation, request_id);
        }
        if !with_client(handle, |c| {
            c.generation == generation && c.region_request == request_id
        }) {
            return;
        }
        let result: anyhow::Result<SharedRegion> = async {
            let cached = crate::read_cache::get(&url).await;
            let mut request = Request::get(&url).header("Accept", "application/cbor");
            if let Some(cached) = &cached {
                request = request.header("If-None-Match", &cached.etag);
            }
            let response = request.send().await?;
            if response.status() == 304 {
                let cached =
                    cached.context("Server revalidated an unavailable local construction")?;
                region_loading_stage(
                    handle,
                    generation,
                    request_id,
                    "Opening local construction",
                    "Reusing the verified construction and spatial index…",
                );
                gloo_timers::future::TimeoutFuture::new(16).await;
                return Ok(ciborium::from_reader(cached.bytes.as_slice())?);
            }
            if !response.ok() {
                if [401, 403].contains(&response.status()) {
                    crate::read_cache::forget(&url).await;
                }
                anyhow::bail!(response.text().await?);
            }
            region_loading_stage(
                handle,
                generation,
                request_id,
                "Receiving construction",
                "Downloading the prepared region…",
            );
            if response
                .headers()
                .get("content-type")
                .is_some_and(|v| v.starts_with("application/cbor"))
            {
                let bytes = response.binary().await?;
                region_loading_stage(
                    handle,
                    generation,
                    request_id,
                    "Decoding construction",
                    "Reading brick geometry…",
                );
                gloo_timers::future::TimeoutFuture::new(16).await;
                let region = ciborium::from_reader(bytes.as_slice())?;
                if let Some(etag) = response.headers().get("etag") {
                    if with_client(handle, |c| {
                        c.generation == generation
                            && c.region_request == request_id
                            && !c.ui.signing_out.get_untracked()
                    }) {
                        crate::read_cache::put(&url, &etag, &bytes).await;
                    }
                }
                Ok(region)
            } else {
                // Rolling-deployment compatibility with the previous JSON route.
                let text = response.text().await?;
                region_loading_stage(
                    handle,
                    generation,
                    request_id,
                    "Decoding construction",
                    "Reading brick geometry…",
                );
                gloo_timers::future::TimeoutFuture::new(16).await;
                Ok(serde_json::from_str(&text)?)
            }
        }
        .await;
        if let Ok(region) = &result {
            region_loading_stage(
                handle,
                generation,
                request_id,
                "Preparing display",
                &format!(
                    "Preparing {} bricks for the 3D view…",
                    number(region.scene.bricks.len())
                ),
            );
            gloo_timers::future::TimeoutFuture::new(16).await;
        }
        with_client(handle, |c| {
            if c.generation != generation
                || c.region_request != request_id
                || c.shared.as_ref().is_none_or(|s| s.id != id)
            {
                return;
            }
            c.ui.loading.set(None);
            if let Some(shared) = &mut c.shared {
                shared.loading = false;
                shared.last_check = js_sys::Date::now();
                shared.coord = coord;
            }
            match result {
                Ok(region) => {
                    if let Some(shared) = &mut c.shared {
                        shared.assemblies = region.assemblies;
                        shared.fully_resident_assemblies = region.fully_resident_assemblies;
                        shared.revision = region.revision.clone();
                    }
                    let scene = region.scene;
                    let mut blocks = match ergo_world::partition(&scene.bricks) {
                        Ok(blocks) => blocks,
                        Err(error) => {
                            c.ui.fail(error);
                            return;
                        }
                    };
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                let cell =
                                    BlockCoord::from([coord[0] + dx, coord[1] + dy, coord[2] + dz]);
                                blocks.entry(cell).or_insert(Block {
                                    schema: ergo_world::SCHEMA_VERSION,
                                    coord: cell,
                                    bricks: Vec::new(),
                                });
                            }
                        }
                    }
                    let manifest = shared_manifest(&id, &scene, coord, &blocks);
                    let focus = c.ui.focused_assembly.get_untracked().and_then(|assembly| {
                        let shared = c.shared.as_ref()?;
                        scene
                            .bricks
                            .iter()
                            .filter(|brick| shared.assemblies.get(&brick.id) == Some(&assembly))
                            .map(|brick| brick.render_bounds)
                            .reduce(ergo_world::Aabb::union)
                    });
                    c.blocks = blocks;
                    c.known_cells = c.blocks.keys().copied().collect();
                    c.world = Some(manifest.clone());
                    if let Some(renderer) = &mut c.renderer {
                        // Keep unchanged LOD/GPU prototypes on region refresh.
                        // set_meshes invalidates only changed/removed parts.
                        renderer.selected = None;
                        renderer.highlighted.clear();
                        renderer.explode = 0.;
                        renderer.set_meshes(scene.meshes);
                        renderer.set_bricks(scene.bricks);
                        if fit_view {
                            renderer.fit(focus.unwrap_or(manifest.bounds));
                        }
                    }
                    c.ready = true;
                    c.ui.current.set(Some(manifest));
                    c.ui.selected.set(None);
                    c.ui.busy.set(false);
                    c.ui.message.set(format!(
                        "Shared-world region loaded · revision {} · edit using MCP / API",
                        &region.revision[..region.revision.len().min(12)]
                    ));
                }
                Err(error) => c.ui.fail(error),
            }
        });
    });
}

fn region_loading_stage(
    handle: Handle,
    generation: u64,
    request_id: u64,
    title: &str,
    detail: &str,
) {
    with_client(handle, |c| {
        if c.generation == generation && c.region_request == request_id {
            c.ui.loading.update(|loading| {
                if let Some(loading) = loading {
                    loading.client_stage(title, detail);
                }
            });
        }
    });
}

fn poll_region_progress(
    handle: Handle,
    url: String,
    revision: String,
    generation: u64,
    request_id: u64,
) {
    spawn_local(async move {
        loop {
            let waiting = with_client(handle, |c| {
                c.generation == generation
                    && c.region_request == request_id
                    && c.ui
                        .loading
                        .get_untracked()
                        .is_some_and(|loading| loading.waiting_for_server)
            });
            if !waiting {
                return;
            }
            if let Ok(status) =
                json::<crate::loading::ReadStatus>(&format!("{url}&status=true")).await
            {
                with_client(handle, |c| {
                    if c.generation == generation
                        && c.region_request == request_id
                        && status.revision == revision
                    {
                        c.ui.loading.update(|loading| {
                            if let Some(loading) = loading {
                                loading.server_status(status);
                            }
                        });
                    }
                });
            }
            // Progress is optional: a failed poll never aborts or retries the read.
            gloo_timers::future::TimeoutFuture::new(2000).await;
        }
    });
}

fn pump_shared(handle: Handle) {
    let request = with_client(handle, |c| {
        let eye = c.renderer.as_ref()?.eye();
        let shared = c.shared.as_mut()?;
        let coord: [i64; 3] = BlockCoord::at(eye).into();
        if shared.loading
            || (coord == shared.coord && js_sys::Date::now() - shared.last_check < 5000.)
        {
            return None;
        }
        let same_region = coord == shared.coord;
        shared.loading = true;
        Some((
            shared.id.clone(),
            coord,
            same_region,
            c.generation,
            shared.revision.clone(),
        ))
    });
    if let Some((id, coord, same_region, generation, revision)) = request {
        if !same_region {
            request_shared_region(handle, id, coord, false);
            return;
        }
        spawn_local(async move {
            #[derive(Deserialize)]
            struct HeadRevision {
                revision: String,
            }
            match json::<HeadRevision>(&format!("/api/shared-worlds/{id}")).await {
                Ok(head) if head.revision != revision => {
                    if with_client(handle, |c| c.generation == generation) {
                        request_shared_region(handle, id, coord, false);
                    }
                }
                result => with_client(handle, |c| {
                    if c.generation != generation {
                        return;
                    }
                    if let Some(shared) = &mut c.shared {
                        shared.loading = false;
                        shared.last_check = js_sys::Date::now();
                    }
                    if let Err(error) = result {
                        c.ui.fail(error);
                    }
                }),
            }
        });
    }
}

fn loading_indicator(ui: Ui) -> impl IntoView {
    view! {
        <Show when=move ||ui.loading.get().is_some() fallback=move ||view!{
            <Show when=move ||ui.busy.get()><div class="loading-pill" role="status"><span class="spinner" aria-hidden="true"></span>"Loading construction…"</div></Show>
        }>
            <div class="loading-pill loading-panel">
                <div class="loading-announcement" role="status" aria-live="polite" aria-atomic="true">
                    <div class="loading-heading">
                        <strong class="loading-stage">{move ||ui.loading.get().map(|loading|loading.title).unwrap_or_default()}</strong>
                        <span class="loading-percent">{move ||ui.loading.get().and_then(|loading|loading.percent).map(|percent|format!("{percent}%"))}</span>
                    </div>
                    <div class="loading-detail">{move ||ui.loading.get().map(|loading|loading.detail).unwrap_or_default()}</div>
                </div>
                <progress max="100" value=move ||ui.loading.get().and_then(|loading|loading.percent)
                    aria-label=move ||ui.loading.get().map(|loading|loading.title).unwrap_or_default()
                    aria-valuetext=move ||ui.loading.get().map(|loading|loading.detail).unwrap_or_default()/>
                <div class="loading-footnote" aria-hidden="true"><span>"Current stage"</span><span>{move ||ui.loading.get().map(|loading|loading.elapsed()).unwrap_or_default()}</span></div>
            </div>
        </Show>
    }
}

fn more_shared_regions(handle: Handle) {
    let cursor = with_client(handle, |c| c.ui.shared_cursor.get_untracked());
    if let Some(cursor) = cursor {
        with_client(handle, |c| c.ui.shared_cursor.set(None));
        spawn_local(async move {
            let encoded = js_sys::encode_uri_component(&cursor)
                .as_string()
                .unwrap_or_default();
            match json::<SharedPage>(&format!(
                "/api/shared-worlds?quad=&limit=32&cursor={encoded}"
            ))
            .await
            {
                Ok(page) => with_client(handle, |c| {
                    c.ui.shared_locations
                        .update(|items| items.extend(page.items));
                    c.ui.shared_cursor.set(page.next);
                }),
                Err(error) => with_client(handle, |c| {
                    c.ui.shared_cursor.set(Some(cursor));
                    c.ui.fail(error);
                }),
            }
        });
    }
}

fn open_world(handle: Handle, world: WorldManifest) {
    let generation = with_client(handle, |c| {
        c.generation += 1;
        c.ui.loading.set(None);
        c.shared = None;
        c.world = Some(world.clone());
        c.known_cells = world.blocks.iter().map(|b| b.coord).collect();
        c.navigation_controls.clear();
        c.blocks.clear();
        c.pending.clear();
        c.ground_pending.clear();
        c.ground_retry.clear();
        c.ready = false;
        c.load_all = false;
        c.start_when_loaded = false;
        c.simulation = None;
        c.playing = false;
        c.undo = None;
        c.joints = world.joints.clone();
        c.fixed = world.fixed_bricks.clone();
        c.groups = world.groups.clone();
        c.ui.authoring_groups.set(c.groups.clone());
        c.ui.group_selection.set(Vec::new());
        if let Some(r) = &mut c.renderer {
            r.clear();
            r.fit(world.bounds);
            for block in &world.ground {
                if c.ground_provider
                    .as_ref()
                    .is_none_or(|provider| provider == &block.source)
                {
                    r.set_ground(block.clone());
                }
            }
            if world.bounds.size().max_element() > 1024.0
                && let Some(first) = world.blocks.first()
            {
                r.target = first.coord.origin() + DVec3::new(512.0, 32.0, 512.0);
                r.distance = 900.0;
                r.changed();
            }
        }
        c.ui.current.set(Some(world.clone()));
        c.ui.dirty.set(false);
        c.ui.playing.set(false);
        c.ui.stats.set(SimulationStats::default());
        c.ui.selected.set(None);
        c.ui.first.set(None);
        c.ui.error.set(None);
        c.ui.message.set("Loading part geometries…".into());
        c.ui.busy.set(true);
        c.ui.joint_count.set(c.joints.len());
        c.generation
    });
    spawn_local(async move {
        let result: anyhow::Result<BTreeMap<String, ergo_world::geometry::Mesh>> = async {
            let mut meshes = BTreeMap::new();
            for (part, hash) in &world.geometry {
                let mut mesh: ergo_world::geometry::Mesh = object(&world.id, hash).await?;
                if mesh.quality != "source" {
                    #[derive(Deserialize)]
                    struct PartAsset {
                        mesh: ergo_world::geometry::Mesh,
                    }
                    if let Ok(asset) = json::<PartAsset>(&format!("/api/parts/{part}")).await
                        && asset.mesh.quality == "source"
                    {
                        mesh = asset.mesh;
                    }
                }
                meshes.insert(part.clone(), mesh);
            }
            Ok(meshes)
        }
        .await;
        with_client(handle, |c| {
            if c.generation != generation {
                return;
            }
            match result {
                Ok(meshes) => {
                    c.renderer.as_mut().unwrap().set_meshes(meshes);
                    c.ready = true;
                    c.ui.message
                        .set("Streaming visible blocks inside the fog horizon…".into());
                }
                Err(e) => c.ui.fail(e),
            }
        });
    });
}

fn open_draft(handle: Handle, scene: WorldExport) -> anyhow::Result<()> {
    let blocks = ergo_world::partition(&scene.bricks)?;
    let bounds = ergo_world::bounds(&scene.bricks)
        .unwrap_or_else(|| ergo_world::Aabb::new(DVec3::ZERO, DVec3::splat(10.)));
    with_client(handle, |c| {
        c.common_binding = None;
        c.common_attempt = None;
        c.common_fenced = false;
        c.ui.editing_shared.set(false);
        c.generation += 1;
        c.shared = None;
        c.ui.loading.set(None);
        c.pending.clear();
        c.ground_pending.clear();
        c.ground_retry.clear();
        c.simulation = None;
        c.playing = false;
        c.ready = true;
        c.load_all = true;
        c.start_when_loaded = false;
        c.undo = None;
        c.joints = scene.joints.clone();
        c.fixed = scene.fixed_bricks.clone();
        c.groups = scene.groups.clone();
        c.ui.authoring_groups.set(c.groups.clone());
        c.ui.group_selection.set(Vec::new());
        let world = WorldManifest {
            schema: scene.schema,
            id: "draft".into(),
            name: scene.name,
            source: "Unpublished workspace".into(),
            brick_count: scene.bricks.len(),
            bounds,
            geo: scene.geo,
            blocks: blocks
                .values()
                .map(|b| ergo_world::BlockInfo {
                    coord: b.coord,
                    key: b.coord.key(),
                    brick_count: b.bricks.len(),
                })
                .collect(),
            geometry: BTreeMap::new(),
            proxy_parts: Vec::new(),
            notes: Vec::new(),
            joints: c.joints.clone(),
            fixed_bricks: c.fixed.clone(),
            validation: ergo_world::validation::ValidationReport {
                collision_model: "bounding boxes".into(),
                method: "Not yet validated".into(),
                inferred_joints: 0,
                checked_bricks: 0,
                max_displacement: 0.0,
                joint_strength_required: None,
                compression_issue: None,
                physical_calibration: "Not yet validated".into(),
                tolerances: Default::default(),
            },
            owner: "workspace".into(),
            ground: scene.ground.clone(),
            groups: scene.groups.clone(),
            model: scene.model.clone(),
        };
        c.blocks = blocks;
        c.world = Some(world.clone());
        c.known_cells = world.blocks.iter().map(|b| b.coord).collect();
        c.navigation_controls.clear();
        let r = c.renderer.as_mut().unwrap();
        r.clear();
        r.set_meshes(scene.meshes);
        r.set_bricks(scene.bricks);
        r.fit(bounds);
        for block in scene.ground {
            if c.ground_provider
                .as_ref()
                .is_none_or(|provider| provider == &block.source)
            {
                r.set_ground(block);
            }
        }
        c.ui.current.set(Some(world));
        c.ui.focused_assembly.set(None);
        c.ui.dirty.set(true);
        c.ui.playing.set(false);
        c.ui.busy.set(false);
        c.ui.message.set(
            "Imported into the workspace. Saving checks overlaps and gravity stability".into(),
        );
        c.refresh_selection();
    });
    Ok(())
}

fn pump(handle: Handle) {
    pump_ground(handle);
    if with_client(handle, |c| c.common_binding.is_some()) {
        return;
    }
    if with_client(handle, |c| c.shared.is_some()) {
        pump_shared(handle);
        return;
    }
    let requests = with_client(handle, |c| {
        if !c.ready || c.simulation.is_some() || c.ui.dirty.get_untracked() {
            return Vec::new();
        }
        let Some(world) = c.world.clone() else {
            return Vec::new();
        };
        let Some(r) = &mut c.renderer else {
            return Vec::new();
        };
        let camera = BlockCoord::at(r.eye());
        let mut desired;
        loop {
            let visibility = r.visibility();
            desired = world
                .blocks
                .iter()
                .filter(|b| {
                    c.load_all
                        || visibility.contains(ergo_world::Aabb::new(
                            b.coord.origin(),
                            b.coord.origin() + DVec3::splat(1024.0),
                        ))
                })
                .cloned()
                .collect::<Vec<_>>();
            if c.load_all
                || (desired.len() <= 128
                    && desired.iter().map(|b| b.brick_count).sum::<usize>() <= 200_000)
            {
                break;
            }
            if r.fog_distance <= 200.0 {
                c.ui.fail("A visible storage cell exceeds the resident budget; subdivide this dense block into render pages");
                return Vec::new();
            }
            r.fog_distance = (r.fog_distance * 0.75).max(200.0);
            r.distance = r.distance.min(r.fog_distance * 0.85);
            r.changed();
        }
        desired.sort_by_key(|b| {
            (b.coord.0 - camera.0).abs()
                + (b.coord.1 - camera.1).abs()
                + (b.coord.2 - camera.2).abs()
        });
        let keep: HashSet<_> = desired.iter().map(|b| b.coord).collect();
        let before = c.blocks.len();
        c.blocks.retain(|coord, _| keep.contains(coord));
        if before != c.blocks.len() {
            c.rebuild();
        }
        let available = 4_usize.saturating_sub(c.pending.len());
        let requests: Vec<_> = desired
            .into_iter()
            .filter(|b| !c.blocks.contains_key(&b.coord) && !c.pending.contains(&b.key))
            .take(available)
            .collect();
        if requests.is_empty() && c.pending.is_empty() {
            c.ui.busy.set(false);
            if c.blocks.is_empty() {
                c.ui.message.set(
                    "No occupied blocks near the camera. Fit the construction or go to a location"
                        .into(),
                );
            }
        }
        requests
            .into_iter()
            .map(|b| {
                c.pending.insert(b.key.clone());
                (world.id.clone(), b, c.generation)
            })
            .collect::<Vec<_>>()
    });
    for (world, info, generation) in requests {
        spawn_local(async move {
            let block: anyhow::Result<Block> = object(&world, &info.key).await;
            with_client(handle, |c| {
                if c.generation != generation {
                    return;
                }
                c.pending.remove(&info.key);
                match block {
                    Ok(block) => {
                        if !c.load_all
                            && c.renderer.as_ref().is_some_and(|r| {
                                !r.visibility().contains(ergo_world::Aabb::new(
                                    block.coord.origin(),
                                    block.coord.origin() + DVec3::splat(1024.0),
                                ))
                            })
                        {
                            return;
                        }
                        if c.blocks.values().map(|b| b.bricks.len()).sum::<usize>()
                            + block.bricks.len()
                            > 200_000
                        {
                            c.ready = false;
                            c.ui.fail(
                                "Resident block budget exceeded. Reduce the fog/view distance",
                            );
                            return;
                        }
                        c.blocks.insert(block.coord, block);
                        c.rebuild();
                        c.ui.busy.set(!c.pending.is_empty());
                        c.ui.message.set(
                            if c.complete() {
                                "Construction loaded · browser download cache enabled"
                            } else {
                                "Visible blocks loaded · distance LOD and fog enabled"
                            }
                            .into(),
                        );
                        if c.start_when_loaded && c.complete() {
                            c.start_when_loaded = false;
                            if let Err(e) = c.begin_physics() {
                                c.ui.fail(e);
                            }
                        }
                    }
                    Err(e) => {
                        c.ready = false;
                        c.ui.fail(e);
                    }
                }
            });
        });
    }
}

fn pump_ground(handle: Handle) {
    let requests = with_client(handle, |c| {
        let Some(renderer) = &mut c.renderer else {
            return Vec::new();
        };
        let geo = c.world.as_ref().map(|w| w.geo).unwrap_or(c.default_geo);
        let mut cells = renderer.visible_ground_cells();
        cells.sort_by(|a, b| {
            let distance = |(x, y): (i64, i64)| {
                ((x as f64 + 0.5) * 1024.0 - renderer.eye().x)
                    .hypot((y as f64 + 0.5) * 1024.0 - renderer.eye().z)
            };
            distance(*a).total_cmp(&distance(*b))
        });
        let keep: HashSet<_> = cells.iter().copied().collect();
        let before = renderer.ground.len();
        renderer.ground.retain(|key, _| keep.contains(key));
        if before != renderer.ground.len() {
            renderer.changed();
        }
        for key in &cells {
            if !renderer.ground.contains_key(key)
                && let Some(block) = c.world.as_ref().and_then(|w| {
                    w.ground.iter().find(|b| {
                        (b.coord.0, b.coord.2) == *key
                            && c.ground_provider
                                .as_ref()
                                .is_none_or(|provider| provider == &b.source)
                    })
                })
            {
                renderer.set_ground(block.clone());
            }
        }
        let now = js_sys::Date::now();
        let available = 4_usize.saturating_sub(c.ground_pending.len());
        let needed: Vec<_> = cells
            .into_iter()
            .filter(|key| {
                !c.ground_pending.contains(key)
                    && renderer.ground.get(key).is_none_or(|block| {
                        block.fallback_reason.is_some()
                            && now - c.ground_retry.get(key).copied().unwrap_or(0.0) > 60_000.0
                    })
            })
            .take(available)
            .collect();
        needed
            .into_iter()
            .map(|(x, y)| {
                c.ground_pending.insert((x, y));
                (x, y, geo, c.generation)
            })
            .collect::<Vec<_>>()
    });
    for (x, y, geo, generation) in requests {
        spawn_local(async move {
            let north = !y;
            let url = format!(
                "/api/ground/{x}/{north}?latitude={}&longitude={}&arcseconds_per_block={}",
                geo.latitude, geo.longitude, geo.arcseconds_per_block
            );
            let result = json::<ergo_world::terrain::GroundBlock>(&url).await;
            with_client(handle, |c| {
                if c.generation != generation {
                    return;
                }
                c.ground_pending.remove(&(x, y));
                c.ground_retry.insert((x, y), js_sys::Date::now());
                let block = result.unwrap_or_else(|e| {
                    ergo_world::terrain::GroundBlock::fallback(x, north, e.to_string())
                });
                if let Some(reason) = &block.fallback_reason {
                    c.ui.ground_status
                        .set(format!("Ground: palette fallback ({reason})"));
                } else {
                    c.ui.ground_status
                        .set(format!("Ground: {} · standard brick palette", block.source));
                }
                if let Some(url) = &block.attribution_url {
                    c.ui.ground_attribution
                        .set(Some((block.attribution.clone(), url.clone())));
                }
                if let Some(renderer) = &mut c.renderer {
                    renderer.set_ground(block);
                }
            });
        });
    }
}

#[component]
pub fn App() -> impl IntoView {
    let ui = Ui::new();
    let handle = StoredValue::new_local(Rc::new(RefCell::new(Client::new(ui))));
    provide_context(handle);
    provide_context(ui);
    presence::install(handle);
    let canvas = NodeRef::<html::Canvas>::new();
    let mounted = StoredValue::new(false);
    let fullscreen_listener =
        leptos::leptos_dom::helpers::window_event_listener_untyped("fullscreenchange", move |_| {
            let active = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.fullscreen_element())
                .is_some_and(|e| e.id() == "world-viewport");
            ui.fullscreen.set(active);
            focus_world_canvas();
        });
    let escape_listener =
        leptos::leptos_dom::helpers::window_event_listener(leptos::ev::keydown, move |event| {
            if event.key() == "Escape" && ui.fullscreen.get_untracked() {
                event.prevent_default();
                toggle_fullscreen(ui);
            }
        });
    on_cleanup(move || {
        fullscreen_listener.remove();
        escape_listener.remove();
    });
    Effect::new(move |_| {
        if mounted.get_value() {
            return;
        }
        let Some(node) = canvas.get() else {
            return;
        };
        mounted.set_value(true);
        let renderer = Renderer::new(node);
        match renderer {
            Ok(r) => with_client(handle, |c| c.renderer = Some(r)),
            Err(e) => {
                ui.fail(e.as_string().unwrap_or_default());
                return;
            }
        }
        let mut last = js_sys::Date::now();
        let mut frames = 0;
        let mut measured = last;
        let mut ticks = 0;
        gloo_timers::callback::Interval::new(16, move || {
            let now = js_sys::Date::now();
            let delta = ((now - last) / 1000.0).clamp(0.0, 0.1);
            last = now;
            frames += 1;
            ticks += 1;
            with_client(handle, |c| {
                if c.playing {
                    c.accumulator = (c.accumulator + delta).min(8.0 * physics::TIMESTEP);
                    if let Some(sim) = &mut c.simulation {
                        while c.accumulator >= physics::TIMESTEP {
                            sim.step();
                            c.accumulator -= physics::TIMESTEP;
                        }
                        c.renderer.as_mut().unwrap().set_bricks(sim.bricks());
                        if ticks % 15 == 0 {
                            ui.stats.set(sim.stats());
                        }
                    }
                }
                let geo = c
                    .world
                    .as_ref()
                    .map(|world| world.geo)
                    .unwrap_or(c.default_geo);
                if let Some(r) = &mut c.renderer {
                    r.geo = geo;
                    let input =
                        NavigationInput::from_controls(c.navigation_controls.iter().copied());
                    let known = &c.known_cells;
                    let loaded = &c.blocks;
                    let shared_view = c.shared.is_some();
                    r.navigate(input, delta, &|cell| {
                        (shared_view || known.contains(&cell)) && !loaded.contains_key(&cell)
                    });
                    r.render();
                    let status = r.navigation.status();
                    if status.mode != NavigationMode::Orbit || ticks % 15 == 0 {
                        let _ = r
                            .canvas
                            .set_attribute("data-navigation", status.mode.name());
                        let _ = r
                            .canvas
                            .set_attribute("data-navigation-speed", &status.speed.to_string());
                        let _ = r.canvas.set_attribute(
                            "data-ground-studs",
                            &r.ground_stud_instances.to_string(),
                        );
                        let _ = r.canvas.set_attribute(
                            "data-camera-position",
                            &serde_json::to_string(&ergo_world::coordinates::public(r.eye()))
                                .unwrap(),
                        );
                        let _ = r.canvas.set_attribute(
                            "data-camera-rotation",
                            &serde_json::to_string(&[r.yaw, r.pitch]).unwrap(),
                        );
                    }
                    if ticks % 15 == 0 {
                        if let Some(angle) = crate::location::north_arrow_degrees(r.eye(), r.target)
                            && ui.north_angle.get_untracked() != angle
                        {
                            ui.north_angle.set(angle);
                        }
                        let location = CameraLocation::new(geo, r.eye());
                        if ui.camera_location.get_untracked() != Some(location) {
                            ui.camera_location.set(Some(location));
                        }
                        ui.count.set(r.bricks.len());
                        ui.blocks.set(c.blocks.len());
                        ui.gpu.set(r.gpu_bytes);
                        ui.draws.set(r.draw_calls);
                        ui.nodes.set(r.tree.node_count());
                        if ui.fog.get_untracked() != r.fog_distance {
                            ui.fog.set(r.fog_distance);
                        }
                        if ui.navigation.get_untracked() != status {
                            ui.navigation.set(status);
                        }
                    }
                }
                if ticks % 15 == 0 {
                    c.refresh_selection();
                }
            });
            if ticks % 12 == 0 {
                pump(handle);
                common::pump(handle);
                presence::pump(handle);
            }
            if now - measured > 1000.0 {
                if let Some(mut loading) = ui.loading.get_untracked() {
                    loading.tick(now);
                    ui.loading.set(Some(loading));
                }
                ui.fps
                    .set((frames as f64 * 1000.0 / (now - measured)) as u32);
                frames = 0;
                measured = now;
            }
        })
        .forget();
        spawn_local(async move {
            #[derive(Deserialize)]
            struct Config {
                #[serde(default)]
                common_world: Option<String>,
                storage: String,
                geo: ergo_world::GeoReference,
                #[serde(default)]
                ground_provider: Option<String>,
            }
            #[derive(Deserialize)]
            struct Me {
                name: String,
                google_enabled: bool,
                signed_in: bool,
            }
            if let Ok(config) = json::<Config>("/api/config").await {
                ui.common_world.set(config.common_world);
                ui.storage.set(config.storage);
                with_client(handle, |c| {
                    c.default_geo = config.geo;
                    c.ground_provider = config.ground_provider;
                });
            }
            if let Ok(me) = json::<Me>("/api/auth/me").await {
                ui.user.set(me.name);
                ui.google.set(me.google_enabled);
                ui.signed_in.set(me.signed_in);
            }
            if let Some(common) = ui.common_world.get_untracked() {
                let query = web_sys::window()
                    .and_then(|w| w.location().search().ok())
                    .and_then(|q| web_sys::UrlSearchParams::new_with_str(&q).ok());
                let coord = BlockCoord::from(["x", "y", "z"].map(|key| {
                    query
                        .as_ref()
                        .and_then(|q| q.get(key))
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(0)
                }));
                #[derive(Deserialize)]
                struct Head {
                    metadata: SharedMetadata,
                }
                match json::<Head>(&format!("/api/shared-worlds/{common}")).await {
                    Ok(head) => {
                        ui.shared_locations.set(vec![SharedLocation {
                            world: common.clone(),
                            metadata: head.metadata.clone(),
                            coord: None,
                        }]);
                        open_shared(
                            handle,
                            SharedLocation {
                                world: common,
                                metadata: head.metadata,
                                coord: Some(coord),
                            },
                        );
                        common::refresh(handle);
                    }
                    Err(error) => ui.fail(error),
                }
                return;
            }
            if let Ok(page) = json::<SharedPage>("/api/shared-worlds?quad=&limit=32").await {
                ui.shared_locations.set(page.items);
                ui.shared_cursor.set(page.next);
            }
            let parameters = web_sys::window()
                .and_then(|w| w.location().search().ok())
                .and_then(|query| web_sys::UrlSearchParams::new_with_str(&query).ok());
            if let Some(parameters) = parameters
                && let Some(id) = parameters.get("shared_world")
            {
                #[derive(Deserialize)]
                struct SharedHead {
                    metadata: SharedMetadata,
                }
                let encoded = js_sys::encode_uri_component(&id)
                    .as_string()
                    .unwrap_or_default();
                match json::<SharedHead>(&format!("/api/shared-worlds/{encoded}")).await {
                    Ok(head) => {
                        let coord = ["x", "y", "z"].map(|key| {
                            parameters
                                .get(key)
                                .and_then(|n| n.parse::<i64>().ok())
                                .unwrap_or(0)
                        });
                        open_shared(
                            handle,
                            SharedLocation {
                                world: id,
                                metadata: head.metadata,
                                coord: Some(BlockCoord::from(coord)),
                            },
                        );
                    }
                    Err(error) => ui.fail(error),
                }
                return;
            }
            match json::<Vec<WorldManifest>>("/api/worlds").await {
                Ok(worlds) => {
                    let first = worlds
                        .iter()
                        .find(|w| w.name.contains("pavilion"))
                        .or(worlds.first())
                        .cloned();
                    ui.worlds.set(worlds);
                    if let Some(world) = first {
                        open_world(handle, world);
                    } else {
                        if let Some(location) = ui.shared_locations.get_untracked().first().cloned()
                        {
                            open_shared(handle, location);
                        } else {
                            ui.busy.set(false);
                            ui.message
                                .set("Import a Studio construction to start exploring".into());
                        }
                    }
                }
                Err(e) => ui.fail(e),
            }
        });
    });
    view! {
        <header class="app-header">
            <a class="brand" href="https://www.ergosandbox.com/" aria-label="Ergo home"><img src="/favicon.svg" alt="" width="38" height="38"/>"ergo"<span class="brand-description">"A world, brick by brick."</span></a>
            <div class="header-actions"><span class="account-name">{move ||ui.user.get()}</span><Show when=move ||ui.google.get()&&!ui.signed_in.get()><a class="button quiet" href="/auth/google">"Sign in with Google"</a></Show><Show when=move ||ui.google.get()&&ui.signed_in.get()><button class="button quiet sign-out" disabled=move ||ui.signing_out.get() aria-busy=move ||ui.signing_out.get().to_string() on:click=move |_|logout(ui)>{move ||if ui.signing_out.get(){"Signing out…"}else{"Sign out"}}</button></Show><button class="icon-button agent-handoff" title="Edit here with an agent" aria-label="Edit here with an agent" disabled=move ||ui.busy.get() on:click=move |_|common::agent_handoff(handle)>"⌘"</button><button class="button primary" disabled=move ||ui.signing_out.get()||ui.busy.get() on:click=move |_|show_dialog("import-dialog")>"＋ Import construction"</button></div>
        </header>
        <main class="workbench">
            <aside class="left-dock"><ConstructionBrowser/><div class="dock-bottom"><span class="storage-dot"></span>{move ||ui.storage.get()}<p>"Standard parts · native Ergo models"</p></div></aside>
            <section id="world-viewport" class="viewport-area" class:is-fullscreen=move ||ui.fullscreen.get() aria-label="3D construction workbench">
                <div class="scene-heading"><div><LiveLocation/><h1>{move ||ui.current.get().map(|w|w.name).unwrap_or_else(||"Your next construction".into())}</h1><p>{move ||ui.construction_details()}</p><NavigationModes mobile=true/><span class="canvas-focus-status">"Keyboard navigation active"</span></div><span class="snapshot-badge" class:draft=move ||ui.dirty.get()>{move ||ui.scene_label()}</span></div>
                <canvas node_ref=canvas id="world-canvas" tabindex="0" aria-label=move ||format!("3D brick construction. {} navigation. Drag to look; focus canvas for keyboard controls.",ui.navigation.get().mode.name())
                    on:contextmenu=move |e|e.prevent_default()
                    on:pointerdown=move |e|with_client(handle,|c|{let x=e.client_x() as f64;let y=e.client_y() as f64;c.pointer=Some((x,y,x,y,e.button(),e.shift_key()));if let Some(r)=&c.renderer{let _=r.canvas.set_pointer_capture(e.pointer_id());}})
                    on:pointermove=move |e|with_client(handle,|c|{if let Some((sx,sy,lx,ly,button,shift))=c.pointer {let x=e.client_x() as f64;let y=e.client_y() as f64;if let Some(r)=&mut c.renderer {if (button==2||shift)&&r.navigation.mode()==NavigationMode::Orbit{r.pan(x-lx,y-ly);}else{r.orbit(x-lx,y-ly);}}c.pointer=Some((sx,sy,x,y,button,shift));}})
                    on:pointerup=move |e|with_client(handle,|c|{if let Some((sx,sy,_,_,button,_))=c.pointer.take(){let x=e.client_x() as f64;let y=e.client_y() as f64;if (x-sx).hypot(y-sy)<5.0&&button==0 {if let Some(r)=&mut c.renderer{let rect=r.canvas.get_bounding_client_rect();r.pick(x-rect.left(),y-rect.top());}c.refresh_selection();}}})
                    on:wheel=move |e|{e.prevent_default();with_client(handle,|c|{if let Some(r)=&mut c.renderer{r.zoom(e.delta_y());}});}
                    on:keydown=move |e| {
                        if e.ctrl_key()||e.meta_key()||e.alt_key(){return;}
                        if let Some(control)=Control::from_key(&e.key()){e.prevent_default();navigation_control(handle,control,true);}
                        if e.key().eq_ignore_ascii_case("f") && !e.repeat() {if e.shift_key(){e.prevent_default();toggle_fullscreen(ui);}else{fit(handle);}}
                        if e.key().eq_ignore_ascii_case("n") && !e.repeat() {e.prevent_default();nearest_construction(handle);}
                    }
                    on:keyup=move |e|{if let Some(control)=Control::from_key(&e.key()){navigation_control(handle,control,false);}}
                    on:blur=move |_|with_client(handle,|c|c.navigation_controls.clear())
                ></canvas>
                <div class="view-tools"><button title="Fit construction (F)" aria-label="Fit construction" on:click=move |_|fit(handle)>"⊡"</button><button title="Zoom in / faster" aria-label="Zoom in" on:click=move |_|with_client(handle,|c|{if let Some(r)=&mut c.renderer{r.zoom(-180.0);}})>"＋"</button><button title="Zoom out / slower" aria-label="Zoom out" on:click=move |_|with_client(handle,|c|{if let Some(r)=&mut c.renderer{r.zoom(180.0);}})>"−"</button><button title="Top view" aria-label="Top view" on:click=move |_|with_client(handle,|c|{if let Some(r)=&mut c.renderer{r.set_navigation(NavigationMode::Orbit);r.pitch=1.55;r.changed();}})>"⌑"</button><button title="Full screen (Shift+F; Esc to exit)" aria-label=move ||if ui.fullscreen.get(){"Exit full screen"}else{"Enter full screen"} aria-pressed=move ||ui.fullscreen.get().to_string() on:click=move |_|toggle_fullscreen(ui)>"⛶"</button></div>
                <Show when=move ||ui.navigation.get().mode!=NavigationMode::Orbit><MovementPad/></Show>
                <div class="viewport-status-stack">
                <div class="north-marker" role="img" aria-label="Geographic north relative to camera heading"><div class="north-pointer" data-angle=move ||ui.north_angle.get().to_string() style:transform=move ||format!("rotate({}deg)",ui.north_angle.get())><span>"N"</span><svg viewBox="0 0 40 40" aria-hidden="true"><path d="M20 3 L29 33 L20 27 L11 33 Z" fill="#334b61"/><path d="M20 3 L20 27 L11 33 Z" fill="#8c9faf"/></svg></div></div>
                <button class="nearest-construction" title="Go to nearest construction (N)" aria-label="Nearest construction" on:click=move |_|nearest_construction(handle)>"⌖ Nearest construction"</button>
                <div class="map-attribution" title=move ||ui.ground_status.get()><Show when=move ||ui.ground_attribution.get().is_some() fallback=move ||view!{<span>"Ground: palette fallback"</span>}><a target="_blank" rel="noopener noreferrer" href=move ||ui.ground_attribution.get().map(|(_,url)|url).unwrap_or_default()>{move ||ui.ground_attribution.get().map(|(label,_)|label).unwrap_or_default()}</a></Show></div>
                <span class="render-badge">"WebGL2"<b>{move ||format!("{} fps",ui.fps.get())}</b></span>
                </div>
                <div class="viewport-footer"><span>{move ||ui.navigation.get().mode.instructions()}</span></div>
                {loading_indicator(ui)}
                <Show when=move ||ui.fullscreen.get()><ErrorToast/></Show>
            </section>
            <aside class="right-dock"><BrickInspector/><Show when=move ||ui.current.get().is_some_and(|w|w.owner=="shared") fallback=move ||view!{<SimulationControls/>}><div class="dock-section"><h2>"Shared-world navigation"</h2><p class="subtle compact">"This is a bounded spatial view. Edit complete constructions through MCP or the API; the server checks permissions, edit allowance, and measured physics."</p><a class="button full quiet" href="/#connect">"Edit with MCP / API"</a></div></Show></aside>
        </main>
        <footer class="status-bar"><span class="status-message" role="status">{move ||ui.message.get()}</span><span>{move ||format!("{} blocks · {} bricks · {} draw calls · {:.1} MB GPU",ui.blocks.get(),number(ui.count.get()),ui.draws.get(),ui.gpu.get() as f64/1e6)}</span></footer>
        <Show when=move ||!ui.fullscreen.get()><ErrorToast/></Show>
        <ImportDialog/><LocationDialog/><ConstraintDialog/><SolidityDialog/><GroupsDialog/>
    }
}

#[component]
fn LiveLocation() -> impl IntoView {
    let ui = expect_context::<Ui>();
    view! {
        <div class="camera-location" aria-label="Current camera position">
            <p class="camera-gps"><span>"Camera GPS"</span><strong
                data-latitude=move ||ui.camera_location.get().map(|p|p.gps[0].to_string())
                data-longitude=move ||ui.camera_location.get().map(|p|p.gps[1].to_string())
            >{move ||ui.camera_location.get().map(|p|p.gps_label()).unwrap_or_else(||"Locating view…".into())}</strong></p>
            <p class="camera-cell"><span>"Cell "<b>{move ||ui.camera_location.get().map(|p|p.cell_label()).unwrap_or_default()}</b></span><span title="Height in construction units, not GPS altitude">{move ||ui.camera_location.get().map(|p|format!("Height Z {:.1}",p.position[2])).unwrap_or_default()}</span></p>
        </div>
    }
}

#[component]
fn NavigationModes(#[prop(optional)] mobile: bool) -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    view! {
        <div class=if mobile {"mobile-navigation"} else {"navigation-modes"} role="group" aria-label=if mobile {"Mobile navigation mode"} else {"Navigation mode"}>
            {NavigationMode::ALL.into_iter().map(|mode| view! {
                <button class:active=move ||ui.navigation.get().mode==mode aria-pressed=move ||(ui.navigation.get().mode==mode).to_string() on:click=move |_|navigation_mode(handle,mode)>{mode.label()}</button>
            }).collect_view()}
        </div>
    }
}

#[component]
fn ErrorToast() -> impl IntoView {
    let ui = expect_context::<Ui>();
    view! {<Show when=move ||ui.error.get().is_some()><div class="error-toast" role="alert"><div><strong>"Action could not be completed"</strong><p>{move ||ui.error.get().unwrap_or_default()}</p></div><button aria-label="Dismiss error" on:click=move |_|ui.error.set(None)>"×"</button></div></Show>}
}

#[component]
fn MovementPad() -> impl IntoView {
    view! {
        <div class="movement-pad" role="group" aria-label="Movement controls">
            {Control::ALL.into_iter().map(|control| view! {<MovementButton control/>}).collect_view()}
        </div>
    }
}

#[component]
fn MovementButton(control: Control) -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    let label = move || control.label(ui.navigation.get().mode);
    view! {
        <button hidden=move ||control==Control::Jump&&ui.navigation.get().mode!=NavigationMode::Walk aria-label=label title=label
            on:pointerdown=move |e|{e.prevent_default();navigation_control(handle,control,true);}
            on:pointerup=move |_|navigation_control(handle,control,false)
            on:pointerleave=move |_|navigation_control(handle,control,false)
            on:pointercancel=move |_|navigation_control(handle,control,false)
            on:keydown=move |e|{if e.key()==" "||e.key()=="Enter"{e.prevent_default();navigation_control(handle,control,true);}}
            on:keyup=move |_|navigation_control(handle,control,false)
            on:blur=move |_|navigation_control(handle,control,false)
        >{control.symbol()}</button>
    }
}

fn navigation_control(handle: Handle, control: Control, pressed: bool) {
    with_client(handle, |c| {
        if pressed {
            c.navigation_controls.insert(control);
            if let Some(r) = &mut c.renderer
                && r.navigation.mode() == NavigationMode::Orbit
            {
                let input = NavigationInput::from_controls([control]);
                if input.turn != 0.0 || input.vertical != 0.0 {
                    r.pan(-input.turn * 30.0, -input.vertical * 30.0);
                }
            }
        } else {
            c.navigation_controls.remove(&control);
        }
    });
}

fn number(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
fn fit(handle: Handle) {
    with_client(handle, |c| {
        if let Some(w) = &c.world
            && let Some(r) = &mut c.renderer
        {
            let b = ergo_world::bounds(&r.bricks).unwrap_or(w.bounds);
            r.fit(b);
        }
    });
}
fn nearest_construction(handle: Handle) {
    let (ui, current, gps) = with_client(handle, |c| {
        let geo = c.world.as_ref().map(|w| w.geo).unwrap_or(c.default_geo);
        (
            c.ui,
            c.world.as_ref().map(|w| w.id.clone()),
            geo.to_gps(c.renderer.as_ref().map(|r| r.eye()).unwrap_or_default()),
        )
    });
    let mut worlds = ui.worlds.get_untracked();
    if let Some(mut world) = ui.current.get_untracked()
        && !worlds.iter().any(|w| w.id == world.id)
    {
        if let Some(bounds) = with_client(handle, |c| {
            c.renderer
                .as_ref()
                .and_then(|r| ergo_world::bounds(&r.bricks))
        }) {
            world.bounds = bounds;
        }
        worlds.push(world);
    }
    let nearest = worlds
        .into_iter()
        .filter_map(|world| {
            world
                .geo
                .distance_to_bounds_m(gps, world.bounds)
                .ok()
                .map(|distance| (world, distance))
        })
        .min_by(|(a, da), (b, db)| da.total_cmp(db).then_with(|| a.id.cmp(&b.id)));
    let shared = ui
        .shared_locations
        .get_untracked()
        .into_iter()
        .filter_map(|location| {
            let coord = location.coord?;
            let bounds =
                ergo_world::Aabb::new(coord.origin(), coord.origin() + DVec3::splat(1024.));
            location
                .metadata
                .geo
                .distance_to_bounds_m(gps, bounds)
                .ok()
                .map(|distance| (location, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b));
    if let Some((location, distance)) = shared
        && nearest.as_ref().is_none_or(|(_, d)| distance < *d)
    {
        if ui.dirty.get_untracked() {
            ui.fail("Save or export your working copy before opening another construction.");
            return;
        }
        let name = location.metadata.name.clone();
        open_shared(handle, location);
        ui.message.set(format!(
            "Nearest shared construction region: {name} ({distance:.0} m from the camera)"
        ));
        focus_world_canvas();
        return;
    }
    let Some((world, distance)) = nearest else {
        ui.fail("No accessible constructions were found. Import one to start exploring.");
        return;
    };
    if current.as_ref() == Some(&world.id) {
        fit(handle);
    } else {
        if ui.dirty.get_untracked() {
            ui.fail("Save or export your working copy before opening another construction.");
            return;
        }
        open_world(handle, world.clone());
    }
    ui.message.set(format!(
        "Nearest construction: {} ({distance:.0} m from the camera)",
        world.name
    ));
    focus_world_canvas();
}
fn focus_world_canvas() {
    if let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("world-canvas"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let options = web_sys::FocusOptions::new();
        options.set_prevent_scroll(true);
        let _ = canvas.focus_with_options(&options);
    }
}
fn toggle_fullscreen(ui: Ui) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let entering = !ui.fullscreen.get_untracked();
    ui.fullscreen.set(entering);
    if entering {
        // The CSS immersive layout is also the fallback on browsers that do not
        // support element fullscreen or reject the request (e.g. embedded views).
        if let Some(element) = document.get_element_by_id("world-viewport")
            && let Ok(method) =
                js_sys::Reflect::get(element.as_ref(), &JsValue::from_str("requestFullscreen"))
            && let Ok(method) = method.dyn_into::<js_sys::Function>()
            && let Ok(promise) = method
                .call0(element.as_ref())
                .and_then(|v| v.dyn_into::<js_sys::Promise>())
        {
            spawn_local(async move {
                let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                focus_world_canvas();
            });
        }
    } else if document.fullscreen_element().is_some() {
        document.exit_fullscreen();
    }
    focus_world_canvas();
}
fn navigation_mode(handle: Handle, mode: NavigationMode) {
    with_client(handle, |c| {
        c.navigation_controls.clear();
        if let Some(r) = &mut c.renderer {
            r.set_navigation(mode);
            c.ui.navigation.set(r.navigation.status());
        }
        c.ui.message.set(format!(
            "{} navigation · camera changes stay local",
            mode.name()
        ));
    });
    focus_world_canvas();
}
fn show_dialog(id: &str) {
    if let Some(dialog) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlDialogElement>().ok())
    {
        let _ = dialog.show_modal();
    }
}
fn close_dialog(id: &str) {
    if let Some(dialog) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlDialogElement>().ok())
    {
        dialog.close();
    }
}

fn group_action(handle: Handle, edit: impl FnOnce(&mut WorldExport) -> anyhow::Result<()>) {
    act(handle, |c| {
        c.require_all()?;
        c.freeze_simulation();
        let mut scene = c.scene()?;
        edit(&mut scene)?;
        ergo_world::groups::validate(&scene.bricks, &scene.groups)?;
        c.remember()?;
        c.groups = scene.groups;
        c.joints = scene.joints;
        c.fixed = scene.fixed_bricks;
        c.renderer.as_mut().unwrap().set_bricks(scene.bricks);
        c.ui.authoring_groups.set(c.groups.clone());
        c.refresh_selection();
        c.ui.message
            .set("Group edit applied to the local working copy".into());
        Ok(())
    });
}
fn group_ids(value: &str) -> anyhow::Result<Vec<u32>> {
    value
        .split([',', ' ', '\n', ';'])
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().parse::<u32>().map_err(Into::into))
        .collect()
}

#[component]
fn GroupsDialog() -> impl IntoView {
    let handle = expect_context::<Handle>();
    let ui = expect_context::<Ui>();
    let name = RwSignal::new("New group".to_owned());
    let pieces = RwSignal::new(String::new());
    let children = RwSignal::new(String::new());
    let selected = RwSignal::new(0_u32);
    let reparent = RwSignal::new(false);
    view! {
        <dialog id="groups-dialog" class="modal">
            <div class="modal-heading"><h2>"Piece and assembly groups"</h2><button type="button" class="icon-button" aria-label="Close groups dialog" on:click=move |_|close_dialog("groups-dialog")>"×"</button></div>
            <p>"Groups contain pieces and other groups. Rigid edits move their descendants together; physical joints still determine attachment and stability."</p>
            <label>"Selected group"<select prop:value=move ||selected.get().to_string() on:change=move |e|{let id=event_target_value(&e).parse().unwrap_or(0);selected.set(id);if let Some(group)=ui.authoring_groups.get_untracked().iter().find(|g|g.id==id){pieces.set(group.members.iter().filter_map(|m|if let ergo_world::groups::Member::Piece{id}=m{Some(id.to_string())}else{None}).collect::<Vec<_>>().join(", "));children.set(group.members.iter().filter_map(|m|if let ergo_world::groups::Member::Group{id}=m{Some(id.to_string())}else{None}).collect::<Vec<_>>().join(", "));}}><option value="0">"Choose a group"</option>{move ||ui.authoring_groups.get().into_iter().map(|g|view!{<option value=g.id.to_string()>{format!("#{} · {}",g.id,g.name)}</option>}).collect_view()}</select></label>
            <label>"New group name"<input type="text" maxlength="120" prop:value=move ||name.get() on:input=move |e|name.set(event_target_value(&e))/></label>
            <div class="form-columns"><label>"Piece IDs"<input type="text" placeholder="1, 2, 3" prop:value=move ||pieces.get() on:input=move |e|pieces.set(event_target_value(&e))/></label><label>"Child group IDs"<input type="text" placeholder="4, 5" prop:value=move ||children.get() on:input=move |e|children.set(event_target_value(&e))/></label></div>
            <button class="button quiet small" on:click=move |_|pieces.set(ui.group_selection.get_untracked().iter().map(u32::to_string).collect::<Vec<_>>().join(", "))>{move ||format!("Use viewport selection ({} pieces)",ui.group_selection.get().len())}</button>
            <label class="toggle-row"><span>"Reparent when replacing members"</span><input type="checkbox" on:change=move |e|reparent.set(event_target_checked(&e))/></label>
            <div class="inspector-actions"><button class="button primary" on:click=move |_|group_action(handle,|scene|{let mut members=group_ids(&pieces.get_untracked())?.into_iter().map(|id|ergo_world::groups::Member::Piece{id}).collect::<Vec<_>>();members.extend(group_ids(&children.get_untracked())?.into_iter().map(|id|ergo_world::groups::Member::Group{id}));let id=ergo_world::groups::create(scene,name.get_untracked(),members,None)?;selected.set(id);Ok(())})>"Create group"</button>
                <button class="button quiet" disabled=move ||selected.get()==0 on:click=move |_|group_action(handle,|scene|{let mut members=group_ids(&pieces.get_untracked())?.into_iter().map(|id|ergo_world::groups::Member::Piece{id}).collect::<Vec<_>>();members.extend(group_ids(&children.get_untracked())?.into_iter().map(|id|ergo_world::groups::Member::Group{id}));ergo_world::groups::set_members(scene,selected.get_untracked(),members,reparent.get_untracked())})>"Replace members"</button>
            </div>
            <h3 class="group-transform-heading">"Transform selected group"</h3>
            <div class="move-grid">{[("−X",DVec3::NEG_X),("+X",DVec3::X),("−Y",DVec3::Z),("+Y",DVec3::NEG_Z),("−Z",DVec3::NEG_Y),("+Z",DVec3::Y)].into_iter().map(|(label,delta)|view!{<button class="button quiet small" disabled=move ||selected.get()==0 on:click=move |_|group_action(handle,|scene|{ergo_world::groups::transform(scene,selected.get_untracked(),delta,DVec3::Y,0.0,None)?;Ok(())})>{label}</button>}).collect_view()}</div>
            <div class="inspector-actions"><button class="button quiet small" disabled=move ||selected.get()==0 on:click=move |_|group_action(handle,|scene|{ergo_world::groups::transform(scene,selected.get_untracked(),DVec3::ZERO,DVec3::Y,std::f64::consts::FRAC_PI_2,None)?;Ok(())})>"Rotate 90° about Z"</button>
                <button class="button quiet small" disabled=move ||selected.get()==0 on:click=move |_|act(handle,|c|{c.require_all()?;let scene=c.scene()?;let (ids,_)=ergo_world::groups::descendants(&scene.groups,selected.get_untracked())?;let bounds=ergo_world::groups::bounds(&scene,selected.get_untracked())?;let r=c.renderer.as_mut().unwrap();r.highlighted=ids;if let Some(bounds)=bounds{r.fit(bounds);}r.changed();Ok(())})>"Focus / highlight"</button>
                <button class="button quiet small" disabled=move ||selected.get()==0 on:click=move |_|group_action(handle,|scene|{ergo_world::groups::ungroup(scene,selected.get_untracked())?;selected.set(0);Ok(())})>"Ungroup"</button>
            </div>
            <p class="subtle">"Transforms preserve real piece dimensions. Destination overlaps, floor crossings, anchored pieces, cycles, and constraints to outside pieces are rejected."</p>
        </dialog>
    }
}

#[component]
fn ConstructionBrowser() -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    view! {
        <div class="dock-section world-coordinates"><div class="section-heading"><h2>"World coordinates"</h2><button class="icon-button" title="Go to GPS position" on:click=move |_|show_dialog("location-dialog")>"◎"</button></div>
            <div class="reference-origin"><span>"Coordinate reference"</span><strong>{move ||ui.current.get().map(|w|format!("{:.7}°, {:.7}°",w.geo.latitude,w.geo.longitude)).unwrap_or_default()}</strong><small>"Fixed origin, not your camera position. X east · Y north · Z up."</small></div>
            <div class="coordinate-rule"><strong>"1,024 × 1,024 × 1,024"</strong><span>"brick units per storage cell"</span></div><p class="subtle">"Each cell spans 1 arc-second east and north. Cells locate pieces; they do not physically anchor them. Constructions rest on the ground or use explicit ground anchors."</p>
            <button class="text-button" on:click=move |_|show_dialog("location-dialog")>"Go to a location"<span>"↗"</span></button>
        </div>
        <div class="dock-section browser"><div class="section-heading"><h2>"Constructions"</h2><span class="count-badge">{move ||if ui.common_world.get().is_some(){ui.common_assemblies.get().len()}else{ui.worlds.get().len()}}</span></div>
            <Show when=move ||ui.common_world.get().is_some()><CommonConstructionList/></Show>
            <div class="world-list">{move ||ui.worlds.get().into_iter().map(|world|{let id=world.id.clone();let model=world.clone();view!{
                <button class="world-item" class:active=move ||ui.current.get().is_some_and(|w|w.id==id) on:click=move |_|open_world(handle,model.clone())><span class="world-thumbnail">{if world.name.contains("pavilion"){ "⌂" }else{"▦"}}</span><span><strong>{world.name.clone()}</strong><small>{format!("{} bricks",number(world.brick_count))}</small></span><span class="world-more">"›"</span></button>
            }}).collect_view()}</div>
            <Show when=move ||ui.common_world.get().is_none()&&!ui.shared_locations.get().is_empty()><div class="section-heading shared-world-heading"><h2>"Shared worlds"</h2></div><div class="world-list shared-world-list">{move ||ui.shared_locations.get().into_iter().map(|location|(location.world.clone(),location)).collect::<BTreeMap<_,_>>().into_values().map(|location|{
                let name=location.metadata.name.clone();let item=location.clone();
                view!{<button class="world-item" on:click=move |_|open_shared(handle,item.clone())><span class="world-thumbnail">"◎"</span><span><strong>{name}</strong><small>"Navigate shared regions"</small></span><span class="world-more">"›"</span></button>}
            }).collect_view()}</div></Show>
            <Show when=move ||ui.shared_cursor.get().is_some()><button class="text-button" on:click=move |_|more_shared_regions(handle)>"More shared regions"</button></Show>
        </div>
        <div class="dock-section"><h2>"Viewport"</h2><label class="toggle-row"><span>"Base grid"</span><input type="checkbox" checked on:change=move |e|with_client(handle,|c|{if let Some(r)=&mut c.renderer{r.grid=event_target_checked(&e);}})/></label>
            <NavigationModes/>
            <p class="subtle compact">{move ||ui.navigation.get().mode.instructions()}</p>
            <Show when=move ||ui.navigation.get().mode!=NavigationMode::Orbit><p class="subtle compact">"Focus the canvas for keyboard controls."</p><p class="subtle compact">{move ||format!("Speed: {:.1} studs/s",ui.navigation.get().speed)}</p></Show>
            <Show when=move ||ui.navigation.get().mode==NavigationMode::Walk>
                <p class="subtle compact">{format!("Step up ≤{STEP_HEIGHT} vertical units · Jump height {JUMP_HEIGHT} units with gravity and collisions.")}</p>
                <label class="range-label" for="eye-height">"Eye height"<span>"brick units"</span></label>
                <input id="eye-height" type="number" min=MIN_EYE_HEIGHT max=MAX_EYE_HEIGHT step="0.5" value=ui.navigation.get_untracked().eye_height on:input=move |e|with_client(handle,|c|{if let Some(r)=&mut c.renderer{r.set_eye_height(event_target_value(&e).parse().unwrap_or(DEFAULT_EYE_HEIGHT));}})/>
            </Show>
            <label class="range-label" for="explode">"Separate layers"<span>"Visual only"</span></label><input id="explode" type="range" min="0" max="1" step="0.02" value="0" on:input=move |e|with_client(handle,|c|{if c.simulation.is_none()&&let Some(r)=&mut c.renderer{r.explode=event_target_value(&e).parse().unwrap_or(0.0);r.changed();}})/>
            <label class="range-label" for="fog-distance">"Viewing distance (fog)"<span>{move ||format!("{:.0} studs",ui.fog.get())}</span></label><input id="fog-distance" type="range" min="200" max="4096" step="100" prop:value=move ||ui.fog.get().to_string() on:input=move |e|{let value=event_target_value(&e).parse::<f64>().unwrap_or(2200.0);ui.fog.set(value);with_client(handle,|c|{if let Some(r)=&mut c.renderer{r.fog_distance=value;r.distance=r.distance.min(value*0.9);r.changed();}});}/>
            <p class="subtle compact">"Visible blocks only · automatic distance LOD"</p>
            <label class="range-label" for="render-quality">"Render detail"<span>"Visual only"</span></label>
            <select id="render-quality" aria-label="Render detail" prop:value=move ||ui.render_quality.get() on:change=move |event|{
                let value=event_target_value(&event);
                let policy=match value.as_str(){"high"=>ergo_client::scene::RenderPolicy::DETAILED_INTERACTIVE,"performance"=>ergo_client::scene::RenderPolicy::PERFORMANCE,_=>ergo_client::scene::RenderPolicy::INTERACTIVE};
                ui.render_quality.set(value);with_client(handle,|client|if let Some(renderer)=&mut client.renderer{renderer.set_policy(policy);});
            }><option value="auto">"Auto (balanced)"</option><option value="performance">"Performance"</option><option value="high">"High (per-brick LOD)"</option></select>
            <p class="subtle compact">{move ||format!("{} octree nodes · geometry instanced on GPU",ui.nodes.get())}</p>
        </div>
    }
}

#[component]
fn CommonConstructionList() -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    let sort = RwSignal::new("name".to_owned());
    let descending = RwSignal::new(false);
    let export_format = RwSignal::new("ldraw".to_owned());
    view! {
        <div class="construction-sort"><label for="construction-sort">"Sort"</label>
            <select id="construction-sort" aria-label="Sort constructions" on:change=move |e|{let key=event_target_value(&e);descending.set(key=="date"||key=="size");sort.set(key);}>
                <option value="name">"Name"</option><option value="author">"Author"</option><option value="date">"Date"</option><option value="size">"Size"</option>
            </select><button class="icon-button" aria-label="Reverse construction sort order" title=move ||if descending.get(){"Descending"}else{"Ascending"} on:click=move |_|descending.update(|v|*v=!*v)>{move ||if descending.get(){"↓"}else{"↑"}}</button>
        </div>
        <Show when=move ||ui.common_more.get().is_some()><small class="construction-sort-scope">"Sorts loaded constructions"</small></Show>
        <ul class="world-list construction-list" aria-label="Constructions in shared world" tabindex="0"
            aria-busy=move ||ui.common_loading.get().to_string()
            on:scroll=move |event| {
                if let Some(element)=event.current_target().and_then(|target|target.dyn_into::<web_sys::HtmlElement>().ok()) {
                    if element.scroll_height()-element.scroll_top()-element.client_height()<96 {common::more(handle);}
                }
            }>
            {move ||{let mut items=ui.common_assemblies.get();common::sort(&mut items,&sort.get(),descending.get());items.into_iter().map(|assembly| {
                let author=assembly.author().to_owned();let date=assembly.date();let size=assembly.piece_count.map(number).unwrap_or_else(||"—".into());
                let id=assembly.id;let active=id.clone();
                let attribute=id.clone();let name=assembly.name;let title=name.clone();
                view!{<li><button class="world-item" data-assembly-id=attribute class:active=move ||ui.focused_assembly.get().as_ref()==Some(&active)
                    title=title disabled=move ||ui.busy.get() on:click=move |_|common::open(handle,id.clone())>
                    <span class="world-thumbnail" aria-hidden="true">"▦"</span><span class="construction-summary"><strong>{name}</strong><small class="construction-author" title=author.clone()>{author.clone()}</small><time class="construction-date">{date}</time></span><small class="construction-size" title="Piece count">{size}</small>
                </button></li>}
            }).collect_view()}}
        </ul>
        <div class="construction-list-actions">
            <button class="text-button" disabled=move ||ui.common_loading.get() on:click=move |_|common::refresh(handle)>"Refresh list"</button>
            <Show when=move ||ui.common_more.get().is_some()><button class="text-button" disabled=move ||ui.common_loading.get() on:click=move |_|common::more(handle)>"Load more"</button></Show>
        </div>
        <div class="construction-export"><select aria-label="Construction export format" on:change=move |e|export_format.set(event_target_value(&e))><option value="ldraw">"LDraw / MPD"</option><option value="ergo">"Ergo JSON"</option></select>
            <button class="text-button" disabled=move ||ui.focused_assembly.get().is_none() on:click=move |_|common::download(handle,&export_format.get_untracked())>"Download construction"</button>
        </div>
    }
}

#[component]
fn BrickInspector() -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    let pull = RwSignal::new(false);
    view! {
        <div class="dock-section inspector"><div class="section-heading"><h2>"Brick inspector"</h2><span class="selection-dot" class:has-selection=move ||ui.selected.get().is_some()></span></div>
            <Show when=move ||ui.selected.get().is_some() fallback=move ||view!{<div class="empty-inspector"><span class="catalog-brick-outline" inner_html=ergo_world::illustration::brick_outline("3003.dat").unwrap_or_default()></span><strong>"Every brick has a story."</strong><p>"Select one in the scene to inspect its geometry and connections."</p></div>}>
                <div class="selected-part"><span class="part-color" style:background=move ||ui.selected.get().map(|b|format!("#{:06x}",b.rgb)).unwrap_or_default()></span><div><h3>{move ||ui.selected.get().map(|b|catalog::part_name(&b.part).to_owned()).unwrap_or_default()}</h3><p>{move ||ui.selected.get().map(|b|format!("{} · #{}",b.part.trim_end_matches(".dat"),b.id)).unwrap_or_default()}</p></div></div>
                <dl class="part-properties"><dt>"Color"</dt><dd>{move ||ui.selected.get().map(|b|catalog::color_name(b.color)).unwrap_or_default()}</dd><dt>"Origin (X,Y,Z)"</dt><dd>{move ||ui.selected.get().map(|b|format!("{:.1}, {:.1}, {:.1}",b.position.x,-b.position.z,b.position.y)).unwrap_or_default()}</dd><dt>"Base height Z"</dt><dd>{move ||ui.selected.get().map(|b|format!("{:.3}",b.bounds.min.y)).unwrap_or_default()}</dd><dt>"Connections"</dt><dd>{move ||ui.relations.get().iter().filter(|r|r.certainty=="inferred").count()}</dd></dl>
                <div class="swatches" aria-label="Available brick colors">{catalog::COLORS.iter().map(|&(code,name)|view!{<button class="swatch" title=name aria-label=name style:background=format!("#{:06x}",ergo_world::color_rgb(code)) on:click=move |_|act(handle,|c|{c.remember()?;c.freeze_simulation();let r=c.renderer.as_mut().unwrap();if let Some(id)=r.selected{for b in &mut r.bricks{if b.id==id{b.color=code;b.rgb=ergo_world::color_rgb(code);}}r.changed();}c.refresh_selection();Ok(())})></button>}).collect_view()}</div>
                <div class="inspector-actions"><button class="button quiet small" on:click=move |_|{ui.first.set(ui.selected.get_untracked().map(|b|b.id));ui.message.set("First brick selected. Select a second brick, then add a connection".into());}>"Use as first brick"</button><button class="button quiet small" disabled=move ||ui.first.get().is_none() on:click=move |_|show_dialog("constraint-dialog")>"Connect…"</button><button class="button quiet small" on:click=move |_|{if let Some(brick)=ui.selected.get_untracked(){ui.group_selection.update(|ids|{if !ids.contains(&brick.id){ids.push(brick.id);}});ui.message.set(format!("{} pieces selected for grouping",ui.group_selection.get_untracked().len()));}}>"Add to group selection"</button></div>
                <label class="toggle-row"><span>"Pull in simulation"</span><input type="checkbox" on:change=move |e|pull.set(event_target_checked(&e))/></label>
                <div class="move-grid" aria-label="Move selected connected group">{[("−X",DVec3::NEG_X),("+X",DVec3::X),("−Y",DVec3::Z),("+Y",DVec3::NEG_Z),("−Z",DVec3::NEG_Y),("+Z",DVec3::Y)].into_iter().map(|(label,delta)|view!{<button class="button quiet small" on:click=move |_|act(handle,|c|c.move_selected(delta, pull.get_untracked()))>{label}</button>}).collect_view()}</div>
                <button class="text-button" on:click=move |_|act(handle,|c|{c.remember()?;c.freeze_simulation();let id=c.renderer.as_ref().and_then(|r|r.selected).unwrap();if c.fixed.contains(&id){c.fixed.retain(|i|*i!=id);c.ui.message.set("Brick released from its anchor".into());}else{c.fixed.push(id);c.ui.message.set("Brick anchored in world space".into());}Ok(())})>"Toggle world anchor"</button>
                <details class="connections"><summary>"Nearby contacts"</summary>{move ||ui.relations.get().into_iter().take(12).map(|r|view!{<p>{format!("#{} · {} ({})",r.brick_id,r.kind,r.certainty)}</p>}).collect_view()}</details>
            </Show>
        </div>
    }
}

#[component]
fn SimulationControls() -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    view! {
        <div class="dock-section simulation"><div class="section-heading"><h2>"Physics sandbox"</h2><span class="beta-badge">"Rigid bodies"</span></div><p class="subtle">"Gravity, body-box collisions, and fixed or axle constraints."</p>
            <div class="simulation-buttons"><button class="button primary" on:click=move |_|act(handle,|c|{if c.playing{c.playing=false;c.ui.playing.set(false);if let Some(s)=&mut c.simulation{s.release_drive();}Ok(())}else{if !c.complete(){c.request_all()?;c.start_when_loaded=true;c.ui.busy.set(true);c.ui.message.set("Loading all supports and connected bodies before simulation…".into());Ok(())}else{c.begin_physics()}}})>{move ||if ui.playing.get(){"Ⅱ Pause"}else{"▷ Run gravity"}}</button><button class="button quiet" title="Restore last saved or imported state" on:click=move |_|act(handle,|c|{c.freeze_simulation();let scene=c.undo.take().ok_or_else(||anyhow::anyhow!("No workspace changes to reset"))?;c.joints=scene.joints;c.fixed=scene.fixed_bricks;c.groups=scene.groups;c.ui.authoring_groups.set(c.groups.clone());c.renderer.as_mut().unwrap().set_bricks(scene.bricks);c.ui.stats.set(SimulationStats::default());c.ui.dirty.set(c.world.as_ref().is_some_and(|w|w.id=="draft"));c.refresh_selection();c.ui.message.set("Workspace reset".into());Ok(())})>"Reset"</button></div>
            <dl class="simulation-stats"><div><dt>"Connections"</dt><dd>{move ||number(ui.joint_count.get())}</dd></div><div><dt>"Moved > 0.1"</dt><dd>{move ||ui.stats.get().moved}</dd></div><div><dt>"Time"</dt><dd>{move ||format!("{:.1}s",ui.stats.get().elapsed)}</dd></div></dl>
            <p class="physics-note">"A sandbox uses the complete construction, up to 5,000 bricks. Saved models must be overlap-free and stable under gravity."</p>
            <button class="button quiet full" on:click=move |_|show_dialog("solidity-dialog")>"Test solidity…"</button>
            <button class="button quiet full group-button" on:click=move |_|show_dialog("groups-dialog")>{move ||format!("Groups… ({})",ui.authoring_groups.get().len())}</button>
            <button class="button full save-button" disabled=move ||ui.busy.get()||!ui.dirty.get() on:click=move |_|save(handle)>{move ||if ui.common_world.get().is_some(){"Commit to common world"}else{"Validate & save snapshot"}}</button>
            <button class="text-button" on:click=move |_|export_scene(handle)>"Export workspace JSON"<span>"↓"</span></button>
            <button class="text-button" on:click=move |_|export_scene_format(handle,true)>"Export workspace MPD"<span>"↓"</span></button>
        </div>
    }
}

#[component]
fn SolidityDialog() -> impl IntoView {
    let handle = expect_context::<Handle>();
    let ui = expect_context::<Ui>();
    let kind = RwSignal::new("earthquake".to_owned());
    let amplitude = RwSignal::new("2".to_owned());
    let frequency = RwSignal::new("5".to_owned());
    let duration = RwSignal::new("2".to_owned());
    let mass = RwSignal::new("0.05".to_owned());
    let height = RwSignal::new("0.1".to_owned());
    let size = RwSignal::new("0.01".to_owned());
    let busy = RwSignal::new(false);
    let report = RwSignal::new(None::<ergo_world::solidity::SolidityReport>);
    view! {
        <dialog id="solidity-dialog" class="modal">
            <form on:submit=move |e| {
                e.prevent_default();report.set(None);busy.set(true);
                let input: anyhow::Result<_>=with_client(handle,|c| {
                    c.require_all()?;c.freeze_simulation();let scene=c.scene()?;
                    let case=if kind.get_untracked()=="earthquake" {
                        ergo_world::solidity::LoadCase::Earthquake(ergo_world::solidity::Earthquake{displacement_amplitude_m:amplitude.get_untracked().parse::<f64>()?/1000.0,frequency_hz:frequency.get_untracked().parse()?,duration_s:duration.get_untracked().parse()?,direction:DVec3::X})
                    } else {
                        let bounds=ergo_world::bounds(&scene.bricks).ok_or_else(||anyhow::anyhow!("Empty construction"))?;
                        let target=c.renderer.as_ref().and_then(|r|r.selected_brick()).map(|b|b.bounds.center()).unwrap_or(bounds.center());
                        ergo_world::solidity::LoadCase::FallingObject(ergo_world::solidity::FallingObject{mass_kg:mass.get_untracked().parse()?,dimensions_m:DVec3::splat(size.get_untracked().parse()?),target_world:DVec3::new(target.x,bounds.max.y,target.z),release_height_m:height.get_untracked().parse()?})
                    };
                    Ok((scene,case))
                });
                spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(30).await;
                    match input.and_then(|(scene,case)|ergo_world::solidity::test(&scene,case)) {
                        Ok((_,_,result))=>report.set(Some(result)),Err(e)=>ui.fail(e),
                    }
                    busy.set(false);
                });
            }>
                <div class="modal-heading"><h2>"Test the construction's solidity"</h2><button type="button" class="icon-button" aria-label="Close solidity dialog" on:click=move |_|close_dialog("solidity-dialog")>"×"</button></div>
                <p>"Run a non-destructive load case. Results apply to this waveform or impact and the model's available connector calibration."</p>
                <label>"Load case"<select on:change=move |e|kind.set(event_target_value(&e))><option value="earthquake">"Earthquake / horizontal baseplate shake"</option><option value="falling_object">"Falling box above the selected brick or center"</option></select></label>
                <Show when=move ||kind.get()=="earthquake" fallback=move ||view! {
                    <div class="form-columns"><label>"Object mass (kg)"<input type="number" min="0.0001" max="10" step="any" value="0.05" on:input=move |e|mass.set(event_target_value(&e))/></label><label>"Release height (m)"<input type="number" min="0.01" max="2" step="any" value="0.1" on:input=move |e|height.set(event_target_value(&e))/></label></div>
                    <label>"Box side length (m)"<input type="number" min="0.001" max="1" step="any" value="0.01" on:input=move |e|size.set(event_target_value(&e))/></label>
                }>
                    <div class="form-columns"><label>"Displacement amplitude (mm)"<input type="number" min="0" max="50" step="any" value="2" on:input=move |e|amplitude.set(event_target_value(&e))/></label><label>"Frequency (Hz)"<input type="number" min="0.1" max="20" step="any" value="5" on:input=move |e|frequency.set(event_target_value(&e))/></label></div>
                    <label>"Shake duration (s)"<input type="number" min="0.1" max="4" step="any" value="2" on:input=move |e|duration.set(event_target_value(&e))/></label>
                </Show>
                <Show when=move ||report.get().is_some()>
                    <div class="solidity-result" role="status"><h3>{move ||report.get().map(|r|if r.survived_declared_case{"Survived the declared load case"}else{"Movement or failure detected"}).unwrap_or_default()}</h3>
                        <p>{move ||report.get().map(|r|format!("Peak movement: {:.2} mm · broken connections: {} · final drift: {:.3} brick units",r.peak_displacement_m*1000.0,r.broken_connections,r.statistics.max_displacement)).unwrap_or_default()}</p>
                        <p>{move ||report.get().map(|r|if r.connector_capacities_complete{"Connector thresholds are present; friction and box inertia remain approximations."}else{"Connector capacities are missing. This is an idealized diagnostic, not calibrated strength certification."}).unwrap_or_default()}</p>
                    </div>
                </Show>
                <div class="modal-footer"><span>"Working copy remains unchanged"</span><button class="button primary" type="submit" disabled=move ||busy.get()>{move ||if busy.get(){"Testing…"}else{"Run load test"}}</button></div>
            </form>
        </dialog>
    }
}

fn save(handle: Handle) {
    if with_client(handle, |c| c.ui.common_world.get_untracked().is_some()) {
        with_client(handle, |c| c.common_attempt = None);
        common::pump(handle);
        return;
    }
    let scene = with_client(handle, |c| {
        c.freeze_simulation();
        c.ui.error.set(None);
        c.ui.busy.set(true);
        c.ui.message
            .set("Checking overlaps and gravity stability before saving…".into());
        c.scene()
    });
    spawn_local(async move {
        let result: anyhow::Result<WorldManifest> = async {
            let scene = scene?;
            let response = Request::post("/api/worlds")
                .header("X-Ergo-Request", "1")
                .json(&scene)?
                .send()
                .await?;
            if !response.ok() {
                let value: serde_json::Value = response.json().await?;
                anyhow::bail!(value["error"].as_str().unwrap_or("Save failed").to_owned());
            }
            Ok(response.json().await?)
        }
        .await;
        match result {
            Ok(world) => {
                let ui = with_client(handle, |c| c.ui);
                ui.worlds.update(|list| list.push(world.clone()));
                open_world(handle, world);
            }
            Err(e) => with_client(handle, |c| c.ui.fail(e)),
        }
    });
}

fn export_scene(handle: Handle) {
    export_scene_format(handle, false);
}
fn export_scene_format(handle: Handle, standard: bool) {
    act(handle, |c| {
        c.freeze_simulation();
        let scene = c.scene()?;
        let text = if standard {
            ergo_world::exchange::mpd(&scene)?
        } else {
            serde_json::to_string_pretty(&ergo_world::storage::Construction::from(&scene))?
        };
        let parts = js_sys::Array::new();
        parts.push(&JsValue::from_str(&text));
        let blob = web_sys::Blob::new_with_str_sequence(&parts)
            .map_err(|_| anyhow::anyhow!("Cannot create download"))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|_| anyhow::anyhow!("Cannot create download URL"))?;
        let document = web_sys::window().unwrap().document().unwrap();
        let a = document
            .create_element("a")
            .unwrap()
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .unwrap();
        a.set_href(&url);
        a.set_download(if standard {
            "construction.mpd"
        } else {
            "construction.ergo.json"
        });
        a.click();
        web_sys::Url::revoke_object_url(&url).ok();
        Ok(())
    });
}

#[derive(Deserialize)]
struct ImportPreview {
    scene: WorldExport,
    notes: Vec<String>,
}

#[component]
fn ImportDialog() -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    let file = NodeRef::<html::Input>::new();
    let name = RwSignal::new(String::new());
    let lat = RwSignal::new("48.8566".to_owned());
    let lon = RwSignal::new("2.3522".to_owned());
    view! {<dialog id="import-dialog" class="modal"><form on:submit=move |e|{e.prevent_default();let Some(selected)=file.get().and_then(|e|e.files()).and_then(|f|f.get(0))else{ui.fail("Choose a file first");return;};let data=web_sys::FormData::new().unwrap();data.append_with_blob_and_filename("file",&selected,&selected.name()).unwrap();data.append_with_str("name",&name.get_untracked()).unwrap();if !selected.name().ends_with(".json"){data.append_with_str("latitude",&lat.get_untracked()).unwrap();data.append_with_str("longitude",&lon.get_untracked()).unwrap();}ui.busy.set(true);close_dialog("import-dialog");spawn_local(async move{let result:anyhow::Result<ImportPreview>=async{let response=Request::post("/api/import").header("X-Ergo-Request","1").body(data)?.send().await?;if !response.ok(){let value:serde_json::Value=response.json().await?;anyhow::bail!(value["error"].as_str().unwrap_or("Import failed").to_owned());}Ok(response.json().await?)}.await;match result{Ok(preview)=>{if let Err(e)=open_draft(handle,preview.scene){ui.fail(e);}else if !preview.notes.is_empty(){ui.message.set(format!("Imported with {} geometry notes; catalog parametric meshes used where needed",preview.notes.len()));}},Err(e)=>ui.fail(e)}});}><div class="modal-heading"><h2>"Bring your bricks into the world."</h2><button type="button" class="icon-button" aria-label="Close import dialog" on:click=move |_|close_dialog("import-dialog")>"×"</button></div><p>"Open a BrickLink Studio model or an exported LDraw construction. Imports enter a working copy before validation."</p><label class="file-input" for="model-file"><strong>"Choose a construction"</strong><span>"Studio .io · LDraw .ldr / .mpd · Ergo .json"</span><input node_ref=file id="model-file" type="file" accept=".io,.ldr,.mpd,.json" required/></label><label>"Construction name"<input type="text" placeholder="My Paris construction" on:input=move |e|name.set(event_target_value(&e))/></label><div class="form-columns"><label>"Latitude"<input type="number" step="any" min="-90" max="90" value="48.8566" on:input=move |e|lat.set(event_target_value(&e))/></label><label>"Longitude"<input type="number" step="any" min="-180" max="180" value="2.3522" on:input=move |e|lon.set(event_target_value(&e))/></label></div><details><summary>"Supported parts and colors"</summary><p>"The starter catalog includes 27 common basic bricks, plates, and tiles, and 20 colors. Unsupported pieces and colors stop the import."</p><div class="catalog-parts">{catalog::PARTS.iter().map(|(part,name)|view!{<span>{format!("{} · {}",part.trim_end_matches(".dat"),name)}</span>}).collect_view()}</div></details><div class="modal-footer"><span>"Maximum upload 128 MiB"</span><button class="button primary" type="submit">"Open in workspace"</button></div></form></dialog>}
}

#[component]
fn LocationDialog() -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    let lat = RwSignal::new("48.8566".to_owned());
    let lon = RwSignal::new("2.3522".to_owned());
    view! {
        <dialog id="location-dialog" class="modal narrow">
            <form on:submit=move |e| {
                e.prevent_default();
                act(handle,|c| {
                    let geo=c.world.as_ref().map(|w|w.geo).unwrap_or_default();
                    let position=geo.to_world(lat.get_untracked().parse()?,lon.get_untracked().parse()?)?;
                    if let Some(r)=&mut c.renderer { r.teleport(position); }
                    c.ui.message.set(format!("Moved to block {}",BlockCoord::at(position).key()));
                    Ok(())
                });
                close_dialog("location-dialog");
            }>
                <div class="modal-heading"><h2>"Go to a location"</h2><button type="button" class="icon-button" aria-label="Close location dialog" on:click=move |_|close_dialog("location-dialog")>"×"</button></div>
                <p>"A storage block maps to one arc-second in latitude and longitude."</p>
                <label>"Latitude"<input type="number" step="any" min="-90" max="90" prop:value=move ||lat.get() on:input=move |e|lat.set(event_target_value(&e))/></label>
                <label>"Longitude"<input type="number" step="any" min="-180" max="180" prop:value=move ||lon.get() on:input=move |e|lon.set(event_target_value(&e))/></label>
                <button type="button" class="button quiet full" on:click=move |_| {
                    let Ok(geo)=web_sys::window().unwrap().navigator().geolocation() else { ui.fail("Geolocation is unavailable"); return; };
                    let success=wasm_bindgen::closure::Closure::once_into_js(move |position:web_sys::Position| {
                        lat.set(position.coords().latitude().to_string());lon.set(position.coords().longitude().to_string());
                    });
                    let error=wasm_bindgen::closure::Closure::once_into_js(move |_:JsValue|ui.fail("Location access failed. Enter coordinates manually or allow location access in your browser"));
                    geo.get_current_position_with_error_callback(success.unchecked_ref(),Some(error.unchecked_ref())).ok();
                }>"Use my current location"</button>
                <div class="modal-footer"><span>"X east · Y north · Z up"</span><button class="button primary" type="submit">"Go to coordinates"</button></div>
            </form>
        </dialog>
    }
}

#[component]
fn ConstraintDialog() -> impl IntoView {
    let ui = expect_context::<Ui>();
    let handle = expect_context::<Handle>();
    let kind = RwSignal::new("fixed".to_owned());
    let axis = RwSignal::new("x".to_owned());
    view! {
        <dialog id="constraint-dialog" class="modal narrow">
            <form on:submit=move |e| {
                e.prevent_default();
                act(handle,|c| {
                    c.remember()?; c.freeze_simulation();
                    let a=ui.first.get_untracked().ok_or_else(||anyhow::anyhow!("Choose the first brick"))?;
                    let b=c.renderer.as_ref().and_then(|r|r.selected).ok_or_else(||anyhow::anyhow!("Select the second brick"))?;
                    anyhow::ensure!(a!=b,"Choose two different bricks");
                    let r=c.renderer.as_ref().unwrap();
                    let ba=r.bricks.iter().find(|b|b.id==a).ok_or_else(||anyhow::anyhow!("First brick is no longer loaded"))?;
                    let bb=r.bricks.iter().find(|brick|brick.id==b).unwrap();
                    let anchor=(ba.bounds.center()+bb.bounds.center())*0.5;
                    let kind=if kind.get_untracked()=="fixed" { JointKind::Fixed } else {
                        JointKind::Revolute{axis:match axis.get_untracked().as_str(){"y"=>DVec3::NEG_Z,"z"=>DVec3::Y,_=>DVec3::X},limits:None}
                    };
                    c.joints.retain(|j|!((j.a==a&&j.b==b)||(j.a==b&&j.b==a)));
                    c.joints.push(JointSpec{a,b,anchor,kind,break_force:None,break_torque:None,strength_source:None});
                    c.refresh_selection();ui.message.set("Connection added to the working copy; measured capacity is required for publication".into());
                    Ok(())
                });
                close_dialog("constraint-dialog");
            }>
                <div class="modal-heading"><h2>"Connect two bricks"</h2><button type="button" class="icon-button" aria-label="Close connection dialog" on:click=move |_|close_dialog("constraint-dialog")>"×"</button></div>
                <p>{move ||format!("Brick #{} to brick #{}",ui.first.get().unwrap_or(0),ui.selected.get().map(|b|b.id).unwrap_or(0))}</p>
                <label>"Connection type"<select on:change=move |e|kind.set(event_target_value(&e))><option value="fixed">"Fixed connection"</option><option value="revolute">"Axle / revolute hinge"</option></select></label>
                <Show when=move ||kind.get()=="revolute"><label>"World-space axle direction"<select on:change=move |e|axis.set(event_target_value(&e))><option value="x">"X (east–west)"</option><option value="y">"Y (north–south)"</option><option value="z">"Z (vertical)"</option></select></label></Show>
                <p class="subtle">"The anchor starts halfway between the brick centers. JSON exports support exact anchor coordinates and optional angle limits."</p>
                <div class="modal-footer"><span>"Explicit constraint in working copy"</span><button class="button primary" type="submit">"Add connection"</button></div>
            </form>
        </dialog>
    }
}
