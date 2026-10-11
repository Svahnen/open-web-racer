//! The driver field: decisions (lane, target speed, recovery) and the steering
//! servo for every computer-controlled car of a race.
//!
//! Decisions run at a lower rate than motion (`Settings::decision_every`
//! ticks, staggered per car); the servo runs every tick from the stored
//! decision and the current state. Humans are observed, never controlled.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use racer_sim::{RaceWorld, ScriptedControl};

use crate::config::{AiConfig, Skill};
use crate::q16::{self, ONE};
use crate::rules::{
    AccelInput, GLUE_LEN, RuleConstants, TargetInput, Trim, accel_limit, brake_limit, pick_lane,
    row_delta, target_speed,
};
use crate::track::{AiTrack, FIRST_LEFT_LANE};
use racer_pack_schema::ai::VehicleAi;

/// State of one car as seen by the drivers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarObs {
    pub id: u32,
    pub position: [f64; 2],
    pub heading: f64,
    pub velocity: [f64; 2],
    pub yaw_rate: f64,
    /// Completed laps.
    pub laps: u32,
    pub finished: bool,
    pub human: bool,
    /// Half of the car's width (metres).
    pub half_width: f64,
}

impl CarObs {
    /// Observe a simulation world.
    pub fn from_world(id: u32, world: &RaceWorld, human: bool, half_width: f64) -> Self {
        let v = world.vehicle;
        Self {
            id,
            position: [v.position.x, v.position.y],
            heading: v.heading,
            velocity: [v.velocity.x, v.velocity.y],
            yaw_rate: v.angular_velocity,
            laps: v.completed_laps,
            finished: v.finished,
            human,
            half_width,
        }
    }
}

/// Race-level settings of the drivers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub skill: Skill,
    /// Rubber-banding: pace and acceleration catch-up curves.
    pub catch_up: bool,
    /// +1 racing in the row direction, -1 against it.
    pub direction: i32,
    pub laps: u32,
    pub seed: u32,
    pub tick_seconds: f64,
    /// Ticks between decisions of one car.
    pub decision_every: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            skill: Skill::Advanced,
            catch_up: true,
            direction: 1,
            laps: 2,
            seed: 1,
            tick_seconds: 1.0 / 60.0,
            decision_every: 8,
        }
    }
}

/// What a driver currently intends (for diagnostics and tests).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decision {
    pub lane: i32,
    pub aim_offset: f64,
    pub target_speed: f64,
    pub free_target_speed: f64,
    pub pace: f64,
    pub reversing: bool,
    pub aggressive: bool,
}

/// Small deterministic generator for the lateral slack draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
}

#[derive(Clone, Debug)]
struct Driver {
    id: u32,
    personality: usize,
    rng: Rng,
    lane: i32,
    lane_init: bool,
    slack: f64,
    aim_x: f64,
    /// Signed target speed (m/s) after blocker limits and recovery.
    target: f64,
    /// Target before blocker limits (used to judge blockers).
    free_target: f64,
    pace: i32,
    reverse_left: u32,
    stuck: u32,
    off_road: u32,
    contact: u32,
    aggressive_left: u32,
    aggressive_aim: Option<[f64; 2]>,
    /// Limits of this driver's own car; the shared configuration's otherwise.
    vehicle: Option<DriverVehicle>,
}

/// Limits of one car with its acceleration table.
#[derive(Clone, Debug)]
struct DriverVehicle {
    ai: Arc<VehicleAi>,
    accel_table: Vec<i16>,
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    id: u32,
    row: usize,
    /// Lateral offset from the centre line, metres, positive right.
    x: f64,
    /// Speed along the heading (signed).
    speed_forward: f64,
    /// Speed along the driving direction of the road.
    road_speed: f64,
    progress: i32,
    human: bool,
    finished: bool,
    half_width: f64,
    position: [f64; 2],
}

/// The computer drivers of one race.
#[derive(Clone, Debug)]
pub struct AiField {
    track: Arc<AiTrack>,
    config: Arc<AiConfig>,
    settings: Settings,
    drivers: Vec<Driver>,
    row_cache: Vec<(u32, usize)>,
    accel_table: Vec<i16>,
    constants: RuleConstants,
    tick: u64,
}

