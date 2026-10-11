//! Small project-owned racing simulation. The placeholder handling model is
//! deterministic for a given platform and fixed input sequence; it is not a
//! claim of compatibility with any commercial game.

#[cfg(test)]
use std::f64::consts::TAU;

use track_edge::NO_GROUND_RESET_SECONDS;

pub mod arcade_model;
pub mod classic_model;
pub mod drivetrain;
pub mod model_ext;
pub mod preview_route_audit;
mod provisional_model;
pub mod road_pull;
pub mod scripted_model;
mod state_frame;
pub mod track_edge;
mod vehicle_model;

pub use arcade_model::{ArcadeConfig, ARCADE_MODEL_REVISION};
pub use classic_model::{ClassicConfig, CLASSIC_MODEL_REVISION};
pub use drivetrain::{DriveState, DriveTelemetry, Transmission};
pub use scripted_model::{ScriptedControl, SCRIPTED_MODEL_REVISION};
pub use state_frame::{SimStateFrame, SIMULATION_MODEL_REVISION, SIM_STATE_FRAME_VERSION};
pub use track_edge::{TrackEdge, TrackEdgeRow, TRACK_EDGE_MODEL_REVISION};
pub use vehicle_model::VehicleModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimTiming {
    pub physics_hz: u32,
    pub input_hz: u32,
    pub race_clock_hz: u32,
}

impl Default for SimTiming {
    fn default() -> Self {
        Self {
            physics_hz: 60,
            input_hz: 60,
            race_clock_hz: 60,
        }
    }
}

impl SimTiming {
    pub fn step_seconds(self) -> f64 {
        1.0 / f64::from(self.physics_hz.max(1))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
    fn distance(self, rhs: Self) -> f64 {
        (self - rhs).length()
    }
    pub fn dot(self, rhs: Self) -> f64 {
        self.x * rhs.x + self.y * rhs.y
    }
    fn cross(self, rhs: Self) -> f64 {
        self.x * rhs.y - self.y * rhs.x
    }
    fn normalized(self) -> Option<Self> {
        let length = self.length();
        (length.is_finite() && length > 1.0e-9).then(|| self * (1.0 / length))
    }
}

impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}
impl std::ops::Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}
impl std::ops::Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriverInput {
    /// Forward intent in [0, 1]. Values outside the range are clamped.
    pub throttle: f64,
    /// Brake intent in [0, 1]. Values outside the range are clamped.
    pub brake: f64,
    /// Steering intent in [-1, 1]. Positive turns counter-clockwise.
    pub steer: f64,
    pub handbrake: bool,
    /// Edge-style intent to request one gear up (default binding: A).
    pub shift_up: bool,
    /// Edge-style intent to request one gear down (default binding: Z).
    pub shift_down: bool,
    pub reset_request: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vehicle {
    pub position: Vec2,
    /// Render-space support height used only by an installed local preview.
    /// Network race state remains the existing planar simulation contract.
    pub position_z: f64,
    /// Radians counter-clockwise from +X.
    pub heading: f64,
    pub velocity: Vec2,
    pub angular_velocity: f64,
    pub checkpoint: usize,
    pub completed_laps: u32,
    pub race_ticks: u64,
    pub finished: bool,
}

/// One vehicle presented to the deterministic post-integration contact pass.
/// Geometry and vertical bounds must come from the same project-owned vehicle
/// profile on authoritative and predicting peers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VehicleContactBody {
    pub stable_id: u32,
    pub vehicle: Vehicle,
    pub previous_position: Vec2,
    pub radius: f64,
    pub min_z: f64,
    pub max_z: f64,
}

const MAX_CONTACT_BODIES: usize = 8;
const VEHICLE_CONTACT_ITERATIONS: usize = 16;
const VEHICLE_CONTACT_EPSILON: f64 = 1.0e-6;
/// Bump when the pairwise contact algorithm, iteration count, epsilon, or
/// resulting contact behavior changes. Aggregate server frame identity binds it.
pub const VEHICLE_CONTACT_MODEL_REVISION: &str = "provisional-pairwise-contact-v1";

/// Resolve one fixed tick of horizontal car-to-car contact after every vehicle
/// has been stepped once. Bodies are sorted by stable ID internally, so input
/// map iteration order cannot change the result. Contact preserves each car's
/// vertical position; callers should reapply its track constraint afterward.
pub fn resolve_pairwise_vehicle_contacts(
    bodies: &mut [VehicleContactBody],
) -> Result<usize, &'static str> {
    if bodies.len() > MAX_CONTACT_BODIES {
        return Err("vehicle contact body count exceeds its limit");
    }
    for body in bodies.iter() {
        if !body.radius.is_finite()
            || !(0.01..=100.0).contains(&body.radius)
            || !body.min_z.is_finite()
            || !body.max_z.is_finite()
            || body.min_z >= body.max_z
            || body.min_z.abs() > 1.0e6
            || body.max_z.abs() > 1.0e6
            || !body.previous_position.x.is_finite()
            || !body.previous_position.y.is_finite()
            || !body.vehicle.position.x.is_finite()
            || !body.vehicle.position.y.is_finite()
            || !body.vehicle.position_z.is_finite()
            || !body.vehicle.velocity.x.is_finite()
            || !body.vehicle.velocity.y.is_finite()
            || !body.vehicle.heading.is_finite()
            || [
                body.previous_position.x,
                body.previous_position.y,
                body.vehicle.position.x,
                body.vehicle.position.y,
                body.vehicle.position_z,
                body.vehicle.velocity.x,
                body.vehicle.velocity.y,
            ]
            .iter()
            .any(|value| value.abs() > 1.0e6)
        {
            return Err("vehicle contact body contains invalid bounds or state");
        }
    }
    bodies.sort_by_key(|body| body.stable_id);
    if bodies
        .windows(2)
        .any(|pair| pair[0].stable_id == pair[1].stable_id)
    {
        return Err("vehicle contact stable IDs must be unique");
    }

    let mut contacted = vec![false; bodies.len()];
    let mut swept_resolved = vec![vec![false; bodies.len()]; bodies.len()];
    for _ in 0..VEHICLE_CONTACT_ITERATIONS {
        for left in 0..bodies.len() {
            for right in (left + 1)..bodies.len() {
                if !vertical_intervals_overlap(&bodies[left], &bodies[right]) {
                    continue;
                }
                let radius = bodies[left].radius + bodies[right].radius;
                let start = bodies[left].previous_position - bodies[right].previous_position;
                let end = bodies[left].vehicle.position - bodies[right].vehicle.position;
                let relative_motion = end - start;
                let start_sq = start.dot(start);
                let end_sq = end.dot(end);
                let mut normal = end.normalized();
                let penetration = (radius - end_sq.sqrt()).max(0.0);
                let has_motion = relative_motion.dot(relative_motion) > 1.0e-12;
                let closest = closest_point_on_relative_segment(start, end);
                let closest_sq = closest.dot(closest);
                let begins_touching_or_outside = start_sq >= radius * radius - 1.0e-10;
                let crosses_centers_from_overlap =
                    start_sq < radius * radius && start.dot(end) < 0.0;
                let swept_hit = !swept_resolved[left][right]
                    && has_motion
                    && closest_sq < radius * radius
                    && (begins_touching_or_outside || crosses_centers_from_overlap);

                if end_sq >= radius * radius && !swept_hit {
                    continue;
                }
                if normal.is_none() {
                    normal = relative_motion
                        .normalized()
                        .map(|direction| direction * -1.0)
                        .or_else(|| {
                            Some(Vec2::new(
                                bodies[left].vehicle.heading.cos(),
                                bodies[left].vehicle.heading.sin(),
                            ))
                        });
                }
                let mut normal = normal.unwrap_or(Vec2::new(1.0, 0.0));
                if swept_hit {
                    let starts_overlapping = start_sq < radius * radius;
                    let t = if starts_overlapping {
                        0.0
                    } else {
                        first_circle_contact(start, relative_motion, radius).unwrap_or(0.0)
                    };
                    let contact = start + relative_motion * t;
                    let contact_normal = contact
                        .normalized()
                        .or_else(|| {
                            relative_motion
                                .normalized()
                                .map(|direction| direction * -1.0)
                        })
                        .unwrap_or(normal);
                    normal = contact_normal;
                    let left_delta = bodies[left].vehicle.position - bodies[left].previous_position;
                    let right_delta =
                        bodies[right].vehicle.position - bodies[right].previous_position;
                    let shared = (left_delta + right_delta) * 0.5;
                    let relative_remainder = (left_delta - right_delta) * (1.0 - t);
                    let inward = relative_remainder.dot(contact_normal);
                    let tangent_remainder = relative_remainder - contact_normal * inward.min(0.0);
                    let contact_midpoint = (bodies[left].previous_position
                        + bodies[right].previous_position
                        + (left_delta + right_delta) * t)
                        * 0.5;
                    let target_relative = contact_normal * (radius + VEHICLE_CONTACT_EPSILON);
                    bodies[left].vehicle.position = contact_midpoint
                        + shared * (1.0 - t)
                        + tangent_remainder * 0.5
                        + target_relative * 0.5;
                    bodies[right].vehicle.position = contact_midpoint + shared * (1.0 - t)
                        - tangent_remainder * 0.5
                        - target_relative * 0.5;
                    swept_resolved[left][right] = true;
                } else if penetration > 0.0 {
                    let correction = normal * (penetration * 0.5 + VEHICLE_CONTACT_EPSILON);
                    bodies[left].vehicle.position = bodies[left].vehicle.position + correction;
                    bodies[right].vehicle.position = bodies[right].vehicle.position - correction;
                }

                let relative_velocity =
                    (bodies[left].vehicle.velocity - bodies[right].vehicle.velocity).dot(normal);
                if relative_velocity < 0.0 {
                    let impulse = normal * (relative_velocity * 0.5);
                    bodies[left].vehicle.velocity = bodies[left].vehicle.velocity - impulse;
                    bodies[right].vehicle.velocity = bodies[right].vehicle.velocity + impulse;
                }
                contacted[left] = true;
                contacted[right] = true;
            }
        }
    }
    Ok(contacted.into_iter().filter(|contacted| *contacted).count())
}

fn vertical_intervals_overlap(left: &VehicleContactBody, right: &VehicleContactBody) -> bool {
    let left_min = left.vehicle.position_z + left.min_z;
    let left_max = left.vehicle.position_z + left.max_z;
    let right_min = right.vehicle.position_z + right.min_z;
    let right_max = right.vehicle.position_z + right.max_z;
    left_min <= right_max && right_min <= left_max
}

fn closest_point_on_relative_segment(start: Vec2, end: Vec2) -> Vec2 {
    let movement = end - start;
    let length_sq = movement.dot(movement);
    if length_sq <= 1.0e-12 {
        return start;
    }
    start + movement * (-start.dot(movement) / length_sq).clamp(0.0, 1.0)
}

fn first_circle_contact(start: Vec2, movement: Vec2, radius: f64) -> Option<f64> {
    let a = movement.dot(movement);
    let b = 2.0 * start.dot(movement);
    let c = start.dot(start) - radius * radius;
    let discriminant = b * b - 4.0 * a * c;
    if a <= 1.0e-12 || discriminant < 0.0 {
        return None;
    }
    let time = (-b - discriminant.sqrt()) / (2.0 * a);
    (0.0..=1.0).contains(&time).then_some(time)
}

