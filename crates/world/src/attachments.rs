//! Source-datum mechanical interfaces shared by editing, admission and physics.
//! Connector fit is geometric evidence, never a measured holding capacity.
use crate::{Aabb, Brick, JointKind, JointSpec, physical};
use glam::DVec3;
use serde::{Deserialize, Serialize};

/// Maximum port overhang beyond a reviewed bearing box, in construction units.
/// Server spatial indexes contain body boxes, so querying both sides' overhang
/// keeps peripheral connectors discoverable without changing stored geometry.
pub const PORT_OVERHANG: f64 = 1.;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Bar,
    Grip,
    Pin,
    RoundHole,
    Axle,
    AxleHole,
    WheelPin,
    WheelSocket,
    Rim,
    Tyre,
    ShoulderPeg,
    ShoulderSocket,
    WristPeg,
    WristSocket,
}

/// Endpoints, radius and optional cross-axle key are in official source LDU.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Connector {
    pub kind: Kind,
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub radius: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inset: f64,
}
fn is_zero(value: &f64) -> bool {
    *value == 0.
}

pub fn connectors(part: &str) -> &'static [Connector] {
    crate::part_library::get(part).map_or(&[], |entry| &entry.catalogue.connectors)
}

pub fn stud_connection(a: &Brick, b: &Brick, tolerance: f64) -> bool {
    stud_anchor(a, b, tolerance).is_some()
}

fn stud_anchor(a: &Brick, b: &Brick, tolerance: f64) -> Option<DVec3> {
    if !physical::is_proper_rotation(physical::metric_transform(a.transform))
        || !physical::is_proper_rotation(physical::metric_transform(b.transform))
    {
        return None;
    }
    for (source, target) in [(a, b), (b, a)] {
        let grids = crate::part_profiles::receiving_grids(&target.part);
        if grids.is_empty() {
            continue;
        }
        if (physical::metric_transform(source.transform).y_axis
            - physical::metric_transform(target.transform).y_axis)
            .length()
            > 1e-7
        {
            continue;
        }
        let mut engaged = Vec::new();
        for stud in crate::part_profiles::stud_centres(&source.part) {
            let p = target.transform.inverse()
                * (source.position + source.transform * stud - target.position);
            if grids.iter().any(|grid| {
                let u = p.x - grid.first[0];
                let v = p.z - grid.first[1];
                (p.y - grid.bottom).abs() <= tolerance
                    && (u - u.round()).abs() <= tolerance
                    && (v - v.round()).abs() <= tolerance
                    && u >= -tolerance
                    && u <= f64::from(grid.width - 1) + tolerance
                    && v >= -tolerance
                    && v <= f64::from(grid.depth - 1) + tolerance
            }) {
                engaged.push(source.position + source.transform * stud);
            }
        }
        if !engaged.is_empty() {
            return Some(engaged.iter().copied().sum::<DVec3>() / engaged.len() as f64);
        }
    }
    None
}

