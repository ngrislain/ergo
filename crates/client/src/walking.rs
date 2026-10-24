//! Shared kinematic collision scene for flying, walking and ballistic jumping.
use ergo_world::{Aabb, BlockCoord, Brick, physical, spatial::Octree};
use glam::DVec3;

pub const STEP_HEIGHT: f64 = 3.0;
pub const JUMP_HEIGHT: f64 = 12.0;
const GRAVITY: f64 = physical::GRAVITY_M_S2 / physical::BRICK_HEIGHT_M;
const RADIUS: f64 = 0.28;
const SKIN: f64 = 1e-5;
const MAX_SUBSTEP: f64 = 1.0 / 240.0;

pub struct Scene<'a> {
    pub bricks: &'a [Brick],
    pub tree: &'a Octree,
    pub eye_height: f64,
    pub blocked: &'a dyn Fn(BlockCoord) -> bool,
}

impl<'a> Scene<'a> {
    /// Continuous camera-volume sweep and wall sliding; flight has no stepping
    /// or gravity. Reuses the walker's broad phase and unloaded-cell policy.
    pub fn fly_slide(&self, eye: DVec3, movement: DVec3) -> DVec3 {
        let radius = DVec3::new(
            RADIUS,
            RADIUS * physical::STUD_PITCH_M / physical::BRICK_HEIGHT_M,
            RADIUS,
        );
        let mut position = eye;
        position.y = position.y.max(radius.y + SKIN);
        let mut remaining = movement;
        for _ in 0..3 {
            if remaining.length_squared() < 1e-20 {
                break;
            }
            let sweep = Aabb::new(
                position.min(position + remaining) - radius,
                position.max(position + remaining) + radius,
            );
            let mut obstacles = self.colliders(sweep).collect::<Vec<_>>();
            let lo = BlockCoord::at(sweep.min);
            let hi = BlockCoord::at(sweep.max);
            if [hi.0 - lo.0, hi.1 - lo.1, hi.2 - lo.2]
                .iter()
                .any(|n| *n > 4)
            {
                return eye;
            }
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        let cell = BlockCoord(x, y, z);
                        if (self.blocked)(cell) {
                            let p = cell.origin();
                            obstacles.push(Aabb::new(p, p + DVec3::splat(ergo_world::BLOCK_SIZE)));
                        }
                    }
                }
            }
            let mut collision = None;
            for bounds in obstacles {
                if let Some((time, normal)) = sweep_box(
                    position,
                    remaining,
                    Aabb::new(bounds.min - radius, bounds.max + radius),
                ) {
                    if collision.is_none_or(|(old, _)| time < old) {
                        collision = Some((time, normal));
                    }
                }
            }
            if remaining.y < 0. {
                let time = (radius.y + SKIN - position.y) / remaining.y;
                if (0.0..=1.0).contains(&time) && collision.is_none_or(|(old, _)| time < old) {
                    collision = Some((time, DVec3::Y));
                }
            }
            let Some((time, normal)) = collision else {
                position += remaining;
                break;
            };
            let travel = (time - SKIN / remaining.length()).clamp(0., 1.);
            position += remaining * travel;
            remaining *= 1. - travel;
            remaining -= normal * remaining.dot(normal).min(0.);
        }
        position
    }
    pub fn new(
        bricks: &'a [Brick],
        tree: &'a Octree,
        eye_height: f64,
        blocked: &'a dyn Fn(BlockCoord) -> bool,
    ) -> Self {
        Self {
            bricks,
            tree,
            eye_height,
            blocked,
        }
    }

    fn colliders(&self, bounds: Aabb) -> impl Iterator<Item = Aabb> + '_ {
        self.tree
            .query(bounds)
            .into_iter()
            .map(|i| self.bricks[i].bounds)
    }

    fn loaded_between(&self, from: DVec3, to: DVec3) -> bool {
        self.loaded(self.body(from).union(self.body(to)))
    }
    fn height(&self) -> f64 {
        self.eye_height + 0.25
    }

    fn body(&self, feet: DVec3) -> Aabb {
        Aabb::new(
            feet + DVec3::new(-RADIUS, SKIN, -RADIUS),
            feet + DVec3::new(RADIUS, self.height(), RADIUS),
        )
    }

    fn loaded(&self, bounds: Aabb) -> bool {
        let min = BlockCoord::at(bounds.min + DVec3::splat(SKIN));
        let max = BlockCoord::at(bounds.max - DVec3::splat(SKIN));
        (min.0..=max.0).all(|x| {
            (min.1..=max.1).all(|y| (min.2..=max.2).all(|z| !(self.blocked)(BlockCoord(x, y, z))))
        })
    }

    pub fn clear(&self, feet: DVec3) -> bool {
        let body = self.body(feet);
        self.loaded(body)
            && !self
                .colliders(body)
                .any(|b| (body.max.min(b.max) - body.min.max(b.min)).min_element() > SKIN)
    }

    pub fn support_height(&self, x: f64, z: f64, ceiling: f64) -> f64 {
        let footprint = Aabb::new(
            DVec3::new(x - RADIUS, -SKIN, z - RADIUS),
            DVec3::new(
                x + RADIUS,
                ceiling.clamp(0.0, ergo_world::MAX_WORLD_COORD),
                z + RADIUS,
            ),
        );
        self.colliders(footprint)
            .filter(|b| b.max.y <= ceiling + SKIN && horizontal_overlap(footprint, *b))
            .map(|b| b.max.y)
            .fold(0.0, f64::max)
    }

    /// Sweep the head, rather than checking only the final body position.
    fn upward_limit(&self, feet: DVec3, next_y: f64) -> f64 {
        let head = feet.y + self.height();
        let sweep = Aabb::new(
            DVec3::new(feet.x - RADIUS, head - SKIN, feet.z - RADIUS),
            DVec3::new(feet.x + RADIUS, next_y + self.height(), feet.z + RADIUS),
        );
        self.colliders(sweep)
            .filter(|b| b.min.y >= head - SKIN && horizontal_overlap(sweep, *b))
            .map(|b| b.min.y - self.height())
            .fold(next_y, f64::min)
            .max(feet.y)
    }

    fn grounded(&self, feet: DVec3) -> bool {
        (feet.y - self.support_height(feet.x, feet.z, feet.y + SKIN)).abs() <= SKIN
    }

    fn move_horizontal(&self, feet: DVec3, axis: usize, movement: f64, can_step: bool) -> DVec3 {
        if movement == 0.0 {
            return feet;
        }
        let mut next = feet;
        next[axis] += movement;
        if self.clear(next) {
            return next;
        }
        if !can_step || !self.grounded(feet) {
            return feet;
        }
        let support = self.support_height(next.x, next.z, feet.y + STEP_HEIGHT);
        let raised = DVec3::new(feet.x, support, feet.z);
        next.y = support;
        // Check the upward sweep and raised motion, not just the destination.
        if support > feet.y + SKIN
            && self.upward_limit(feet, support) >= support - SKIN
            && self.loaded_between(feet, raised)
            && self.clear(raised)
            && self.clear(next)
        {
            next
        } else {
            feet
        }
    }
}

