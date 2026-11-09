use super::*;
use ergo_transactions::Repository;
use ergo_world::{
    WorldExport,
    demo::Demo,
    equilibrium::MemoryWorkspace,
    validation::{self, StaticTolerances},
};
use glam::{DMat3, DVec3};
use std::sync::Arc;

async fn transaction() -> Transaction {
    let repo = Repository::new(
        Arc::new(object_store::memory::InMemory::new()),
        "physical-input",
    )
    .unwrap();
    repo.create("main").await.unwrap();
    repo.begin("main").await.unwrap()
}
fn scene(demo: Demo) -> WorldExport {
    WorldExport {
        schema: ergo_world::SCHEMA_VERSION,
        name: "physical input".into(),
        geo: Default::default(),
        bricks: demo.bricks,
        meshes: demo.meshes,
        joints: vec![],
        fixed_bricks: vec![],
        groups: vec![],
        model: None,
        ground: vec![],
    }
}
fn input(
    tx: &Transaction,
    scene: &WorldExport,
    model: &ConnectionMeasurements,
) -> NativeValidationInput {
    let mut input = NativeValidationInput::new(tx).unwrap();
    for body in &scene.bricks {
        input
            .push(ContentId::of(&body.id.to_le_bytes()), body.clone(), model)
            .unwrap();
    }
    input.finish().unwrap()
}
async fn compare(
    mut source: WorldExport,
    expected_error: Option<&str>,
) -> Option<validation::ValidationReport> {
    let tx = transaction().await;
    let model = ConnectionMeasurements::default();
    let input = input(&tx, &source, &model);
    let mut joints = source.joints.clone();
    let paged = validation::validate_input_with_workspace(
        &input,
        &mut joints,
        &source.fixed_bricks,
        StaticTolerances::default(),
        &model,
        &mut MemoryWorkspace::default(),
    );
    let resident = validation::validate_static(&mut source);
    assert!(input.cache.borrow().len() <= CACHE_BODIES);
    if let Some(message) = expected_error {
        let paged = paged.unwrap_err();
        let resident = resident.unwrap_err();
        assert!(paged.to_string().contains(message), "{paged:#}");
        assert_eq!(paged.to_string(), resident.to_string());
        assert_eq!(
            paged.downcast_ref::<Inconclusive>().is_some(),
            resident.downcast_ref::<Inconclusive>().is_some()
        );
        return None;
    }
    let paged = paged.unwrap();
    assert_eq!(
        serde_json::to_value(&paged).unwrap(),
        serde_json::to_value(resident.unwrap()).unwrap()
    );
    assert_eq!(
        canonical_cbor(&joints).unwrap(),
        canonical_cbor(&source.joints).unwrap()
    );
    Some(paged)
}