struct Port<'a> {
    source: &'a Connector,
    centre: DVec3,
    axis: DVec3,
    radial: DVec3,
    half: f64,
    radius: f64,
}
impl<'a> Port<'a> {
    fn new(brick: &Brick, source: &'a Connector) -> Self {
        let c = crate::ldraw::conversion();
        let start = physical::to_meters(
            brick.position + brick.transform * c * DVec3::from_array(source.start),
        );
        let end = physical::to_meters(
            brick.position + brick.transform * c * DVec3::from_array(source.end),
        );
        let axis = (end - start).normalize();
        let radial = if let Some(key) = source.key {
            physical::to_meters(brick.transform * c * DVec3::from_array(key)).normalize()
        } else {
            // Keep the square nominal clearance prism in the part's local frame.
            let rotation = physical::metric_transform(brick.transform);
            let local_axis = rotation.transpose() * axis;
            let basis = if local_axis.x.abs() < 0.9 {
                DVec3::X
            } else {
                DVec3::Z
            };
            rotation * (basis - local_axis * local_axis.dot(basis)).normalize()
        };
        Self {
            source,
            centre: (start + end) * 0.5,
            axis,
            radial,
            half: (end - start).length() * 0.5,
            radius: source.radius * 0.0004,
        }
    }
    fn contains_clearance(&self, point: DVec3, epsilon: f64) -> bool {
        let delta = point - self.centre;
        delta.dot(self.axis).abs() <= self.half + epsilon
            && delta.dot(self.radial).abs() <= self.radius + epsilon
            && delta.dot(self.axis.cross(self.radial)).abs() <= self.radius + epsilon
    }
    fn contains_radial(&self, point: DVec3, epsilon: f64) -> bool {
        let delta = point - self.centre;
        delta.dot(self.radial).abs() <= self.radius + epsilon
            && delta.dot(self.axis.cross(self.radial)).abs() <= self.radius + epsilon
    }
}

pub struct Connection {
    pub anchor: DVec3,
    pub kind: JointKind,
    pub label: &'static str,
}

fn compatible(male: &Connector, female: &Connector) -> Option<(&'static str, bool)> {
    use Kind::*;
    Some(match (male.kind, female.kind) {
        (Bar, Grip) => ("minifigure grip / bar connection", false),
        (Pin, RoundHole) => ("Technic pin connection", false),
        (Axle, RoundHole) => ("Technic axle / round bearing", true),
        (Axle, AxleHole) => ("Technic keyed axle connection", false),
        (WheelPin, WheelSocket) => ("wheel pin / bearing connection", true),
        (ShoulderPeg, ShoulderSocket) => ("minifigure shoulder connection", false),
        (WristPeg, WristSocket) => ("minifigure wrist connection", false),
        (Rim, Tyre) if male.group.is_some() && male.group == female.group => {
            ("wheel rim / tyre connection", false)
        }
        _ => return None,
    })
}

fn fits(male: &Port<'_>, female: &Port<'_>, epsilon: f64, rotation_tolerance: f64) -> bool {
    let delta = female.centre - male.centre;
    if male.axis.cross(female.axis).length() > rotation_tolerance
        || (delta - male.axis * delta.dot(male.axis)).length() > epsilon
        || (male.radius - female.radius).abs() > 1e-7
            && !matches!(
                (male.source.kind, female.source.kind),
                (Kind::Pin | Kind::Axle, Kind::RoundHole)
            )
    {
        return false;
    }
    let engagement_half = female.half - female.source.inset * 0.0004;
    if engagement_half <= 0. || delta.dot(male.axis).abs() + engagement_half > male.half + epsilon {
        return false;
    }
    if female.source.kind == Kind::Tyre
        && ((male.half - female.half).abs() > epsilon || delta.length() > epsilon)
    {
        return false;
    }
    if female.source.kind == Kind::AxleHole {
        let key = male.radial.dot(female.radial).abs();
        // Cross axles engage at quarter turns, never at arbitrary roll angles.
        if key > 1e-7 && key < 1. - 1e-7 {
            return false;
        }
    }
    true
}

