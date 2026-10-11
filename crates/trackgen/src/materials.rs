//! Material table shared by all geometry of one track.

use crate::style::{Rgb, Style};
use crate::textures::id as tex;
use racer_pack_schema::common::{AlphaMode, Material};

/// Indices into the track's material list.
#[derive(Clone, Debug, Default)]
pub struct Mats {
    pub list: Vec<Material>,
    pub road: u32,
    pub startline: u32,
    pub kerb: u32,
    pub shoulder: u32,
    pub verge: u32,
    pub grass: u32,
    pub sand: u32,
    pub rock: u32,
    pub water: u32,
    pub barrier: u32,
    pub bark: u32,
    pub pine: u32,
    pub leaf: u32,
    pub leaf_olive: u32,
    pub palm: u32,
    pub boulder: u32,
    pub plaster: u32,
    pub roof: u32,
    pub windows: u32,
    pub container: [u32; 4],
    pub metal: u32,
    pub pole: u32,
    pub glow: u32,
    pub chevron: u32,
    pub crowd: u32,
    pub stand: u32,
    pub checker: u32,
    pub cactus: u32,
    pub snow: u32,
    pub dirt: u32,
    pub log: u32,
    pub shingle: u32,
    pub signs: u32,
    pub armco: u32,
    pub concrete: u32,
    pub tunnel_wall: u32,
    pub cliff: u32,
    pub stone: u32,
}

impl Mats {
    fn add(&mut self, name: &str, texture: Option<u32>, color: Rgb, unlit: bool) -> u32 {
        self.list.push(Material {
            name: name.to_string(),
            texture,
            base_color: [
                color[0].clamp(0.0, 1.0) as f32,
                color[1].clamp(0.0, 1.0) as f32,
                color[2].clamp(0.0, 1.0) as f32,
                1.0,
            ],
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: true,
            unlit,
            animation: None,
        });
        (self.list.len() - 1) as u32
    }

    pub fn build(style: &Style) -> Mats {
        let l = style.light;
        let dim = |c: Rgb| [c[0] * l, c[1] * l, c[2] * l];
        let mut m = Mats::default();
        m.road = m.add("road", Some(tex::ROAD), dim([1.0, 1.0, 1.0]), false);
        m.startline = m.add("startline", Some(tex::CHECKER), dim([1.0, 1.0, 1.0]), false);
        m.kerb = m.add("kerb", Some(tex::KERB), dim([1.0, 1.0, 1.0]), false);
        m.shoulder = m.add("shoulder", Some(tex::SHOULDER), dim([1.0, 1.0, 1.0]), false);
        m.verge = m.add("verge", Some(tex::VERGE), dim([1.0, 1.0, 1.0]), false);
        m.grass = m.add("grass", Some(tex::GRASS), dim([1.0, 1.0, 1.0]), false);
        m.sand = m.add("sand", Some(tex::SAND), dim([1.0, 1.0, 1.0]), false);
        m.rock = m.add("rock", Some(tex::ROCK), dim([1.0, 1.0, 1.0]), false);
        m.water = m.add("water", Some(tex::WATER), dim([1.0, 1.0, 1.0]), false);
        m.barrier = m.add("barrier", Some(tex::BARRIER), dim([1.0, 1.0, 1.0]), false);
        m.bark = m.add("bark", Some(tex::BARK), dim([0.55, 0.40, 0.28]), false);
        m.pine = m.add(
            "pine-needles",
            Some(tex::FOLIAGE),
            dim([0.10, 0.34, 0.16]),
            false,
        );
        m.leaf = m.add("leaves", Some(tex::FOLIAGE), dim([0.28, 0.58, 0.20]), false);
        m.leaf_olive = m.add(
            "leaves-olive",
            Some(tex::FOLIAGE),
            dim([0.50, 0.55, 0.22]),
            false,
        );
        m.palm = m.add(
            "palm-fronds",
            Some(tex::FOLIAGE),
            dim([0.30, 0.62, 0.24]),
            false,
        );
        m.boulder = m.add("boulder", Some(tex::ROCK), dim([0.85, 0.85, 0.85]), false);
        m.plaster = m.add(
            "plaster",
            Some(tex::PLASTER),
            dim([0.95, 0.92, 0.84]),
            false,
        );
        m.roof = m.add(
            "roof-tiles",
            Some(tex::CORRUGATED),
            dim([0.72, 0.32, 0.22]),
            false,
        );
        m.windows = m.add(
            "windows",
            Some(tex::WINDOWS),
            if style.night {
                [1.0, 1.0, 1.0]
            } else {
                dim([1.0, 1.0, 1.0])
            },
            style.night,
        );
        let cols: [Rgb; 4] = [
            [0.75, 0.15, 0.12],
            [0.12, 0.30, 0.65],
            [0.15, 0.50, 0.28],
            [0.90, 0.55, 0.10],
        ];
        for (i, c) in cols.iter().enumerate() {
            m.container[i] = m.add(
                &format!("container-{i}"),
                Some(tex::CORRUGATED),
                dim(*c),
                false,
            );
        }
        m.metal = m.add(
            "metal",
            Some(tex::CORRUGATED),
            dim([0.62, 0.64, 0.68]),
            false,
        );
        m.pole = m.add(
            "pole",
            Some(tex::CORRUGATED),
            dim([0.30, 0.32, 0.36]),
            false,
        );
        m.glow = m.add("lamp-glow", Some(tex::PLASTER), [1.0, 0.92, 0.65], true);
        m.chevron = m.add(
            "chevron-sign",
            Some(tex::CHEVRON),
            dim([1.0, 1.0, 1.0]),
            false,
        );
        m.crowd = m.add("crowd", Some(tex::CROWD), dim([1.0, 1.0, 1.0]), false);
        m.stand = m.add("stand", Some(tex::BARRIER), dim([0.9, 0.9, 0.92]), false);
        m.checker = m.add(
            "checker-banner",
            Some(tex::CHECKER),
            dim([1.0, 1.0, 1.0]),
            false,
        );
        m.cactus = m.add("cactus", Some(tex::FOLIAGE), dim([0.28, 0.48, 0.22]), false);
        m.snow = m.add("snow", Some(tex::SNOW), dim([1.0, 1.0, 1.0]), false);
        m.dirt = m.add("dirt", Some(tex::DIRT), dim([1.0, 1.0, 1.0]), false);
        m.log = m.add("log-wall", Some(tex::LOG), dim([0.62, 0.40, 0.22]), false);
        m.shingle = m.add("shingle", Some(tex::BARK), dim([0.34, 0.20, 0.16]), false);
        m.signs = m.add("signs", Some(tex::SIGNS), dim([1.0, 1.0, 1.0]), false);
        m.armco = m.add("armco", Some(tex::ARMCO), dim([1.0, 1.0, 1.0]), false);
        m.concrete = m.add(
            "concrete",
            Some(tex::PLASTER),
            dim([0.70, 0.71, 0.73]),
            false,
        );
        m.tunnel_wall = m.add(
            "tunnel-wall",
            Some(tex::PLASTER),
            dim([0.46, 0.47, 0.50]),
            false,
        );
        m.cliff = m.add("cliff", Some(tex::ROCK), dim([0.92, 0.90, 0.88]), false);
        m.stone = m.add("stone", Some(tex::STONE), dim([0.82, 0.78, 0.72]), false);
        m
    }
}