fn wrap_angle(a: f64) -> f64 {
    (a + PI).rem_euclid(TAU) - PI
}

impl AiField {
    /// `ai_ids` are the car ids to control; opponent `k` (1-based, in order)
    /// uses personality `(k - 1) % len`.
    pub fn new(
        track: Arc<AiTrack>,
        config: Arc<AiConfig>,
        settings: Settings,
        ai_ids: &[u32],
    ) -> Self {
        let personalities = config.set.personalities.len().max(1);
        let drivers = ai_ids
            .iter()
            .enumerate()
            .map(|(k, id)| {
                let mut rng = Rng(settings
                    .seed
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(*id)
                    .max(1));
                rng.next();
                Driver {
                    id: *id,
                    personality: k % personalities,
                    rng,
                    lane: 7,
                    lane_init: false,
                    slack: 0.0,
                    aim_x: 0.0,
                    target: 8.0,
                    free_target: 10.0,
                    pace: ONE,
                    reverse_left: 0,
                    stuck: 0,
                    off_road: 0,
                    contact: 0,
                    aggressive_left: 0,
                    aggressive_aim: None,
                    vehicle: None,
                }
            })
            .collect();
        let c = &config.set.catch_up;
        let window = c.start_boost_segments as i32;
        let constants = RuleConstants {
            step: (c.step_segments as i32).max(1),
            far: (c.step_segments as i32).max(1) * 2,
            near_human: c.clamp_within_segments as i32,
            clamp_lo: q16::from_f64(f64::from(c.clamp_range[0])),
            clamp_hi: q16::from_f64(f64::from(c.clamp_range[1])),
            boost_window: window,
            boost_per_segment: if window > 0 {
                q16::from_f64(f64::from(c.start_boost_max) / f64::from(window))
            } else {
                0
            },
            late_after: track.late_after,
        };
        let accel_table = config.accel_table();
        Self {
            track,
            config,
            settings,
            drivers,
            row_cache: Vec::new(),
            accel_table,
            constants,
            tick: 0,
        }
    }

    /// Give controlled car `id` its own limits (top speed, acceleration,
    /// braking, grip); cars without them use the shared configuration's.
    pub fn set_vehicle(&mut self, id: u32, vehicle: VehicleAi) {
        let accel_table = crate::config::accel_table_for(&vehicle);
        if let Some(d) = self.drivers.iter_mut().find(|d| d.id == id) {
            d.vehicle = Some(DriverVehicle {
                ai: Arc::new(vehicle),
                accel_table,
            });
        }
    }

    pub fn track(&self) -> &AiTrack {
        &self.track
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Ids of the controlled cars.
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.drivers.iter().map(|d| d.id)
    }

    /// Diagnostic view of one driver's current decision.
    pub fn decision(&self, id: u32) -> Option<Decision> {
        self.drivers.iter().find(|d| d.id == id).map(|d| Decision {
            lane: d.lane,
            aim_offset: d.aim_x,
            target_speed: d.target,
            free_target_speed: d.free_target,
            pace: q16::to_f64(d.pace),
            reversing: d.reverse_left > 0,
            aggressive: d.aggressive_left > 0,
        })
    }

    /// World pose of grid position `index` of `total` racers.
    pub fn grid_slot(&self, index: usize, total: usize) -> ([f64; 2], f64) {
        self.track.grid_slot(index, total, self.settings.direction)
    }

    fn lap_class(&self) -> usize {
        match self.settings.laps {
            0..=2 => 0,
            3..=4 => 1,
            _ => 2,
        }
    }