fn horizontal_overlap(a: Aabb, b: Aabb) -> bool {
    let overlap = a.max.min(b.max) - a.min.max(b.min);
    overlap.x > SKIN && overlap.z > SKIN
}

fn sweep_box(origin: DVec3, delta: DVec3, bounds: Aabb) -> Option<(f64, DVec3)> {
    let mut enter = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    let mut normal = DVec3::ZERO;
    for axis in 0..3 {
        if delta[axis].abs() < 1e-12 {
            if origin[axis] < bounds.min[axis] || origin[axis] > bounds.max[axis] {
                return None;
            }
        } else {
            let a = (bounds.min[axis] - origin[axis]) / delta[axis];
            let b = (bounds.max[axis] - origin[axis]) / delta[axis];
            let near = a.min(b);
            exit = exit.min(a.max(b));
            if near > enter {
                enter = near;
                normal = DVec3::ZERO;
                normal[axis] = -delta[axis].signum();
            }
        }
    }
    (enter >= 0. && enter <= 1. && enter <= exit).then_some((enter, normal))
}

#[derive(Default)]
pub struct Walker {
    vertical_velocity: f64,
    jump_held: bool,
}

impl Walker {
    pub fn advance(
        &mut self,
        scene: &Scene<'_>,
        mut feet: DVec3,
        horizontal_velocity: DVec3,
        dt: f64,
        jump: bool,
    ) -> DVec3 {
        let dt = dt.clamp(0.0, crate::navigation::MAX_FRAME_DT);
        if dt == 0.0 {
            return feet;
        }
        let launch = jump && !self.jump_held;
        self.jump_held = jump;
        if !scene.clear(feet) {
            self.vertical_velocity = 0.0;
            return feet;
        }
        if launch && scene.grounded(feet) && self.vertical_velocity <= 0.0 {
            self.vertical_velocity = (2.0 * GRAVITY * JUMP_HEIGHT).sqrt();
        }

        // Small horizontal increments prevent tunneling through thin walls.
        // Temporal substeps keep collision/stepping interactions responsive.
        let count = (horizontal_velocity.x.abs().max(horizontal_velocity.z.abs()) * dt / 0.1)
            .max(dt / MAX_SUBSTEP)
            .ceil() as usize;
        let step = dt / count as f64;
        for _ in 0..count {
            for axis in [0, 2] {
                feet = scene.move_horizontal(
                    feet,
                    axis,
                    horizontal_velocity[axis] * step,
                    self.vertical_velocity <= 0.0,
                );
            }

            // Split at the apex so a ceiling cannot be skipped during a frame
            // that contains both ascent and descent. Each leg integrates the
            // constant-gravity trajectory exactly, without Euler height loss.
            let ascent = (self.vertical_velocity / GRAVITY).clamp(0.0, step);
            if ascent > 0.0 {
                self.vertical_step(scene, &mut feet, ascent);
            }
            if step > ascent {
                self.vertical_step(scene, &mut feet, step - ascent);
            }
        }
        feet
    }

