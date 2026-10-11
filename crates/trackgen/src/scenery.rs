//! Scenery placement: where trees, rocks, signs, stands and buildings go.

use crate::math::{Rng, V3, atan2, fbm2, v3};
use crate::props::ProtoIds;
use crate::road::Road;
use crate::spec::Theme;
use crate::style::Style;
use crate::world::{World, edge_lat};

#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub proto: u32,
    pub pos: V3,
    /// Rotation about +Y; local +Z maps to (sin yaw, 0, cos yaw).
    pub yaw: f64,
    pub scale: f64,
}

struct Ctx<'a> {
    road: &'a Road,
    world: &'a World<'a>,
    style: &'a Style,
    seed: u64,
}

struct Spot {
    pos: V3,
    normal_y: f64,
    /// Distance beyond the barrier line.
    outside: f64,
}

impl Ctx<'_> {
    /// Random ground spot beside section `i` on `side`, `d` metres beyond the barrier.
    fn spot(&self, i: usize, side: f64, lo: f64, hi: f64, rng: &mut Rng) -> Option<Spot> {
        let sec = &self.road.secs[i];
        let edge = edge_lat(self.style, sec);
        let d = lo + (hi - lo) * rng.f() * rng.f().max(0.15).sqrt();
        let rh = v3(sec.right.x, 0.0, sec.right.z).norm();
        let fh = v3(sec.fwd.x, 0.0, sec.fwd.z).norm();
        let jit = rng.range(-5.0, 5.0);
        let lat = side * (edge + d);
        let x = sec.pos.x + rh.x * lat + fh.x * jit;
        let z = sec.pos.z + rh.z * lat + fh.z * jit;
        let outside = self.world.outside_distance(x, z)?;
        if outside < 3.5 {
            return None;
        }
        let y = self.world.height(x, z);
        if let Some(sea) = self.style.sea_level {
            if y < sea + 0.35 {
                return None;
            }
        }
        let normal_y = self.world.normal(x, z, 3.0).y;
        Some(Spot {
            pos: v3(x, y, z),
            normal_y,
            outside,
        })
    }

    fn cluster(&self, p: V3, scale: f64) -> f64 {
        fbm2(self.seed ^ 0xC1u64, p.x / scale, p.z / scale, 2, 0, 0)
    }
}

fn pick(ids: &[u32], rng: &mut Rng) -> Option<u32> {
    if ids.is_empty() {
        None
    } else {
        Some(ids[rng.below(ids.len() as u64) as usize])
    }
}

