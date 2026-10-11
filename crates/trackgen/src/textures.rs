//! Procedural RGBA8 textures. Everything is computed from noise and simple
//! shapes; no external images are involved.

use crate::math::{clamp, cos, fbm2, hash_str, lerp, sin, smoothstep, v3};
use crate::style::{BarrierKind, Rgb, ShoulderKind, Style};
use std::f64::consts::{PI, TAU};

/// Texture slots shared by every generated track.
pub mod id {
    pub const ROAD: u32 = 0;
    pub const KERB: u32 = 1;
    pub const SHOULDER: u32 = 2;
    pub const GRASS: u32 = 3;
    pub const VERGE: u32 = 4;
    pub const SAND: u32 = 5;
    pub const ROCK: u32 = 6;
    pub const WATER: u32 = 7;
    pub const BARRIER: u32 = 8;
    pub const CORRUGATED: u32 = 9;
    pub const BARK: u32 = 10;
    pub const FOLIAGE: u32 = 11;
    pub const PLASTER: u32 = 12;
    pub const WINDOWS: u32 = 13;
    pub const CROWD: u32 = 14;
    pub const CHEVRON: u32 = 15;
    pub const CHECKER: u32 = 16;
    pub const SNOW: u32 = 17;
    pub const DIRT: u32 = 18;
    pub const LOG: u32 = 19;
    pub const SIGNS: u32 = 20;
    pub const ARMCO: u32 = 21;
    pub const STONE: u32 = 22;
    pub const SKY0: u32 = 23; // four strips: 23..=26
    pub const COUNT: u32 = 27;
}

/// Metres of road covered by one repeat of the road texture along its length.
pub const ROAD_TEX_LENGTH_M: f64 = 12.0;
/// Metres of kerb covered by one repeat of the kerb texture.
pub const KERB_TEX_LENGTH_M: f64 = 4.8;
/// Metres covered by one repeat of ground textures.
pub const GROUND_TEX_M: f64 = 8.0;
/// Edge in metres covered by one repeat of the barrier texture.
pub const BARRIER_TEX_M: f64 = 2.0;
/// Panorama strips and their pixel size.
pub const SKY_STRIPS: usize = 4;
pub const SKY_SIZE: u32 = 256;
/// Fraction of the strip height (from the top) at which the horizon lies.
pub const SKY_HORIZON_FRACTION: f64 = 0.72;
/// Total vertical angle covered by the strips, in degrees.
pub const SKY_SPAN_DEG: f64 = 100.0;

#[derive(Clone, Debug)]
pub struct Tex {
    pub name: String,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

fn byte(v: f64) -> u8 {
    (clamp(v, 0.0, 1.0) * 255.0 + 0.5) as u8
}

fn build(name: &str, w: u32, h: u32, f: impl Fn(f64, f64) -> [f64; 4]) -> Tex {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let u = (f64::from(x) + 0.5) / f64::from(w);
            let v = (f64::from(y) + 0.5) / f64::from(h);
            let c = f(u, v);
            rgba.extend([byte(c[0]), byte(c[1]), byte(c[2]), byte(c[3])]);
        }
    }
    Tex {
        name: name.to_string(),
        w,
        h,
        rgba,
    }
}

fn n(seed: u64, u: f64, v: f64, cx: i64, cy: i64, oct: u32) -> f64 {
    fbm2(seed, u * cx as f64, v * cy as f64, oct, cx, cy)
}

fn rgb(c: Rgb, k: f64) -> [f64; 4] {
    [c[0] * k, c[1] * k, c[2] * k, 1.0]
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    [
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
    ]
}

fn fract(x: f64) -> f64 {
    x - x.floor()
}