    fn frame(&mut self, car: &CarObs) -> Frame {
        let track = &self.track;
        let hint = self
            .row_cache
            .iter()
            .find(|(id, _)| *id == car.id)
            .map(|(_, r)| *r);
        let row_index = track.locate(car.position, hint);
        match self.row_cache.iter_mut().find(|(id, _)| *id == car.id) {
            Some(slot) => slot.1 = row_index,
            None => self.row_cache.push((car.id, row_index)),
        }
        let row = &track.rows[row_index];
        let dx = car.position[0] - row.position[0];
        let dy = car.position[1] - row.position[1];
        let dir = [car.heading.cos(), car.heading.sin()];
        let forward_road = [
            row.forward[0] * f64::from(self.settings.direction),
            row.forward[1] * f64::from(self.settings.direction),
        ];
        let n = track.len() as i32;
        let along = if self.settings.direction >= 0 {
            row_index as i32
        } else {
            n - 1 - row_index as i32
        };
        Frame {
            id: car.id,
            row: row_index,
            x: dx * row.right[0] + dy * row.right[1],
            speed_forward: car.velocity[0] * dir[0] + car.velocity[1] * dir[1],
            road_speed: car.velocity[0] * forward_road[0] + car.velocity[1] * forward_road[1],
            progress: car.laps as i32 * n + along,
            human: car.human,
            finished: car.finished,
            half_width: car.half_width,
            position: car.position,
        }
    }

    /// Rows `other` is ahead of `me` in the driving direction (negative = behind).
    fn ahead(&self, me: usize, other: usize) -> i32 {
        let n = self.track.len() as i32;
        let d = if self.track.closed {
            row_delta(n, other as i32, me as i32)
        } else {
            other as i32 - me as i32
        };
        d * self.settings.direction
    }

    /// Advance one tick: returns the control of every controlled car.
    pub fn control(&mut self, cars: &[CarObs]) -> Vec<(u32, ScriptedControl)> {
        self.tick += 1;
        let frames: Vec<Frame> = cars.iter().map(|c| self.frame(c)).collect();
        let leader = frames
            .iter()
            .filter(|f| f.human)
            .max_by_key(|f| f.progress)
            .copied();
        let mut out = Vec::with_capacity(self.drivers.len());
        for di in 0..self.drivers.len() {
            let id = self.drivers[di].id;
            let Some(mi) = frames.iter().position(|f| f.id == id) else {
                continue;
            };
            if frames[mi].finished {
                out.push((id, ScriptedControl::default()));
                continue;
            }
            let every = u64::from(self.settings.decision_every.max(1));
            if (self.tick + u64::from(id)) % every == 0 || !self.drivers[di].lane_init {
                let mut d = self.drivers[di].clone();
                self.decide(&mut d, &frames, mi, leader.as_ref(), &cars[mi]);
                self.drivers[di] = d;
            }
            let control = self.servo(&self.drivers[di], &frames[mi], &cars[mi], leader.as_ref());
            out.push((id, control));
        }
        out
    }

