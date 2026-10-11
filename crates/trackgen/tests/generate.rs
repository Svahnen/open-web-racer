use racer_pack_schema::Pack;
use racer_trackgen::{builtin_specs, find_spec, generate_files, generate_pack};

/// Vertices the renderer expands for all prop instances.
fn prop_vertices(pack: &Pack) -> usize {
    let t = &pack.tracks[0];
    t.props
        .iter()
        .map(|p| t.data.accessors[t.prop_meshes[p.mesh as usize].positions as usize].count as usize)
        .sum()
}

fn triangle_counts(pack: &Pack) -> (usize, usize, usize) {
    let t = &pack.tracks[0];
    let count = |m: &racer_pack_schema::common::Mesh| {
        t.data.accessors[m.indices as usize].count as usize / 3
    };
    let road: usize = t
        .chunks
        .iter()
        .filter(|c| c.mesh.name.starts_with("road"))
        .map(|c| count(&c.mesh))
        .sum();
    let terrain: usize = t
        .chunks
        .iter()
        .filter(|c| c.mesh.name.starts_with("terrain"))
        .map(|c| count(&c.mesh))
        .sum();
    let props: usize = t
        .props
        .iter()
        .map(|p| count(&t.prop_meshes[p.mesh as usize]))
        .sum();
    (road, terrain, props)
}

#[test]
fn every_builtin_track_validates_and_has_expected_content() {
    for spec in builtin_specs() {
        let pack = generate_pack(&spec);
        pack.validate()
            .unwrap_or_else(|e| panic!("{}: {e}", spec.id));
        let t = &pack.tracks[0];
        let road = racer_trackgen::build_road(&spec);
        let st = road.stats();
        assert!(
            (2000.0..5000.0).contains(&st.length_m),
            "{} length {}",
            spec.id,
            st.length_m
        );
        assert!(t.closed && t.variants.allow_mirror && t.variants.allow_reverse);
        assert_eq!(t.spawn_slots.len(), 8);
        assert!(t.checkpoints.len() >= 6);
        assert_eq!(t.default_laps, spec.laps);
        assert!(t.sky.is_some());
        assert!(!t.props.is_empty() && !t.walls.is_empty());
        let ai = t.ai.as_ref().unwrap();
        let profile = ai.speed_profile.as_ref().unwrap();
        assert_eq!(profile.forward_mps.len(), t.centerline.len().div_ceil(2));
        assert!(t.centerline.iter().all(|s| s.lanes.is_some()));
        assert!(profile.forward_mps.iter().all(|v| (14..=64).contains(v)));
        // corners are slower than straights
        let (lo, hi) = (
            profile.forward_mps.iter().min().unwrap(),
            profile.forward_mps.iter().max().unwrap(),
        );
        assert!(hi > lo, "{} profile is flat", spec.id);
        assert!(
            st.min_clearance_m > 100.0,
            "{} clearance {}",
            spec.id,
            st.min_clearance_m
        );
        let (road_tris, terrain_tris, prop_tris) = triangle_counts(&pack);
        eprintln!(
            "{}: {:.2} km, road {} tris, terrain {} tris, props {} ({} instances, {} verts), textures {}, buffer {:.2} MiB",
            spec.id,
            st.length_m / 1000.0,
            road_tris,
            terrain_tris,
            prop_tris,
            t.props.len(),
            prop_vertices(&pack),
            t.textures.len(),
            pack.buffers.values().map(Vec::len).sum::<usize>() as f64 / 1048576.0
        );
    }
}

#[test]
fn same_spec_generates_identical_bytes() {
    let spec = find_spec("night-harbour").unwrap();
    let a = generate_files(&spec);
    let b = generate_files(&spec);
    assert_eq!(a, b);
}