pub fn generate(style: &Style, seed: u64, road_width_m: f64) -> Vec<Tex> {
    let s = |label: &str| seed ^ hash_str(label);
    let mut out: Vec<Tex> = Vec::new();

    // ROAD with lane markings: u across the road, v along it.
    let sd = s("road");
    let line_w = 0.30 / road_width_m;
    out.push(build("road", 256, 256, |u, v| {
        let grain = n(sd, u, v, 64, 64, 2);
        let blot = n(sd ^ 1, u, v, 6, 6, 3);
        let mut c = rgb(style.asphalt, (0.80 + 0.4 * grain) * (0.90 + 0.20 * blot));
        let strip =
            smoothstep(0.07, 0.0, (u - 0.30).abs()) + smoothstep(0.07, 0.0, (u - 0.70).abs());
        for ch in c.iter_mut().take(3) {
            *ch *= 1.0 - 0.14 * strip;
        }
        let edge = smoothstep(0.0, 0.12, u.min(1.0 - u));
        for ch in c.iter_mut().take(3) {
            *ch *= 0.82 + 0.18 * edge;
        }
        let wear = n(sd ^ 2, u, v, 48, 48, 2);
        let on_edge = (u > 0.035 && u < 0.035 + line_w) || (u < 0.965 && u > 0.965 - line_w);
        let on_centre = (u - 0.5).abs() < line_w * 0.5 && fract(v * 3.0) < 0.5;
        if on_edge || on_centre {
            let mark = if on_edge {
                style.edge_marking
            } else {
                style.marking
            };
            let k = if wear < 0.3 { 0.55 } else { 0.9 + 0.1 * wear };
            let base = [c[0], c[1], c[2]];
            let m = mix(base, mark, k);
            c = [m[0], m[1], m[2], 1.0];
        }
        c
    }));

    // KERB stripes.
    let sd = s("kerb");
    out.push(build("kerb", 128, 128, |u, v| {
        let stripe = (v * 4.0).floor() as i64 % 2 == 0;
        let base = if stripe { style.kerb_a } else { style.kerb_b };
        let grain = n(sd, u, v, 32, 32, 2);
        let shade = 0.82 + 0.25 * grain;
        let edge = smoothstep(0.0, 0.10, u.min(1.0 - u));
        rgb(base, shade * (0.8 + 0.2 * edge))
    }));

    // SHOULDER surface by kind.
    let sd = s("shoulder");
    out.push(build("shoulder", 128, 128, |u, v| match style.shoulder {
        ShoulderKind::Sand => {
            let k = 0.88 + 0.22 * n(sd, u, v, 8, 8, 3) + 0.08 * n(sd ^ 1, u, v, 64, 64, 1);
            rgb(style.sand, k)
        }
        ShoulderKind::Gravel => {
            let base = [0.42, 0.40, 0.37];
            let speck = n(sd ^ 1, u, v, 64, 64, 1);
            let k = 0.75 + 0.25 * n(sd, u, v, 10, 10, 3) + if speck > 0.82 { 0.25 } else { 0.0 };
            rgb(base, k)
        }
        ShoulderKind::RedDirt => {
            let k = 0.85 + 0.3 * n(sd, u, v, 8, 8, 4) + 0.06 * n(sd ^ 1, u, v, 64, 64, 1);
            rgb(style.verge, k)
        }
        ShoulderKind::Concrete => {
            let joint = fract(u * 2.0).min(1.0 - fract(u * 2.0)) < 0.02
                || fract(v * 2.0).min(1.0 - fract(v * 2.0)) < 0.02;
            let k = 0.55 + 0.18 * n(sd, u, v, 6, 6, 3) + 0.06 * n(sd ^ 1, u, v, 64, 64, 1);
            let k = if joint { k * 0.6 } else { k };
            [0.55 * k, 0.56 * k, 0.60 * k, 1.0]
        }
    }));

    // GRASS.
    let sd = s("grass");
    out.push(build("grass", 128, 128, |u, v| {
        let patch = n(sd, u, v, 6, 6, 3);
        let blades = n(sd ^ 1, u, v, 96, 24, 2);
        let c = mix(style.grass_a, style.grass_b, patch);
        rgb(c, 0.72 + 0.5 * blades)
    }));

    // VERGE (worn ground beside the barrier).
    let sd = s("verge");
    out.push(build("verge", 128, 128, |u, v| {
        let patch = n(sd, u, v, 7, 7, 3);
        let c = mix(style.verge, style.grass_a, smoothstep(0.35, 0.7, patch));
        let peb = n(sd ^ 1, u, v, 64, 64, 1);
        rgb(
            c,
            0.75 + 0.35 * n(sd ^ 2, u, v, 24, 24, 2) + if peb > 0.85 { 0.2 } else { 0.0 },
        )
    }));

    // SAND with ripples.
    let sd = s("sand");
    out.push(build("sand", 128, 128, |u, v| {
        let warp = n(sd, u, v, 6, 6, 3);
        let ripple = 0.5 + 0.5 * sin((v * 16.0 + warp * 3.0) * TAU / 2.0);
        rgb(
            style.sand,
            0.82 + 0.12 * ripple + 0.14 * n(sd ^ 1, u, v, 48, 48, 1),
        )
    }));

    // ROCK strata.
    let sd = s("rock");
    out.push(build("rock", 128, 128, |u, v| {
        let warp = n(sd, u, v, 4, 4, 3);
        let band = 0.5 + 0.5 * sin((v * 12.0 + warp * 4.0) * PI);
        let grain = n(sd ^ 1, u, v, 40, 40, 2);
        let crack = n(sd ^ 2, u, v, 10, 3, 3);
        let c = mix(
            style.rock_dark,
            style.rock_light,
            smoothstep(0.2, 0.8, band * 0.7 + grain * 0.5),
        );
        let k = if crack < 0.30 {
            0.55
        } else {
            0.85 + 0.3 * grain
        };
        rgb(c, k)
    }));

    // WATER.
    let sd = s("water");
    out.push(build("water", 128, 128, |u, v| {
        let w = n(sd, u, v, 8, 8, 4);
        let r = 1.0 - (2.0 * n(sd ^ 3, u, v, 16, 16, 3) - 1.0).abs();
        let c = mix(style.water_deep, style.water_light, smoothstep(0.3, 0.8, w));
        let glint = smoothstep(0.86, 0.97, r) * 0.6;
        [
            c[0] + glint * 0.5,
            c[1] + glint * 0.55,
            c[2] + glint * 0.6,
            1.0,
        ]
    }));

    // BARRIER: texture covers 2 m x 2 m; the 1 m high face uses v in [0, 0.5].
    let sd = s("barrier");
    out.push(build("barrier", 128, 128, |u, v| {
        barrier_pixel(style, style.barrier, sd, u, v)
    }));

    // Grey-scale detail textures, tinted by materials.
    let sd = s("corrugated");
    out.push(build("corrugated", 64, 64, |u, v| {
        let ridge = 0.5 + 0.5 * cos(u * TAU * 8.0);
        let k = 0.55 + 0.35 * ridge + 0.1 * n(sd, u, v, 16, 16, 2);
        rgb([1.0, 1.0, 1.0], k)
    }));
    let sd = s("bark");
    out.push(build("bark", 64, 64, |u, v| {
        let k = 0.45 + 0.55 * n(sd, u, v, 12, 3, 3);
        rgb([1.0, 1.0, 1.0], k)
    }));
    let sd = s("foliage");
    out.push(build("foliage", 64, 64, |u, v| {
        let k = 0.50 + 0.5 * n(sd, u, v, 10, 10, 3);
        let sparkle = if n(sd ^ 1, u, v, 32, 32, 1) > 0.78 {
            0.15
        } else {
            0.0
        };
        rgb([1.0, 1.0, 1.0], k + sparkle)
    }));
    let sd = s("plaster");
    out.push(build("plaster", 64, 64, |u, v| {
        rgb([1.0, 1.0, 1.0], 0.80 + 0.2 * n(sd, u, v, 12, 12, 3))
    }));

    // WINDOWS: 4 x 4 window cells per tile.
    let sd = s("windows");
    let night = style.night;
    out.push(build("windows", 128, 128, |u, v| {
        let (cu, cv) = (u * 4.0, v * 4.0);
        let (iu, iv) = (cu.floor() as i64, cv.floor() as i64);
        let (fu, fv) = (fract(cu), fract(cv));
        let inside = fu > 0.18 && fu < 0.82 && fv > 0.2 && fv < 0.78;
        let h = n(
            sd,
            (iu as f64 + 0.5) / 4.0,
            (iv as f64 + 0.5) / 4.0,
            64,
            64,
            1,
        );
        if !inside {
            return rgb([0.30, 0.30, 0.34], 0.85 + 0.15 * n(sd ^ 1, u, v, 32, 32, 2));
        }
        if night {
            if h > 0.52 {
                [1.0, 0.86, 0.50, 1.0]
            } else {
                [0.05, 0.07, 0.12, 1.0]
            }
        } else {
            let k = 0.45 + 0.35 * fv;
            [0.35 * k + 0.1, 0.55 * k + 0.12, 0.72 * k + 0.12, 1.0]
        }
    }));

    // CROWD.
    let sd = s("crowd");
    let palette: [Rgb; 6] = [
        [0.9, 0.2, 0.2],
        [0.2, 0.4, 0.9],
        [0.95, 0.85, 0.2],
        [0.95, 0.95, 0.95],
        [0.2, 0.7, 0.3],
        [0.9, 0.5, 0.15],
    ];
    out.push(build("crowd", 64, 64, |u, v| {
        let (bx, by) = ((u * 8.0).floor(), (v * 8.0).floor());
        let h = fbm2(sd, bx + 0.5, by + 0.5, 1, 0, 0);
        let h2 = fbm2(sd ^ 9, bx + 0.5, by + 0.5, 1, 0, 0);
        let (fu, fv) = (fract(u * 8.0), fract(v * 8.0));
        let head = (fu - 0.5).powi(2) + (fv - 0.3).powi(2) < 0.06;
        if h < 0.85 && (head || fv > 0.45) {
            let c = palette[(h2 * 5.99) as usize];
            if head && fv < 0.5 {
                rgb([0.85, 0.65, 0.5], 1.0)
            } else {
                rgb(c, 0.9)
            }
        } else {
            rgb([0.12, 0.12, 0.15], 1.0)
        }
    }));

    // CHEVRON sign board.
    out.push(build("chevron", 64, 64, |u, v| {
        let t = fract(u * 2.0 - (v - 0.5).abs() * 2.0 + 0.25);
        if t < 0.45 {
            rgb([0.05, 0.05, 0.06], 1.0)
        } else {
            rgb([0.96, 0.78, 0.10], 1.0)
        }
    }));

    // CHECKER (start/finish).
    out.push(build("checker", 64, 64, |u, v| {
        let on = ((u * 8.0).floor() as i64 + (v * 8.0).floor() as i64) % 2 == 0;
        let k = if on { 0.95 } else { 0.06 };
        rgb([1.0, 1.0, 1.0], k)
    }));

    // SNOW: soft drifts with blue shadows and sparkle.
    let sd = s("snow");
    out.push(build("snow", 128, 128, |u, v| {
        let drift = n(sd, u, v, 5, 5, 4);
        let fine = n(sd ^ 1, u, v, 40, 40, 2);
        let sparkle = if n(sd ^ 2, u, v, 64, 64, 1) > 0.9 {
            0.08
        } else {
            0.0
        };
        let c = mix(
            [0.74, 0.80, 0.90],
            [0.98, 0.98, 1.0],
            smoothstep(0.25, 0.75, drift),
        );
        rgb(c, 0.92 + 0.08 * fine + sparkle)
    }));

    // DIRT: packed earth with pebbles and tyre-less ruts.
    let sd = s("dirt");
    out.push(build("dirt", 128, 128, |u, v| {
        let patch = n(sd, u, v, 6, 6, 4);
        let fine = n(sd ^ 1, u, v, 56, 56, 2);
        let peb = n(sd ^ 2, u, v, 48, 48, 1);
        let c = mix(
            [0.34, 0.25, 0.16],
            [0.52, 0.40, 0.26],
            smoothstep(0.2, 0.8, patch),
        );
        rgb(c, 0.78 + 0.3 * fine + if peb > 0.88 { 0.22 } else { 0.0 })
    }));

    // LOG: horizontal round timbers, grey-scale so materials can tint them.
    let sd = s("log");
    out.push(build("log", 128, 128, |u, v| {
        let row = (v * 6.0).floor();
        let f = fract(v * 6.0);
        let round = (f * PI).sin();
        let grain = n(sd ^ (row as u64), u, v, 3, 24, 2);
        let end_gap = fract(u * 2.0 + row * 0.37).min(1.0 - fract(u * 2.0 + row * 0.37)) < 0.012;
        let k = (0.35 + 0.65 * round) * (0.75 + 0.35 * grain);
        rgb([1.0, 1.0, 1.0], if end_gap { k * 0.5 } else { k })
    }));

    // SIGNS: four 64 px quadrants (yellow diamond, red-ring disc, blue square, hazard board).
    let sd = s("signs");
    out.push(build("signs", 128, 128, |u, v| {
        let (qu, qv) = (u * 2.0, v * 2.0);
        let (qx, qy) = (fract(qu) * 2.0 - 1.0, fract(qv) * 2.0 - 1.0);
        let quad = (qu.floor() as i64) + 2 * (qv.floor() as i64);
        let grime = 0.92 + 0.08 * n(sd, u, v, 24, 24, 2);
        let border = qx.abs().max(qy.abs()) > 0.9;
        let c: Rgb = match quad {
            0 => {
                // yellow warning diamond with a curve arrow
                let d = qx.abs() + qy.abs();
                if d > 1.0 {
                    [0.85, 0.87, 0.9]
                } else if d > 0.86 {
                    [0.05, 0.05, 0.06]
                } else {
                    let arrow = (qy + 0.55 * qx - 0.1).abs() < 0.12 && qx > -0.45 && qx < 0.4
                        || ((qx - 0.1).abs() < 0.12 && qy > -0.5 && qy < 0.3);
                    if arrow {
                        [0.05, 0.05, 0.06]
                    } else {
                        [0.97, 0.80, 0.10]
                    }
                }
            }
            1 => {
                // white disc with red ring and a dark bar
                let r = (qx * qx + qy * qy).sqrt();
                if r > 0.95 {
                    [0.85, 0.87, 0.9]
                } else if r > 0.72 {
                    [0.82, 0.08, 0.08]
                } else if (qx.abs() < 0.12 && qy.abs() < 0.5) || (qy.abs() < 0.12 && qx.abs() < 0.5)
                {
                    [0.08, 0.08, 0.1]
                } else {
                    [0.96, 0.96, 0.96]
                }
            }
            2 => {
                // blue board with a white arrow
                if border {
                    [0.9, 0.9, 0.92]
                } else {
                    let arrow = (qx.abs() < 0.12 && qy > -0.6 && qy < 0.5)
                        || (qy > 0.1
                            && qy < 0.55
                            && qx.abs() < (0.55 - qy) * 1.2 + 0.0
                            && qx.abs() > 0.0
                            && qy - 0.1 < (0.55 - qx.abs()) * 0.9);
                    if arrow {
                        [0.96, 0.96, 0.98]
                    } else {
                        [0.08, 0.28, 0.62]
                    }
                }
            }
            _ => {
                // orange hazard board with black diagonal stripes
                if border {
                    [0.08, 0.08, 0.09]
                } else if fract((qx + qy) * 3.0) < 0.5 {
                    [0.95, 0.55, 0.08]
                } else {
                    [0.08, 0.08, 0.09]
                }
            }
        };
        rgb(c, grime)
    }));

    // ARMCO: galvanised guard rail on posts, whatever the theme's barrier.
    let sd = s("armco");
    out.push(build("armco", 128, 128, |u, v| {
        barrier_pixel(style, BarrierKind::Armco, sd, u, v)
    }));

    // STONE: dressed blocks for bridges and walls, whatever the theme.
    let sd = s("stone");
    out.push(build("stone", 128, 128, |u, v| {
        barrier_pixel(style, BarrierKind::Stone, sd, u, v)
    }));

    // SKY panorama strips.
    for strip in 0..SKY_STRIPS {
        out.push(sky_strip(style, seed, strip));
    }
    debug_assert_eq!(out.len() as u32, id::COUNT);
    out
}