    fn decide(
        &self,
        d: &mut Driver,
        frames: &[Frame],
        mi: usize,
        leader: Option<&Frame>,
        obs: &CarObs,
    ) {
        let track = &*self.track;
        let me = &frames[mi];
        let row = &track.rows[me.row];
        let cfg = &*self.config;
        let personality = &cfg.set.personalities[d.personality];
        let dir = self.settings.direction;
        let skill = self.settings.skill.index();

        // Lane bookkeeping: follow the car when it is far from its lane.
        let near_lane = (me.x - row.lane_centre(d.lane)).abs() < 2.2 * row.right_width();
        if !d.lane_init || !near_lane {
            d.lane = row.lane_of(me.x).clamp(FIRST_LEFT_LANE - 2, 9);
            d.lane_init = true;
            d.aim_x = me.x.clamp(-row.left_dist, row.right_dist);
        }

        // Recovery: reverse for a bounded time when stuck or off the road.
        if d.reverse_left > 0 {
            d.reverse_left -= 1;
        } else {
            let wants = d.free_target.abs() > 3.0;
            if me.speed_forward.abs() < 0.8 && wants && d.target.abs() >= 3.0 {
                d.stuck += 1;
            } else {
                d.stuck = 0;
            }
            if me.x < -(row.left_dist + 1.5) || me.x > row.right_dist + 1.5 {
                d.off_road += 1;
            } else {
                d.off_road = 0;
            }
            if d.stuck >= 8 || d.off_road >= 6 {
                d.reverse_left = 14;
                d.stuck = 0;
                d.off_road = 0;
            }
        }
        let reversing = d.reverse_left > 0;

        // Aggression: repeated contact with a human (advanced skill only).
        let touching = frames.iter().enumerate().any(|(i, f)| {
            i != mi && f.human && {
                let dx = f.position[0] - me.position[0];
                let dy = f.position[1] - me.position[1];
                dx.hypot(dy) < me.half_width + f.half_width + 0.4
            }
        });
        d.contact = if touching {
            d.contact + 1
        } else {
            d.contact.saturating_sub(1)
        };
        if self.settings.skill == Skill::Advanced {
            if let Some(a) = &personality.aggression {
                if d.contact >= a.contact_ticks && d.aggressive_left == 0 {
                    d.aggressive_left = a.duration_ticks;
                }
            }
        }
        d.aggressive_aim = None;
        if d.aggressive_left > 0 {
            d.aggressive_left -= 1;
            if let Some(h) = frames
                .iter()
                .filter(|f| f.human && !f.finished)
                .filter(|f| {
                    let ahead = self.ahead(me.row, f.row);
                    (0..=30).contains(&ahead)
                })
                .min_by_key(|f| self.ahead(me.row, f.row))
            {
                d.aggressive_aim = Some(h.position);
            }
        }

        // Lane choice: one lane per decision.
        let mut scores = self.lane_scores(d, frames, mi, leader);
        if me.road_speed.abs() > f64::from(personality.lane_change_speed_limit_mps) {
            scores[0] = i64::MIN / 4;
            scores[2] = i64::MIN / 4;
        }
        if !reversing {
            d.lane = pick_lane(d.lane, scores[0], scores[1], scores[2]);
        }
        if d.rng.next() % personality.slack_redraw_one_in.max(1) == 0 {
            d.slack = f64::from(personality.lane_slack_m[(d.rng.next() % 4) as usize]);
        }
        let mut raw_x = row.lane_centre(d.lane) + d.slack;
        if let Some(p) = d.aggressive_aim {
            let h = track.locate(p, Some(me.row));
            let r = &track.rows[h];
            raw_x = ((p[0] - r.position[0]) * r.right[0] + (p[1] - r.position[1]) * r.right[1])
                .clamp(
                    -row.left_dist + me.half_width,
                    row.right_dist - me.half_width,
                );
        }
        raw_x = raw_x.clamp(
            -row.left_dist + me.half_width,
            row.right_dist - me.half_width,
        );
        let step = 0.3;
        d.aim_x += (raw_x - d.aim_x).clamp(-step, step);

        // Target speed from the composition rules.
        let own = d.vehicle.as_ref().map(|v| Arc::clone(&v.ai));
        let vehicle = own.as_deref().unwrap_or(&cfg.vehicle);
        let rows = track.len() as i32;
        let (lo, hi, slope) = misalignment_limits(vehicle);
        let road_heading = row.heading() + if dir < 0 { PI } else { 0.0 };
        let heading_error =
            ((wrap_angle(road_heading - obs.heading)) / TAU * 1024.0).round() as i32;
        let neutral = [ONE; GLUE_LEN];
        let curve = |v: &[f32]| -> [i32; GLUE_LEN] {
            let mut out = [ONE; GLUE_LEN];
            for (o, x) in out.iter_mut().zip(v) {
                *o = q16::from_f64(f64::from(*x));
            }
            out
        };
        let speed_glue = if self.settings.catch_up && leader.is_some() {
            curve(&personality.speed_glue)
        } else {
            neutral
        };
        let human_rows: Vec<i32> = frames
            .iter()
            .filter(|f| f.human)
            .map(|f| f.row as i32)
            .collect();
        let late = q16::from_f64(track.late_pace[skill]);
        let trim = track.pace_trim.map(|(after, f)| Trim {
            after_rows: after,
            sim: q16::from_f64(f),
            alt: q16::from_f64(f),
        });
        let (leader_p, leader_speed, leader_hold) = match leader {
            Some(l) => (l.progress, l.road_speed, i32::from(l.finished)),
            None => (me.progress, 0.0, 0),
        };
        let fade_cfg = &cfg.set.catch_up.fade_in_segments[self.lap_class()];
        let lane_w = row.right_width();
        let result = target_speed(
            &self.constants,
            &TargetInput {
                direction: dir,
                rows,
                row: me.row as i32,
                progress: me.progress,
                car_type: 0,
                section_speed: track.section_speed(me.row, dir),
                car_cap: Some(q16::from_f64(f64::from(vehicle.speed_cap_mps))),
                heading_error,
                curve_hi: hi,
                curve_lo: lo,
                slope,
                handling: q16::from_f64(f64::from(personality.handling_adjust)),
                speed_glue: &speed_glue,
                g_speed: q16::from_f64(f64::from(cfg.set.catch_up.speed_scale)),
                fade: [
                    fade_cfg[0] as i32,
                    fade_cfg[1] as i32,
                    fade_cfg[2] as i32,
                    fade_cfg[3] as i32,
                ],
                leader_progress: leader_p,
                leader_speed: q16::from_f64(leader_speed),
                leader_hold,
                use_tier: true,
                tier_factor: q16::from_f64(f64::from(personality.speed_factor[skill])),
                late_factor: late,
                trim,
                sim_style: true,
                human_rows: &human_rows,
                finished: false,
                lateral: q16::from_f64(me.x),
                half_width: q16::from_f64(me.half_width),
                lanes_left: i32::from(row.left_count),
                lanes_right: i32::from(row.right_count),
                width_left: (row.left_width() * 2.0).round() as i32,
                width_right: (lane_w * 2.0).round() as i32,
            },
        );
        d.pace = result.pace.unwrap_or(ONE);
        let mut target = q16::to_f64(result.target).abs();
        d.free_target = target;

        // Blockers in the lane the car is heading for.
        let look_m = f64::from(personality.blocker_look_ahead_m);
        let mut cap = f64::INFINITY;
        let mut blocker_gap = f64::INFINITY;
        let mut blocker_lateral = 0.0;
        for (i, f) in frames.iter().enumerate() {
            if i == mi {
                continue;
            }
            let rel = self.ahead(me.row, f.row);
            let gap = f64::from(rel) * track.row_length;
            if rel <= 0 || gap > look_m {
                continue;
            }
            if (f.x - me.x).abs() < me.half_width + f.half_width + 0.3 {
                let allowed = f.road_speed.max(0.0) + 0.4 * (gap - 6.0).max(0.0);
                if allowed < cap {
                    cap = allowed;
                    blocker_gap = gap;
                    blocker_lateral = f.x;
                }
            }
        }
        if cap.is_finite() {
            if cap < 4.0
                && blocker_gap < 10.0
                && (blocker_lateral - d.aim_x).abs() > me.half_width * 2.0 + 0.2
            {
                cap = cap.max(4.0);
            }
            target = target.min(cap);
        }
        d.target = if reversing {
            -target.clamp(2.0, 5.0)
        } else if d.aggressive_left > 0 {
            target
        } else {
            target.max(if blocker_gap.is_finite() { 0.0 } else { 3.0 })
        };
        if reversing {
            d.aim_x = row
                .lane_centre(d.lane)
                .clamp(-row.left_dist + 1.0, row.right_dist - 1.0);
        }
        // Forward racing never commands a speed below the roll-back floor.
        if dir >= 0 && d.target < 0.0 && !reversing {
            d.target = 0.0;
        }
    }