fn matches(
    a: &Brick,
    b: &Brick,
    tolerance: f64,
    rotation_tolerance: f64,
    mut visit: impl FnMut(Port<'_>, Port<'_>, &'static str, bool) -> bool,
) -> bool {
    if connectors(&a.part).is_empty()
        || connectors(&b.part).is_empty()
        || !physical::is_proper_rotation(physical::metric_transform(a.transform))
        || !physical::is_proper_rotation(physical::metric_transform(b.transform))
    {
        return false;
    }
    let epsilon = tolerance * physical::SCALE.min_element();
    for (male, female) in [(a, b), (b, a)] {
        for m in connectors(&male.part) {
            for f in connectors(&female.part) {
                if let Some((label, rotating)) = compatible(m, f) {
                    let m = Port::new(male, m);
                    let f = Port::new(female, f);
                    if fits(&m, &f, epsilon, rotation_tolerance) && visit(m, f, label, rotating) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

pub fn connection(a: &Brick, b: &Brick, tolerance: f64) -> Option<Connection> {
    connection_with_tolerances(a, b, tolerance, 1e-4)
}

pub fn connection_with_tolerances(
    a: &Brick,
    b: &Brick,
    tolerance: f64,
    rotation_tolerance: f64,
) -> Option<Connection> {
    let mut result = None;
    matches(
        a,
        b,
        tolerance,
        rotation_tolerance,
        |male, female, label, rotating| {
            let free_pin = male.source.kind == Kind::Pin
                && crate::catalog::canonical_id(
                    if connectors(&a.part)
                        .iter()
                        .any(|c| std::ptr::eq(c, male.source))
                    {
                        &a.part
                    } else {
                        &b.part
                    },
                ) == "3673.dat";
            result = Some(Connection {
                anchor: physical::to_world(female.centre),
                kind: if rotating || free_pin {
                    JointKind::Revolute {
                        axis: physical::to_world(female.axis).normalize(),
                        limits: None,
                    }
                } else {
                    JointKind::Fixed
                },
                label,
            });
            true
        },
    );
    result
}

/// Broad-phase includes hands, axle pegs and other ports outside bearing cores.
pub fn bounds(brick: &Brick) -> Aabb {
    let mut bounds = brick.bounds;
    for source in connectors(&brick.part) {
        let p = Port::new(brick, source);
        let radial = (DVec3::ONE - p.axis * p.axis)
            .max(DVec3::ZERO)
            .map(f64::sqrt)
            * p.radius;
        let half = p.axis.abs() * p.half + radial;
        bounds = bounds.union(Aabb::new(
            physical::to_world(p.centre - half),
            physical::to_world(p.centre + half),
        ));
    }
    bounds
}

/// A matching connector only clears the volume of its finite receiving bore.
/// Every vertex of the nominal body-box intersection must fit that prism. This
/// does not disable collisions elsewhere on a pair, e.g. an axe blade in a torso.
pub fn registered_clearance(a: &Brick, b: &Brick, tolerance: f64) -> bool {
    let mut intersection = None;
    matches(a, b, tolerance, 1e-4, |male, female, _label, _rotating| {
        let vertices = intersection.get_or_insert_with(|| body_intersection(a, b));
        vertices.as_ref().is_some_and(|points| {
            points.iter().all(|p| {
                let epsilon = tolerance * physical::SCALE.min_element();
                if female.source.kind == Kind::Grip
                    || male.source.kind == Kind::Axle
                        && female.source.kind == Kind::RoundHole
                        && male.radius <= female.radius
                {
                    // A round bar can roll inside a hand. Its square box
                    // proxy rotates with the tool, not with the fingers.
                    // Likewise a cross axle's outer lobes fit inside a round
                    // bearing at any roll; the empty corners of its box are
                    // not solid material. Keep the finite bore's axial range
                    // and the shaft's own radial envelope; clear no collars.
                    (*p - female.centre).dot(female.axis).abs() <= female.half + epsilon
                        && male.contains_radial(*p, epsilon)
                } else {
                    female.contains_clearance(*p, epsilon)
                }
            })
        })
    })
}

struct BoxPose {
    centre: DVec3,
    axes: [DVec3; 3],
    half: DVec3,
}
impl BoxPose {
    fn new(brick: &Brick) -> Option<Self> {
        let local = crate::geometry::nominal_body_bounds(&brick.part)?;
        let expected = local.transformed(brick.transform, brick.position);
        if (expected.min - brick.bounds.min).abs().max_element() > 1e-7
            || (expected.max - brick.bounds.max).abs().max_element() > 1e-7
        {
            return None;
        }
        Some(Self::at(brick, local))
    }
    fn at(brick: &Brick, local: Aabb) -> Self {
        let rotation = physical::metric_transform(brick.transform);
        Self {
            centre: physical::to_meters(brick.position + brick.transform * local.center()),
            axes: [rotation.x_axis, rotation.y_axis, rotation.z_axis],
            half: physical::to_meters(local.size()) * 0.5,
        }
    }
    fn local(&self, p: DVec3) -> DVec3 {
        let d = p - self.centre;
        DVec3::new(
            d.dot(self.axes[0]),
            d.dot(self.axes[1]),
            d.dot(self.axes[2]),
        )
    }
    fn vertices(&self) -> [DVec3; 8] {
        std::array::from_fn(|i| {
            self.centre
                + (0..3)
                    .map(|axis| {
                        self.axes[axis]
                            * self.half[axis]
                            * if i & (1 << axis) == 0 { -1. } else { 1. }
                    })
                    .sum::<DVec3>()
        })
    }
    fn clip(&self, a: DVec3, b: DVec3) -> Option<[DVec3; 2]> {
        let p = self.local(a);
        let d = self.local(b) - p;
        let (mut lo, mut hi): (f64, f64) = (0., 1.);
        for axis in 0..3 {
            if d[axis].abs() < 1e-14 {
                if p[axis].abs() > self.half[axis] + 1e-12 {
                    return None;
                }
            } else {
                let t0 = (-self.half[axis] - p[axis]) / d[axis];
                let t1 = (self.half[axis] - p[axis]) / d[axis];
                lo = lo.max(t0.min(t1));
                hi = hi.min(t0.max(t1));
                if lo > hi {
                    return None;
                }
            }
        }
        Some([a + (b - a) * lo, a + (b - a) * hi])
    }
}
fn body_intersection(a: &Brick, b: &Brick) -> Option<Vec<DVec3>> {
    BoxPose::new(a)?;
    BoxPose::new(b)?;
    let mut points = Vec::new();
    for left in crate::part_profiles::collision_boxes(&a.part) {
        for right in crate::part_profiles::collision_boxes(&b.part) {
            let left = BoxPose::at(a, left);
            let right = BoxPose::at(b, right);
            for (source, target) in [(&left, &right), (&right, &left)] {
                let corners = source.vertices();
                for i in 0..8 {
                    for axis in 0..3 {
                        let j = i ^ (1 << axis);
                        if i < j
                            && let Some(edge) = target.clip(corners[i], corners[j])
                        {
                            points.extend(edge);
                        }
                    }
                }
            }
        }
    }
    Some(points)
}

/// Separating-axis test for nominal oriented boxes. An AABB alone would make a
/// correctly tilted held tool collide with empty space around its rotated box.
pub fn boxes_penetrate(a: &Brick, b: &Brick, tolerance: f64) -> Option<bool> {
    if !physical::is_proper_rotation(physical::metric_transform(a.transform))
        || !physical::is_proper_rotation(physical::metric_transform(b.transform))
    {
        return None;
    }
    BoxPose::new(a)?;
    BoxPose::new(b)?;
    for left in crate::part_profiles::collision_boxes(&a.part) {
        for right in crate::part_profiles::collision_boxes(&b.part) {
            if boxes_overlap(&BoxPose::at(a, left), &BoxPose::at(b, right), tolerance) {
                return Some(true);
            }
        }
    }
    Some(false)
}
fn boxes_overlap(a: &BoxPose, b: &BoxPose, tolerance: f64) -> bool {
    let axes = a
        .axes
        .into_iter()
        .chain(b.axes)
        .chain(a.axes.into_iter().flat_map(|x| b.axes.map(|y| x.cross(y))));
    for axis in axes.filter(|axis| axis.length_squared() > 1e-14) {
        let axis = axis.normalize();
        let radius = |body: &BoxPose| {
            (0..3)
                .map(|i| body.half[i] * axis.dot(body.axes[i]).abs())
                .sum::<f64>()
        };
        if radius(a) + radius(b) - (a.centre - b.centre).dot(axis).abs()
            <= tolerance * physical::SCALE.min_element()
        {
            return false;
        }
    }
    true
}

pub fn joint(a: &Brick, b: &Brick) -> JointSpec {
    joint_with_tolerance(a, b, 0.002)
}

pub fn joint_with_tolerance(a: &Brick, b: &Brick, tolerance: f64) -> JointSpec {
    let connection = connection(a, b, tolerance);
    let anchor = connection
        .as_ref()
        .map(|c| c.anchor)
        .or_else(|| stud_anchor(a, b, tolerance))
        .unwrap_or((a.bounds.min.max(b.bounds.min) + a.bounds.max.min(b.bounds.max)) * 0.5);
    JointSpec {
        a: a.id,
        b: b.id,
        anchor,
        kind: connection.map_or(JointKind::Fixed, |c| c.kind),
        break_force: None,
        break_torque: None,
        strength_source: None,
    }
}

/// Capacity records do not authorize a remote weld, shifted hand grip, invented
/// hinge axis or stop. Publication validates every claimed joint independently.
pub fn validate_joints(
    bricks: &[Brick],
    joints: &[JointSpec],
    tolerance: f64,
    rotation_tolerance: f64,
) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::collections::{HashMap, HashSet};
    let index: HashMap<_, _> = bricks.iter().map(|b| (b.id, b)).collect();
    let mut pairs = HashSet::new();
    for joint in joints {
        ensure!(
            pairs.insert((joint.a.min(joint.b), joint.a.max(joint.b))),
            "Duplicate attachment joint for pieces {} and {}",
            joint.a,
            joint.b
        );
        let a = index
            .get(&joint.a)
            .context("Attachment endpoint is absent")?;
        let b = index
            .get(&joint.b)
            .context("Attachment endpoint is absent")?;
        if connection(a, b, tolerance).is_some() {
            ensure!(
                connection_with_tolerances(a, b, tolerance, rotation_tolerance).is_some(),
                "Invalid attachment alignment for {} ↔ {}: connector axes exceed the server rotation tolerance",
                a.id,
                b.id
            );
        }
        ensure!(
            crate::spatial::relation_with_tolerance(a, b, tolerance)
                .is_some_and(|r| r.certainty == "inferred"),
            "Invalid attachment {} ({}) ↔ {} ({}): no registered connector fits these poses",
            a.id,
            a.part,
            b.id,
            b.part
        );
        let expected = joint_with_tolerance(a, b, tolerance);
        ensure!(
            (joint.anchor - expected.anchor).abs().max_element() <= tolerance,
            "Invalid attachment anchor for {} ↔ {}: use the registered interface datum",
            a.id,
            b.id
        );
        match (&joint.kind, &expected.kind) {
            (JointKind::Fixed, JointKind::Fixed) => {}
            (JointKind::Revolute { axis, limits }, JointKind::Revolute { axis: correct, .. }) => {
                let axis = physical::to_meters(*axis).normalize();
                let correct = physical::to_meters(*correct).normalize();
                ensure!(
                    axis.cross(correct).length() <= rotation_tolerance,
                    "Invalid attachment hinge axis for {} ↔ {}: it must follow the registered bearing",
                    a.id,
                    b.id
                );
                ensure!(
                    limits.is_none(),
                    "Invalid attachment limits for {} ↔ {}: this bearing has no registered rotation stops",
                    a.id,
                    b.id
                );
            }
            _ => anyhow::bail!(
                "Invalid attachment joint kind for {} ↔ {}: fixed/revolute behavior must match the registered connector",
                a.id,
                b.id
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DMat3, DQuat};

    fn part(id: &str, index: u32) -> Brick {
        let mut demo = crate::demo::Demo::new();
        demo.add(id.trim_end_matches(".dat"), 0, 0., 0., 0.);
        let mut brick = demo.bricks.remove(0);
        brick.id = index;
        brick.position = DVec3::ZERO;
        update(&mut brick);
        brick
    }
    fn update(brick: &mut Brick) {
        brick.bounds = crate::geometry::nominal_body_bounds(&brick.part)
            .unwrap()
            .transformed(brick.transform, brick.position);
    }
    fn mate(male: &mut Brick, female: &Brick, mi: usize, fi: usize) {
        let m = Port::new(male, &connectors(&male.part)[mi]);
        let f = Port::new(female, &connectors(&female.part)[fi]);
        let rotation = DMat3::from_quat(DQuat::from_rotation_arc(m.axis, f.axis));
        male.position = physical::to_world(f.centre - rotation * m.centre);
        male.transform = physical::world_transform(rotation);
        update(male);
    }
    fn rotate_pair(bricks: &mut [Brick], rotation: DMat3) {
        let rotation = physical::world_transform(rotation);
        for b in bricks {
            b.transform = rotation * b.transform;
            b.position = rotation * b.position + DVec3::new(10., 10., 10.);
            update(b);
        }
    }

    #[test]
    fn hands_grip_real_bars_outside_torso_bearing_boxes_at_arbitrary_rigid_poses() {
        for torso in ["3820.dat", "76382p00.dat", "76382p4f.dat"] {
            let hand = part(torso, 1);
            for grip in
                (0..connectors(torso).len()).filter(|i| connectors(torso)[*i].kind == Kind::Grip)
            {
                let mut bar = part("30374.dat", 2);
                mate(&mut bar, &hand, 0, grip);
                assert!(connection(&bar, &hand, 0.002).is_some(), "{torso}");
                assert_eq!(
                    crate::constraints::inferred_joints(&[hand.clone(), bar.clone()]).len(),
                    1,
                    "Ports outside bearing boxes must survive broad phase"
                );
                // Common rigid rotation/translation preserves the same fit.
                let mut pair = [bar.clone(), hand.clone()];
                rotate_pair(
                    &mut pair,
                    DMat3::from_rotation_z(0.73) * DMat3::from_rotation_x(0.31),
                );
                assert!(connection(&pair[0], &pair[1], 0.002).is_some());
                bar.translated(DVec3::X * 0.02);
                assert!(connection(&bar, &hand, 0.002).is_none());
            }
        }
    }
    #[test]
    fn pins_fit_only_real_bores_and_never_clear_the_rest_of_a_pair() {
        let beam = part("3700.dat", 1);
        let mut pin = part("2780.dat", 2);
        mate(&mut pin, &beam, 0, 0);
        assert!(matches!(joint(&pin, &beam).kind, JointKind::Fixed));
        assert!(registered_clearance(&pin, &beam, 0.002));
        assert!(!crate::interference::bodies_penetrate(&pin, &beam, 0.002));
        let mut rotated = [pin.clone(), beam.clone()];
        rotate_pair(
            &mut rotated,
            DMat3::from_rotation_y(0.43) * DMat3::from_rotation_z(0.53),
        );
        assert!(registered_clearance(&rotated[0], &rotated[1], 0.002));
        pin.translated(DVec3::X * 0.1);
        assert!(connection(&pin, &beam, 0.002).is_none());
        assert!(crate::interference::bodies_penetrate(&pin, &beam, 0.002));
        let solid = part("3004.dat", 3);
        assert!(!registered_clearance(&pin, &solid, 0.002));
    }
    #[test]
    fn cross_axles_lock_in_bushes_but_turn_in_round_bearings() {
        let bush = part("3713.dat", 1);
        let mut axle = part("3705.dat", 2);
        mate(&mut axle, &bush, 0, 0);
        assert!(connection(&axle, &bush, 0.002).is_some());
        assert!(registered_clearance(&axle, &bush, 0.002));
        assert!(matches!(joint(&axle, &bush).kind, JointKind::Fixed));
        let axis = Port::new(&bush, &connectors(&bush.part)[0]).axis;
        axle.transform =
            physical::world_transform(DMat3::from_axis_angle(axis, std::f64::consts::FRAC_PI_4))
                * axle.transform;
        update(&mut axle);
        assert!(
            connection(&axle, &bush, 0.002).is_none(),
            "Mis-keyed cross axles must not fit"
        );
        let beam = part("32523.dat", 3);
        axle = part("3705.dat", 2);
        mate(&mut axle, &beam, 0, 0);
        assert!(matches!(
            joint(&axle, &beam).kind,
            JointKind::Revolute { .. }
        ));
        assert!(registered_clearance(&axle, &beam, 0.002));
        let axis = Port::new(&beam, &connectors(&beam.part)[0]).axis;
        let original = axle.transform;
        for angle in [0.13, std::f64::consts::FRAC_PI_4, 0.9] {
            axle.transform =
                physical::world_transform(DMat3::from_axis_angle(axis, angle)) * original;
            update(&mut axle);
            assert!(matches!(
                joint(&axle, &beam).kind,
                JointKind::Revolute { limits: None, .. }
            ));
            assert!(
                registered_clearance(&axle, &beam, 0.002),
                "A free cross axle must not acquire square-box corner interference at roll {angle}"
            );
        }
        axle.translated(DVec3::X * 0.02);
        assert!(connection(&axle, &beam, 0.002).is_none());
        assert!(!registered_clearance(&axle, &beam, 0.002));
    }
    #[test]
    fn wheel_bearings_rotate_and_tyres_require_their_matching_rim() {
        let wheel = part("4624.dat", 1);
        let mut holder = part("4600.dat", 2);
        mate(&mut holder, &wheel, 0, 0);
        assert!(connection(&holder, &wheel, 0.002).is_some());
        assert!(matches!(
            joint(&holder, &wheel).kind,
            JointKind::Revolute { .. }
        ));
        let tyre = part("3641.dat", 3);
        assert!(registered_clearance(&wheel, &tyre, 0.002));
        let wrong = part("3483.dat", 4);
        assert!(connection(&wheel, &wrong, 0.002).is_none());
        let mut sheared = holder.clone();
        sheared.transform *= 1.01;
        assert!(connection(&sheared, &wheel, 0.002).is_none());
        let mut joints = vec![joint(&holder, &wheel)];
        assert!(
            physical::ConnectionMeasurements::default()
                .apply(&[holder, wheel], &mut joints)
                .is_err()
        );
    }
    #[test]
    fn arch_feet_accept_studs_but_the_opening_cannot_be_welded() {
        let mut arch = part("3308a.dat", 1);
        arch.position.y = 2.;
        update(&mut arch);
        for x in [-3.5, 3.5] {
            let mut support = part("3005.dat", 2);
            support.position.x = x;
            update(&mut support);
            assert!(stud_connection(&support, &arch, 1e-6));
        }
        let unsupported = part("3005.dat", 3);
        assert!(!stud_connection(&unsupported, &arch, 1e-6));
    }
    #[test]
    fn corner_panel_receivers_do_not_mirror_source_z() {
        let mut panel = part("2345.dat", 1);
        panel.position.y = 7.;
        update(&mut panel);
        let mut support = part("3005.dat", 2);
        support.position =
            panel.position + crate::ldraw::conversion() * DVec3::new(-20., 144., 20.);
        update(&mut support);
        assert!(stud_connection(&support, &panel, 1e-6));
        support.position =
            panel.position + crate::ldraw::conversion() * DVec3::new(-20., 144., -20.);
        update(&mut support);
        assert!(!stud_connection(&support, &panel, 1e-6));
    }
}
