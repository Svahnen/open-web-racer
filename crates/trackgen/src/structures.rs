//! Large road structures built straight from the road frames: a tunnel through
//! a rocky mound and a stone overpass. Both are emitted as single prop meshes
//! in world coordinates (placed with an identity transform), so they never take
//! part in the road/ground surface.

use crate::materials::Mats;
use crate::math::{V3, cos, sin, smoothstep, v3};
use crate::mesh::MeshData;
use crate::road::Road;
use crate::spec::Theme;
use crate::style::Style;
use crate::world::{World, edge_lat};
use std::f64::consts::PI;

/// Tunnel length in road sections.
const TUNNEL_SECTIONS: usize = 36;
/// Sections of straight road required around the overpass.
const BRIDGE_SECTIONS: usize = 4;
const ARCH_SEGMENTS: usize = 12;
/// Height of the vertical tunnel walls and of the crown above the road centre.
const WALL_TOP: f64 = 4.4;
const CROWN: f64 = 9.4;
/// Height of the overpass deck above the road centre.
const DECK_HEIGHT: f64 = 9.8;
const RAMP_LENGTH: f64 = 38.0;
const DECK_HALF_WIDTH: f64 = 4.2;

#[derive(Default)]
pub struct Structures {
    pub meshes: Vec<(String, MeshData)>,
    /// Ground circles (x, z, radius) that scenery must keep clear.
    pub keep_clear: Vec<(f64, f64, f64)>,
    /// Section ranges covered by structures (first, last), inclusive.
    pub spans: Vec<(usize, usize)>,
}

fn horizontal(v: V3) -> V3 {
    v3(v.x, 0.0, v.z).norm()
}

/// Smooth-ish sin(t)^0.75 using only sqrt, so output is platform independent.
fn blunt(s: f64) -> f64 {
    let s = s.max(0.0);
    let r = s.sqrt();
    r * r.sqrt()
}

/// First window of `len` sections whose curvature stays below `limit`,
/// keeping `margin` sections away from the start line and every taken span.
fn find_window(
    road: &Road,
    shift: usize,
    len: usize,
    limit: f64,
    margin: usize,
    taken: &[(usize, usize)],
) -> Option<usize> {
    let n = road.len();
    let mut best: Option<(f64, usize)> = None;
    // `shift` moves the start line along the lap for scenic variants; windows
    // are measured from the designed start so every variant shows the same
    // structures in the same places.
    for a in 0..n.saturating_sub(len) {
        let origin = (a + shift) % n;
        if origin < margin || origin + len + margin > n {
            continue;
        }
        let b = a + len;
        if taken
            .iter()
            .any(|&(t0, t1)| a < t1 + margin / 2 && b + margin / 2 > t0)
        {
            continue;
        }
        let mut worst = 0.0f64;
        let mut sum = 0.0;
        let mut climb = 0.0f64;
        for j in a..=b {
            let k = road.secs[j].kappa.abs();
            worst = worst.max(k);
            sum += k;
            climb = climb.max((road.secs[j].pos.y - road.secs[a].pos.y).abs());
        }
        if worst > limit || road.secs[a..=b].iter().any(|s| s.bank.abs() > 0.05) {
            continue;
        }
        let score = sum + climb * 1.0e-4;
        if best.is_none_or(|(bs, _)| score < bs - 1e-9) {
            best = Some((score, a));
        }
    }
    best.map(|(_, a)| a)
}

pub fn build(road: &Road, shift: usize, world: &World, style: &Style, mats: &Mats) -> Structures {
    let mut out = Structures::default();
    if style.theme == Theme::NightHarbour {
        return out;
    }
    // Tunnel through a rocky mound: mountain road and desert plateau only.
    let tunnel_window = matches!(style.theme, Theme::PineRidge | Theme::RedMesa)
        .then(|| {
            // winding desert roads only offer sweepers, so allow a gentler curve there
            let limit = if style.theme == Theme::RedMesa {
                1.0 / 320.0
            } else {
                1.0 / 900.0
            };
            find_window(road, shift, TUNNEL_SECTIONS, limit, 160, &out.spans)
        })
        .flatten();
    if let Some(a) = tunnel_window {
        let b = a + TUNNEL_SECTIONS;
        let mesh = tunnel(road, world, style, mats, a, b);
        out.meshes.push(("tunnel".into(), mesh));
        out.spans.push((a, b));
        for j in (a..=b).step_by(3) {
            let p = road.secs[j].pos;
            out.keep_clear.push((p.x, p.z, 52.0));
        }
    }
    if let Some(a) = find_window(road, shift, BRIDGE_SECTIONS, 1.0 / 700.0, 140, &out.spans) {
        let mesh = overpass(road, world, style, mats, a + BRIDGE_SECTIONS / 2);
        out.meshes.push(("overpass".into(), mesh));
        out.spans.push((a, a + BRIDGE_SECTIONS));
        let p = road.secs[a + BRIDGE_SECTIONS / 2].pos;
        let r = horizontal(road.secs[a + BRIDGE_SECTIONS / 2].right);
        for d in [-60.0, -30.0, 0.0, 30.0, 60.0] {
            out.keep_clear.push((p.x + r.x * d, p.z + r.z * d, 26.0));
        }
    }
    out
}