#[tokio::test]
async fn paged_and_resident_geometry_and_complete_components_match() {
    let mut demo = Demo::new();
    for x in 0..160 {
        demo.add("3003", 4, -400. + x as f64 * 4., 0., -20.);
    }
    let source = scene(demo);
    let tx = transaction().await;
    let paged = input(&tx, &source, &Default::default());
    let bounds = source
        .bricks
        .iter()
        .map(|b| b.bounds)
        .reduce(Aabb::union)
        .unwrap();
    assert_eq!(
        paged
            .query(bounds)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap(),
        (0..160).collect::<Vec<_>>()
    );
    for i in (0..160).chain(0..160) {
        assert_eq!(
            canonical_cbor(paged.body(i).unwrap().as_ref()).unwrap(),
            canonical_cbor(&source.bricks[i]).unwrap()
        );
        assert!(paged.cache.borrow().len() <= CACHE_BODIES);
    }
    assert_eq!(
        compare(source, None).await.unwrap().stability_components,
        160
    );

    let mut demo = Demo::new();
    for h in 0..160 {
        demo.add("3005", 4, 0., h as f64, 0.);
    }
    let report = compare(scene(demo), None).await.unwrap();
    assert_eq!(report.stability_components, 1);
    assert_eq!(report.inferred_joints, 159);

    let mut demo = Demo::new();
    demo.add("3005", 4, 127., 0., 0.);
    demo.add("3005", 4, 130., 0., 0.);
    demo.add("3001", 4, 127., 1., 0.);
    assert_eq!(
        compare(scene(demo), None)
            .await
            .unwrap()
            .stability_components,
        1
    );

    // The wing and rear shelf lie outside 4444's main mass envelope.
    let mut demo = Demo::new();
    for (part, x, y, z) in [
        ("4444", 1., 0., 1.),
        ("3004", -1., 5., 1.),
        ("3004", 4., 5., 1.),
        ("3623", 1., 17. / 3., 0.),
    ] {
        demo.add(part, 71, x, y, z);
        let b = demo.bricks.last_mut().unwrap();
        b.position = DVec3::new(x, y, z)
            - ergo_world::geometry::nominal_body_bounds(&b.part)
                .unwrap()
                .min;
    }
    assert_eq!(
        compare(scene(demo), None)
            .await
            .unwrap()
            .stability_components,
        1
    );

    let mut demo = Demo::new();
    demo.add("3003", 4, 0., 0., 0.);
    demo.add("3003", 4, 0., 1., 0.);
    let mut source = scene(demo);
    source.joints = ergo_world::constraints::inferred_joints(&source.bricks);
    source.fixed_bricks = vec![1];
    // Claimed bounds cannot narrow the index built from the frozen model.
    source.bricks[0].bounds = Aabb::new(DVec3::ZERO, DVec3::splat(0.01));
    assert_eq!(
        compare(source, None)
            .await
            .unwrap()
            .largest_equilibrium_component,
        2
    );
}

#[tokio::test]
async fn paged_and_resident_rejections_match_without_fictitious_boundary_support() {
    let mut demo = Demo::new();
    demo.add("3003", 4, 0., 0., 0.);
    demo.add("3003", 4, 0., 0.5, 0.);
    compare(scene(demo), Some("overlap")).await;
    let mut demo = Demo::new();
    demo.add("3003", 4, 0., 0., 0.);
    demo.add("3003", 4, 200., 4., 0.);
    compare(scene(demo), Some("not static")).await;
    let mut demo = Demo::new();
    demo.add("3003", 4, 0., 0., 0.);
    demo.add("3003", 4, 0., 1., 0.);
    let mut source = scene(demo);
    source.joints = ergo_world::constraints::inferred_joints(&source.bricks);
    source.bricks[1].translated(DVec3::X * 0.1);
    compare(source, Some("no registered connector fits")).await;

    let mut demo = Demo::new();
    demo.add("41539", 19, 0., 0., 0.);
    demo.add("3003", 14, 2., 1. / 3., 2.);
    let b = &mut demo.bricks[1];
    b.transform =
        ergo_world::physical::world_transform(DMat3::from_rotation_z(std::f64::consts::FRAC_PI_2));
    let local = ergo_world::geometry::nominal_body_bounds(&b.part)
        .unwrap()
        .transformed(b.transform, DVec3::ZERO);
    b.position = DVec3::new(3., 1. / 3. - local.min.y, 3.);
    compare(scene(demo), Some("Stud interference")).await;

    let mut demo = Demo::new();
    demo.add("3003", 4, 0., 4., 0.);
    demo.bricks[0].transform = ergo_world::physical::world_transform(DMat3::from_rotation_z(0.3));
    compare(scene(demo), Some("Cannot certify tilted brick")).await;
}

#[tokio::test]
async fn missing_and_corrupt_bodies_are_inconclusive_not_empty_space() {
    let mut demo = Demo::new();
    for i in 0..10 {
        demo.add("3003", 4, i as f64 * 4., 0., 0.);
    }
    let source = scene(demo);
    for attack in ["missing", "changed", "wrong-key"] {
        let tx = transaction().await;
        let mut input = input(&tx, &source, &Default::default());
        match attack {
            "missing" => {
                input.bodies.remove(&5).unwrap();
            }
            "changed" => {
                let mut body = input.bodies.get(&5).unwrap().unwrap();
                body.body.translated(DVec3::X);
                input.bodies.insert(5, body).unwrap();
            }
            _ => {
                let body = input.bodies.get(&6).unwrap().unwrap();
                input.bodies.insert(5, body).unwrap();
            }
        }
        let error = input.body(5).unwrap_err();
        assert!(
            error.downcast_ref::<Inconclusive>().is_some(),
            "{attack}: {error:#}"
        );
    }
}