impl Vehicle {
    pub fn speed(&self) -> f64 {
        self.velocity.length()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Checkpoint {
    pub center: Vec2,
    pub radius: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackRuntime {
    /// Inclusive axis-aligned driving bounds.
    pub min: Vec2,
    pub max: Vec2,
    /// Ordered gates. A lap is credited after the final gate and crossing the
    /// first gate again; start position is the first gate's center.
    pub checkpoints: Vec<Checkpoint>,
    pub laps_to_finish: u32,
}

/// Triangle on the selected local track's walkable surface, in simulation XY
/// and scene-space Z. This is a bounded preview collision input, not a claim
/// that every source triangle is a game collision surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewGroundTriangle {
    pub vertices: [[f64; 3]; 3],
}

/// A near-vertical face reduced to its lowest XY edge for conservative local
/// preview blocking. Props and non-track meshes are excluded by the importer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewWallSegment {
    pub start: Vec2,
    pub end: Vec2,
    pub min_z: f64,
    pub max_z: f64,
}

/// Project-owned provisional contact policy for local imported-scene preview.
/// All geometry is supplied by the importer and is never sent to a server.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewCollisionWorld {
    pub ground: Vec<PreviewGroundTriangle>,
    pub walls: Vec<PreviewWallSegment>,
    pub vehicle_radius: f64,
    pub vehicle_min_z: f64,
    pub vehicle_max_z: f64,
    pub initial_ground_z: f64,
    /// Scene units per simulation XY unit, used only to recover scene-metric normals.
    pub units_per_sim_unit: f64,
    /// Road-edge rows in simulation coordinates.
    /// Empty = no road-edge wall (legacy scenes, synthetic worlds). When
    /// present the edge wall is authoritative.
    pub edge_rows: Vec<TrackEdgeRow>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewSupport {
    pub height: f64,
    pub normal: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewGripPolicy {
    pub lateral_recovery_per_second: f64,
    pub handbrake_recovery_per_second: f64,
}

impl Default for PreviewGripPolicy {
    fn default() -> Self {
        Self {
            lateral_recovery_per_second: 5.0,
            handbrake_recovery_per_second: 1.0,
        }
    }
}

impl PreviewGripPolicy {
    fn is_valid(self) -> bool {
        self.lateral_recovery_per_second.is_finite()
            && (0.0..=50.0).contains(&self.lateral_recovery_per_second)
            && self.handbrake_recovery_per_second.is_finite()
            && (0.0..=50.0).contains(&self.handbrake_recovery_per_second)
    }
}

/// Clearly provisional drive tuning for an imported, collision-backed local
/// preview. Values are simulation units per second (or per second squared),
/// not physical speed units or measured vehicle behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewDrivePolicy {
    pub acceleration_per_second: f64,
    pub braking_per_second: f64,
    pub max_speed: f64,
}

impl Default for PreviewDrivePolicy {
    fn default() -> Self {
        Self {
            acceleration_per_second: 18.0,
            braking_per_second: 30.0,
            max_speed: 67.5,
        }
    }
}

impl PreviewDrivePolicy {
    fn is_valid(self) -> bool {
        self.acceleration_per_second.is_finite()
            && (0.0..=60.0).contains(&self.acceleration_per_second)
            && self.braking_per_second.is_finite()
            && (0.0..=100.0).contains(&self.braking_per_second)
            && self.max_speed.is_finite()
            && (0.0..=200.0).contains(&self.max_speed)
            && self.max_speed > 0.0
    }
}

impl PreviewCollisionWorld {
    pub const MAX_GROUND_TRIANGLES: usize = 100_000;
    pub const MAX_WALL_SEGMENTS: usize = 50_000;
    const MIN_GROUND_NORMAL_Z: f64 = 0.65;
    const CONTACT_EPSILON: f64 = 1.0e-4;

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.ground.is_empty()
            || self.ground.len() > Self::MAX_GROUND_TRIANGLES
            || self.walls.len() > Self::MAX_WALL_SEGMENTS
        {
            return Err("preview collision geometry exceeds its count limits");
        }
        if !self.vehicle_radius.is_finite()
            || self.vehicle_radius <= 0.0
            || !self.vehicle_min_z.is_finite()
            || !self.vehicle_max_z.is_finite()
            || self.vehicle_min_z >= self.vehicle_max_z
            || !self.initial_ground_z.is_finite()
            || !self.units_per_sim_unit.is_finite()
            || !(0.01..=100.0).contains(&self.units_per_sim_unit)
        {
            return Err("preview vehicle contact bounds are invalid");
        }
        TrackEdge::validate_rows(&self.edge_rows)?;
        if self.ground.iter().any(|triangle| {
            triangle
                .vertices
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || value.abs() > 1.0e6)
                || !triangle_normal_z(triangle.vertices).is_finite()
                || triangle_normal_z(triangle.vertices).abs() < Self::MIN_GROUND_NORMAL_Z
        }) {
            return Err("preview ground contains invalid or non-walkable triangles");
        }
        if self.walls.iter().any(|wall| {
            !wall.start.x.is_finite()
                || !wall.start.y.is_finite()
                || !wall.end.x.is_finite()
                || !wall.end.y.is_finite()
                || !wall.min_z.is_finite()
                || !wall.max_z.is_finite()
                || [
                    wall.start.x,
                    wall.start.y,
                    wall.end.x,
                    wall.end.y,
                    wall.min_z,
                    wall.max_z,
                ]
                .iter()
                .any(|value| value.abs() > 1.0e6)
                || wall.min_z >= wall.max_z
                || wall.start.distance(wall.end) <= 1.0e-6
        }) {
            return Err("preview wall geometry is invalid");
        }
        Ok(())
    }

    /// Ensure the declared preview anchor is supported by an unambiguous
    /// surface before changing either simulation or renderer state.
    pub fn validate_initial_support(&self) -> Result<PreviewSupport, &'static str> {
        self.validate()?;
        self.support_surface(Vec2::default(), self.initial_ground_z, 0.25, None)
            .ok_or("preview spawn has no unambiguous supported ground surface")
    }

    fn support_surface(
        &self,
        point: Vec2,
        previous_z: f64,
        max_delta: f64,
        previous_normal: Option<[f64; 3]>,
    ) -> Option<PreviewSupport> {
        let mut nearest: Option<(PreviewSupport, f64, f64)> = None;
        let mut ambiguous = false;
        for triangle in &self.ground {
            let Some(height) = triangle_height_at(triangle.vertices, point) else {
                continue;
            };
            let delta = (height - previous_z).abs();
            if delta > max_delta {
                continue;
            }
            let Some(normal) = triangle_normal(triangle.vertices) else {
                continue;
            };
            let mut scene_metric_normal = [
                normal[0] / self.units_per_sim_unit,
                normal[1] / self.units_per_sim_unit,
                normal[2],
            ];
            let metric_length = dot3(scene_metric_normal, scene_metric_normal).sqrt();
            if !metric_length.is_finite() || metric_length <= 1.0e-9 {
                continue;
            }
            for component in &mut scene_metric_normal {
                *component /= metric_length;
            }
            let candidate = PreviewSupport {
                height,
                normal: scene_metric_normal,
            };
            let alignment =
                previous_normal.map_or(0.0, |previous| dot3(previous, candidate.normal));
            match nearest {
                None => nearest = Some((candidate, delta, alignment)),
                Some((_, best, _)) if delta + 1.0e-5 < best => {
                    nearest = Some((candidate, delta, alignment));
                    ambiguous = false;
                }
                Some((existing, best, best_alignment)) if (delta - best).abs() <= 1.0e-5 => {
                    if (existing.height - height).abs() > 1.0e-5 {
                        ambiguous = true;
                    } else if alignment > best_alignment + 1.0e-5 {
                        nearest = Some((candidate, delta, alignment));
                    }
                }
                _ => {}
            }
        }
        if ambiguous {
            None
        } else {
            nearest.map(|(surface, _, _)| surface)
        }
    }

    fn move_vehicle(
        &self,
        start: Vec2,
        target: Vec2,
        velocity: Vec2,
        vehicle_origin_z: f64,
        support: PreviewSupport,
        max_grade_step: f64,
    ) -> (Vec2, Vec2, f64, PreviewSupport, bool) {
        let mut position = start;
        let mut remaining = target - start;
        let mut resolved_velocity = velocity;
        let mut current_support = support.height;
        let mut current_origin_z = vehicle_origin_z;
        let candidate_support = self
            .support_surface(
                target,
                support.height,
                max_grade_step * start.distance(target) + 1.0e-3,
                Some(support.normal),
            )
            .unwrap_or(support);
        let candidate_origin_z =
            candidate_support.height - self.vehicle_min_z / candidate_support.normal[2];
        let current_bounds = self.vertical_bounds(current_origin_z, support.normal);
        let candidate_bounds = self.vertical_bounds(candidate_origin_z, candidate_support.normal);
        let swept_vehicle_min_z = current_bounds.0.min(candidate_bounds.0);
        let swept_vehicle_max_z = current_bounds.1.max(candidate_bounds.1);
        for _ in 0..3 {
            if remaining.length() <= 1.0e-8 {
                break;
            }
            let mut earliest: Option<(f64, Vec2, f64)> = None;
            for wall in &self.walls {
                if swept_vehicle_max_z < wall.min_z || swept_vehicle_min_z > wall.max_z {
                    continue;
                }
                if let Some((time, normal, penetration)) = sweep_circle_segment(
                    position,
                    position + remaining,
                    wall.start,
                    wall.end,
                    self.vehicle_radius,
                ) {
                    if earliest.is_none_or(|(best, _, _)| time < best) {
                        earliest = Some((time, normal, penetration));
                    }
                }
            }
            let Some((time, normal, penetration)) = earliest else {
                position = position + remaining;
                break;
            };
            let contact = position + remaining * time;
            position = contact + normal * (Self::CONTACT_EPSILON + penetration);
            let mut leftover = remaining * (1.0 - time);
            let into_wall = leftover.dot(normal);
            if into_wall < 0.0 {
                leftover = leftover - normal * into_wall;
            }
            let velocity_into_wall = resolved_velocity.dot(normal);
            if velocity_into_wall < 0.0 {
                resolved_velocity = resolved_velocity - normal * velocity_into_wall;
            }
            remaining = leftover;
        }
        let horizontal_distance = start.distance(position);
        let max_height_delta = max_grade_step * horizontal_distance + 1.0e-3;
        let found = self.support_surface(
            position,
            current_support,
            max_height_delta,
            Some(support.normal),
        );
        let supported = found.is_some();
        let support = found.unwrap_or(PreviewSupport {
            height: current_support,
            normal: support.normal,
        });
        current_support = support.height;
        current_origin_z = current_support - self.vehicle_min_z / support.normal[2];
        (
            position,
            resolved_velocity,
            current_origin_z,
            support,
            supported,
        )
    }

    fn vertical_bounds(&self, origin_z: f64, normal: [f64; 3]) -> (f64, f64) {
        let lateral_extent =
            self.vehicle_radius * self.units_per_sim_unit * normal[0].hypot(normal[1]);
        (
            origin_z + self.vehicle_min_z * normal[2] - lateral_extent,
            origin_z + self.vehicle_max_z * normal[2] + lateral_extent,
        )
    }
}

fn triangle_normal(vertices: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let a = [
        vertices[1][0] - vertices[0][0],
        vertices[1][1] - vertices[0][1],
        vertices[1][2] - vertices[0][2],
    ];
    let b = [
        vertices[2][0] - vertices[0][0],
        vertices[2][1] - vertices[0][1],
        vertices[2][2] - vertices[0][2],
    ];
    let mut normal = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if !length.is_finite() || length <= 1.0e-9 {
        return None;
    }
    for value in &mut normal {
        *value /= length;
    }
    if normal[2] < 0.0 {
        for value in &mut normal {
            *value = -*value;
        }
    }
    Some(normal)
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn triangle_normal_z(vertices: [[f64; 3]; 3]) -> f64 {
    let a = [
        vertices[1][0] - vertices[0][0],
        vertices[1][1] - vertices[0][1],
        vertices[1][2] - vertices[0][2],
    ];
    let b = [
        vertices[2][0] - vertices[0][0],
        vertices[2][1] - vertices[0][1],
        vertices[2][2] - vertices[0][2],
    ];
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if length <= 1.0e-12 {
        f64::NAN
    } else {
        cross[2] / length
    }
}

fn triangle_height_at(vertices: [[f64; 3]; 3], point: Vec2) -> Option<f64> {
    let a = Vec2::new(vertices[0][0], vertices[0][1]);
    let b = Vec2::new(vertices[1][0], vertices[1][1]);
    let c = Vec2::new(vertices[2][0], vertices[2][1]);
    let v0 = b - a;
    let v1 = c - a;
    let v2 = point - a;
    let denom = v0.x * v1.y - v1.x * v0.y;
    if denom.abs() <= 1.0e-10 {
        return None;
    }
    let u = (v2.x * v1.y - v1.x * v2.y) / denom;
    let v = (v0.x * v2.y - v2.x * v0.y) / denom;
    if u < -1.0e-6 || v < -1.0e-6 || u + v > 1.0 + 1.0e-6 {
        return None;
    }
    Some(
        vertices[0][2]
            + u * (vertices[1][2] - vertices[0][2])
            + v * (vertices[2][2] - vertices[0][2]),
    )
}

fn sweep_circle_segment(
    start: Vec2,
    end: Vec2,
    a: Vec2,
    b: Vec2,
    radius: f64,
) -> Option<(f64, Vec2, f64)> {
    let movement = end - start;
    let wall = b - a;
    let wall_length = wall.length();
    if wall_length <= 1.0e-9 {
        return None;
    }
    let tangent = wall * (1.0 / wall_length);
    let normal = Vec2::new(-tangent.y, tangent.x);
    let start_distance = (start - a).dot(normal);
    let end_distance = (end - a).dot(normal);
    let mut best: Option<(f64, Vec2, f64)> = None;
    for side in [-1.0, 1.0] {
        let d0 = start_distance * side;
        let d1 = end_distance * side;
        let signed_start = start_distance;
        let outward = if signed_start.abs() > 1.0e-9 {
            normal * signed_start.signum()
        } else {
            normal * side
        };
        if signed_start.abs() <= radius && movement.dot(outward) < 0.0 {
            let projection = (start - a).dot(tangent);
            if (0.0..=wall_length).contains(&projection) {
                best = Some((0.0, outward, (radius - signed_start.abs()).max(0.0)));
            }
        } else if d0 > radius && d1 <= radius {
            let time = (d0 - radius) / (d0 - d1);
            let at = start + movement * time;
            let projection = (at - a).dot(tangent);
            if (0.0..=wall_length).contains(&projection) && best.is_none_or(|v| time < v.0) {
                best = Some((time, normal * side, 0.0));
            }
        }
    }
    let movement_sq = movement.dot(movement);
    if movement_sq > 1.0e-12 {
        for endpoint in [a, b] {
            let offset = start - endpoint;
            let start_distance = offset.length();
            if start_distance <= radius {
                let contact_normal = offset
                    .normalized()
                    .or_else(|| (movement * -1.0).normalized());
                if let Some(contact_normal) = contact_normal {
                    if movement.dot(contact_normal) < 0.0 && best.is_none_or(|v| v.0 > 0.0) {
                        best = Some((0.0, contact_normal, (radius - start_distance).max(0.0)));
                    }
                }
                continue;
            }
            let bterm = 2.0 * offset.dot(movement);
            let cterm = offset.dot(offset) - radius * radius;
            let discriminant = bterm * bterm - 4.0 * movement_sq * cterm;
            if discriminant < 0.0 {
                continue;
            }
            let time = (-bterm - discriminant.sqrt()) / (2.0 * movement_sq);
            if !(0.0..=1.0).contains(&time) || best.is_some_and(|v| time >= v.0) {
                continue;
            }
            let at = start + movement * time;
            if let Some(contact_normal) = (at - endpoint).normalized() {
                if movement.dot(contact_normal) < 0.0 {
                    best = Some((time, contact_normal, 0.0));
                }
            }
        }
    }
    best
}

impl TrackRuntime {
    /// Non-racing free-drive mode used to inspect a locally imported render
    /// scene. It has no gates, finish state, or artificial circuit boundary.
    /// This is project preview movement, not imported-game physics.
    pub fn open_preview() -> Self {
        Self {
            min: Vec2::new(-f64::MAX, -f64::MAX),
            max: Vec2::new(f64::MAX, f64::MAX),
            checkpoints: Vec::new(),
            laps_to_finish: 0,
        }
    }

    pub fn synthetic_circuit() -> Self {
        Self::parse(include_str!(
            "../../../content/demo/tracks/test-circuit.prtrack"
        ))
        .expect("bundled synthetic circuit must remain valid")
    }

    /// Parse the intentionally small, text-based project-owned circuit format.
    /// It is versioned and contains no external asset references.
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut header = false;
        let mut bounds = None;
        let mut laps = None;
        let mut checkpoints = Vec::new();
        for (line_index, raw) in source.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut fields = line.split_whitespace();
            let key = fields.next().expect("non-empty line has first field");
            let values: Vec<&str> = fields.collect();
            let err = |message: &str| format!("circuit line {}: {message}", line_index + 1);
            if !header {
                if key != "RACER_CIRCUIT_V1" || !values.is_empty() {
                    return Err(err("expected RACER_CIRCUIT_V1 header"));
                }
                header = true;
                continue;
            }
            match key {
                "bounds" if values.len() == 4 && bounds.is_none() => {
                    let v = parse_numbers::<4>(&values)
                        .map_err(|_| err("bounds require four finite numbers"))?;
                    let min = Vec2::new(v[0], v[1]);
                    let max = Vec2::new(v[2], v[3]);
                    if min.x >= max.x || min.y >= max.y {
                        return Err(err("bounds must have positive width and height"));
                    }
                    bounds = Some((min, max));
                }
                "laps" if values.len() == 1 && laps.is_none() => {
                    let count = values[0]
                        .parse::<u32>()
                        .map_err(|_| err("lap count must be a positive integer"))?;
                    if count == 0 {
                        return Err(err("lap count must be at least one"));
                    }
                    laps = Some(count);
                }
                "checkpoint" if values.len() == 3 => {
                    let v = parse_numbers::<3>(&values)
                        .map_err(|_| err("checkpoint requires finite x, y, and radius"))?;
                    if v[2] <= 0.0 {
                        return Err(err("checkpoint radius must be positive"));
                    }
                    checkpoints.push(Checkpoint {
                        center: Vec2::new(v[0], v[1]),
                        radius: v[2],
                    });
                }
                _ => return Err(err("unknown, duplicated, or malformed field")),
            }
        }
        if !header {
            return Err("missing RACER_CIRCUIT_V1 header".to_string());
        }
        let (min, max) = bounds.ok_or_else(|| "missing bounds field".to_string())?;
        let laps_to_finish = laps.ok_or_else(|| "missing laps field".to_string())?;
        if checkpoints.len() < 2 {
            return Err("circuit needs at least two checkpoints".to_string());
        }
        if checkpoints.iter().any(|gate| {
            gate.center.x < min.x
                || gate.center.x > max.x
                || gate.center.y < min.y
                || gate.center.y > max.y
        }) {
            return Err("all checkpoint centers must lie within bounds".to_string());
        }
        Ok(Self {
            min,
            max,
            checkpoints,
            laps_to_finish,
        })
    }

    fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    fn spawn(&self) -> Vec2 {
        self.checkpoints
            .first()
            .map(|c| c.center)
            .unwrap_or_default()
    }
}