    fn vertical_step(&mut self, scene: &Scene<'_>, feet: &mut DVec3, dt: f64) {
        let next_y = feet.y + self.vertical_velocity * dt - 0.5 * GRAVITY * dt * dt;
        self.vertical_velocity -= GRAVITY * dt;
        let y = if next_y > feet.y {
            scene.upward_limit(*feet, next_y)
        } else {
            // The highest crossed top catches even arbitrarily thin platforms.
            next_y.max(scene.support_height(feet.x, feet.z, feet.y + SKIN))
        };
        let next = DVec3::new(feet.x, y, feet.z);
        if !scene.loaded_between(*feet, next) {
            self.vertical_velocity = 0.0;
            return;
        }
        if (y - next_y).abs() > SKIN {
            self.vertical_velocity = 0.0;
        }
        feet.y = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::{DEFAULT_EYE_HEIGHT, MIN_EYE_HEIGHT};
    use glam::DMat3;

    #[test]
    fn flight_sweeps_thin_walls_and_slides_without_tunneling() {
        let world = TestWorld::new([brick([1., 0., -10.], [1.01, 10., 10.])]);
        let scene = world.scene();
        let position = scene.fly_slide(DVec3::new(0., 3., 0.), DVec3::new(100., 0., 4.));
        assert!(position.x < 1. - RADIUS && position.x > 0.7);
        assert!((position.z - 4.).abs() < 1e-6);
        let away = scene.fly_slide(position, DVec3::new(-3., 0., 0.));
        assert!(away.x < position.x - 2.9);
    }
    #[test]
    fn flight_hits_floor_ceiling_and_unloaded_cells() {
        let world = TestWorld::new([brick([-3., 5., -3.], [3., 5.01, 3.])]);
        let scene = world.scene();
        assert!(scene.fly_slide(DVec3::new(0., 3., 0.), DVec3::Y * 50.).y < 5.);
        assert!(
            scene
                .fly_slide(DVec3::new(0., 3., 0.), DVec3::NEG_Y * 50.)
                .y
                > 0.
        );
        let blocked = |c: BlockCoord| c.0 >= 1;
        let scene = Scene::new(&world.bricks, &world.tree, 4., &blocked);
        let position = scene.fly_slide(DVec3::new(1020., 3., 0.5), DVec3::X * 20.);
        assert!(position.x <= 1024. - RADIUS);
    }

    fn brick(min: [f64; 3], max: [f64; 3]) -> Brick {
        let bounds = Aabb::new(min.into(), max.into());
        Brick {
            id: 1,
            part: "3001.dat".into(),
            color: 4,
            rgb: 0,
            position: bounds.min,
            transform: DMat3::IDENTITY,
            bounds,
            render_bounds: bounds,
            step: 0,
        }
    }

    struct TestWorld {
        bricks: Vec<Brick>,
        tree: Octree,
    }

    impl TestWorld {
        fn new(bricks: impl IntoIterator<Item = Brick>) -> Self {
            let bricks: Vec<_> = bricks.into_iter().collect();
            let tree = Octree::new(bricks.iter().map(|b| b.bounds).collect());
            Self { bricks, tree }
        }

        fn scene(&self) -> Scene<'_> {
            Scene::new(&self.bricks, &self.tree, DEFAULT_EYE_HEIGHT, &|_| false)
        }
    }