pub fn place(
    road: &Road,
    world: &World,
    style: &Style,
    ids: &ProtoIds,
    seed: u64,
    with_start: bool,
    keep_clear: &[(f64, f64, f64)],
) -> Vec<Placement> {
    let ctx = Ctx {
        road,
        world,
        style,
        seed,
    };
    let mut rng = Rng::new(seed ^ 0x5CE9_E001);
    let mut out: Vec<Placement> = Vec::new();
    let n = road.len();
    let near_start = |i: usize| {
        let d = i.min(n - i) as f64 * road.ds;
        with_start && d < 70.0
    };

    // Start/finish gantry across the road at s = 0 and grandstands beside it.
    if with_start {
        let sec = &road.secs[0];
        out.push(Placement {
            proto: ids.gantry,
            pos: sec.pos,
            yaw: atan2(sec.fwd.x, sec.fwd.z),
            scale: 1.0,
        });
        let rh = v3(sec.right.x, 0.0, sec.right.z).norm();
        let fh = v3(sec.fwd.x, 0.0, sec.fwd.z).norm();
        for side in [-1.0, 1.0] {
            let lat = side * (edge_lat(style, sec) + 11.0);
            let (x, z) = (
                sec.pos.x + rh.x * lat + fh.x * 24.0,
                sec.pos.z + rh.z * lat + fh.z * 24.0,
            );
            let toward = v3(-side * rh.x, 0.0, -side * rh.z);
            out.push(Placement {
                proto: ids.stand,
                pos: v3(x, world.height(x, z).max(sec.pos.y - 2.0), z),
                yaw: atan2(toward.x, toward.z),
                scale: 1.0,
            });
        }
    }

    // Chevron signs on the outside of tight corners.
    let mut last_sign = [-1000.0f64; 2];
    for i in (0..n).step_by(3) {
        let sec = &road.secs[i];
        let k = sec.kappa;
        if k.abs() < 1.0 / 130.0 || near_start(i) {
            continue;
        }
        let side_idx = usize::from(k > 0.0);
        if sec.s - last_sign[side_idx] < 14.0 && sec.s > last_sign[side_idx] {
            continue;
        }
        last_sign[side_idx] = sec.s;
        // outside of a right turn (k > 0) is the left side (-1)
        let side = if k > 0.0 { -1.0 } else { 1.0 };
        let rh = v3(sec.right.x, 0.0, sec.right.z).norm();
        let lat = side * (edge_lat(style, sec) + 2.2);
        let (x, z) = (sec.pos.x + rh.x * lat, sec.pos.z + rh.z * lat);
        let toward = v3(-side * rh.x, 0.0, -side * rh.z);
        let proto = if k > 0.0 {
            ids.chevron_right
        } else {
            ids.chevron_left
        };
        out.push(Placement {
            proto,
            pos: v3(x, world.height(x, z), z),
            yaw: atan2(toward.x, toward.z),
            scale: 1.0,
        });
    }

    // Ground circles (x, z, radius) that props must keep clear of.
    let mut clear: Vec<(f64, f64, f64)> = keep_clear.to_vec();
    let blocked = |clear: &[(f64, f64, f64)], x: f64, z: f64, extra: f64| {
        clear.iter().any(|&(cx, cz, r)| {
            (x - cx) * (x - cx) + (z - cz) * (z - cz) < (r + extra) * (r + extra)
        })
    };
    if style.theme != Theme::NightHarbour {
        // rock cuttings where the hillside rises beside the road
        for i in (0..n).step_by(5) {
            if near_start(i) {
                continue;
            }
            for side in [-1.0, 1.0] {
                if rng.f() > 0.75 {
                    continue;
                }
                let sec = &road.secs[i];
                let edge = edge_lat(style, sec);
                let rh = v3(sec.right.x, 0.0, sec.right.z).norm();
                let at = |d: f64| {
                    let lat = side * (edge + d);
                    (sec.pos.x + rh.x * lat, sec.pos.z + rh.z * lat)
                };
                let (x, z) = at(9.0);
                let (fx, fz) = at(30.0);
                let rise = world.height(fx, fz) - sec.pos.y;
                if rise < 5.0 || blocked(&clear, x, z, 6.0) {
                    continue;
                }
                if world.outside_distance(x, z).is_none_or(|o| o < 3.5) {
                    continue;
                }
                let y = world.height(x, z);
                if y < sec.pos.y - 3.0 {
                    continue;
                }
                let toward = v3(-side * rh.x, 0.0, -side * rh.z);
                if let Some(proto) = pick(&ids.cliff, &mut rng) {
                    out.push(Placement {
                        proto,
                        pos: v3(x, y - 0.5, z),
                        yaw: atan2(toward.x, toward.z) + rng.range(-0.12, 0.12),
                        scale: rng.range(0.85, 1.5),
                    });
                    clear.push((x, z, 10.0));
                }
            }
        }
        // warning signs shortly before each hard corner
        let mut last_sign = f64::MIN;
        for i in 0..n {
            let sec = &road.secs[i];
            let prev = &road.secs[(i + n - 1) % n];
            let entering = sec.kappa.abs() >= 1.0 / 105.0 && prev.kappa.abs() < 1.0 / 105.0;
            if !entering || sec.s - last_sign < 160.0 || sec.s < 160.0 {
                continue;
            }
            let at_i = (i + n - 12) % n;
            let base = &road.secs[at_i];
            if near_start(at_i) {
                continue;
            }
            let rh = v3(base.right.x, 0.0, base.right.z).norm();
            let fh = v3(base.fwd.x, 0.0, base.fwd.z).norm();
            let lat = edge_lat(style, base) + 2.4;
            let (x, z) = (base.pos.x + rh.x * lat, base.pos.z + rh.z * lat);
            if let Some(proto) = ids.sign.get(if sec.kappa > 0.0 { 0 } else { 2 }) {
                out.push(Placement {
                    proto: *proto,
                    pos: v3(x, world.height(x, z), z),
                    yaw: atan2(-fh.x, -fh.z),
                    scale: 1.15,
                });
                last_sign = sec.s;
            }
        }
    }

    let step = 2usize;
    match style.theme {
        Theme::PineRidge => {
            const TREE_LINE: f64 = 80.0;
            // log cabins on level ground, well apart from each other
            let mut cabins: Vec<(f64, f64)> = Vec::new();
            let mut tries = 0;
            while cabins.len() < 18 && tries < 900 {
                tries += 1;
                let i = rng.below(n as u64) as usize;
                let side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
                if near_start(i) {
                    continue;
                }
                let Some(spot) = ctx.spot(i, side, 22.0, 90.0, &mut rng) else {
                    continue;
                };
                if spot.normal_y < 0.94
                    || spot.pos.y > 66.0
                    || blocked(&clear, spot.pos.x, spot.pos.z, 8.0)
                    || cabins.iter().any(|&(cx, cz)| {
                        (cx - spot.pos.x).powi(2) + (cz - spot.pos.z).powi(2) < 85.0 * 85.0
                    })
                {
                    continue;
                }
                let sec = &road.secs[i];
                let toward = v3(sec.pos.x - spot.pos.x, 0.0, sec.pos.z - spot.pos.z);
                if let Some(proto) = pick(&ids.cabin, &mut rng) {
                    out.push(Placement {
                        proto,
                        pos: spot.pos,
                        yaw: atan2(toward.x, toward.z),
                        scale: rng.range(1.0, 1.3),
                    });
                    cabins.push((spot.pos.x, spot.pos.z));
                    clear.push((spot.pos.x, spot.pos.z, 15.0));
                }
            }
            // forest: full trees near the road, cheap pines out to the ridges
            for i in (0..n).step_by(step) {
                if near_start(i) {
                    continue;
                }
                for side in [-1.0, 1.0] {
                    for _ in 0..3 {
                        let Some(s) = ctx.spot(i, side, 5.0, 62.0, &mut rng) else {
                            continue;
                        };
                        if s.normal_y < 0.66
                            || s.pos.y > TREE_LINE
                            || ctx.cluster(s.pos, 110.0) < 0.12
                            || blocked(&clear, s.pos.x, s.pos.z, 0.0)
                        {
                            continue;
                        }
                        let roll = rng.f();
                        let (proto, sc) = if roll < 0.72 {
                            (pick(&ids.pine, &mut rng), rng.range(0.9, 1.7))
                        } else if roll < 0.86 {
                            (pick(&ids.broad, &mut rng), rng.range(0.9, 1.4))
                        } else {
                            (pick(&ids.bush, &mut rng), rng.range(0.8, 1.6))
                        };
                        if let Some(proto) = proto {
                            out.push(Placement {
                                proto,
                                pos: s.pos,
                                yaw: rng.range(0.0, std::f64::consts::TAU),
                                scale: sc,
                            });
                        }
                    }
                    for _ in 0..2 {
                        let Some(s) = ctx.spot(i, side, 58.0, 270.0, &mut rng) else {
                            continue;
                        };
                        if s.normal_y < 0.6
                            || s.pos.y > TREE_LINE - 6.0 * ctx.cluster(s.pos, 40.0)
                            || ctx.cluster(s.pos, 160.0) < 0.2
                            || blocked(&clear, s.pos.x, s.pos.z, 0.0)
                        {
                            continue;
                        }
                        if let Some(proto) = pick(&ids.far_pine, &mut rng) {
                            out.push(Placement {
                                proto,
                                pos: s.pos,
                                yaw: rng.range(0.0, std::f64::consts::TAU),
                                scale: rng.range(1.1, 2.2),
                            });
                        }
                    }
                    if rng.f() < 0.22 {
                        if let Some(s) = ctx.spot(i, side, 2.5, 35.0, &mut rng) {
                            if blocked(&clear, s.pos.x, s.pos.z, 0.0) {
                                continue;
                            }
                            if let Some(proto) = pick(&ids.boulder, &mut rng) {
                                out.push(Placement {
                                    proto,
                                    pos: s.pos,
                                    yaw: rng.range(0.0, std::f64::consts::TAU),
                                    scale: rng.range(0.6, 2.2),
                                });
                            }
                        }
                    }
                }
            }
        }
        Theme::Coastline => {
            for i in (0..n).step_by(step) {
                for side in [-1.0, 1.0] {
                    if !near_start(i) {
                        for _ in 0..4 {
                            let Some(s) = ctx.spot(i, side, 5.0, 70.0, &mut rng) else {
                                continue;
                            };
                            if s.normal_y < 0.8
                                || ctx.cluster(s.pos, 70.0) < 0.28
                                || blocked(&clear, s.pos.x, s.pos.z, 0.0)
                            {
                                continue;
                            }
                            let roll = rng.f();
                            let proto = if s.pos.y < 6.0 && roll < 0.6 {
                                pick(&ids.palm, &mut rng)
                            } else if roll < 0.8 {
                                pick(&ids.pine, &mut rng)
                            } else {
                                pick(&ids.bush, &mut rng)
                            };
                            if let Some(proto) = proto {
                                out.push(Placement {
                                    proto,
                                    pos: s.pos,
                                    yaw: rng.range(0.0, std::f64::consts::TAU),
                                    scale: rng.range(0.8, 1.4),
                                });
                            }
                        }
                    }
                    if rng.f() < 0.18 {
                        if let Some(s) = ctx.spot(i, side, 2.5, 40.0, &mut rng) {
                            if let Some(proto) = pick(&ids.boulder, &mut rng) {
                                out.push(Placement {
                                    proto,
                                    pos: s.pos,
                                    yaw: rng.range(0.0, std::f64::consts::TAU),
                                    scale: rng.range(0.6, 2.0),
                                });
                            }
                        }
                    }
                    if rng.f() < 0.045 && !near_start(i) {
                        if let Some(s) = ctx.spot(i, side, 28.0, 120.0, &mut rng) {
                            if s.normal_y > 0.9 && s.pos.y > 2.0 {
                                if let Some(proto) = pick(&ids.house, &mut rng) {
                                    let sec = &road.secs[i];
                                    let toward = v3(sec.pos.x - s.pos.x, 0.0, sec.pos.z - s.pos.z);
                                    out.push(Placement {
                                        proto,
                                        pos: s.pos,
                                        yaw: atan2(toward.x, toward.z),
                                        scale: rng.range(1.0, 1.4),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            // lighthouse on the seaward side of the road's most easterly point
            let (mut best, mut bx) = (0usize, f64::MIN);
            for (i, s) in road.secs.iter().enumerate() {
                if s.pos.x > bx {
                    bx = s.pos.x;
                    best = i;
                }
            }
            for side in [1.0, -1.0] {
                for d in [60.0, 90.0, 130.0] {
                    let sec = &road.secs[best];
                    let rh = v3(sec.right.x, 0.0, sec.right.z).norm();
                    let lat = side * (edge_lat(style, sec) + d);
                    let (x, z) = (sec.pos.x + rh.x * lat, sec.pos.z + rh.z * lat);
                    if world.outside_distance(x, z).is_some_and(|o| o > 30.0)
                        && world.height(x, z) > 1.0
                        && world.normal(x, z, 3.0).y > 0.9
                    {
                        out.push(Placement {
                            proto: ids.lighthouse,
                            pos: v3(x, world.height(x, z), z),
                            yaw: 0.0,
                            scale: 1.0,
                        });
                        break;
                    }
                }
                if out.iter().any(|p| p.proto == ids.lighthouse) {
                    break;
                }
            }
        }
        Theme::RedMesa => {
            for i in (0..n).step_by(step) {
                if near_start(i) {
                    continue;
                }
                for side in [-1.0, 1.0] {
                    for _ in 0..3 {
                        let Some(s) = ctx.spot(i, side, 6.0, 130.0, &mut rng) else {
                            continue;
                        };
                        if s.normal_y < 0.8
                            || ctx.cluster(s.pos, 120.0) < 0.30
                            || blocked(&clear, s.pos.x, s.pos.z, 0.0)
                        {
                            continue;
                        }
                        let roll = rng.f();
                        let (proto, sc) = if roll < 0.25 {
                            (pick(&ids.cactus, &mut rng), rng.range(0.8, 1.4))
                        } else {
                            (pick(&ids.bush, &mut rng), rng.range(0.6, 1.2))
                        };
                        if let Some(proto) = proto {
                            out.push(Placement {
                                proto,
                                pos: s.pos,
                                yaw: rng.range(0.0, std::f64::consts::TAU),
                                scale: sc,
                            });
                        }
                    }
                    if rng.f() < 0.38 {
                        if let Some(s) = ctx.spot(i, side, 3.0, 70.0, &mut rng) {
                            if let Some(proto) = pick(&ids.boulder, &mut rng) {
                                out.push(Placement {
                                    proto,
                                    pos: s.pos,
                                    yaw: rng.range(0.0, std::f64::consts::TAU),
                                    scale: rng.range(0.7, 2.6),
                                });
                            }
                        }
                    }
                }
            }
            // distant mesas and spires
            let mut placed = 0;
            let mut tries = 0;
            while placed < 34 && tries < 400 {
                tries += 1;
                let i = rng.below(n as u64) as usize;
                let side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
                if let Some(s) = ctx.spot(i, side, 90.0, 380.0, &mut rng) {
                    if s.outside < 80.0 {
                        continue;
                    }
                    if let Some(proto) = pick(&ids.mesa, &mut rng) {
                        out.push(Placement {
                            proto,
                            pos: s.pos,
                            yaw: rng.range(0.0, std::f64::consts::TAU),
                            scale: rng.range(0.7, 2.0),
                        });
                        placed += 1;
                    }
                }
            }
        }
        Theme::NightHarbour => {
            for i in (0..n).step_by(5) {
                let sec = &road.secs[i];
                for side in [-1.0, 1.0] {
                    let rh = v3(sec.right.x, 0.0, sec.right.z).norm();
                    let lat = side * (edge_lat(style, sec) + 1.4);
                    let (x, z) = (sec.pos.x + rh.x * lat, sec.pos.z + rh.z * lat);
                    if world.outside_distance(x, z).is_some_and(|o| o < 1.5) {
                        // another road part is too close; skip
                    }
                    let toward = v3(-side * rh.x, 0.0, -side * rh.z);
                    let y = world.height(x, z);
                    if style.sea_level.is_none_or(|sea| y > sea + 0.5) {
                        out.push(Placement {
                            proto: ids.lamp,
                            pos: v3(x, y, z),
                            yaw: atan2(toward.x, toward.z),
                            scale: 1.0,
                        });
                    }
                }
            }
            for i in (0..n).step_by(step) {
                if near_start(i) {
                    continue;
                }
                for side in [-1.0, 1.0] {
                    if rng.f() < 0.12 {
                        if let Some(s) = ctx.spot(i, side, 8.0, 55.0, &mut rng) {
                            if s.pos.y > -0.5 && s.normal_y > 0.95 {
                                if let Some(proto) = pick(&ids.containers, &mut rng) {
                                    let sec = &road.secs[i];
                                    let along = v3(sec.fwd.x, 0.0, sec.fwd.z);
                                    out.push(Placement {
                                        proto,
                                        pos: s.pos,
                                        yaw: atan2(along.x, along.z),
                                        scale: 1.0,
                                    });
                                }
                            }
                        }
                    }
                    if rng.f() < 0.03 {
                        if let Some(s) = ctx.spot(i, side, 35.0, 120.0, &mut rng) {
                            if s.pos.y > -0.5 && s.normal_y > 0.95 {
                                let sec = &road.secs[i];
                                let along = v3(sec.fwd.x, 0.0, sec.fwd.z);
                                let proto = if rng.f() < 0.65 {
                                    ids.warehouse
                                } else {
                                    ids.tank
                                };
                                out.push(Placement {
                                    proto,
                                    pos: s.pos,
                                    yaw: atan2(along.x, along.z),
                                    scale: 1.0,
                                });
                            }
                        }
                    }
                    if rng.f() < 0.02 {
                        if let Some(s) = ctx.spot(i, side, 5.0, 40.0, &mut rng) {
                            let far = world.height(s.pos.x + 30.0, s.pos.z) < -1.0
                                || world.height(s.pos.x - 30.0, s.pos.z) < -1.0
                                || world.height(s.pos.x, s.pos.z + 30.0) < -1.0
                                || world.height(s.pos.x, s.pos.z - 30.0) < -1.0;
                            if far && s.pos.y > -0.5 {
                                let sec = &road.secs[i];
                                let along = v3(sec.fwd.x, 0.0, sec.fwd.z);
                                out.push(Placement {
                                    proto: ids.crane,
                                    pos: s.pos,
                                    yaw: atan2(along.x, along.z),
                                    scale: 1.0,
                                });
                            }
                        }
                    }
                    if rng.f() < 0.04 {
                        if let Some(s) = ctx.spot(i, side, 70.0, 300.0, &mut rng) {
                            if s.pos.y > -0.5 && s.normal_y > 0.95 {
                                if let Some(proto) = pick(&ids.building, &mut rng) {
                                    let sec = &road.secs[i];
                                    let toward = v3(sec.pos.x - s.pos.x, 0.0, sec.pos.z - s.pos.z);
                                    out.push(Placement {
                                        proto,
                                        pos: s.pos,
                                        yaw: atan2(toward.x, toward.z),
                                        scale: rng.range(0.9, 1.4),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}