fn parse_numbers<const N: usize>(values: &[&str]) -> Result<[f64; N], ()> {
    if values.len() != N {
        return Err(());
    }
    let mut result = [0.0; N];
    for (slot, value) in result.iter_mut().zip(values) {
        *slot = value.parse::<f64>().map_err(|_| ())?;
        if !slot.is_finite() {
            return Err(());
        }
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq)]
pub struct RaceWorld {
    pub timing: SimTiming,
    pub track: TrackRuntime,
    pub vehicle: Vehicle,
    pub tick: u64,
    pub replay_trace: Vec<Vehicle>,
    spawn: Vec2,
    spawn_heading: f64,
    record_trace: bool,
    preview_collision: Option<PreviewCollisionWorld>,
    shared_preview_handling: bool,
    preview_spawn_origin_z: f64,
    preview_ground_z: Option<f64>,
    preview_ground_normal: Option<[f64; 3]>,
    preview_grip_policy: PreviewGripPolicy,
    preview_drive_policy: PreviewDrivePolicy,
    vehicle_model: VehicleModel,
    /// Drivetrain state of the built-in gearbox (Arcade) model.
    drive: DriveState,
    /// Gearbox mode chosen for this vehicle's driver; `None` until set (the
    /// provider model keeps its own default then).
    transmission: Option<Transmission>,
    /// Parameters of the Arcade model (None until installed).
    arcade: Option<ArcadeConfig>,
    /// Parameters of the Classic model (None until installed).
    classic: Option<ClassicConfig>,
    /// Provider-supplied model (None until installed).
    custom: Option<model_ext::CustomModel>,
    lap_times: LapTimes,
    /// Road-edge wall built from `PreviewCollisionWorld::edge_rows` (None when
    /// the installed collision has no rows).
    track_edge: Option<TrackEdge>,
    /// Consecutive steps without ground support (project reset rule). Not part
    /// of the rollback frame; cleared on restore.
    edge_unsupported_steps: u32,
    /// Command for the next step of a `VehicleModel::Scripted` world. Driver
    /// state, not part of the rollback frame.
    scripted: ScriptedControl,
}

/// Per-vehicle lap timing in race ticks (the 60 Hz physics step counter in
/// `Vehicle::race_ticks`). Project semantics only
/// timer. This is HUD/progression data only: it never feeds back into motion
/// and is not part of the rollback state frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LapTimes {
    lap_start_ticks: u64,
    pub last_lap_ticks: Option<u64>,
    pub best_lap_ticks: Option<u64>,
    /// `race_ticks` at the moment the finishing lap was credited.
    pub finish_ticks: Option<u64>,
}

/// One competitor presented to [`rank_race`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RankEntry {
    /// Stable tie-break identity (lower wins a complete tie).
    pub id: u32,
    pub checkpoint: usize,
    pub completed_laps: u32,
    pub finished: bool,
    pub finish_ticks: Option<u64>,
    /// Distance from the vehicle to its next gate center; smaller is ahead.
    pub distance_to_next: f64,
}

/// Number of gates passed in the current lap. `checkpoint` is the index of
/// the next gate; a fresh vehicle targets gate 1 (gate 0 is the start line).
fn gates_passed(checkpoint: usize, gate_count: usize) -> usize {
    if gate_count == 0 {
        0
    } else {
        (checkpoint % gate_count + gate_count - 1) % gate_count
    }
}

/// Rank competitors, returning 1-based positions parallel to `entries`.
/// Finished vehicles rank first, ordered by finish tick then id. Racing
/// vehicles rank by completed laps, then gates passed this lap, then smaller
/// distance to the next gate, then id. Non-finite distances sort last.
pub fn rank_race(gate_count: usize, entries: &[RankEntry]) -> Vec<u16> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        let distance = |e: &RankEntry| {
            if e.distance_to_next.is_finite() {
                e.distance_to_next
            } else {
                f64::INFINITY
            }
        };
        y.finished
            .cmp(&x.finished)
            .then_with(|| {
                if x.finished && y.finished {
                    x.finish_ticks
                        .unwrap_or(u64::MAX)
                        .cmp(&y.finish_ticks.unwrap_or(u64::MAX))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| {
                if x.finished {
                    return std::cmp::Ordering::Equal;
                }
                y.completed_laps
                    .cmp(&x.completed_laps)
                    .then_with(|| {
                        gates_passed(y.checkpoint, gate_count)
                            .cmp(&gates_passed(x.checkpoint, gate_count))
                    })
                    .then_with(|| distance(x).total_cmp(&distance(y)))
            })
            .then_with(|| x.id.cmp(&y.id))
    });
    let mut positions = vec![0u16; entries.len()];
    for (rank, index) in order.into_iter().enumerate() {
        positions[index] = (rank + 1).min(usize::from(u16::MAX)) as u16;
    }
    positions
}

impl Default for RaceWorld {
    fn default() -> Self {
        Self::new(SimTiming::default(), TrackRuntime::synthetic_circuit())
    }
}

impl RaceWorld {
    pub fn new(timing: SimTiming, track: TrackRuntime) -> Self {
        let spawn = track.spawn();
        let vehicle = Vehicle {
            position: spawn,
            position_z: 0.0,
            heading: 0.0,
            velocity: Vec2::default(),
            angular_velocity: 0.0,
            checkpoint: 1.min(track.checkpoints.len()),
            completed_laps: 0,
            race_ticks: 0,
            finished: false,
        };
        Self {
            timing,
            track,
            vehicle,
            tick: 0,
            replay_trace: Vec::new(),
            spawn,
            spawn_heading: 0.0,
            record_trace: false,
            preview_collision: None,
            shared_preview_handling: false,
            preview_spawn_origin_z: 0.0,
            preview_ground_z: None,
            preview_ground_normal: None,
            preview_grip_policy: PreviewGripPolicy::default(),
            preview_drive_policy: PreviewDrivePolicy::default(),
            vehicle_model: VehicleModel::default(),
            drive: DriveState::default(),
            transmission: None,
            arcade: None,
            classic: None,
            custom: None,
            lap_times: LapTimes::default(),
            track_edge: None,
            edge_unsupported_steps: 0,
            scripted: ScriptedControl::default(),
        }
    }

    pub fn lap_times(&self) -> LapTimes {
        self.lap_times
    }

    /// Ticks elapsed in the lap in progress (zero once finished).
    pub fn current_lap_ticks(&self) -> u64 {
        if self.vehicle.finished {
            return 0;
        }
        self.vehicle
            .race_ticks
            .saturating_sub(self.lap_times.lap_start_ticks)
    }

    /// Straight-line distance to the next gate center, or 0 with no gates.
    pub fn distance_to_next_checkpoint(&self) -> f64 {
        let count = self.track.checkpoints.len();
        if count == 0 {
            return 0.0;
        }
        self.track.checkpoints[self.vehicle.checkpoint % count]
            .center
            .distance(self.vehicle.position)
    }

    /// Progression snapshot for [`rank_race`].
    pub fn rank_entry(&self, id: u32) -> RankEntry {
        RankEntry {
            id,
            checkpoint: self.vehicle.checkpoint,
            completed_laps: self.vehicle.completed_laps,
            finished: self.vehicle.finished,
            finish_ticks: self.lap_times.finish_ticks,
            distance_to_next: self.distance_to_next_checkpoint(),
        }
    }

    pub fn vehicle_model(&self) -> VehicleModel {
        self.vehicle_model
    }