#[test]
fn different_tracks_and_variants_differ() {
    let a = generate_files(&find_spec("pine-ridge").unwrap());
    let b = generate_files(&find_spec("pine-ridge-mirror").unwrap());
    let c = generate_files(&find_spec("pine-ridge-reverse").unwrap());
    assert_ne!(
        a["buffers/pine-ridge.bin"],
        b["buffers/pine-ridge-mirror.bin"]
    );
    assert_ne!(
        b["buffers/pine-ridge-mirror.bin"],
        c["buffers/pine-ridge-reverse.bin"]
    );
}

/// Where (percent of the lap) the named structure's centre vertex lies.
fn lap_percent(pack: &Pack, name: &str) -> Option<f32> {
    let t = &pack.tracks[0];
    let inst = t
        .props
        .iter()
        .find(|p| t.prop_meshes[p.mesh as usize].name == name)?;
    let acc = &t.data.accessors[t.prop_meshes[inst.mesh as usize].positions as usize];
    let bytes = pack.buffers.values().next()?;
    let view = &t.data.views[acc.view as usize];
    let off = view.byte_offset as usize + acc.byte_offset as usize;
    let rd = |i: usize| f32::from_le_bytes(bytes[off + i * 4..off + i * 4 + 4].try_into().unwrap());
    let mid = (acc.count as usize / 2) * 3;
    let (x, z) = (rd(mid), rd(mid + 2));
    let (mut best, mut at) = (f32::MAX, 0usize);
    for (k, c) in t.centerline.iter().enumerate() {
        let d = (c.position[0] - x).hypot(c.position[2] - z);
        if d < best {
            best = d;
            at = k;
        }
    }
    Some(100.0 * at as f32 / t.centerline.len() as f32)
}

#[test]
fn pine_ridge_has_tunnel_overpass_and_dressing() {
    let pack = generate_pack(&find_spec("pine-ridge").unwrap());
    let t = &pack.tracks[0];
    let names: Vec<&str> = t.prop_meshes.iter().map(|m| m.name.as_str()).collect();
    for wanted in ["tunnel", "overpass"] {
        assert!(names.contains(&wanted), "missing {wanted}: {names:?}");
    }
    let count = |prefix: &str| {
        t.props
            .iter()
            .filter(|p| t.prop_meshes[p.mesh as usize].name.starts_with(prefix))
            .count()
    };
    eprintln!(
        "cabins {} cliffs {} signs {} far pines {}",
        count("cabin"),
        count("cliff"),
        count("sign"),
        count("far-pine")
    );
    assert!(count("cabin") >= 6, "cabins {}", count("cabin"));
    assert!(count("cliff") >= 20, "cliffs {}", count("cliff"));
    assert!(count("sign") >= 3, "signs {}", count("sign"));
    assert!(count("far-pine") >= 600, "far pines {}", count("far-pine"));
    for name in ["tunnel", "overpass"] {
        eprintln!(
            "pine-ridge {name}: {:?}% of the lap",
            lap_percent(&pack, name)
        );
    }
    // fog needs the horizon haze the renderer reads
    let haze = t.sky.as_ref().unwrap().horizon.haze_distance_m;
    assert!(haze.is_some_and(|d| d > 100.0));
}

#[test]
fn other_themes_get_their_structures() {
    for (id, wanted) in [
        ("coastline-run", "overpass"),
        ("red-mesa", "tunnel"),
        ("red-mesa", "overpass"),
    ] {
        let pack = generate_pack(&find_spec(id).unwrap());
        let t = &pack.tracks[0];
        assert!(
            t.prop_meshes.iter().any(|m| m.name == wanted),
            "{id} has no {wanted}"
        );
        eprintln!(
            "{id} {wanted}: {:?}% of the lap",
            lap_percent(&pack, wanted)
        );
    }
    let night = generate_pack(&find_spec("night-harbour").unwrap());
    assert!(
        !night.tracks[0]
            .prop_meshes
            .iter()
            .any(|m| m.name == "tunnel" || m.name == "overpass")
    );
}
