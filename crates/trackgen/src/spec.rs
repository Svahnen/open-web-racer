//! Track specifications: seed, spline control points and theme.

use crate::math::hash_str;
use sha2::{Digest, Sha256};

/// Bumped whenever generator output changes for an unchanged spec.
pub const GENERATOR_VERSION: u32 = 2;

/// Visual and gameplay theme of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    /// Seaside road: beach sand, palms, villas, sea, bright sky.
    Coastline,
    /// Mountain road: pines, rock faces, wooden fences, cloudy sky.
    PineRidge,
    /// Desert plateau: red rock, mesas, cacti, hazy sky.
    RedMesa,
    /// Industrial port after dark: containers, cranes, lamps, water.
    NightHarbour,
}

impl Theme {
    pub fn name(self) -> &'static str {
        match self {
            Theme::Coastline => "coastline",
            Theme::PineRidge => "pine-ridge",
            Theme::RedMesa => "red-mesa",
            Theme::NightHarbour => "night-harbour",
        }
    }
}

/// One spline control point. `x`/`z` are metres on the ground plane (x east,
/// z south), `elev` the road surface height in metres and `width` the paved
/// width in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlPoint {
    pub x: f64,
    pub z: f64,
    pub elev: f64,
    pub width: f64,
}

const fn cp(x: f64, z: f64, elev: f64, width: f64) -> ControlPoint {
    ControlPoint { x, z, elev, width }
}

/// Everything needed to generate one track deterministically.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackSpec {
    /// Lowercase identifier (`a-z 0-9 -`), also the pack id.
    pub id: String,
    pub name: String,
    pub description: String,
    pub theme: Theme,
    pub seed: u64,
    /// Closed loop; the first control point is near the start line.
    pub control: Vec<ControlPoint>,
    pub laps: u32,
    /// Variant flags applied to the layout (see [`TrackSpec::mirrored`]).
    pub mirrored: bool,
    pub reversed: bool,
    /// Where on the lap the start line lies, as a fraction of its length
    /// (0 for the designed start; other values give scenic spot variants).
    pub start_fraction: f64,
}

impl TrackSpec {
    /// Mirror the layout left/right (x -> -x). Driving direction is kept.
    pub fn mirrored(&self) -> TrackSpec {
        let mut s = self.clone();
        for c in &mut s.control {
            c.x = -c.x;
        }
        s.mirrored = !self.mirrored;
        s.id = format!("{}-mirror", self.id);
        s.name = format!("{} (Mirror)", self.name);
        s
    }

    /// Drive the same road the other way round.
    pub fn reversed(&self) -> TrackSpec {
        let mut s = self.clone();
        s.control.reverse();
        s.reversed = !self.reversed;
        s.id = format!("{}-reverse", self.id);
        s.name = format!("{} (Reverse)", self.name);
        s
    }

    /// The same road with the start line moved to `percent` of the lap
    /// (1..=99). Used for scenic spot screenshots and tests, not for racing.
    pub fn starting_at(&self, percent: u32) -> TrackSpec {
        let mut s = self.clone();
        s.start_fraction = f64::from(percent.clamp(0, 99)) / 100.0;
        s.id = format!("{}-s{:02}", self.id, percent.clamp(0, 99));
        s.name = format!("{} (start {}%)", self.name, percent.clamp(0, 99));
        s
    }