    #[test]
    fn walks_up_three_units_but_not_taller_obstacles() {
        for (height, climbs) in [(3.0, true), (3.01, false), (12.0, false)] {
            let world = TestWorld::new([brick([1.0, 0.0, -2.0], [8.0, height, 2.0])]);
            let scene = world.scene();
            let mut walker = Walker::default();
            let mut feet = DVec3::ZERO;
            for _ in 0..20 {
                feet = walker.advance(&scene, feet, DVec3::X * 18.0, 1.0 / 60.0, false);
                assert!(scene.clear(feet));
            }
            if climbs {
                assert!(feet.x > 5.0);
                assert!((feet.y - height).abs() < SKIN);
            } else {
                assert!(feet.x + RADIUS <= 1.0 + SKIN);
                assert!(feet.y.abs() < SKIN);
            }
        }
    }

    #[test]
    fn step_checks_the_entire_upward_head_sweep() {
        let world = TestWorld::new([
            brick([1.0, 0.0, -2.0], [8.0, 3.0, 2.0]),
            brick([-2.0, 2.0, -2.0], [0.7, 2.01, 2.0]),
        ]);
        let mut scene = world.scene();
        // The shortest selectable body fits both below and above this plate;
        // endpoint-only clearance would incorrectly teleport through it.
        scene.eye_height = MIN_EYE_HEIGHT;
        let mut walker = Walker::default();
        let mut feet = DVec3::ZERO;
        for _ in 0..20 {
            feet = walker.advance(&scene, feet, DVec3::X * 18.0, 1.0 / 60.0, false);
            assert!(scene.clear(feet));
        }
        assert!(feet.x + RADIUS <= 1.0 + SKIN);
        assert!(feet.y.abs() < SKIN);
    }

    #[test]
    fn airborne_character_cannot_auto_step_or_wall_climb() {
        let world = TestWorld::new([brick([1.0, 0.0, -2.0], [8.0, 7.0, 2.0])]);
        let scene = world.scene();
        let mut walker = Walker::default();
        let mut feet = DVec3::Y * 4.0;
        for _ in 0..20 {
            feet = walker.advance(&scene, feet, DVec3::X * 18.0, 1.0 / 60.0, false);
            assert!(feet.y <= 4.0);
            assert!(scene.clear(feet));
        }
        assert!(feet.x + RADIUS <= 1.0 + SKIN);
        assert!(feet.y.abs() < SKIN);
    }

    #[test]
    fn jump_trajectory_uses_metric_gravity_at_different_frame_rates() {
        let world = TestWorld::new([]);
        let scene = world.scene();
        let launch = (2.0 * GRAVITY * JUMP_HEIGHT).sqrt();
        for count in [3, 6, 12, 24] {
            let mut walker = Walker::default();
            let mut feet = DVec3::ZERO;
            for i in 0..count {
                feet = walker.advance(&scene, feet, DVec3::ZERO, 0.1 / count as f64, i == 0);
            }
            let expected = launch * 0.1 - 0.5 * GRAVITY * 0.1_f64.powi(2);
            assert!((feet.y - expected).abs() < 1e-8, "{count}: {feet:?}");
        }
    }

    #[test]
    fn jump_peaks_at_twelve_units_above_takeoff_and_lands() {
        let world = TestWorld::new([brick([-5.0, 0.0, -5.0], [5.0, 3.0, 5.0])]);
        let scene = world.scene();
        let mut walker = Walker::default();
        let mut feet = DVec3::Y * 3.0;
        let mut peak: f64 = feet.y;
        for i in 0..120 {
            feet = walker.advance(&scene, feet, DVec3::ZERO, 1.0 / 240.0, i == 0);
            peak = peak.max(feet.y);
            assert!(scene.clear(feet));
        }
        assert!((peak - 15.0).abs() < 0.003, "peak: {peak}");
        assert!((feet.y - 3.0).abs() < SKIN);
        assert_eq!(walker.vertical_velocity, 0.0);
    }

