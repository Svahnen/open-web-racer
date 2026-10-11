use racer_content::project_tracks::{
    load_project_track, project_track_catalog, project_track_id_from_profile,
    project_track_profile_id,
};
use racer_content::rcp_track::RcpTrackScene;
use racer_sim::{DriverInput, RaceWorld, SimTiming, Vec2};

fn world_for(track: &RcpTrackScene) -> RaceWorld {
    let mut world = RaceWorld::new(SimTiming::default(), track.runtime.clone());
    world.set_spawn(Vec2::new(0.0, 0.0));
    world
        .install_preview_collision(track.collision.clone())
        .unwrap();
    world
}

/// Pure-pursuit driver on the first AI lane (the racing line).
fn drive_lap(track: &RcpTrackScene, max_ticks: u64) -> (RaceWorld, u64) {
    let points: Vec<[f64; 2]> = track
        .collision
        .edge_rows
        .iter()
        .map(|r| [r.x, r.y])
        .collect();
    let profile = track
        .pack_track
        .ai
        .as_ref()
        .unwrap()
        .speed_profile
        .as_ref()
        .unwrap();
    let mut world = world_for(track);
    let n = points.len();
    let mut nearest = 0usize;
    let mut ticks = 0;
    while ticks < max_ticks && world.vehicle.completed_laps < 1 {
        let v = world.vehicle;
        // advance the nearest index monotonically
        loop {
            let d = |i: usize| {
                let p = points[i % n];
                (p[0] - v.position.x).hypot(p[1] - v.position.y)
            };
            if d(nearest + 1) < d(nearest) && nearest < n * 2 {
                nearest += 1;
            } else {
                break;
            }
        }
        let speed = v.speed();
        let look = 4 + (speed / 6.0) as usize;
        let target = points[(nearest + look) % n];
        let desired = (target[1] - v.position.y).atan2(target[0] - v.position.x);
        let mut err = desired - v.heading;
        while err > std::f64::consts::PI {
            err -= std::f64::consts::TAU;
        }
        while err < -std::f64::consts::PI {
            err += std::f64::consts::TAU;
        }
        let speed_target = f64::from(profile.forward_mps[((nearest + 3) % n) / 2]) * 0.8;
        world.step(DriverInput {
            throttle: if speed < speed_target { 1.0 } else { 0.0 },
            brake: if speed > speed_target + 4.0 { 0.6 } else { 0.0 },
            steer: (err * 2.5).clamp(-1.0, 1.0),
            ..DriverInput::default()
        });
        ticks += 1;
    }
    (world, ticks)
}

#[test]
fn catalog_lists_four_generated_tracks() {
    let catalog = project_track_catalog();
    assert_eq!(catalog.len(), 4);
    for info in &catalog {
        assert_eq!(info.fingerprint.len(), 64);
        let profile = project_track_profile_id(&info.id);
        assert_eq!(
            project_track_id_from_profile(&profile),
            Some(info.id.as_str())
        );
    }
}

#[test]
fn every_track_builds_a_scene_with_collision_and_gates() {
    for info in project_track_catalog() {
        let t = load_project_track(&info.id).unwrap_or_else(|e| panic!("{}: {e}", info.id));
        assert!(
            t.scene.meshes.iter().all(|m| m.is_well_formed()),
            "{}",
            info.id
        );
        assert!(!t.scene.background_mesh_indices.is_empty());
        assert!(t.runtime.checkpoints.len() >= 6);
        assert_eq!(t.spawn_slots.len(), 8);
        assert!(t.collision.edge_rows.len() > 400);
        assert!(
            t.ground_triangles > 4000,
            "{} ground {}",
            info.id,
            t.ground_triangles
        );
        assert!(!t.track_mesh_indices.is_empty());
        assert!(
            t.pack_track.ai.is_some() && t.pack_track.centerline.iter().all(|s| s.lanes.is_some())
        );
        // spawn slot 0 is the simulation origin, facing +X
        assert!(
            t.spawn_slots[0].x.abs() < 1e-6
                && t.spawn_slots[0].y.abs() < 1e-6
                && t.spawn_slots[0].heading.abs() < 1e-6
        );
        // the world accepts the geometry
        let world = world_for(&t);
        assert!(world.has_preview_collision());
    }
}

#[test]
fn demo_cars_replace_the_placeholder_car_and_register_on_the_track_pages() {
    use racer_content::project_cars::project_cars;
    use racer_render::{MeshInstance, vehicle_source_scene_on_white_page};
    let cars = project_cars();
    let mut track = load_project_track("pine-ridge").unwrap();
    let materials_before = track.scene.materials.len();
    let pages_before = track.scene.textures.layers.len();
    let mesh = cars.vehicle_mesh(2, 3).unwrap();
    track.set_vehicle(&mesh).unwrap();
    // the designated vehicle instance now uses the demo car's geometry
    let instance: MeshInstance = track.scene.instances[track.vehicle_instance_index];
    let scene_mesh = &track.scene.meshes[instance.mesh_index as usize];
    assert_eq!(scene_mesh.vertices.len(), mesh.mesh.vertices.len());
    assert!(scene_mesh.is_well_formed());
    assert_eq!(
        track.scene.materials.len(),
        materials_before + mesh.materials.len()
    );
    // paint materials tint the scene's white page, no page is added
    assert_eq!(track.scene.textures.layers.len(), pages_before);
    let white = track
        .scene
        .materials
        .iter()
        .find(|m| m.name == "car-paint")
        .unwrap()
        .texture;
    assert!(
        track.scene.materials[materials_before..]
            .iter()
            .all(|m| m.texture == white)
    );
    assert!(
        track.scene.textures.layers[white]
            .iter()
            .all(|byte| *byte == 255)
    );
    assert!(
        scene_mesh
            .vertices
            .iter()
            .all(|v| (v.material as usize) < track.scene.materials.len()
                && v.material as usize >= materials_before)
    );
    // other cars register as profiles on a matching white page
    let other = cars.vehicle_mesh(0, 0).unwrap();
    let source = vehicle_source_scene_on_white_page(
        &other,
        track.scene.textures.width,
        track.scene.textures.height,
    );
    assert!(source.textures.is_well_formed());
    assert_eq!(
        source.textures.layers[0],
        track.scene.textures.layers[white]
    );
}

#[test]
fn a_driven_lap_is_completed_on_every_track() {
    for info in project_track_catalog() {
        let t = load_project_track(&info.id).unwrap();
        let (world, ticks) = drive_lap(&t, 60 * 400);
        assert!(
            world.vehicle.completed_laps >= 1,
            "{}: lap not completed in {ticks} ticks (checkpoint {}, pos {:?})",
            info.id,
            world.vehicle.checkpoint,
            world.vehicle.position
        );
        eprintln!("{}: lap in {:.1} s", info.id, ticks as f64 / 60.0);
    }
}