    /// Select the vehicle model. `Arcade` and `Custom` need their parameters or
    /// provider model installed first; otherwise the selection is rejected.
    pub fn set_vehicle_model(&mut self, model: VehicleModel) -> Result<(), &'static str> {
        if !model.is_selectable() {
            return Err("vehicle model is not selectable in this build");
        }
        if model == VehicleModel::Arcade && self.arcade.is_none() {
            return Err("arcade vehicle model parameters are not installed");
        }
        if model == VehicleModel::Classic && self.classic.is_none() {
            return Err("classic vehicle model parameters are not installed");
        }
        if model == VehicleModel::Custom && self.custom.is_none() {
            return Err("custom vehicle model is not installed");
        }
        if self.vehicle_model != model {
            self.drive = self.fresh_drive();
            if let Some(custom) = &mut self.custom {
                custom.dynamics.reset();
            }
        }
        self.vehicle_model = model;
        Ok(())
    }

    /// Install (or replace) the Arcade model's parameters. The model is not
    /// activated; call `set_vehicle_model`.
    pub fn install_arcade_parameters(&mut self, config: ArcadeConfig) -> Result<(), &'static str> {
        config.validate()?;
        self.arcade = Some(config);
        Ok(())
    }

    /// Install `config` and make Arcade the active model. Resets the drivetrain
    /// state when the parameters change.
    pub fn use_arcade_model(&mut self, config: ArcadeConfig) -> Result<(), &'static str> {
        if self.arcade.as_ref() != Some(&config) {
            self.install_arcade_parameters(config)?;
            self.drive = self.fresh_drive();
        }
        self.set_vehicle_model(VehicleModel::Arcade)
    }

    /// Install (or replace) the Classic model's parameters. The model is not
    /// activated; call `set_vehicle_model`.
    pub fn install_classic_parameters(
        &mut self,
        config: ClassicConfig,
    ) -> Result<(), &'static str> {
        config.validate()?;
        self.classic = Some(config);
        Ok(())
    }

    /// Install `config` and make Classic the active model. Resets the
    /// drivetrain state when the parameters change.
    pub fn use_classic_model(&mut self, config: ClassicConfig) -> Result<(), &'static str> {
        if self.classic.as_ref() != Some(&config) {
            self.install_classic_parameters(config)?;
            self.drive = self.fresh_drive();
        }
        self.set_vehicle_model(VehicleModel::Classic)
    }

    /// Build a provider model from `registry` by id and opaque parameter bytes,
    /// install it and make `VehicleModel::Custom` the active model. An unknown
    /// id or rejected parameters leave the world untouched.
    pub fn install_custom_model(
        &mut self,
        registry: &model_ext::VehicleModelRegistry,
        id: &str,
        params: &[u8],
    ) -> Result<(), String> {
        let mut model = registry.instantiate(id, params)?;
        model.dynamics.set_step_seconds(self.timing.step_seconds());
        if let Some(transmission) = self.transmission {
            model.dynamics.set_transmission(transmission.is_manual());
        }
        model.dynamics.set_auto_reverse(self.drive.auto_reverse);
        self.custom = Some(model);
        self.drive = self.fresh_drive();
        self.vehicle_model = VehicleModel::Custom;
        Ok(())
    }

    /// Gearbox mode of this vehicle's driver (automatic until set).
    pub fn transmission(&self) -> Transmission {
        self.transmission.unwrap_or_default()
    }

    /// Choose the gearbox mode. A change restarts the drivetrain in its start
    /// state (neutral or first gear, depending on the model and mode); it is
    /// meant for the set-up and lobby, not for a race in progress. The mode
    /// is bound into the world identity when manual.
    pub fn set_transmission(&mut self, transmission: Transmission) {
        if self.transmission == Some(transmission) {
            return;
        }
        let changed = self.transmission() != transmission;
        self.transmission = Some(transmission);
        self.drive = self.fresh_drive();
        if let Some(custom) = &mut self.custom {
            custom.dynamics.set_transmission(transmission.is_manual());
            if changed {
                custom.dynamics.reset();
            }
        }
    }

    /// Start-of-race drivetrain state for the current gearbox mode.
    fn fresh_drive(&self) -> DriveState {
        let mut drive = DriveState::fresh(self.transmission());
        drive.auto_reverse = self.drive.auto_reverse;
        drive
    }

    /// Whether auto reverse is on (see `drivetrain::apply_auto_reverse`).
    pub fn auto_reverse(&self) -> bool {
        self.drive.auto_reverse
    }

    /// Switch auto reverse on or off. Takes effect at once and keeps the
    /// current gear selection; it survives resets, and is bound into the
    /// world identity when on.
    pub fn set_auto_reverse(&mut self, enabled: bool) {
        self.drive.auto_reverse = enabled;
        if let Some(custom) = &mut self.custom {
            custom.dynamics.set_auto_reverse(enabled);
        }
    }

    /// Id of the installed provider model, if any.
    pub fn custom_model_id(&self) -> Option<&str> {
        self.custom.as_ref().map(|c| c.id())
    }

    /// The installed Arcade parameters, if any.
    pub fn arcade_parameters(&self) -> Option<&ArcadeConfig> {
        self.arcade.as_ref()
    }

    /// The installed Classic parameters, if any.
    pub fn classic_parameters(&self) -> Option<&ClassicConfig> {
        self.classic.as_ref()
    }

    /// Drivetrain state: the active provider's when it keeps one, otherwise
    /// the built-in gearbox state (default/idle for other models).
    pub fn drive_state(&self) -> DriveState {
        if self.vehicle_model == VehicleModel::Custom {
            if let Some(state) = self.custom.as_ref().and_then(|c| c.dynamics.drive_state()) {
                return state;
            }
        }
        self.drive
    }

    /// The active provider's serialised dynamic state (what rollback saves),
    /// when a provider model is installed.
    pub fn custom_model_state(&self) -> Option<Vec<u8>> {
        self.custom.as_ref().map(|c| c.dynamics.save_state())
    }

    /// Gear and engine-speed estimate for HUD/audio. `gear == 0` when the
    /// active model has no gearbox.
    pub fn drive_telemetry(&self) -> DriveTelemetry {
        let none = DriveTelemetry {
            gear: 0,
            rpm01: 0.0,
        };
        match self.vehicle_model {
            VehicleModel::Arcade => match &self.arcade {
                Some(cfg) => arcade_model::telemetry(cfg, &self.drive, self.vehicle.speed()),
                None => none,
            },
            VehicleModel::Classic => match &self.classic {
                Some(cfg) => classic_model::telemetry(cfg, &self.drive, self.vehicle.speed()),
                None => none,
            },
            VehicleModel::Custom => self
                .custom
                .as_ref()
                .and_then(|c| c.dynamics.telemetry(&self.vehicle))
                .unwrap_or(none),
            _ => none,
        }
    }

    /// Enable optional per-tick trace collection for tests and offline replay.
    /// Authoritative long-running worlds leave this off to keep memory bounded.
    pub fn set_trace_recording(&mut self, enabled: bool) {
        self.record_trace = enabled;
    }

    /// Attach bounded geometry to a local preview world. The world is replaced
    /// only after full validation succeeds; compatibility race worlds never
    /// receive this data through the network protocol.
    pub fn install_preview_collision(
        &mut self,
        mut collision: PreviewCollisionWorld,
    ) -> Result<(), &'static str> {
        collision.validate()?;
        let spawn_support = collision.validate_initial_support()?;
        self.track_edge = if collision.edge_rows.is_empty() {
            None
        } else {
            let edge = TrackEdge::new(collision.edge_rows.clone(), collision.units_per_sim_unit)?;
            // The edge wall is authoritative at the road edge: mesh walls that
            // sit on or beyond an edge are redundant (their circle would stop
            // the car short of the observed rest offset).
            collision.walls.retain(|wall| {
                let midpoint = (wall.start + wall.end) * 0.5;
                let z = (wall.min_z + wall.max_z) * 0.5;
                !edge.mesh_wall_is_redundant(midpoint, z)
            });
            Some(edge)
        };
        self.edge_unsupported_steps = 0;
        self.vehicle.position_z =
            spawn_support.height - collision.vehicle_min_z / spawn_support.normal[2];
        self.preview_spawn_origin_z = self.vehicle.position_z;
        self.preview_ground_z = Some(spawn_support.height);
        self.preview_ground_normal = Some(spawn_support.normal);
        self.preview_collision = Some(collision);
        Ok(())
    }

    /// Move the respawn point (and the vehicle, before the first step) to
    /// `position`, keeping heading 0. Used by tracks whose start grid lies
    /// behind the first checkpoint gate; the simulation frame is anchored at
    /// the spawn so that heading 0 follows the road.
    pub fn set_spawn(&mut self, position: Vec2) {
        self.spawn = position;
        if self.tick == 0 {
            self.vehicle.position = position;
        }
    }

    pub fn has_preview_collision(&self) -> bool {
        self.preview_collision.is_some()
    }

    /// Enable the approved project preview's fixed-step driving policy in a
    /// shared/predicted world. This changes vehicle handling only; it does not
    /// install collision geometry or make client geometry authoritative.
    pub fn enable_shared_preview_handling(&mut self) {
        self.shared_preview_handling = true;
    }

    fn uses_preview_handling(&self) -> bool {
        self.preview_collision.is_some() || self.shared_preview_handling
    }

    pub fn preview_ground_normal(&self) -> Option<[f64; 3]> {
        self.preview_ground_normal
    }

    /// Side slope of the road under the vehicle: the component of the unit
    /// ground normal along the vehicle's left axis (the sine of the roll angle).
    /// Positive when the downhill side is on the left; 0 without ground
    /// support data (flat roads, worlds without preview collision).
    pub fn road_side_slope(&self) -> f64 {
        let Some(normal) = self.preview_ground_normal else {
            return 0.0;
        };
        let (sin, cos) = self.vehicle.heading.sin_cos();
        let slope = -normal[0] * sin + normal[1] * cos;
        if slope.is_finite() {
            slope.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }

    /// Reapply this world's track constraint after pairwise car contact has
    /// displaced the vehicle. Ground support follows the validated local plane;
    /// this pass never teleports the vehicle's render origin vertically.
    pub fn constrain_after_vehicle_contact(&mut self, previous_position: Vec2) {
        let proposed = self.vehicle.position;
        if let (Some(collision), Some(ground_z)) = (&self.preview_collision, self.preview_ground_z)
        {
            let max_grade = ((1.0 - PreviewCollisionWorld::MIN_GROUND_NORMAL_Z.powi(2))
                .max(0.0)
                .sqrt()
                / PreviewCollisionWorld::MIN_GROUND_NORMAL_Z)
                .max(0.0);
            let (position, velocity, origin_z, support, _) = collision.move_vehicle(
                previous_position,
                proposed,
                self.vehicle.velocity,
                self.vehicle.position_z,
                PreviewSupport {
                    height: ground_z,
                    normal: self.preview_ground_normal.unwrap_or([0.0, 0.0, 1.0]),
                },
                max_grade,
            );
            self.vehicle.position = position;
            self.vehicle.velocity = velocity;
            self.vehicle.position_z = origin_z;
            self.preview_ground_z = Some(support.height);
            self.preview_ground_normal = Some(support.normal);
            self.apply_track_edge();
        } else if !self.track.contains(proposed) {
            self.vehicle.position = Vec2::new(
                proposed.x.clamp(self.track.min.x, self.track.max.x),
                proposed.y.clamp(self.track.min.y, self.track.max.y),
            );
            self.vehicle.velocity = Vec2::default();
            self.vehicle.angular_velocity = 0.0;
        }
    }

    pub fn set_preview_grip_policy(
        &mut self,
        policy: PreviewGripPolicy,
    ) -> Result<(), &'static str> {
        if !policy.is_valid() {
            return Err("preview grip rates must be finite and within 0..=50 per second");
        }
        self.preview_grip_policy = policy;
        Ok(())
    }

    /// Replace the provisional drive tuning for a collision-backed local
    /// preview only. Generic project races retain their existing parameters.
    pub fn set_preview_drive_policy(
        &mut self,
        policy: PreviewDrivePolicy,
    ) -> Result<(), &'static str> {
        if !policy.is_valid() {
            return Err("preview drive values must be finite and within their documented bounds");
        }
        if self.preview_collision.is_none() {
            return Err("preview drive policy requires installed preview collision geometry");
        }
        self.preview_drive_policy = policy;
        Ok(())
    }

    /// Advance exactly one fixed physics step.
    pub fn step(&mut self, input: DriverInput) -> &Vehicle {
        self.tick = self.tick.wrapping_add(1);
        if self.vehicle.finished {
            if self.record_trace {
                self.replay_trace.push(self.vehicle);
            }
            return &self.vehicle;
        }
        if input.reset_request {
            self.respawn();
        } else {
            let dt = self.timing.step_seconds();
            let previous = self.vehicle.position;
            let sanitized_input = DriverInput {
                throttle: finite_clamp(input.throttle, 0.0, 1.0),
                brake: finite_clamp(input.brake, 0.0, 1.0),
                steer: finite_clamp(input.steer, -1.0, 1.0),
                handbrake: input.handbrake,
                shift_up: input.shift_up,
                shift_down: input.shift_down,
                reset_request: false,
            };
            let road_slope = self.road_side_slope();
            let kinematics = self.vehicle_model.advance(
                self.vehicle,
                sanitized_input,
                dt,
                provisional_model::ProvisionalPolicy {
                    use_preview_handling: self.uses_preview_handling(),
                    drive: self.preview_drive_policy,
                    grip: self.preview_grip_policy,
                },
                &mut self.drive,
                self.scripted,
                self.arcade.as_ref(),
                self.classic.as_ref(),
                self.custom.as_mut(),
                road_slope,
            );
            self.vehicle.heading = kinematics.heading;
            self.vehicle.angular_velocity = kinematics.angular_velocity;
            self.vehicle.velocity = kinematics.velocity;
            let proposed = kinematics.proposed_position;
            if let (Some(collision), Some(ground_z)) =
                (&self.preview_collision, self.preview_ground_z)
            {
                let max_grade = ((1.0 - PreviewCollisionWorld::MIN_GROUND_NORMAL_Z.powi(2))
                    .max(0.0)
                    .sqrt()
                    / PreviewCollisionWorld::MIN_GROUND_NORMAL_Z)
                    .max(0.0);
                let (position, velocity, origin_z, new_support, supported) = collision
                    .move_vehicle(
                        previous,
                        proposed,
                        self.vehicle.velocity,
                        self.vehicle.position_z,
                        PreviewSupport {
                            height: ground_z,
                            normal: self.preview_ground_normal.unwrap_or([0.0, 0.0, 1.0]),
                        },
                        max_grade,
                    );
                self.vehicle.position = position;
                self.vehicle.velocity = velocity;
                self.vehicle.position_z = origin_z;
                self.preview_ground_z = Some(new_support.height);
                if new_support.normal[2] > 0.0 {
                    self.preview_ground_normal = Some(new_support.normal);
                }
                self.edge_unsupported_steps = if supported {
                    0
                } else {
                    self.edge_unsupported_steps.saturating_add(1)
                };
                self.apply_track_edge();
            } else {
                self.vehicle.position = proposed;
                self.vehicle.position_z = 0.0;
            }
            self.vehicle.race_ticks = self.vehicle.race_ticks.wrapping_add(1);

            if !self.track.contains(self.vehicle.position) {
                // Bounds are solid and remove outward velocity on the impacted axis.
                self.vehicle.position.x = self
                    .vehicle
                    .position
                    .x
                    .clamp(self.track.min.x, self.track.max.x);
                self.vehicle.position.y = self
                    .vehicle
                    .position
                    .y
                    .clamp(self.track.min.y, self.track.max.y);
                self.vehicle.velocity = Vec2::default();
                self.vehicle.angular_velocity = 0.0;
            } else {
                self.update_checkpoint(previous, self.vehicle.position);
            }
        }
        if self.record_trace {
            self.replay_trace.push(self.vehicle);
        }
        &self.vehicle
    }

    /// Road-edge wall pass and the project reset rules for
    /// unknown behavior: no ground support for `NO_GROUND_RESET_SECONDS`, or
    /// outside all rows / beyond the reset margin, resets the car to the road
    /// centre at the nearest row, facing the track direction.
    fn apply_track_edge(&mut self) {
        let (Some(edge), Some(collision)) = (&self.track_edge, &self.preview_collision) else {
            return;
        };
        let ground_z = self.preview_ground_z.unwrap_or(0.0);
        let dt = self.timing.step_seconds();
        let limit = (NO_GROUND_RESET_SECONDS / dt).ceil().max(1.0) as u32;
        let body = track_edge::EdgeBody::observed(collision.units_per_sim_unit, dt);
        let outcome = if self.edge_unsupported_steps >= limit {
            edge.frame_at(self.vehicle.position, ground_z)
                .map(track_edge::EdgeOutcome::Reset)
                .unwrap_or(track_edge::EdgeOutcome::Clear)
        } else {
            edge.resolve(
                self.vehicle.position,
                self.vehicle.heading,
                self.vehicle.velocity,
                ground_z,
                body,
            )
        };
        match outcome {
            track_edge::EdgeOutcome::Clear => {}
            track_edge::EdgeOutcome::Contact {
                position,
                velocity,
                heading,
                ..
            } => {
                self.vehicle.position = position;
                self.vehicle.velocity = velocity;
                self.vehicle.heading = heading;
            }
            track_edge::EdgeOutcome::Reset(frame) => {
                let min_z = collision.vehicle_min_z;
                self.vehicle.position = frame.road_centre();
                self.vehicle.heading = frame
                    .forward
                    .y
                    .atan2(frame.forward.x)
                    .rem_euclid(std::f64::consts::TAU);
                self.vehicle.velocity = Vec2::default();
                self.vehicle.angular_velocity = 0.0;
                self.vehicle.position_z = frame.z - min_z;
                self.preview_ground_z = Some(frame.z);
                self.preview_ground_normal = Some([0.0, 0.0, 1.0]);
                self.drive = self.fresh_drive();
                if let Some(custom) = &mut self.custom {
                    custom.dynamics.reset();
                }
                self.edge_unsupported_steps = 0;
            }
        }
    }

    /// Put the vehicle back on the grid. Completed laps and the race clock are
    /// kept, but the gate target returns to the first gate after the start so
    /// the car is ranked (and must drive) from where it now is, not from the
    /// gate it was at before the reset.
    pub fn respawn(&mut self) {
        self.edge_unsupported_steps = 0;
        self.vehicle.checkpoint = 1.min(self.track.checkpoints.len());
        self.vehicle.position = self.spawn;
        self.vehicle.position_z = self.preview_spawn_origin_z;
        self.vehicle.heading = self.spawn_heading;
        self.vehicle.velocity = Vec2::default();
        self.vehicle.angular_velocity = 0.0;
        self.drive = self.fresh_drive();
        if let Some(custom) = &mut self.custom {
            custom.dynamics.reset();
        }
        self.preview_ground_z = self
            .preview_collision
            .as_ref()
            .map(|collision| collision.initial_ground_z);
        self.preview_ground_normal = self.preview_collision.as_ref().and_then(|collision| {
            collision
                .validate_initial_support()
                .ok()
                .map(|surface| surface.normal)
        });
    }

    /// Command applied by each `step` while the model is `VehicleModel::Scripted`.
    /// Non-finite commands are replaced by zero.
    pub fn set_scripted_control(&mut self, control: ScriptedControl) {
        self.scripted = if control.is_finite() {
            control
        } else {
            ScriptedControl::default()
        };
    }

    /// Place the (stationary) vehicle at a grid position and make it the
    /// respawn pose. Race progress is untouched.
    pub fn place_vehicle(&mut self, position: Vec2, heading: f64) {
        self.spawn = position;
        self.spawn_heading = heading.rem_euclid(std::f64::consts::TAU);
        self.vehicle.position = position;
        self.vehicle.heading = self.spawn_heading;
        self.vehicle.velocity = Vec2::default();
        self.vehicle.angular_velocity = 0.0;
        self.drive = self.fresh_drive();
        if let Some(custom) = &mut self.custom {
            custom.dynamics.on_place();
        }
    }

    pub fn scripted_replay(&mut self, inputs: &[DriverInput]) -> &[Vehicle] {
        self.record_trace = true;
        for input in inputs {
            self.step(*input);
        }
        &self.replay_trace
    }

    fn update_checkpoint(&mut self, from: Vec2, to: Vec2) {
        if self.track.checkpoints.is_empty() {
            return;
        }
        let index = self.vehicle.checkpoint % self.track.checkpoints.len();
        let gate = self.track.checkpoints[index];
        if segment_distance(gate.center, from, to) <= gate.radius {
            if index == 0 {
                self.vehicle.completed_laps = self.vehicle.completed_laps.saturating_add(1);
                let now = self.vehicle.race_ticks;
                let lap = now.saturating_sub(self.lap_times.lap_start_ticks);
                self.lap_times.last_lap_ticks = Some(lap);
                self.lap_times.best_lap_ticks = Some(
                    self.lap_times
                        .best_lap_ticks
                        .map_or(lap, |best| best.min(lap)),
                );
                self.lap_times.lap_start_ticks = now;
                if self.vehicle.completed_laps >= self.track.laps_to_finish {
                    self.vehicle.finished = true;
                    self.lap_times.finish_ticks = Some(now);
                }
            }
            self.vehicle.checkpoint = (index + 1) % self.track.checkpoints.len();
        }
    }
}