fn barrier_pixel(style: &Style, kind: BarrierKind, sd: u64, u: f64, v: f64) -> [f64; 4] {
    let grain = n(sd, u, v, 48, 48, 2);
    match kind {
        BarrierKind::Concrete => {
            let seam = fract(u * 4.0).min(1.0 - fract(u * 4.0)) < 0.012;
            let stripe = v > 0.26 && v < 0.40;
            let base = if stripe {
                style.kerb_a
            } else {
                [0.66, 0.66, 0.68]
            };
            let k = (0.78 + 0.22 * grain) * if seam { 0.6 } else { 1.0 };
            rgb(base, k)
        }
        BarrierKind::Armco => {
            let post = fract(u * 4.0 + 0.5).min(1.0 - fract(u * 4.0 + 0.5)) < 0.03;
            let rail = v > 0.18 && v < 0.44;
            if post {
                rgb([0.30, 0.31, 0.33], 0.8 + 0.2 * grain)
            } else if rail {
                let ridge = 0.5 + 0.5 * cos((v - 0.18) / 0.26 * TAU * 1.5);
                rgb(
                    [0.74, 0.76, 0.78],
                    (0.55 + 0.45 * ridge) * (0.85 + 0.15 * grain),
                )
            } else {
                rgb([0.10, 0.11, 0.12], 1.0)
            }
        }
        BarrierKind::Wood => {
            let plank = ((v * 10.0).floor() as i64).rem_euclid(5) as f64;
            let gap = fract(v * 10.0) < 0.07;
            let post = fract(u * 4.0 + 0.5).min(1.0 - fract(u * 4.0 + 0.5)) < 0.04;
            let grainl = n(sd ^ 4, u, v, 6, 40, 2);
            let c = mix(
                [0.45, 0.30, 0.16],
                [0.62, 0.45, 0.25],
                (plank * 0.37 + grainl * 0.6) % 1.0,
            );
            if gap {
                rgb([0.08, 0.06, 0.04], 1.0)
            } else if post {
                rgb(c, 0.55)
            } else {
                rgb(c, 0.8 + 0.3 * grainl)
            }
        }
        BarrierKind::Stone => {
            let row = (v * 8.0).floor();
            let off = if row as i64 % 2 == 0 { 0.0 } else { 0.5 };
            let cu = fract(u * 4.0 + off);
            let mortar = cu < 0.04 || fract(v * 8.0) < 0.08;
            let tone = n(sd ^ (row as u64), u, v, 4, 4, 2);
            if mortar {
                rgb([0.35, 0.34, 0.33], 1.0)
            } else {
                rgb([0.55, 0.52, 0.48], 0.8 + 0.3 * tone + 0.1 * grain)
            }
        }
    }
}