    fn lane_scores(
        &self,
        d: &Driver,
        frames: &[Frame],
        mi: usize,
        leader: Option<&Frame>,
    ) -> [i64; 3] {
        let track = &*self.track;
        let me = &frames[mi];
        let row = &track.rows[me.row];
        let cfg = &*self.config;
        let w = &cfg.set.lane_weights;
        let dir = self.settings.direction;
        let hw = me.half_width;
        let look_rows = ((2.0 * me.road_speed.abs()) / track.row_length)
            .ceil()
            .clamp(2.0, 40.0) as usize;
        let mut out = [0i64; 3];
        for (k, offset) in [-1i32, 0, 1].into_iter().enumerate() {
            let cand = d.lane + offset;
            if !(0..14).contains(&cand) {
                out[k] = -1_000_000;
                continue;
            }
            let mut pen = 0.0f64;
            if !row.has_lane(cand) {
                pen += f64::from(w.absent_now);
            } else {
                let mut r = 2;
                while r <= look_rows {
                    if !track.rows[track.step(me.row, dir, r)].has_lane(cand) {
                        pen += f64::from(w.absent_ahead);
                        break;
                    }
                    r += 2;
                }
                if !track.rows[track.step(me.row, dir, 3)].has_lane(cand) {
                    pen += f64::from(w.absent_near);
                }
                if row.is_shoulder(cand) {
                    pen += f64::from(w.shoulder);
                }
                let c = row.lane_centre(cand);
                let over_edge = if c < 0.0 {
                    -c + hw > row.left_dist + 0.01
                } else {
                    c + hw > row.right_dist + 0.01
                };
                if over_edge {
                    pen += f64::from(w.edge);
                }
                if let Some(h) = track.rows[track.step(me.row, dir, 4)].hint {
                    if i32::from(h) == cand {
                        pen -= f64::from(w.hint);
                    }
                }
            }
            let c = row.lane_centre(cand);
            for (i, f) in frames.iter().enumerate() {
                if i == mi {
                    continue;
                }
                let rel = self.ahead(me.row, f.row);
                let gap = f64::from(rel) * track.row_length;
                let lateral = (f.x - c).abs();
                if offset != 0 && rel.abs() <= 2 && lateral < hw + f.half_width + 0.6 {
                    pen += f64::from(w.adjacent_car);
                }
                let look_m = f64::from(cfg.set.personalities[d.personality].blocker_look_ahead_m);
                if rel > 0 && gap <= look_m && lateral < hw + f.half_width + 0.3 {
                    let cap = f.road_speed.max(0.0) + 0.4 * (gap - 6.0).max(0.0);
                    let wanted = d.free_target.max(me.road_speed);
                    pen += f64::from(w.blocker_speed) * (wanted - cap).max(0.0);
                    pen += f64::from(w.obstacle)
                        * (me.road_speed - f.road_speed).max(0.0)
                        * if f.road_speed < 2.0 { 1.0 } else { 0.0 };
                }
                if self.settings.skill == Skill::Advanced
                    && f.human
                    && rel < 0
                    && gap >= -14.0
                    && lateral < hw + f.half_width + 0.3
                    && leader.is_some()
                {
                    pen -= f64::from(w.block_human);
                }
            }
            out[k] = -(pen.round() as i64);
        }
        out
    }