/// The walls and arch of the tunnel ring at one lateral half-width `w`, from
/// the buried left foot over the crown to the buried right foot.
fn ring(w: f64, wall_top: f64, crown: f64, foot: f64) -> Vec<(f64, f64)> {
    let mut pts = vec![(-w, foot)];
    for k in 0..=ARCH_SEGMENTS {
        let th = PI - PI * k as f64 / ARCH_SEGMENTS as f64;
        pts.push((w * cos(th), wall_top + (crown - wall_top) * sin(th)));
    }
    pts.push((w, foot));
    pts
}

fn tunnel(road: &Road, world: &World, style: &Style, mats: &Mats, a: usize, b: usize) -> MeshData {
    let mut m = MeshData::new();
    let span = (b - a) as f64;
    let at = |j: usize, lat: f64, h: f64| {
        let s = &road.secs[j];
        let r = horizontal(s.right);
        s.pos + r * lat + v3(0.0, h, 0.0)
    };
    let inner_w = |j: usize| edge_lat(style, &road.secs[j]) + 0.9;
    // inner surface
    for j in a..b {
        let (w0, w1) = (inner_w(j), inner_w(j + 1));
        let (r0, r1) = (
            ring(w0, WALL_TOP, CROWN, -1.4),
            ring(w1, WALL_TOP, CROWN, -1.4),
        );
        for k in 0..r0.len() - 1 {
            let p = [
                at(j, r0[k].0, r0[k].1),
                at(j, r0[k + 1].0, r0[k + 1].1),
                at(j + 1, r1[k + 1].0, r1[k + 1].1),
                at(j + 1, r1[k].0, r1[k].1),
            ];
            let axis = at(j, 0.0, 3.0);
            let inward = axis - (p[0] + p[2]) * 0.5;
            let u = j as f64 * road.ds / 6.0;
            let (v0, v1) = (k as f64 * 0.8, (k + 1) as f64 * 0.8);
            m.flat_quad(
                p,
                [
                    [u, v0],
                    [u, v1],
                    [u + road.ds / 6.0, v1],
                    [u + road.ds / 6.0, v0],
                ],
                mats.tunnel_wall,
                0,
                Some(inward),
            );
        }
        // ceiling lamps: a glowing strip on the crown every third section
        if (j - a) % 3 == 1 {
            let lamp = [
                at(j, -0.9, CROWN - 0.06),
                at(j, 0.9, CROWN - 0.06),
                at(j + 1, 0.9, CROWN - 0.06),
                at(j + 1, -0.9, CROWN - 0.06),
            ];
            m.flat_quad(
                lamp,
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                mats.glow,
                0,
                Some(v3(0.0, -1.0, 0.0)),
            );
        }
    }
    // rocky mound over the tunnel
    let outline = |j: usize| -> Vec<V3> {
        let t = (j - a) as f64 / span;
        let edge = t.min(1.0 - t);
        let grow = smoothstep(0.0, 0.26, edge);
        let hw = inner_w(j) + 5.0 + 19.0 * grow;
        let top = CROWN + 2.6 + 10.0 * grow;
        let hs = WALL_TOP + 1.2;
        let mut pts = Vec::with_capacity(ARCH_SEGMENTS + 3);
        let ground = |lat: f64| {
            let s = &road.secs[j];
            let r = horizontal(s.right);
            let (x, z) = (s.pos.x + r.x * lat, s.pos.z + r.z * lat);
            world.height(x, z)
        };
        let base = |lat: f64| {
            let s = &road.secs[j];
            let r = horizontal(s.right);
            v3(s.pos.x + r.x * lat, ground(lat) - 2.2, s.pos.z + r.z * lat)
        };
        pts.push(base(-hw));
        for k in 0..=ARCH_SEGMENTS {
            let th = PI - PI * k as f64 / ARCH_SEGMENTS as f64;
            let lat = hw * cos(th);
            let h = hs + (top - hs) * blunt(sin(th));
            let p = at(j, lat, h);
            pts.push(v3(p.x, p.y.max(ground(lat) + 0.6), p.z));
        }
        pts.push(base(hw));
        pts
    };
    let mut prev = outline(a);
    for j in a..b {
        let next = outline(j + 1);
        for k in 0..prev.len() - 1 {
            let p = [prev[k], prev[k + 1], next[k + 1], next[k]];
            let axis = at(j, 0.0, 2.0);
            let outward = (p[0] + p[2]) * 0.5 - axis;
            let u = j as f64 * road.ds / 9.0;
            m.flat_quad(
                p,
                [
                    [u, k as f64 * 0.9],
                    [u, (k + 1) as f64 * 0.9],
                    [u + road.ds / 9.0, (k + 1) as f64 * 0.9],
                    [u + road.ds / 9.0, k as f64 * 0.9],
                ],
                mats.cliff,
                0,
                Some(outward),
            );
        }
        prev = next;
    }
    // portal faces: concrete frame between the opening and the mound outline
    for (j, forward) in [(a, -1.0), (b, 1.0)] {
        let inner = ring(inner_w(j), WALL_TOP, CROWN, -1.4);
        let outer = outline(j);
        let face = road.secs[j].fwd * forward;
        for k in 0..inner.len() - 1 {
            let p = [
                at(j, inner[k].0, inner[k].1),
                at(j, inner[k + 1].0, inner[k + 1].1),
                outer[k + 1],
                outer[k],
            ];
            m.flat_quad(
                p,
                [[0.0, 0.0], [0.0, 1.0], [1.5, 1.0], [1.5, 0.0]],
                mats.concrete,
                0,
                Some(face),
            );
        }
    }
    m
}