fn sky_color(style: &Style, seed: u64, az: f64, elev_deg: f64) -> Rgb {
    let sk = &style.sky;
    let e = elev_deg.to_radians();
    let t = clamp(elev_deg / (SKY_SPAN_DEG * SKY_HORIZON_FRACTION), 0.0, 1.0);
    let mut c = if elev_deg >= 0.0 {
        let a = smoothstep(0.0, 0.45, t);
        let b = smoothstep(0.35, 1.0, t);
        mix(mix(sk.horizon, sk.mid, a), sk.zenith, b)
    } else {
        sk.horizon
    };
    let dir = v3(cos(e) * cos(az), sin(e), cos(e) * sin(az));
    // city / horizon glow
    if style.night {
        let g = smoothstep(14.0, 0.0, elev_deg.abs());
        c = mix(c, [0.55, 0.38, 0.30], g * 0.55);
    }
    if let Some((saz, sel)) = sk.sun {
        let sd = v3(cos(sel) * cos(saz), sin(sel), cos(sel) * sin(saz));
        let d = 1.0 - dir.dot(sd);
        let halo = clamp(1.0 - d / 0.06, 0.0, 1.0);
        c = mix(c, sk.glow, halo * halo * 0.6);
        if d < 0.0009 {
            c = [1.0, 1.0, 0.95];
        }
    }
    if style.night && elev_deg > 6.0 {
        let u = az / TAU;
        let h = fbm2(seed ^ 0x57A5, u * 700.0, elev_deg * 5.0, 1, 0, 0);
        if h > 0.94 {
            let k = (h - 0.94) / 0.06;
            c = mix(c, [1.0, 1.0, 1.0], 0.5 + 0.5 * k);
        }
        // moon
        let moon = v3(cos(0.9) * cos(5.2), sin(0.9), cos(0.9) * sin(5.2));
        let md = 1.0 - dir.dot(moon);
        if md < 0.0007 {
            c = [0.95, 0.95, 0.86];
        } else if md < 0.006 {
            c = mix(c, [0.8, 0.82, 0.95], 0.25 * (1.0 - md / 0.006));
        }
    }
    // clouds: periodic around the horizon
    if elev_deg > 1.0 {
        let u = az / TAU;
        let v = elev_deg / 90.0;
        let cn = fbm2(seed ^ 0xC10D, u * 7.0, v * 3.0 + 0.5, 5, 7, 0);
        let thr = 1.0 - sk.cloud_cover;
        let dens = smoothstep(thr, thr + 0.22, cn) * smoothstep(1.0, 9.0, elev_deg);
        let shade = fbm2(seed ^ 0x5AD0, u * 7.0 + 0.02, v * 3.0 + 0.55, 4, 7, 0);
        let cc = mix(
            [
                sk.cloud_color[0] * 0.72,
                sk.cloud_color[1] * 0.74,
                sk.cloud_color[2] * 0.80,
            ],
            sk.cloud_color,
            smoothstep(0.35, 0.65, shade),
        );
        c = mix(c, cc, dens * 0.92);
    }
    // snow-capped peaks far behind the hills
    if sk.peaks > 0.0 {
        let u = az / TAU;
        let sd = seed ^ 0x9EAC;
        let ridged = 1.0 - (2.0 * fbm2(sd, u * 11.0, 0.5, 4, 11, 0) - 1.0).abs();
        let fine = fbm2(sd ^ 5, u * 90.0, 0.5, 3, 90, 0);
        let h = sk.peaks * (0.12 + 0.88 * smoothstep(0.45, 0.95, ridged)) * (0.92 + 0.16 * fine);
        if elev_deg < h {
            let t = elev_deg / h.max(0.1);
            let rock = [0.50, 0.57, 0.70];
            let snow = [0.97, 0.98, 1.0];
            let cap = smoothstep(0.50, 0.72, t + 0.12 * fine);
            let base = mix(rock, snow, cap);
            // haze toward the horizon colour so the range sits far away
            c = mix(
                base,
                sk.horizon,
                0.38 * (1.0 - smoothstep(0.0, 0.8, t)) + 0.12,
            );
        }
    }
    // hills (two silhouette layers)
    let layers = [
        (sk.hill_height.0, sk.hill_far, 0u64),
        (sk.hill_height.1, sk.hill_near, 1u64),
    ];
    for (height, col, li) in layers {
        let h = hill_profile(style, seed, az, li, height);
        if elev_deg < h {
            let depth = clamp(1.0 - elev_deg / h.max(0.1), 0.0, 1.0);
            let shaded = [
                col[0] * (0.85 + 0.15 * (1.0 - depth)),
                col[1] * (0.85 + 0.15 * (1.0 - depth)),
                col[2] * (0.85 + 0.15 * (1.0 - depth)),
            ];
            c = shaded;
        }
    }
    if elev_deg < 0.0 {
        // ground haze below the horizon blends toward the near hill colour
        let k = smoothstep(0.0, -22.0, elev_deg);
        c = mix(c, sk.hill_near, k * 0.85);
    }
    c
}