    /// Hex SHA-256 identifying the content this spec generates. It is derived
    /// from the spec and [`GENERATOR_VERSION`], so every build that agrees on
    /// them agrees on the fingerprint without hashing megabytes of output.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        h.update(b"racer-trackgen/spec-v1\0");
        h.update(GENERATOR_VERSION.to_le_bytes());
        for s in [&self.id, &self.name, self.theme.name()] {
            h.update((s.len() as u64).to_le_bytes());
            h.update(s.as_bytes());
        }
        h.update(self.seed.to_le_bytes());
        h.update(self.laps.to_le_bytes());
        h.update([u8::from(self.mirrored), u8::from(self.reversed)]);
        h.update(self.start_fraction.to_bits().to_le_bytes());
        for c in &self.control {
            for v in [c.x, c.z, c.elev, c.width] {
                h.update(v.to_bits().to_le_bytes());
            }
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Seed mixed with a purpose label so independent streams do not correlate.
    pub fn stream(&self, label: &str) -> u64 {
        self.seed ^ hash_str(label)
    }
}

/// The four demo tracks shipped with the project.
pub fn builtin_specs() -> Vec<TrackSpec> {
    vec![coastline_run(), pine_ridge(), red_mesa(), night_harbour()]
}

/// Look up a builtin track by id. `-mirror` and `-reverse` suffixes select
/// the variants (`coastline-run-mirror`, `pine-ridge-reverse`).
pub fn find_spec(id: &str) -> Option<TrackSpec> {
    for base in builtin_specs() {
        if base.id == id {
            return Some(base);
        }
        if format!("{}-mirror", base.id) == id {
            return Some(base.mirrored());
        }
        if format!("{}-reverse", base.id) == id {
            return Some(base.reversed());
        }
        if let Some(rest) = id.strip_prefix(&format!("{}-s", base.id)) {
            if rest.len() == 2 {
                if let Ok(percent) = rest.parse::<u32>() {
                    return Some(base.starting_at(percent));
                }
            }
        }
    }
    None
}

fn coastline_run() -> TrackSpec {
    TrackSpec {
        id: "coastline-run".into(),
        name: "Coastline Run".into(),
        description:
            "Fast seaside road with a long beach straight, a headland hairpin and flowing esses."
                .into(),
        theme: Theme::Coastline,
        seed: 0x00C0_A571_17E0,
        control: vec![
            cp(-300.0, -262.0, 2.5, 14.0),
            cp(-100.0, -262.0, 3.0, 14.0),
            cp(250.0, -245.0, 6.0, 14.0),
            cp(560.0, -215.0, 10.0, 14.0),
            cp(720.0, -150.0, 13.0, 13.0),
            cp(770.0, -40.0, 15.0, 13.0),
            cp(700.0, 60.0, 15.0, 13.0),
            cp(540.0, 35.0, 13.0, 13.0),
            cp(380.0, -40.0, 11.0, 13.0),
            cp(230.0, -70.0, 9.0, 13.0),
            cp(90.0, -10.0, 7.0, 13.0),
            cp(-60.0, 60.0, 5.0, 13.0),
            cp(-200.0, 40.0, 4.0, 13.0),
            cp(-330.0, 130.0, 3.0, 14.0),
            cp(-520.0, 160.0, 2.0, 14.0),
            cp(-650.0, 0.0, 2.0, 14.0),
            cp(-590.0, -190.0, 2.0, 14.0),
            cp(-460.0, -258.0, 2.0, 14.0),
        ],
        laps: 3,
        mirrored: false,
        reversed: false,
        start_fraction: 0.0,
    }
}

fn pine_ridge() -> TrackSpec {
    TrackSpec {
        id: "pine-ridge".into(),
        name: "Pine Ridge".into(),
        description: "Climbing forest road with banked hairpins, crests and a fast descent.".into(),
        theme: Theme::PineRidge,
        seed: 0x0091_E21D_6E00,
        control: vec![
            cp(0.0, 0.0, 10.0, 12.0),
            cp(280.0, -10.0, 13.0, 12.0),
            cp(520.0, -60.0, 20.0, 12.0),
            cp(645.0, -160.0, 27.0, 12.0),
            cp(640.0, -265.0, 34.0, 11.0),
            cp(520.0, -325.0, 40.0, 11.0),
            cp(330.0, -320.0, 45.0, 11.0),
            cp(150.0, -300.0, 48.0, 11.0),
            cp(30.0, -350.0, 50.0, 11.0),
            cp(40.0, -450.0, 53.0, 11.0),
            cp(180.0, -500.0, 56.0, 11.0),
            cp(380.0, -515.0, 58.0, 11.0),
            cp(590.0, -560.0, 58.0, 12.0),
            cp(790.0, -600.0, 52.0, 12.0),
            cp(900.0, -470.0, 42.0, 12.0),
            cp(890.0, -250.0, 32.0, 12.0),
            cp(820.0, -60.0, 24.0, 12.0),
            cp(720.0, 70.0, 18.0, 12.0),
            cp(520.0, 125.0, 15.0, 12.0),
            cp(320.0, 110.0, 13.0, 12.0),
            cp(100.0, 120.0, 12.0, 12.0),
            cp(-130.0, 110.0, 11.0, 12.0),
            cp(-330.0, 100.0, 10.0, 12.0),
            cp(-450.0, 20.0, 10.0, 12.0),
            cp(-400.0, -60.0, 10.0, 12.0),
            cp(-220.0, -8.0, 10.0, 12.0),
        ],
        laps: 3,
        mirrored: false,
        reversed: false,
        start_fraction: 0.0,
    }
}

fn red_mesa() -> TrackSpec {
    TrackSpec {
        id: "red-mesa".into(),
        name: "Red Mesa".into(),
        description:
            "Wide desert plateau circuit with long sweepers, a jump-free crest and a tight infield."
                .into(),
        theme: Theme::RedMesa,
        seed: 0x004E_D3E5_A000,
        control: vec![
            cp(0.0, 0.0, 20.0, 16.0),
            cp(330.0, -10.0, 21.0, 16.0),
            cp(640.0, -60.0, 24.0, 16.0),
            cp(890.0, -200.0, 27.0, 16.0),
            cp(940.0, -420.0, 26.0, 16.0),
            cp(800.0, -590.0, 22.0, 15.0),
            cp(590.0, -610.0, 19.0, 15.0),
            cp(450.0, -500.0, 17.0, 14.0),
            cp(300.0, -420.0, 15.0, 14.0),
            cp(150.0, -500.0, 15.0, 14.0),
            cp(30.0, -600.0, 17.0, 14.0),
            cp(-180.0, -600.0, 20.0, 15.0),
            cp(-360.0, -500.0, 22.0, 16.0),
            cp(-420.0, -300.0, 23.0, 16.0),
            cp(-340.0, -120.0, 22.0, 16.0),
            cp(-190.0, -8.0, 21.0, 16.0),
        ],
        laps: 3,
        mirrored: false,
        reversed: false,
        start_fraction: 0.0,
    }
}

fn night_harbour() -> TrackSpec {
    TrackSpec {
        id: "night-harbour".into(),
        name: "Night Harbour".into(),
        description: "Floodlit street circuit through a container port: tight corners, quayside straights and a swooping bridge ramp.".into(),
        theme: Theme::NightHarbour,
        seed: 0x004A_8B00_17AA,
        control: vec![
            cp(0.0, 0.0, 3.0, 14.0),
            cp(300.0, 0.0, 3.0, 14.0),
            cp(400.0, -40.0, 3.0, 13.0),
            cp(460.0, -120.0, 4.0, 13.0),
            cp(450.0, -260.0, 6.0, 13.0),
            cp(380.0, -340.0, 8.0, 13.0),
            cp(240.0, -350.0, 9.0, 13.0),
            cp(130.0, -300.0, 8.0, 12.0),
            cp(80.0, -210.0, 6.0, 12.0),
            cp(-20.0, -170.0, 4.0, 12.0),
            cp(-130.0, -210.0, 3.0, 12.0),
            cp(-170.0, -310.0, 3.0, 12.0),
            cp(-300.0, -330.0, 3.0, 13.0),
            cp(-390.0, -240.0, 3.0, 13.0),
            cp(-400.0, -110.0, 3.0, 13.0),
            cp(-330.0, -5.0, 3.0, 13.0),
            cp(-150.0, 2.0, 3.0, 14.0),
        ],
        laps: 3,
        mirrored: false,
        reversed: false,
        start_fraction: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_are_unique_and_resolvable() {
        let specs = builtin_specs();
        assert_eq!(specs.len(), 4);
        for s in &specs {
            assert_eq!(find_spec(&s.id).unwrap(), *s);
            assert!(find_spec(&format!("{}-mirror", s.id)).unwrap().mirrored);
            assert!(find_spec(&format!("{}-reverse", s.id)).unwrap().reversed);
        }
        assert!(find_spec("nonexistent").is_none());
    }

    #[test]
    fn fingerprints_differ_between_tracks_and_variants() {
        let a = coastline_run();
        assert_eq!(a.fingerprint(), coastline_run().fingerprint());
        assert_ne!(a.fingerprint(), pine_ridge().fingerprint());
        assert_ne!(a.fingerprint(), a.mirrored().fingerprint());
        assert_eq!(a.fingerprint().len(), 64);
    }
}