#[tokio::test]
async fn lost_and_changed_global_identity_rows_cannot_omit_constraints() {
    let mut demo = Demo::new();
    demo.add("3003", 4, 0., 0., 0.);
    demo.add("3003", 4, 0., 1., 0.);
    let source = scene(demo);
    for attack in ["missing", "changed"] {
        let tx = transaction().await;
        let mut input = input(&tx, &source, &Default::default());
        let global = ContentId::of(&1u32.to_le_bytes());
        if attack == "missing" {
            input.global.remove(&global).unwrap();
        } else {
            let mut identity = input.global.get(&global).unwrap().unwrap();
            identity.id = 2;
            input.global.insert(global, identity).unwrap();
        }
        let error = input
            .finish()
            .err()
            .expect("corrupt identity mapping must fail before inference");
        assert!(
            error.downcast_ref::<Inconclusive>().is_some(),
            "{attack}: {error:#}"
        );
    }
}

#[tokio::test]
async fn paged_pavilion_uses_the_same_coupled_solver_and_complete_witness() {
    let demo = ergo_world::demo::pavilion();
    let joints = ergo_world::demo::authored_bonds(&demo.bricks);
    let mut source = scene(demo);
    source.joints = joints;
    let model = ConnectionMeasurements {
        idealized_fixture: true,
        ..Default::default()
    };
    let tx = transaction().await;
    let input = input(&tx, &source, &model);
    let mut workspace =
        super::super::equilibrium_workspace::NativeSolverWorkspace::new(&tx).unwrap();
    let report = validation::validate_input_with_workspace(
        &input,
        &mut source.joints,
        &[],
        Default::default(),
        &model,
        &mut workspace,
    )
    .unwrap();
    assert!(report.stability_components > 0);
    assert!(report.largest_equilibrium_component > 256);
    assert!(report.equilibrium_solver.partitions > 1);
    assert!(report.equilibrium_solver.spilled_reaction_columns > 0);
    assert!(report.equilibrium_solver.largest_local_kkt_dimension <= 2048);
}

#[tokio::test]
async fn a_large_connected_compression_component_keeps_only_bounded_body_pages() {
    use ergo_world::{
        equilibrium::SolverWorkspace,
        validation::{ValidationConstraints, ValidationRecord},
    };
    let mut demo = Demo::new();
    for h in 0..2048 {
        demo.add("3005", 4, 0., h as f64, 0.);
    }
    let source = scene(demo);
    let tx = transaction().await;
    let input = input(&tx, &source, &Default::default());
    let mut workspace =
        super::super::equilibrium_workspace::NativeSolverWorkspace::new(&tx).unwrap();
    let mut constraints = ValidationConstraints::new(workspace.new_state().unwrap());
    let report = validation::validate_constraints_with_workspace(
        &input,
        &mut constraints,
        Default::default(),
        &Default::default(),
        &mut workspace,
    )
    .unwrap();
    assert_eq!(report.checked_bricks, 2048);
    assert_eq!(report.stability_components, 1);
    assert_eq!(report.largest_equilibrium_component, 0);
    assert_eq!(constraints.len(), 2047);
    assert_eq!(workspace.plan_len(), 0);
    assert!(input.cache.borrow().len() <= CACHE_BODIES);
    let (state, _) = workspace.validation_state();
    assert!(matches!(
        state
            .get(&[vec![0], 0u64.to_be_bytes().to_vec()].concat())
            .unwrap(),
        Some(ValidationRecord::Node {
            component: Some(0),
            ordinal: Some(0),
            ..
        })
    ));
    assert_eq!(
        state
            .page(&[vec![1], 0u64.to_be_bytes().to_vec()].concat(), None, 128)
            .unwrap()
            .len(),
        128
    );
}