fn hill_profile(style: &Style, seed: u64, az: f64, layer: u64, height_deg: f64) -> f64 {
    let u = az / TAU;
    let sd = seed ^ (0x4111 + layer * 131);
    let p = if layer == 0 { 5 } else { 9 };
    let base = fbm2(sd, u * p as f64, 0.5, 4, p, 0);
    let jag = fbm2(sd ^ 3, u * 160.0, 0.5, 2, 160, 0);
    if style.sky.buttes {
        let m = smoothstep(0.52, 0.58, base);
        height_deg * m * (0.92 + 0.08 * jag)
    } else {
        let tree = if style.theme_has_trees() {
            0.15 * jag
        } else {
            0.0
        };
        height_deg * (0.25 + 0.95 * smoothstep(0.2, 0.9, base)) * (0.9 + tree)
    }
}

fn sky_strip(style: &Style, seed: u64, strip: usize) -> Tex {
    let name = format!("sky-{strip}");
    let total_w = (SKY_SIZE as usize * SKY_STRIPS) as f64;
    build(&name, SKY_SIZE, SKY_SIZE, |u, v| {
        let az = ((strip as f64 * f64::from(SKY_SIZE)) + u * f64::from(SKY_SIZE)) / total_w * TAU;
        let elev = (SKY_HORIZON_FRACTION - v) * SKY_SPAN_DEG;
        let c = sky_color(style, seed, az, elev);
        [c[0], c[1], c[2], 1.0]
    })
}

impl Style {
    fn theme_has_trees(&self) -> bool {
        matches!(
            self.theme,
            crate::spec::Theme::PineRidge | crate::spec::Theme::Coastline
        )
    }
}

/// Convenience used by tests: sample a texel.
pub fn texel(t: &Tex, x: u32, y: u32) -> [u8; 4] {
    let o = ((y * t.w + x) * 4) as usize;
    [t.rgba[o], t.rgba[o + 1], t.rgba[o + 2], t.rgba[o + 3]]
}
