//! Deterministic procedural generator of project-owned Racer Content Pack
//! (RCP v1) tracks. See `docs/TRACKGEN.md`.
//!
//! A [`TrackSpec`] (seed, spline control points, theme) is turned into an RCP
//! [`Pack`](racer_pack_schema::Pack): extruded road with banking and elevation,
//! lane markings, kerbs, shoulders, barriers, a terrain skirt, instanced
//! scenery, procedural textures, a panorama sky, a spawn grid, checkpoints and
//! AI lanes with target speeds. Output depends only on the spec: the same spec
//! always yields the same bytes.

pub mod ai;
pub mod generate;
pub mod materials;
pub mod math;
pub mod mesh;
pub mod png;
pub mod props;
pub mod road;
pub mod scenery;
pub mod spec;
pub mod structures;
pub mod style;
pub mod textures;
pub mod world;

pub use generate::{generate_files, generate_pack};
pub use road::{Road, RoadStats, Section};
pub use spec::{ControlPoint, GENERATOR_VERSION, Theme, TrackSpec, builtin_specs, find_spec};

/// Road centreline for a spec with the theme's banking settings.
pub fn build_road(spec: &TrackSpec) -> Road {
    let style = style::style_for(spec.theme);
    Road::build(spec, style.bank_gain, style.max_bank_deg)
}

/// Top-down preview of the road layout as RGBA pixels.
pub fn layout_map(road: &Road, size: u32) -> Vec<u8> {
    let (mut x0, mut x1, mut z0, mut z1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for s in &road.secs {
        x0 = x0.min(s.pos.x);
        x1 = x1.max(s.pos.x);
        z0 = z0.min(s.pos.z);
        z1 = z1.max(s.pos.z);
    }
    let span = (x1 - x0).max(z1 - z0) * 1.1;
    let scale = f64::from(size) / span;
    let mut px = vec![255u8; (size * size * 4) as usize];
    for (i, s) in road.secs.iter().enumerate() {
        let shade = (40.0 + 180.0 * (s.pos.y + 10.0).max(0.0) / 80.0).min(255.0) as u8;
        let x = ((s.pos.x - x0) * scale + f64::from(size) * 0.05) as i64;
        let z = ((s.pos.z - z0) * scale + f64::from(size) * 0.05) as i64;
        for dx in -1..=1 {
            for dz in -1..=1 {
                let (xx, zz) = (x + dx, z + dz);
                if xx >= 0 && zz >= 0 && xx < i64::from(size) && zz < i64::from(size) {
                    let o = ((zz as u32 * size + xx as u32) * 4) as usize;
                    let c = if i == 0 {
                        [220, 30, 30]
                    } else {
                        [shade, 60, 255 - shade]
                    };
                    px[o..o + 3].copy_from_slice(&c);
                }
            }
        }
    }
    px
}