fn overpass(road: &Road, world: &World, style: &Style, mats: &Mats, idx: usize) -> MeshData {
    let mut m = MeshData::new();
    let sec = &road.secs[idx];
    let r = horizontal(sec.right);
    let f = horizontal(sec.fwd);
    let centre = sec.pos;
    let deck_y = centre.y + DECK_HEIGHT;
    let flat = edge_lat(style, sec) + 4.6;
    let half = DECK_HALF_WIDTH;
    let at = |lat: f64, along: f64, y: f64| {
        v3(
            centre.x + r.x * lat + f.x * along,
            y,
            centre.z + r.z * lat + f.z * along,
        )
    };
    let ground_y = |lat: f64, along: f64| {
        let p = at(lat, along, 0.0);
        world.height(p.x, p.z)
    };
    for side in [-1.0f64, 1.0] {
        let end = flat + RAMP_LENGTH;
        // ramp top (road texture) and its stone side walls
        let steps = 10;
        for q in 0..steps {
            let (t0, t1) = (q as f64 / steps as f64, (q + 1) as f64 / steps as f64);
            let (l0, l1) = (flat + RAMP_LENGTH * t0, flat + RAMP_LENGTH * t1);
            let top = |l: f64, t: f64| {
                let g = ground_y(side * l, 0.0).max(centre.y - 1.0);
                deck_y + (g + 0.15 - deck_y) * t * t.sqrt().max(t)
            };
            let (y0, y1) = (top(l0, t0), top(l1, t1));
            let deck = [
                at(side * l0, -half, y0),
                at(side * l0, half, y0),
                at(side * l1, half, y1),
                at(side * l1, -half, y1),
            ];
            let u = l0 / 10.0;
            m.flat_quad(
                deck,
                [[0.0, u], [1.0, u], [1.0, u + 0.3], [0.0, u + 0.3]],
                mats.road,
                0,
                Some(V3::UP),
            );
            for along in [-half, half] {
                let g0 = ground_y(side * l0, along) - 2.0;
                let g1 = ground_y(side * l1, along) - 2.0;
                let wall = [
                    at(side * l0, along, g0),
                    at(side * l0, along, y0),
                    at(side * l1, along, y1),
                    at(side * l1, along, g1),
                ];
                m.flat_quad(
                    wall,
                    [
                        [l0 / 4.0, 0.0],
                        [l0 / 4.0, (y0 - g0) / 4.0],
                        [l1 / 4.0, (y1 - g1) / 4.0],
                        [l1 / 4.0, 0.0],
                    ],
                    mats.stone,
                    0,
                    Some(f * along.signum()),
                );
                // parapet block on the ramp edge
                let par = [
                    at(side * l0, along, y0),
                    at(side * l1, along, y1),
                    at(side * l1, along, y1 + 0.95),
                    at(side * l0, along, y0 + 0.95),
                ];
                m.flat_quad(
                    par,
                    [[0.0, 0.0], [0.25, 0.0], [0.25, 0.25], [0.0, 0.25]],
                    mats.concrete,
                    0,
                    Some(f * along.signum()),
                );
                let cap = [
                    at(side * l0, along, y0 + 0.95),
                    at(side * l1, along, y1 + 0.95),
                    at(side * l1, along * 0.9, y1 + 0.95),
                    at(side * l0, along * 0.9, y0 + 0.95),
                ];
                m.flat_quad(
                    cap,
                    [[0.0, 0.0], [0.5, 0.0], [0.5, 0.2], [0.0, 0.2]],
                    mats.concrete,
                    0,
                    Some(V3::UP),
                );
            }
        }
        // tail end cap of the ramp
        let g_end = ground_y(side * end, 0.0) - 2.0;
        let cap = [
            at(side * end, -half, g_end),
            at(side * end, half, g_end),
            at(side * end, half, g_end + 2.2),
            at(side * end, -half, g_end + 2.2),
        ];
        m.flat_quad(
            cap,
            [[0.0, 0.0], [2.0, 0.0], [2.0, 0.5], [0.0, 0.5]],
            mats.stone,
            0,
            Some(r * side),
        );
    }
    // level deck above the road with parapets and a stone underside
    let (l0, l1) = (-flat, flat);
    let deck = [
        at(l0, -half, deck_y),
        at(l0, half, deck_y),
        at(l1, half, deck_y),
        at(l1, -half, deck_y),
    ];
    m.flat_quad(
        deck,
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        mats.road,
        0,
        Some(V3::UP),
    );
    let under = [
        at(l0, -half, deck_y - 0.9),
        at(l1, -half, deck_y - 0.9),
        at(l1, half, deck_y - 0.9),
        at(l0, half, deck_y - 0.9),
    ];
    m.flat_quad(
        under,
        [[0.0, 0.0], [flat / 2.0, 0.0], [flat / 2.0, 2.0], [0.0, 2.0]],
        mats.stone,
        0,
        Some(v3(0.0, -1.0, 0.0)),
    );
    for along in [-half, half] {
        let face = [
            at(l0, along, deck_y - 0.9),
            at(l1, along, deck_y - 0.9),
            at(l1, along, deck_y + 0.95),
            at(l0, along, deck_y + 0.95),
        ];
        m.flat_quad(
            face,
            [[0.0, 0.0], [flat / 2.0, 0.0], [flat / 2.0, 0.7], [0.0, 0.7]],
            mats.stone,
            0,
            Some(f * along.signum()),
        );
        let cap = [
            at(l0, along, deck_y + 0.95),
            at(l1, along, deck_y + 0.95),
            at(l1, along * 0.9, deck_y + 0.95),
            at(l0, along * 0.9, deck_y + 0.95),
        ];
        m.flat_quad(
            cap,
            [[0.0, 0.0], [flat / 2.0, 0.0], [flat / 2.0, 0.2], [0.0, 0.2]],
            mats.concrete,
            0,
            Some(V3::UP),
        );
    }
    // abutment piers beside the road, from below the ground up to the deck
    for side in [-1.0f64, 1.0] {
        let (a0, a1) = (edge_lat(style, sec) + 1.2, flat);
        let base = centre.y - 2.0;
        for along in [-half, half] {
            let wall = [
                at(side * a0, along, base),
                at(side * a1, along, base),
                at(side * a1, along, deck_y - 0.9),
                at(side * a0, along, deck_y - 0.9),
            ];
            m.flat_quad(
                wall,
                [[0.0, 0.0], [1.5, 0.0], [1.5, 3.0], [0.0, 3.0]],
                mats.stone,
                0,
                Some(f * along.signum()),
            );
        }
        let inner = [
            at(side * a0, -half, base),
            at(side * a0, half, base),
            at(side * a0, half, deck_y - 0.9),
            at(side * a0, -half, deck_y - 0.9),
        ];
        m.flat_quad(
            inner,
            [[0.0, 0.0], [2.0, 0.0], [2.0, 3.0], [0.0, 3.0]],
            mats.stone,
            0,
            Some(-r * side),
        );
    }
    m
}