    #[test]
    fn holding_jump_does_not_bounce_or_allow_midair_relaunch() {
        let world = TestWorld::new([]);
        let scene = world.scene();
        let mut walker = Walker::default();
        let mut feet = DVec3::ZERO;
        for i in 0..120 {
            feet = walker.advance(&scene, feet, DVec3::ZERO, 1.0 / 120.0, true);
            if i > 45 {
                assert!(feet.y.abs() < SKIN);
            }
        }
        feet = walker.advance(&scene, feet, DVec3::ZERO, 1.0 / 120.0, false);
        feet = walker.advance(&scene, feet, DVec3::ZERO, 1.0 / 120.0, true);
        assert!(feet.y > 0.0);
        feet = walker.advance(&scene, feet, DVec3::ZERO, 1.0 / 120.0, false);
        let before = walker.vertical_velocity;
        walker.advance(&scene, feet, DVec3::ZERO, 1.0 / 120.0, true);
        assert!(walker.vertical_velocity < before);
    }

    #[test]
    fn jump_hits_a_thin_ceiling_and_falls_back_down() {
        let world = TestWorld::new([brick([-5.0, 6.0, -5.0], [5.0, 6.01, 5.0])]);
        let scene = world.scene();
        let mut walker = Walker::default();
        let mut feet = DVec3::ZERO;
        let mut peak: f64 = 0.0;
        for i in 0..30 {
            feet = walker.advance(&scene, feet, DVec3::ZERO, 1.0 / 30.0, i == 0);
            peak = peak.max(feet.y);
            assert!(feet.y + scene.height() <= 6.0 + SKIN);
            assert!(scene.clear(feet));
        }
        assert!(peak > 1.0);
        assert!(feet.y.abs() < SKIN);
    }

    #[test]
    fn fast_fall_lands_on_a_thin_platform_without_tunneling() {
        let world = TestWorld::new([brick([-5.0, 4.99, -5.0], [5.0, 5.0, 5.0])]);
        let scene = world.scene();
        let mut walker = Walker {
            vertical_velocity: -2000.0,
            ..Default::default()
        };
        let feet = walker.advance(&scene, DVec3::Y * 20.0, DVec3::ZERO, 0.05, false);
        assert!((feet.y - 5.0).abs() < SKIN);
        assert_eq!(walker.vertical_velocity, 0.0);
        assert!(scene.clear(feet));
    }

    #[test]
    fn jump_clears_an_obstacle_too_tall_to_step() {
        let world = TestWorld::new([brick([1.0, 0.0, -2.0], [3.0, 4.0, 2.0])]);
        let scene = world.scene();
        let mut walker = Walker::default();
        let mut feet = DVec3::ZERO;
        for i in 0..30 {
            feet = walker.advance(&scene, feet, DVec3::X * 18.0, 1.0 / 60.0, i == 0);
            assert!(scene.clear(feet));
        }
        assert!(feet.x > 8.0);
        assert!(feet.y.abs() < SKIN);
    }

    #[test]
    fn maximum_speed_slides_along_thin_walls_without_crossing() {
        let world = TestWorld::new([brick([1.0, 0.0, -10.0], [1.01, 20.0, 20.0])]);
        let scene = world.scene();
        let mut walker = Walker::default();
        let feet = walker.advance(
            &scene,
            DVec3::ZERO,
            DVec3::new(90.0, 0.0, 90.0),
            0.05,
            false,
        );
        assert!(feet.x + RADIUS <= 1.0 + SKIN);
        assert!(feet.z > 4.0);
        assert!(scene.clear(feet));
    }

    #[test]
    fn unloaded_cells_block_the_entire_body_and_vertical_sweep() {
        let world = TestWorld::new([]);
        let blocked = |cell: BlockCoord| cell.1 >= 1;
        let scene = Scene {
            blocked: &blocked,
            ..world.scene()
        };
        let mut walker = Walker::default();
        let feet = walker.advance(&scene, DVec3::Y * 1019.0, DVec3::ZERO, 0.05, true);
        // Start airborne, falling remains allowed away from the blocked cell.
        assert!(feet.y < 1019.0);
        let mut walker = Walker {
            vertical_velocity: 150.0,
            ..Default::default()
        };
        let feet = walker.advance(&scene, DVec3::Y * 1019.0, DVec3::ZERO, 0.05, false);
        assert!(feet.y + scene.height() <= 1024.0);
        assert!(scene.clear(feet));
        assert!(!scene.clear(DVec3::Y * 1020.0));
    }
}