    fn servo(
        &self,
        d: &Driver,
        me: &Frame,
        obs: &CarObs,
        leader: Option<&Frame>,
    ) -> ScriptedControl {
        let track = &*self.track;
        let cfg = &*self.config;
        let vehicle = d.vehicle.as_ref().map_or(&cfg.vehicle, |v| v.ai.as_ref());
        let personality = &cfg.set.personalities[d.personality];
        let dir = self.settings.direction;
        let dt = self.settings.tick_seconds;
        let heading_dir = [obs.heading.cos(), obs.heading.sin()];
        let left = [-heading_dir[1], heading_dir[0]];
        let v_forward = me.speed_forward;
        let v_left = obs.velocity[0] * left[0] + obs.velocity[1] * left[1];
        let speed = (obs.velocity[0].hypot(obs.velocity[1])).abs();
        let reversing = d.reverse_left > 0;

        // Longitudinal: full limit toward the target, no overshoot in one step.
        let target = d.target;
        let mut accel_glue = [ONE; GLUE_LEN];
        if leader.is_some() && self.settings.catch_up {
            for (o, x) in accel_glue.iter_mut().zip(&personality.accel_glue) {
                *o = q16::from_f64(f64::from(*x));
            }
        }
        let (lead_p, lead_speed, lead_hold) = match leader {
            Some(l) => (l.progress, l.road_speed, i32::from(l.finished)),
            None => (me.progress, 0.0, 0),
        };
        let forward_accel = if target == 0.0 {
            0.0
        } else if target <= v_forward {
            // The car cannot stop harder than its tyres and brakes allow.
            let limit =
                q16::to_f64(brake_limit(true, false, d.pace)).min(f64::from(vehicle.brake_mps2));
            ((target - v_forward) / dt).max(-limit)
        } else {
            let limit = q16::to_f64(accel_limit(
                &self.constants,
                &AccelInput {
                    speed: q16::from_f64(v_forward.abs()),
                    shift_timer: 0,
                    car_cap: Some(q16::from_f64(f64::from(vehicle.speed_cap_mps))),
                    tier_skill_factor: q16::from_f64(f64::from(
                        personality.speed_factor[self.settings.skill.index()],
                    )),
                    pace: d.pace,
                    accel_table: d
                        .vehicle
                        .as_ref()
                        .map_or(&self.accel_table, |v| &v.accel_table),
                    is_racer: true,
                    catchup_option: self.settings.catch_up,
                    leader_hold: lead_hold,
                    progress: me.progress,
                    leader_progress: lead_p,
                    direction: dir,
                    leader_speed: q16::from_f64(lead_speed),
                    accel_glue: &accel_glue,
                    single_race_factor: None,
                    beginner: self.settings.skill == Skill::Beginner,
                    aggressive: d.aggressive_left > 0,
                },
            ));
            ((target - v_forward) / dt).min(limit)
        };

        // Steering: P-cascade toward the look-ahead point on the aim line.
        let look_rows = ((f64::from(personality.steer_look_ahead_per_mps) * speed).trunc() as i64)
            .clamp(
                i64::from(personality.steer_look_ahead_min_rows),
                i64::from(personality.steer_look_ahead_max_rows),
            )
            .max(1) as usize;
        let row_here = &track.rows[me.row];
        let aim = d.aggressive_aim.unwrap_or_else(|| {
            let ahead = track.step(me.row, dir, look_rows);
            track.point(ahead, d.aim_x)
        });
        let desired = (aim[1] - obs.position[1]).atan2(aim[0] - obs.position[0]);
        let h = wrap_angle(desired - obs.heading);
        let recovery = reversing || h.abs() > 116.0 / 1024.0 * TAU;
        let gains = if recovery {
            &cfg.set.gains.recovery
        } else {
            &cfg.set.gains.normal
        };
        let low = f64::from(gains.low_speed_mps).max(0.1);
        let blend = (speed / low).clamp(0.0, 1.0);
        let yaw_factor = f64::from(gains.low_speed_yaw_factor)
            + (1.0 - f64::from(gains.low_speed_yaw_factor)) * blend;
        let lat_factor = f64::from(gains.low_speed_lateral_factor)
            + (1.0 - f64::from(gains.low_speed_lateral_factor)) * blend;
        let w_target = (f64::from(gains.heading_to_yaw_rate) * h).clamp(
            -f64::from(gains.max_yaw_rate_rad_s),
            f64::from(gains.max_yaw_rate_rad_s),
        ) * yaw_factor;
        let yaw_cap = f64::from(vehicle.yaw_accel_cap_rad_s2);
        let yaw_accel = (f64::from(gains.yaw_rate_to_accel) * (w_target - obs.yaw_rate))
            .clamp(-yaw_cap, yaw_cap);

        let lateral_error = d.aim_x - me.x;
        let v_road = (lateral_error * f64::from(gains.lateral_pos_to_vel)).clamp(
            -f64::from(gains.max_lateral_vel_mps),
            f64::from(gains.max_lateral_vel_mps),
        ) * lat_factor;
        let target_world = [row_here.right[0] * v_road, row_here.right[1] * v_road];
        let v_left_target = target_world[0] * left[0] + target_world[1] * left[1];
        let scale = 1.0 + 0.0015 * v_forward.abs();
        let lat_cap = f64::from(vehicle.lateral_accel_cap_mps2) * scale;
        let lateral_accel =
            (f64::from(gains.lateral_vel_to_accel) * scale * (v_left_target - v_left))
                .clamp(-lat_cap, lat_cap);

        ScriptedControl {
            forward_accel,
            lateral_accel,
            yaw_accel,
        }
    }
}

fn misalignment_limits(v: &racer_pack_schema::ai::VehicleAi) -> (i32, i32, i32) {
    let m = v
        .misalignment
        .clone()
        .unwrap_or(racer_pack_schema::ai::Misalignment {
            start_deg: 8.0,
            full_deg: 40.0,
            max_slowdown: 0.0,
        });
    let units = |deg: f32| (f64::from(deg) / 360.0 * 1024.0).round() as i32;
    let lo = units(m.start_deg);
    let hi = units(m.full_deg).max(lo + 1);
    let slope = q16::from_f64(f64::from(m.max_slowdown) / f64::from(hi - lo));
    (lo, hi, slope)
}