fn finite_clamp(value: f64, min: f64, max: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        0.0
    }
}

fn segment_distance(point: Vec2, start: Vec2, end: Vec2) -> f64 {
    let segment = end - start;
    let len2 = segment.x * segment.x + segment.y * segment.y;
    if len2 <= f64::EPSILON {
        return point.distance(start);
    }
    let rel = point - start;
    let t = ((rel.x * segment.x + rel.y * segment.y) / len2).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, checkpoint: usize, laps: u32, distance: f64) -> RankEntry {
        RankEntry {
            id,
            checkpoint,
            completed_laps: laps,
            finished: false,
            finish_ticks: None,
            distance_to_next: distance,
        }
    }

    #[test]
    fn ranking_orders_by_laps_gates_then_distance() {
        let entries = [
            entry(1, 2, 0, 5.0),
            entry(2, 1, 1, 90.0), // lap ahead beats gate ahead
            entry(3, 2, 0, 3.0),  // same gate, closer
            entry(4, 0, 0, 1.0),  // 3 of 4 gates passed, still lap 0
        ];
        // Order: id2 (lap ahead), id4 (gates), id3 (closer), id1.
        assert_eq!(rank_race(4, &entries), vec![4, 1, 3, 2]);
    }

    #[test]
    fn ranking_lap_wrap_and_ties_are_deterministic() {
        // checkpoint 0 means the last gate was just passed; lap wrap must not
        // rank that car behind a car that just started the lap.
        let entries = [entry(7, 0, 1, 10.0), entry(3, 1, 1, 10.0)];
        assert_eq!(rank_race(4, &entries), vec![1, 2]);
        // Complete tie: lower id wins.
        let tie = [entry(9, 1, 0, 4.0), entry(2, 1, 0, 4.0)];
        assert_eq!(rank_race(4, &tie), vec![2, 1]);
        // Non-finite distance sorts last among equals.
        let nan = [entry(1, 1, 0, f64::NAN), entry(2, 1, 0, 100.0)];
        assert_eq!(rank_race(4, &nan), vec![2, 1]);
        assert!(rank_race(4, &[]).is_empty());
        // No gates: laps decide, then id.
        assert_eq!(
            rank_race(0, &[entry(2, 0, 0, 0.0), entry(1, 0, 1, 0.0)]),
            vec![2, 1]
        );
    }

    #[test]
    fn finished_players_outrank_racing_and_order_by_finish_tick() {
        let mut late = entry(1, 1, 3, 1.0);
        late.finished = true;
        late.finish_ticks = Some(900);
        let mut early = entry(2, 1, 3, 50.0);
        early.finished = true;
        early.finish_ticks = Some(800);
        let mut same = entry(0, 1, 3, 50.0);
        same.finished = true;
        same.finish_ticks = Some(900);
        let racing = entry(3, 3, 2, 0.1);
        assert_eq!(rank_race(4, &[racing, late, early, same]), vec![4, 3, 1, 2]);
    }

    #[test]
    fn lap_times_track_last_best_and_finish() {
        let track = TrackRuntime {
            laps_to_finish: 3,
            checkpoints: vec![
                Checkpoint {
                    center: Vec2::new(0.0, 0.0),
                    radius: 1.0,
                },
                Checkpoint {
                    center: Vec2::new(100.0, 0.0),
                    radius: 1.0,
                },
            ],
            ..TrackRuntime::synthetic_circuit()
        };
        let mut world = RaceWorld::new(SimTiming::default(), track);
        assert_eq!(world.lap_times(), LapTimes::default());
        let cross = |world: &mut RaceWorld, gate: usize, ticks: u64| {
            world.vehicle.race_ticks = ticks;
            let at = world.track.checkpoints[gate].center;
            world.update_checkpoint(at, at);
        };
        cross(&mut world, 1, 100);
        assert_eq!(world.lap_times().last_lap_ticks, None);
        assert_eq!(world.current_lap_ticks(), 100);
        cross(&mut world, 0, 300); // lap 1: 300
        assert_eq!(world.lap_times().last_lap_ticks, Some(300));
        assert_eq!(world.lap_times().best_lap_ticks, Some(300));
        assert_eq!(world.current_lap_ticks(), 0);
        world.vehicle.race_ticks = 350;
        assert_eq!(world.current_lap_ticks(), 50);
        cross(&mut world, 1, 400);
        cross(&mut world, 0, 500); // lap 2: 200 (best), last 200
        assert_eq!(world.lap_times().last_lap_ticks, Some(200));
        assert_eq!(world.lap_times().best_lap_ticks, Some(200));
        cross(&mut world, 1, 600);
        cross(&mut world, 0, 900); // lap 3: 400, finishes; best unchanged
        assert_eq!(world.lap_times().last_lap_ticks, Some(400));
        assert_eq!(world.lap_times().best_lap_ticks, Some(200));
        assert_eq!(world.lap_times().finish_ticks, Some(900));
        assert!(world.vehicle.finished);
        assert_eq!(world.current_lap_ticks(), 0);
        let entry = world.rank_entry(5);
        assert_eq!(entry.finish_ticks, Some(900));
        assert!(entry.finished);
    }

    #[test]
    fn distance_to_next_checkpoint_uses_target_gate() {
        let world = RaceWorld::default();
        let target = world.track.checkpoints[world.vehicle.checkpoint].center;
        let expected = target.distance(world.vehicle.position);
        assert!((world.distance_to_next_checkpoint() - expected).abs() < 1e-12);
    }

    #[test]
    fn ten_thousand_steps_remain_finite_and_repeatable() {
        let inputs: Vec<_> = (0..10_000)
            .map(|i| DriverInput {
                throttle: if i % 180 < 130 { 1.0 } else { 0.2 },
                brake: if i % 180 >= 130 { 0.4 } else { 0.0 },
                steer: ((i % 240) as f64 / 120.0 - 1.0) * 0.4,
                ..DriverInput::default()
            })
            .collect();
        let track = TrackRuntime {
            laps_to_finish: u32::MAX,
            ..TrackRuntime::synthetic_circuit()
        };
        let mut a = RaceWorld::new(SimTiming::default(), track.clone());
        let mut b = RaceWorld::new(SimTiming::default(), track);
        a.scripted_replay(&inputs);
        b.scripted_replay(&inputs);
        assert_eq!(a.replay_trace, b.replay_trace);
        assert_eq!(a.vehicle.race_ticks, 10_000);
        assert!(!a.vehicle.finished);
        for s in &a.replay_trace {
            assert!(s.position.x.is_finite() && s.position.y.is_finite());
            assert!(s.heading.is_finite() && s.velocity.x.is_finite() && s.velocity.y.is_finite());
        }
    }

    #[test]
    fn scripted_vehicle_crosses_ordered_gates_and_finishes_a_lap() {
        let track = TrackRuntime {
            min: Vec2::new(-30.0, -30.0),
            max: Vec2::new(20.0, 5.0),
            checkpoints: vec![
                Checkpoint {
                    center: Vec2::new(0.0, 0.0),
                    radius: 1.0,
                },
                Checkpoint {
                    center: Vec2::new(10.0, 0.0),
                    radius: 1.0,
                },
            ],
            laps_to_finish: 1,
        };
        let mut world = RaceWorld::new(SimTiming::default(), track);
        for _ in 0..90 {
            world.step(DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            });
            if world.vehicle.checkpoint == 0 {
                break;
            }
        }
        assert_eq!(world.vehicle.checkpoint, 0);

        for _ in 0..180 {
            world.step(DriverInput {
                brake: 1.0,
                ..DriverInput::default()
            });
            if world.vehicle.finished {
                break;
            }
        }
        assert_eq!(world.vehicle.completed_laps, 1);
        assert!(world.vehicle.finished);
    }

    #[test]
    fn bounds_collision_clamps_position_and_stops_vehicle() {
        let mut world = RaceWorld::default();
        world.vehicle.position = Vec2::new(world.track.max.x - 0.01, 0.0);
        world.vehicle.velocity = Vec2::new(10.0, 0.0);
        world.step(DriverInput::default());
        assert_eq!(world.vehicle.position.x, world.track.max.x);
        assert_eq!(world.vehicle.velocity, Vec2::default());
        assert_eq!(world.vehicle.angular_velocity, 0.0);
    }

    #[test]
    fn open_preview_has_no_circuit_boundary_or_finish_gates() {
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world.vehicle.position = Vec2::new(5000.0, -7000.0);
        world.vehicle.velocity = Vec2::new(30.0, -4.0);
        world.step(DriverInput::default());
        assert!(world.vehicle.position.x > 5000.0);
        assert!(world.vehicle.position.y < -7000.0);
        assert!(!world.vehicle.finished);
        assert!(world.track.checkpoints.is_empty());
        assert_eq!(world.track.laps_to_finish, 0);
    }

    fn preview_fixture(wall: Option<PreviewWallSegment>) -> PreviewCollisionWorld {
        PreviewCollisionWorld {
            ground: vec![
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, 0.0], [10.0, -10.0, 0.0], [10.0, 10.0, 0.0]],
                },
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [-10.0, 10.0, 0.0]],
                },
            ],
            walls: wall.into_iter().collect(),
            vehicle_radius: 0.25,
            vehicle_min_z: 0.0,
            vehicle_max_z: 1.0,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: Vec::new(),
        }
    }

    fn preview_flat_car(hz: u32) -> RaceWorld {
        let mut world = RaceWorld::new(
            SimTiming {
                physics_hz: hz,
                ..SimTiming::default()
            },
            TrackRuntime::open_preview(),
        );
        world
            .install_preview_collision(preview_fixture(None))
            .unwrap();
        world
    }

    #[test]
    fn local_preview_lateral_grip_recovers_without_changing_reverse_speed_sign() {
        let mut world = preview_flat_car(60);
        world.vehicle.velocity = Vec2::new(-12.0, 8.0);
        let initial_forward = world.vehicle.velocity.x;
        let initial_lateral = world.vehicle.velocity.y;
        for _ in 0..12 {
            world.step(DriverInput::default());
        }
        assert!(world.vehicle.velocity.y.abs() < initial_lateral * 0.4);
        assert!(world.vehicle.velocity.x < 0.0);
        assert!((world.vehicle.velocity.x - initial_forward).abs() < 1.0);
    }

    #[test]
    fn local_preview_grip_is_finite_and_steering_at_rest_has_no_yaw() {
        let mut world = preview_flat_car(60);
        world.step(DriverInput {
            steer: 1.0,
            ..DriverInput::default()
        });
        assert_eq!(world.vehicle.speed(), 0.0);
        assert_eq!(world.vehicle.heading, 0.0);
        assert_eq!(world.vehicle.angular_velocity, 0.0);
        assert!(world.vehicle.velocity.x.is_finite() && world.vehicle.velocity.y.is_finite());
    }

    #[test]
    fn collision_preview_steering_sign_follows_forward_or_reverse_motion() {
        for (longitudinal_speed, steer, expected_sign) in [
            (4.0, 1.0, 1.0),
            (4.0, -1.0, -1.0),
            (-4.0, 1.0, -1.0),
            (-4.0, -1.0, 1.0),
        ] {
            let mut world = preview_flat_car(60);
            world.vehicle.velocity = Vec2::new(longitudinal_speed, 0.0);
            world.step(DriverInput {
                steer,
                ..DriverInput::default()
            });
            assert_eq!(world.vehicle.angular_velocity.signum(), expected_sign);
        }

        let mut parked = preview_flat_car(60);
        parked.step(DriverInput {
            steer: 1.0,
            handbrake: true,
            ..DriverInput::default()
        });
        assert_eq!(parked.vehicle.angular_velocity, 0.0);

        let mut lateral = preview_flat_car(60);
        lateral.vehicle.velocity = Vec2::new(0.0, 5.0);
        lateral.step(DriverInput {
            steer: -1.0,
            ..DriverInput::default()
        });
        assert_eq!(lateral.vehicle.angular_velocity, 0.0);

        let mut rotated_reverse = preview_flat_car(60);
        rotated_reverse.vehicle.heading = std::f64::consts::FRAC_PI_3;
        rotated_reverse.vehicle.velocity = Vec2::new(
            rotated_reverse.vehicle.heading.cos(),
            rotated_reverse.vehicle.heading.sin(),
        ) * -4.0;
        rotated_reverse.step(DriverInput {
            steer: 1.0,
            ..DriverInput::default()
        });
        assert!(rotated_reverse.vehicle.angular_velocity < 0.0);

        let mut brake_through_zero = preview_flat_car(60);
        brake_through_zero.vehicle.velocity = Vec2::new(0.1, 0.0);
        brake_through_zero.step(DriverInput {
            brake: 0.5,
            steer: 1.0,
            ..DriverInput::default()
        });
        assert!(brake_through_zero.vehicle.velocity.x < 0.0);
        assert!(brake_through_zero.vehicle.angular_velocity < 0.0);
    }

    #[test]
    fn preview_steering_is_continuous_at_zero_and_matches_forward_curve_above_threshold() {
        let gain_at_rest = provisional_model::steering_gain(0.0, 0.0, true, false);
        let gain_forward = provisional_model::steering_gain(0.0, 0.001, true, false);
        let gain_reverse = provisional_model::steering_gain(0.0, -0.001, true, false);
        assert_eq!(gain_at_rest, 0.0);
        assert!(gain_forward > 0.0 && gain_reverse < 0.0);
        assert!(gain_forward.abs() < 0.001 && gain_reverse.abs() < 0.001);
        assert!((gain_forward + gain_reverse).abs() < 1.0e-12);

        for speed in [1.2_f64, 2.0, 4.0, 8.0, 20.0] {
            let previous_forward_curve = (speed / 8.0).clamp(0.15, 1.0);
            assert_eq!(
                provisional_model::steering_gain(speed, speed, true, false),
                previous_forward_curve
            );
            assert_eq!(
                provisional_model::steering_gain(speed, -speed, true, false),
                -previous_forward_curve
            );
        }
        assert_eq!(provisional_model::steering_gain(0.0, 0.0, true, true), 0.0);
    }

    #[test]
    fn preview_steering_uses_post_force_speed_across_fixed_step_rates() {
        for hz in [30, 60, 120] {
            let mut world = preview_flat_car(hz);
            world.vehicle.velocity = Vec2::new(3.0, 0.0);
            let dt = world.timing.step_seconds();
            let expected_forward_speed = (3.0 + 9.0 * dt) * (1.0 - 0.12 * dt);
            world.step(DriverInput {
                throttle: 0.5,
                steer: 1.0,
                ..DriverInput::default()
            });
            let expected_yaw_rate = 1.8 * (expected_forward_speed / 8.0).clamp(-1.0, 1.0);
            assert!((world.vehicle.angular_velocity - expected_yaw_rate).abs() < 1.0e-10);
        }
    }

    #[test]
    fn generic_world_retains_legacy_stationary_steering_response() {
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::synthetic_circuit());
        world.step(DriverInput {
            steer: 1.0,
            ..DriverInput::default()
        });
        assert!((world.vehicle.angular_velocity - 0.27).abs() < 1.0e-12);
    }

    #[test]
    fn local_preview_lateral_grip_is_nearly_independent_of_fixed_step_rate() {
        let mut slow = preview_flat_car(30);
        let mut fast = preview_flat_car(120);
        slow.vehicle.velocity = Vec2::new(20.0, 6.0);
        fast.vehicle.velocity = slow.vehicle.velocity;
        for _ in 0..30 {
            slow.step(DriverInput::default());
        }
        for _ in 0..120 {
            fast.step(DriverInput::default());
        }
        assert!((slow.vehicle.velocity.y - fast.vehicle.velocity.y).abs() < 0.02);
    }

    #[test]
    fn shared_multiplayer_handling_matches_collision_preview_without_sharing_collision() {
        for hz in [30, 60, 120] {
            let mut local = preview_flat_car(hz);
            let mut shared = RaceWorld::new(
                SimTiming {
                    physics_hz: hz,
                    ..SimTiming::default()
                },
                TrackRuntime::open_preview(),
            );
            shared.enable_shared_preview_handling();
            assert!(!shared.has_preview_collision());

            // Forward, coast, handbrake, then reverse: identical fixed-step
            // policy across the local and shared worlds, independent of render FPS.
            for input in [
                DriverInput {
                    throttle: 0.6,
                    steer: 0.35,
                    ..DriverInput::default()
                },
                DriverInput {
                    steer: -0.25,
                    ..DriverInput::default()
                },
                DriverInput {
                    handbrake: true,
                    steer: 0.5,
                    ..DriverInput::default()
                },
                DriverInput {
                    brake: 0.7,
                    steer: 0.4,
                    ..DriverInput::default()
                },
            ] {
                for _ in 0..(hz / 8).max(1) {
                    local.step(input);
                    shared.step(input);
                    assert_eq!(local.vehicle.position, shared.vehicle.position);
                    assert_eq!(local.vehicle.velocity, shared.vehicle.velocity);
                    assert_eq!(local.vehicle.heading, shared.vehicle.heading);
                    assert_eq!(
                        local.vehicle.angular_velocity,
                        shared.vehicle.angular_velocity
                    );
                }
            }
            assert!(shared.vehicle.velocity.x < 0.0);
        }
    }

    #[test]
    fn authoritative_shared_configuration_matches_local_collision_support_and_contact() {
        let collision = PreviewCollisionWorld {
            ground: vec![
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, -1.0], [10.0, -10.0, 1.0], [10.0, 10.0, 1.0]],
                },
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, -1.0], [10.0, 10.0, 1.0], [-10.0, 10.0, -1.0]],
                },
            ],
            walls: vec![PreviewWallSegment {
                start: Vec2::new(3.0, -5.0),
                end: Vec2::new(3.0, 5.0),
                min_z: -1.0,
                max_z: 2.0,
            }],
            vehicle_radius: 0.25,
            vehicle_min_z: 0.0,
            vehicle_max_z: 1.0,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: Vec::new(),
        };
        let mut local = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        let mut authoritative = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        authoritative
            .install_preview_collision(collision.clone())
            .unwrap();
        authoritative.enable_shared_preview_handling();
        // Distinct configuration path: local preview auto-enables policy;
        // authoritative world explicitly opts into the same policy.
        local.install_preview_collision(collision).unwrap();

        fn compare_step(local: &mut RaceWorld, authoritative: &mut RaceWorld, input: DriverInput) {
            local.step(input);
            authoritative.step(input);
            assert_eq!(local.vehicle.position, authoritative.vehicle.position);
            assert_eq!(local.vehicle.position_z, authoritative.vehicle.position_z);
            assert_eq!(local.vehicle.velocity, authoritative.vehicle.velocity);
            assert_eq!(local.vehicle.heading, authoritative.vehicle.heading);
            assert_eq!(
                local.vehicle.angular_velocity,
                authoritative.vehicle.angular_velocity
            );
            assert_eq!(
                local.preview_ground_normal(),
                authoritative.preview_ground_normal()
            );
        }
        for _ in 0..60 {
            compare_step(
                &mut local,
                &mut authoritative,
                DriverInput {
                    throttle: 1.0,
                    ..DriverInput::default()
                },
            );
        }
        assert!(authoritative.vehicle.position.x < 2.751);
        assert!(authoritative.vehicle.velocity.length() < 1.0e-10);
        assert!(authoritative.preview_ground_normal().unwrap()[0].abs() > 0.05);
        for _ in 0..30 {
            compare_step(
                &mut local,
                &mut authoritative,
                DriverInput {
                    brake: 1.0,
                    steer: 0.4,
                    ..DriverInput::default()
                },
            );
        }
        assert!(authoritative.vehicle.velocity.x < 0.0);
        let inputs = [
            DriverInput {
                throttle: 0.5,
                steer: 0.3,
                ..DriverInput::default()
            },
            DriverInput {
                steer: -0.2,
                ..DriverInput::default()
            },
            DriverInput {
                handbrake: true,
                steer: 0.7,
                ..DriverInput::default()
            },
            DriverInput {
                brake: 0.5,
                steer: 0.4,
                ..DriverInput::default()
            },
        ];
        for input in inputs.into_iter().cycle().take(150) {
            compare_step(&mut local, &mut authoritative, input);
        }
    }

    fn contact_body(id: u32, previous: Vec2, position: Vec2, velocity: Vec2) -> VehicleContactBody {
        VehicleContactBody {
            stable_id: id,
            vehicle: Vehicle {
                position,
                position_z: 0.0,
                heading: 0.0,
                velocity,
                angular_velocity: 0.0,
                checkpoint: 0,
                completed_laps: 0,
                race_ticks: 0,
                finished: false,
            },
            previous_position: previous,
            radius: 0.5,
            min_z: 0.0,
            max_z: 1.0,
        }
    }

    #[test]
    fn vehicle_contact_handles_head_on_rear_end_side_glance_and_resting_overlap() {
        let mut head_on = [
            contact_body(
                1,
                Vec2::new(-2.0, 0.0),
                Vec2::new(0.2, 0.0),
                Vec2::new(8.0, 0.0),
            ),
            contact_body(
                2,
                Vec2::new(2.0, 0.0),
                Vec2::new(-0.2, 0.0),
                Vec2::new(-8.0, 0.0),
            ),
        ];
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut head_on), Ok(2));
        assert!(head_on[1].vehicle.position.x - head_on[0].vehicle.position.x >= 1.0);
        assert!((head_on[0].vehicle.velocity.x - head_on[1].vehicle.velocity.x).abs() < 1.0e-10);

        let mut rear_end = [
            contact_body(
                3,
                Vec2::new(-2.0, 0.0),
                Vec2::new(0.5, 0.0),
                Vec2::new(10.0, 0.0),
            ),
            contact_body(
                4,
                Vec2::new(0.0, 0.0),
                Vec2::new(0.6, 0.0),
                Vec2::new(1.0, 0.0),
            ),
        ];
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut rear_end), Ok(2));
        assert!(rear_end[1].vehicle.position.x - rear_end[0].vehicle.position.x >= 1.0);
        assert!(rear_end[0].vehicle.velocity.x <= rear_end[1].vehicle.velocity.x + 1.0e-10);

        let mut side_glance = [
            contact_body(
                5,
                Vec2::new(-1.0, -1.0),
                Vec2::new(0.0, 0.1),
                Vec2::new(5.0, 0.0),
            ),
            contact_body(
                6,
                Vec2::new(1.0, 1.0),
                Vec2::new(0.1, 0.0),
                Vec2::new(0.0, -5.0),
            ),
        ];
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut side_glance), Ok(2));
        assert!(
            side_glance[0]
                .vehicle
                .position
                .distance(side_glance[1].vehicle.position)
                >= 1.0
        );

        let mut resting = [
            contact_body(7, Vec2::default(), Vec2::default(), Vec2::default()),
            contact_body(8, Vec2::default(), Vec2::default(), Vec2::default()),
        ];
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut resting), Ok(2));
        assert!(
            resting[0]
                .vehicle
                .position
                .distance(resting[1].vehicle.position)
                >= 1.0
        );
        assert_eq!(resting[0].vehicle.velocity, Vec2::default());
        assert_eq!(resting[1].vehicle.velocity, Vec2::default());
    }

    #[test]
    fn swept_vehicle_contact_handles_touching_crossed_and_uncrossed_centers() {
        let mut touching_cross = [
            contact_body(
                1,
                Vec2::new(-0.5, 0.0),
                Vec2::new(3.0, 0.0),
                Vec2::new(7.0, 0.0),
            ),
            contact_body(
                2,
                Vec2::new(0.5, 0.0),
                Vec2::new(-3.0, 0.0),
                Vec2::new(-7.0, 0.0),
            ),
        ];
        assert_eq!(
            resolve_pairwise_vehicle_contacts(&mut touching_cross),
            Ok(2)
        );
        assert!(touching_cross[0].vehicle.position.x < touching_cross[1].vehicle.position.x);
        assert!(
            touching_cross[0]
                .vehicle
                .position
                .distance(touching_cross[1].vehicle.position)
                >= 1.0
        );
        assert!((touching_cross[0].vehicle.position.x + 0.5000005).abs() < 1.0e-8);
        assert!((touching_cross[1].vehicle.position.x - 0.5000005).abs() < 1.0e-8);
        assert!(
            (touching_cross[0].vehicle.velocity.x - touching_cross[1].vehicle.velocity.x).abs()
                < 1.0e-10
        );

        let mut crossed_but_overlapping = [
            contact_body(
                1,
                Vec2::new(-2.0, 0.0),
                Vec2::new(0.4, 0.0),
                Vec2::new(8.0, 0.0),
            ),
            contact_body(
                2,
                Vec2::new(2.0, 0.0),
                Vec2::new(-0.4, 0.0),
                Vec2::new(-8.0, 0.0),
            ),
        ];
        assert_eq!(
            resolve_pairwise_vehicle_contacts(&mut crossed_but_overlapping),
            Ok(2)
        );
        assert!(
            crossed_but_overlapping[0].vehicle.position.x
                < crossed_but_overlapping[1].vehicle.position.x
        );
        assert!(
            crossed_but_overlapping[0]
                .vehicle
                .position
                .distance(crossed_but_overlapping[1].vehicle.position)
                >= 1.0
        );

        let mut not_yet_crossed = [
            contact_body(
                1,
                Vec2::new(-2.0, 0.0),
                Vec2::new(-0.4, 0.0),
                Vec2::new(8.0, 0.0),
            ),
            contact_body(
                2,
                Vec2::new(2.0, 0.0),
                Vec2::new(0.4, 0.0),
                Vec2::new(-8.0, 0.0),
            ),
        ];
        assert_eq!(
            resolve_pairwise_vehicle_contacts(&mut not_yet_crossed),
            Ok(2)
        );
        assert!(not_yet_crossed[0].vehicle.position.x < not_yet_crossed[1].vehicle.position.x);
        assert!(
            not_yet_crossed[0]
                .vehicle
                .position
                .distance(not_yet_crossed[1].vehicle.position)
                >= 1.0
        );
    }

    #[test]
    fn vehicle_contact_is_deterministic_and_stable_id_ordered() {
        let source = [
            contact_body(
                11,
                Vec2::new(-1.0, 0.0),
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
            ),
            contact_body(
                22,
                Vec2::new(0.0, 0.0),
                Vec2::new(0.2, 0.0),
                Vec2::default(),
            ),
            contact_body(
                33,
                Vec2::new(1.0, 0.0),
                Vec2::new(0.4, 0.0),
                Vec2::new(-1.0, 0.0),
            ),
        ];
        let mut forward = source;
        let mut reverse = [source[2], source[1], source[0]];
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut forward), Ok(3));
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut reverse), Ok(3));
        assert_eq!(forward, reverse);
        assert!(forward
            .windows(2)
            .all(|pair| pair[0].stable_id < pair[1].stable_id));
        assert!(
            forward[1]
                .vehicle
                .position
                .distance(forward[0].vehicle.position)
                >= 1.0 - 1.0e-5,
            "resolved chain: {:?}",
            forward
        );
        assert!(
            forward[2]
                .vehicle
                .position
                .distance(forward[1].vehicle.position)
                >= 1.0 - 1.0e-5,
            "resolved chain: {:?}",
            forward
        );
    }

    #[test]
    fn vehicle_contact_respects_vertical_separation_and_preserves_height() {
        let mut separated = [
            contact_body(1, Vec2::default(), Vec2::default(), Vec2::default()),
            contact_body(2, Vec2::default(), Vec2::default(), Vec2::default()),
        ];
        separated[1].vehicle.position_z = 3.0;
        let before = separated;
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut separated), Ok(0));
        assert_eq!(separated, before);

        let mut overlapping = [
            contact_body(
                1,
                Vec2::default(),
                Vec2::new(-0.2, 0.0),
                Vec2::new(1.0, 0.0),
            ),
            contact_body(
                2,
                Vec2::default(),
                Vec2::new(0.2, 0.0),
                Vec2::new(-1.0, 0.0),
            ),
        ];
        overlapping[0].vehicle.position_z = 4.25;
        overlapping[1].vehicle.position_z = 4.25;
        assert_eq!(resolve_pairwise_vehicle_contacts(&mut overlapping), Ok(2));
        assert_eq!(overlapping[0].vehicle.position_z, 4.25);
        assert_eq!(overlapping[1].vehicle.position_z, 4.25);
    }

    #[test]
    fn post_contact_constraint_rechecks_wall_without_vertical_teleport() {
        let wall = PreviewWallSegment {
            start: Vec2::new(2.0, -2.0),
            end: Vec2::new(2.0, 2.0),
            min_z: -1.0,
            max_z: 2.0,
        };
        let mut world = preview_flat_car(60);
        world.preview_collision.as_mut().unwrap().walls = vec![wall];
        world.vehicle.position = Vec2::new(1.74, 0.0);
        world.vehicle.position_z = 0.0;
        world.vehicle.velocity = Vec2::new(4.0, 0.0);
        world.constrain_after_vehicle_contact(Vec2::new(1.74, 0.0));
        assert!(world.vehicle.position.x < 1.75);
        assert_eq!(world.vehicle.position_z, 0.0);
        assert_eq!(world.preview_ground_normal(), Some([0.0, 0.0, 1.0]));
    }

    #[test]
    fn local_preview_grip_policy_is_configurable_and_preserves_forward_speed_response() {
        let mut world = preview_flat_car(60);
        assert!(world
            .set_preview_grip_policy(PreviewGripPolicy {
                lateral_recovery_per_second: 0.0,
                handbrake_recovery_per_second: 0.0,
            })
            .is_ok());
        assert!(world
            .set_preview_grip_policy(PreviewGripPolicy {
                lateral_recovery_per_second: f64::NAN,
                handbrake_recovery_per_second: 1.0,
            })
            .is_err());
        world.vehicle.velocity = Vec2::new(10.0, 5.0);
        world.step(DriverInput::default());
        let expected_forward = 10.0 * (1.0 - 0.12 / 60.0);
        assert!((world.vehicle.velocity.x - expected_forward).abs() < 1.0e-8);
    }

    #[test]
    fn collision_backed_preview_drive_policy_is_faster_bounded_and_configurable() {
        let mut preview = preview_flat_car(60);
        let mut generic = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        let policy = PreviewDrivePolicy::default();
        assert_eq!(
            policy,
            PreviewDrivePolicy {
                acceleration_per_second: 18.0,
                braking_per_second: 30.0,
                max_speed: 67.5,
            }
        );
        assert!(preview.set_preview_drive_policy(policy).is_ok());
        for _ in 0..60 {
            preview.step(DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            });
            generic.step(DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            });
        }
        assert!(preview.vehicle.speed() > generic.vehicle.speed());

        for _ in 0..360 {
            preview.step(DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            });
        }
        assert!(preview.vehicle.speed() <= policy.max_speed + 1.0e-9);
        assert!((preview.vehicle.speed() - policy.max_speed).abs() < 0.25);

        preview.vehicle.velocity = Vec2::new(-80.0, 0.0);
        preview.step(DriverInput::default());
        assert!(
            preview.vehicle.velocity.x < 0.0,
            "reverse direction is preserved"
        );
        assert!(preview.vehicle.speed() <= policy.max_speed + 1.0e-9);
    }

    #[test]
    fn preview_drive_policy_preserves_braking_semantics_and_is_preview_only() {
        let mut preview = preview_flat_car(60);
        let mut generic = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        preview.vehicle.velocity = Vec2::new(30.0, 0.0);
        generic.vehicle.velocity = Vec2::new(30.0, 0.0);
        for _ in 0..60 {
            preview.step(DriverInput {
                brake: 1.0,
                ..DriverInput::default()
            });
            generic.step(DriverInput {
                brake: 1.0,
                ..DriverInput::default()
            });
        }
        assert!(preview.vehicle.velocity.x < generic.vehicle.velocity.x);

        let network_track = TrackRuntime::synthetic_circuit();
        let mut network_world = RaceWorld::new(SimTiming::default(), network_track);
        assert_eq!(
            network_world.set_preview_drive_policy(PreviewDrivePolicy::default()),
            Err("preview drive policy requires installed preview collision geometry")
        );
        let mut baseline = RaceWorld::new(SimTiming::default(), TrackRuntime::synthetic_circuit());
        let input = DriverInput {
            throttle: 1.0,
            ..DriverInput::default()
        };
        for _ in 0..30 {
            network_world.step(input);
            baseline.step(input);
        }
        assert_eq!(network_world.vehicle, baseline.vehicle);
    }

    #[test]
    fn preview_drive_policy_rejects_nonfinite_and_unbounded_values() {
        let mut preview = preview_flat_car(60);
        assert!(preview
            .set_preview_drive_policy(PreviewDrivePolicy {
                acceleration_per_second: -0.01,
                ..PreviewDrivePolicy::default()
            })
            .is_err());
        assert!(preview
            .set_preview_drive_policy(PreviewDrivePolicy {
                braking_per_second: f64::INFINITY,
                ..PreviewDrivePolicy::default()
            })
            .is_err());
        assert!(preview
            .set_preview_drive_policy(PreviewDrivePolicy {
                max_speed: 0.0,
                ..PreviewDrivePolicy::default()
            })
            .is_err());
        assert!(preview
            .set_preview_drive_policy(PreviewDrivePolicy {
                acceleration_per_second: f64::NAN,
                ..PreviewDrivePolicy::default()
            })
            .is_err());
        assert!(preview
            .set_preview_drive_policy(PreviewDrivePolicy {
                max_speed: 200.01,
                ..PreviewDrivePolicy::default()
            })
            .is_err());
    }

    #[test]
    fn local_preview_tracks_slope_and_bank_with_normal_and_preserves_it_on_support_miss() {
        let mut geometry = preview_fixture(None);
        geometry.vehicle_min_z = 0.8;
        geometry.vehicle_max_z = 1.8;
        geometry.initial_ground_z = 3.5;
        geometry.ground = vec![
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, -4.0], [10.0, -10.0, 6.0], [10.0, 10.0, 11.0]],
            },
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, -4.0], [10.0, 10.0, 11.0], [-10.0, 10.0, 1.0]],
            },
        ];
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world.install_preview_collision(geometry).unwrap();
        let normal = world.preview_ground_normal().unwrap();
        assert!(normal[0] < 0.0 && normal[1] < 0.0 && normal[2] > 0.0);
        let expected_z = world.preview_ground_z.unwrap()
            - world.preview_collision.as_ref().unwrap().vehicle_min_z / normal[2];
        assert!((world.vehicle.position_z - expected_z).abs() < 1.0e-8);
        world.vehicle.position = Vec2::new(20.0, 0.0);
        world.step(DriverInput::default());
        assert_eq!(world.preview_ground_normal().unwrap(), normal);
    }

    #[test]
    fn support_normals_cover_grade_only_and_bank_only_planes() {
        let cases = [
            (
                PreviewGroundTriangle {
                    vertices: [[-2.0, -2.0, -1.0], [2.0, -2.0, 1.0], [2.0, 2.0, 1.0]],
                },
                true,
            ),
            (
                PreviewGroundTriangle {
                    vertices: [[-2.0, -2.0, -1.0], [2.0, 2.0, 1.0], [2.0, -2.0, -1.0]],
                },
                false,
            ),
        ];
        for (triangle, grade_axis) in cases {
            let mut geometry = preview_fixture(None);
            geometry.ground = vec![triangle];
            let normal = geometry
                .support_surface(Vec2::default(), 0.0, 0.25, None)
                .unwrap()
                .normal;
            assert!(normal[2] > 0.0);
            if grade_axis {
                assert!(normal[0].abs() > 0.1 && normal[1].abs() < 1.0e-6);
            } else {
                assert!(normal[1].abs() > 0.1 && normal[0].abs() < 1.0e-6);
            }
        }
    }

    #[test]
    fn tilted_vertical_bounds_cover_scaled_footprint_and_wall_clearance() {
        let mut geometry = preview_fixture(Some(PreviewWallSegment {
            start: Vec2::new(0.5, -4.0),
            end: Vec2::new(0.5, 4.0),
            min_z: -0.02,
            max_z: -0.01,
        }));
        geometry.units_per_sim_unit = 0.5;
        geometry.vehicle_radius = 0.5;
        geometry.vehicle_min_z = 0.0;
        geometry.vehicle_max_z = 1.0;
        geometry.ground = vec![
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, 7.5], [10.0, -10.0, -7.5], [10.0, 10.0, -7.5]],
            },
            PreviewGroundTriangle {
                vertices: [[-10.0, -10.0, 7.5], [10.0, 10.0, -7.5], [-10.0, 10.0, 7.5]],
            },
        ];
        let support = geometry.validate_initial_support().unwrap();
        let origin_z = support.height - geometry.vehicle_min_z / support.normal[2];
        let (minimum, maximum) = geometry.vertical_bounds(origin_z, support.normal);
        let radius_scene = geometry.vehicle_radius * geometry.units_per_sim_unit;
        for step in 0..64 {
            let angle = f64::from(step) * std::f64::consts::TAU / 64.0;
            let x = radius_scene * angle.cos();
            let y = radius_scene * angle.sin();
            for local_z in [geometry.vehicle_min_z, geometry.vehicle_max_z] {
                let transformed_z = origin_z
                    + support.normal[0] * x
                    + support.normal[1] * y
                    + support.normal[2] * local_z;
                assert!(transformed_z >= minimum - 1.0e-9 && transformed_z <= maximum + 1.0e-9);
            }
        }
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world.install_preview_collision(geometry).unwrap();
        world.vehicle.velocity = Vec2::new(100.0, 0.0);
        world.step(DriverInput::default());
        assert!(
            world.vehicle.position.x <= 0.251,
            "tilted footprint must collide with the low wall"
        );
    }

    #[test]
    fn local_preview_flat_support_normal_is_world_up() {
        let world = preview_flat_car(60);
        assert_eq!(world.preview_ground_normal(), Some([0.0, 0.0, 1.0]));
    }

    #[test]
    fn support_normal_tie_break_prefers_continuous_surface_and_ignores_other_heights() {
        let flat = PreviewGroundTriangle {
            vertices: [[-2.0, -2.0, 0.0], [2.0, -2.0, 0.0], [2.0, 2.0, 0.0]],
        };
        let banked = PreviewGroundTriangle {
            vertices: [[-2.0, -2.0, -1.0], [2.0, 2.0, 1.0], [2.0, -2.0, -1.0]],
        };
        let roof = PreviewGroundTriangle {
            vertices: [[-2.0, -2.0, 4.0], [2.0, -2.0, 4.0], [2.0, 2.0, 4.0]],
        };
        let mut geometry = preview_fixture(None);
        geometry.ground = vec![banked, flat, roof];
        let selected = geometry
            .support_surface(Vec2::default(), 0.0, 0.25, Some([0.0, 0.0, 1.0]))
            .unwrap();
        assert_eq!(selected.normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn preview_swept_circle_stops_at_wall_and_removes_inward_velocity() {
        let wall = PreviewWallSegment {
            start: Vec2::new(0.5, -4.0),
            end: Vec2::new(0.5, 4.0),
            min_z: 0.0,
            max_z: 2.0,
        };
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world
            .install_preview_collision(preview_fixture(Some(wall)))
            .unwrap();
        world.vehicle.velocity = Vec2::new(100.0, 15.0);
        world.step(DriverInput::default());
        assert!(world.vehicle.position.x <= 0.251);
        assert!(
            world.vehicle.position.y > 0.0,
            "wall contact should preserve tangential travel"
        );
        assert!(world.vehicle.velocity.x.abs() < 1.0e-6);
        assert!(world.vehicle.velocity.y > 0.0);
        assert_eq!(world.vehicle.position_z, 0.0);
        assert!(world.vehicle.position.x.is_finite() && world.vehicle.position.y.is_finite());
        assert!(world.vehicle.speed().is_finite());
    }

    #[test]
    fn preview_vehicle_can_pass_under_a_wall_above_its_vertical_extent() {
        let wall = PreviewWallSegment {
            start: Vec2::new(0.5, -4.0),
            end: Vec2::new(0.5, 4.0),
            min_z: 2.0,
            max_z: 3.0,
        };
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world
            .install_preview_collision(preview_fixture(Some(wall)))
            .unwrap();
        world.vehicle.velocity = Vec2::new(100.0, 0.0);
        world.step(DriverInput::default());
        assert!(world.vehicle.position.x > 0.5);
        assert!(world.vehicle.velocity.x > 0.0);
    }

    #[test]
    fn preview_ground_support_rejects_unreachable_upper_surface() {
        let mut geometry = preview_fixture(None);
        geometry.ground.push(PreviewGroundTriangle {
            vertices: [[-10.0, -10.0, 5.0], [10.0, -10.0, 5.0], [10.0, 10.0, 5.0]],
        });
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world.install_preview_collision(geometry).unwrap();
        world.vehicle.velocity = Vec2::new(10.0, 0.0);
        world.step(DriverInput::default());
        assert_eq!(world.vehicle.position_z, 0.0);
    }

    #[test]
    fn preview_ground_support_accepts_reversed_source_winding() {
        let geometry = PreviewCollisionWorld {
            ground: vec![
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, 0.0], [10.0, 10.0, 0.0], [10.0, -10.0, 0.0]],
                },
                PreviewGroundTriangle {
                    vertices: [[-10.0, -10.0, 0.0], [-10.0, 10.0, 0.0], [10.0, 10.0, 0.0]],
                },
            ],
            walls: vec![],
            vehicle_radius: 0.25,
            vehicle_min_z: 0.0,
            vehicle_max_z: 1.0,
            initial_ground_z: 0.0,
            units_per_sim_unit: 1.0,
            edge_rows: Vec::new(),
        };
        let mut world = RaceWorld::new(SimTiming::default(), TrackRuntime::open_preview());
        world.install_preview_collision(geometry).unwrap();
        world.vehicle.velocity = Vec2::new(10.0, 0.0);
        world.step(DriverInput::default());
        assert_eq!(world.vehicle.position_z, 0.0);
    }

    #[test]
    fn preview_collision_validation_rejects_caps_and_invalid_geometry() {
        let mut too_many = preview_fixture(None);
        too_many.ground = vec![too_many.ground[0]; PreviewCollisionWorld::MAX_GROUND_TRIANGLES + 1];
        assert!(too_many.validate().is_err());
        let mut bad_wall = preview_fixture(None);
        bad_wall.walls.push(PreviewWallSegment {
            start: Vec2::new(0.0, 0.0),
            end: Vec2::new(f64::NAN, 1.0),
            min_z: 0.0,
            max_z: 1.0,
        });
        assert!(bad_wall.validate().is_err());
    }

    #[test]
    fn swept_wall_solver_ignores_wall_behind_and_allows_motion_away_from_contact() {
        let wall_start = Vec2::new(0.0, -2.0);
        let wall_end = Vec2::new(0.0, 2.0);
        assert!(sweep_circle_segment(
            Vec2::new(10.0, 0.0),
            Vec2::new(11.0, 0.0),
            wall_start,
            wall_end,
            0.25,
        )
        .is_none());
        assert!(sweep_circle_segment(
            Vec2::new(0.1, 0.0),
            Vec2::new(0.2, 0.0),
            wall_start,
            wall_end,
            0.25,
        )
        .is_none());
        let toward = sweep_circle_segment(
            Vec2::new(0.1, 0.0),
            Vec2::new(-0.2, 0.0),
            wall_start,
            wall_end,
            0.25,
        );
        assert!(toward.is_some_and(|(time, normal, penetration)| {
            time == 0.0 && normal.x > 0.0 && penetration > 0.0
        }));
    }

    #[test]
    fn swept_wall_solver_blocks_crossing_from_both_sides() {
        let wall_start = Vec2::new(0.0, -2.0);
        let wall_end = Vec2::new(0.0, 2.0);
        let left_to_right = sweep_circle_segment(
            Vec2::new(-1.0, 0.0),
            Vec2::new(1.0, 0.0),
            wall_start,
            wall_end,
            0.25,
        )
        .unwrap();
        let right_to_left = sweep_circle_segment(
            Vec2::new(1.0, 0.0),
            Vec2::new(-1.0, 0.0),
            wall_start,
            wall_end,
            0.25,
        )
        .unwrap();
        assert!((left_to_right.0 - 0.375).abs() < 1.0e-6);
        assert!(left_to_right.1.x < 0.0);
        assert!((right_to_left.0 - 0.375).abs() < 1.0e-6);
        assert!(right_to_left.1.x > 0.0);
    }

    #[test]
    fn swept_wall_solver_resolves_start_inside_endpoint_capsule() {
        let hit = sweep_circle_segment(
            Vec2::new(-0.05, -2.05),
            Vec2::new(0.2, -2.05),
            Vec2::new(0.0, -2.0),
            Vec2::new(0.0, 2.0),
            0.25,
        )
        .unwrap();
        assert_eq!(hit.0, 0.0);
        assert!(hit.2 > 0.0);
    }

    #[test]
    fn bounded_input_sequences_preserve_world_invariants() {
        fn random_unit(seed: &mut u32) -> f64 {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 17;
            *seed ^= *seed << 5;
            f64::from(*seed) / f64::from(u32::MAX)
        }

        for initial_seed in 1..=8 {
            let mut seed = initial_seed;
            let track = TrackRuntime {
                laps_to_finish: u32::MAX,
                ..TrackRuntime::synthetic_circuit()
            };
            let mut world = RaceWorld::new(SimTiming::default(), track);
            for _ in 0..2_500 {
                let input = DriverInput {
                    throttle: random_unit(&mut seed),
                    brake: random_unit(&mut seed),
                    steer: random_unit(&mut seed) * 2.0 - 1.0,
                    handbrake: seed & 1 != 0,
                    ..DriverInput::default()
                };
                world.step(input);
                let car = world.vehicle;
                assert!(car.position.x.is_finite() && car.position.y.is_finite());
                assert!(car.heading.is_finite() && (0.0..TAU).contains(&car.heading));
                assert!(car.velocity.x.is_finite() && car.velocity.y.is_finite());
                assert!(car.speed() <= 45.0 + 1e-9);
                assert!(world.track.contains(car.position));
                assert!(car.checkpoint < world.track.checkpoints.len());
                assert!(car.race_ticks <= world.tick);
            }
            assert_eq!(world.vehicle.race_ticks, 2_500);
            assert!(!world.vehicle.finished);
        }
    }

    #[test]
    fn reset_ranks_the_car_last_on_the_very_next_step() {
        let mut world = RaceWorld::default();
        let count = world.track.checkpoints.len();
        world.vehicle.checkpoint = 3 % count;
        world.vehicle.position = world.track.checkpoints[3 % count].center;
        let ahead = world.rank_entry(1);
        let rival = RankEntry { id: 2, checkpoint: 2, completed_laps: 0, finished: false, finish_ticks: None, distance_to_next: 1.0 };
        assert_eq!(rank_race(count, &[ahead, rival]), vec![1, 2]);
        world.step(DriverInput { reset_request: true, ..DriverInput::default() });
        assert_eq!(rank_race(count, &[world.rank_entry(1), rival]), vec![2, 1]);
    }

    #[test]
    fn reset_restores_spawn_keeping_laps_and_clock_and_rewinds_gate_target() {
        let mut world = RaceWorld::default();
        world.vehicle.completed_laps = 1;
        world.vehicle.checkpoint = 2;
        world.vehicle.race_ticks = 40;
        world.tick = 40;
        world.vehicle.heading = 1.0;
        world.vehicle.velocity = Vec2::new(3.0, 2.0);
        world.vehicle.position = Vec2::new(12.0, 3.0);
        world.step(DriverInput {
            reset_request: true,
            ..DriverInput::default()
        });
        assert_eq!(world.vehicle.position, world.track.checkpoints[0].center);
        assert_eq!(world.vehicle.completed_laps, 1);
        assert_eq!(world.vehicle.checkpoint, 1);
        assert_eq!(world.vehicle.race_ticks, 40);
        assert_eq!(world.tick, 41);
        assert_eq!(world.vehicle.heading, 0.0);
        assert_eq!(world.vehicle.velocity, Vec2::default());
    }

    #[test]
    fn non_finite_controls_are_safely_ignored() {
        let mut world = RaceWorld::default();
        world.step(DriverInput {
            throttle: f64::NAN,
            steer: f64::INFINITY,
            ..DriverInput::default()
        });
        assert!(world.vehicle.position.x.is_finite());
        assert_eq!(world.vehicle.velocity, Vec2::default());
    }

    #[test]
    fn scripted_acceleration_completes_a_reproducible_maneuver() {
        let mut world = RaceWorld::default();
        let start = world.vehicle.position;
        let inputs = vec![
            DriverInput {
                throttle: 1.0,
                ..DriverInput::default()
            };
            120
        ];
        world.scripted_replay(&inputs);
        assert!(world.vehicle.position.x > start.x + 10.0);
        assert!(world.vehicle.speed() > 10.0);
        assert_eq!(world.tick, 120);
    }

    #[test]
    fn circuit_text_fixture_loads_and_rejects_invalid_bounds() {
        let track = TrackRuntime::parse(include_str!(
            "../../../content/demo/tracks/test-circuit.prtrack"
        ))
        .unwrap();
        assert_eq!(track.checkpoints.len(), 4);
        assert_eq!(track.laps_to_finish, 2);
        assert!(TrackRuntime::parse(
            "RACER_CIRCUIT_V1\nbounds 0 0 NaN 2\nlaps 1\ncheckpoint 0 0 1\ncheckpoint 1 1 1"
        )
        .is_err());
    }

    #[test]
    fn circuit_parser_rejects_invalid_required_fields() {
        let invalid_tracks = [
            "bounds 0 0 10 10\nlaps 1\ncheckpoint 1 1 1\ncheckpoint 9 9 1",
            "RACER_CIRCUIT_V1\nbounds 0 0 10 10\nbounds 0 0 20 20\nlaps 1\ncheckpoint 1 1 1\ncheckpoint 9 9 1",
            "RACER_CIRCUIT_V1\nbounds 0 0 10 10\nlaps 0\ncheckpoint 1 1 1\ncheckpoint 9 9 1",
            "RACER_CIRCUIT_V1\nbounds 0 0 10 10\nlaps 1\ncheckpoint 1 1 0\ncheckpoint 9 9 1",
            "RACER_CIRCUIT_V1\nbounds 0 0 10 10\nlaps 1\ncheckpoint -1 1 1\ncheckpoint 9 9 1",
            "RACER_CIRCUIT_V1\nbounds 0 0 10 10\nlaps 1\ncheckpoint 1 1 1",
            "RACER_CIRCUIT_V1\nbounds 0 0 10 10\nlaps 1\ncheckpoint 1 1 1\ncheckpoint 9 9 1\nunknown 7",
        ];
        for source in invalid_tracks {
            assert!(
                TrackRuntime::parse(source).is_err(),
                "accepted invalid track: {source}"
            );
        }
    }

    #[test]
    fn each_step_advances_one_fixed_physics_tick() {
        let timing = SimTiming::default();
        assert_eq!(timing.step_seconds(), 1.0 / 60.0);
        let mut world = RaceWorld::new(timing, TrackRuntime::synthetic_circuit());
        world.step(DriverInput::default());
        assert_eq!(world.tick, 1);
        assert_eq!(world.vehicle.race_ticks, 1);
    }

    #[test]
    fn scripted_replay_matches_checked_in_expected_trace() {
        let expected = include_str!("../../../tests/synthetic-content/acceleration.trace.csv");
        let mut world = RaceWorld::default();
        let mut completed = 0u64;
        for raw in expected.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let fields: Vec<_> = line.split(',').collect();
            assert_eq!(fields.len(), 7);
            let tick = fields[0].parse::<u64>().unwrap();
            while completed < tick {
                world.step(DriverInput {
                    throttle: 1.0,
                    ..DriverInput::default()
                });
                completed += 1;
            }
            let expected_floats: Vec<f64> =
                fields[1..5].iter().map(|v| v.parse().unwrap()).collect();
            let actual = [
                world.vehicle.position.x,
                world.vehicle.position.y,
                world.vehicle.heading,
                world.vehicle.speed(),
            ];
            for (got, want) in actual.iter().zip(expected_floats) {
                assert!((got - want).abs() < 1e-9, "tick {tick}: {got} != {want}");
            }
            assert_eq!(
                world.vehicle.completed_laps + 1,
                fields[5].parse::<u32>().unwrap()
            );
            assert_eq!(
                world.vehicle.checkpoint,
                fields[6].parse::<usize>().unwrap()
            );
        }
        assert_eq!(world.tick, 120);
    }
}
