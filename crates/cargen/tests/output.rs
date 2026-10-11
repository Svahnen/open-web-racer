use racer_cargen::{DESIGNS, digest, file_digest, generate};
use racer_pack_schema::Pack;
use std::collections::BTreeSet;
use std::path::Path;

/// Digest of the complete generated folder. Update it (and regenerate
/// `content/demo/cars` with `cargo run -p racer-cargen`) only when the
/// designs are changed on purpose.
const PINNED_DIGEST: &str = "540387263bc9f60d28155bbaa8ab7234a90829e6fa7b319d39615b1fc988038f";

fn content_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/demo/cars")
}

#[test]
fn generation_is_deterministic() {
    let a = generate();
    let b = generate();
    assert_eq!(a, b);
    assert_eq!(digest(&a), digest(&b));
}

#[test]
fn output_digest_is_pinned() {
    assert_eq!(digest(&generate()), PINNED_DIGEST);
}

#[test]
fn committed_content_matches_the_generator() {
    let dir = content_dir();
    for (path, bytes) in generate() {
        let on_disk = std::fs::read(dir.join(&path))
            .unwrap_or_else(|e| panic!("{path} is missing from content/demo/cars: {e}"));
        assert_eq!(
            file_digest(&on_disk),
            file_digest(&bytes),
            "{path} is stale; run `cargo run -p racer-cargen`"
        );
    }
}

#[test]
fn files_round_trip_through_the_pack_loader_and_validate() {
    let files = generate();
    let pack = Pack::from_files(
        &files
            .iter()
            .filter(|(p, _)| !p.starts_with("gltf/"))
            .map(|(p, b)| (p.clone(), b.clone()))
            .collect(),
    )
    .expect("pack parses");
    pack.validate().expect("pack validates");
    assert_eq!(pack.cars.len(), 4);
    assert_eq!(pack.vehicles.len(), 4);
    let manifest = pack.manifest.as_ref().unwrap();
    assert_eq!(manifest.license, "Apache-2.0");
    for v in &pack.vehicles {
        let car = v.car.as_ref().expect("vehicle references its car");
        assert!(pack.cars.iter().any(|c| &c.id == car));
        assert!(v.arcade.is_some());
    }
}

#[test]
fn cars_have_wheels_paint_variants_and_a_wheel_mesh() {
    let pack = racer_cargen::rcp::pack();
    for car in &pack.cars {
        assert_eq!(car.wheels.len(), 4, "{}", car.id);
        assert_eq!(car.wheels.iter().filter(|w| w.steered).count(), 2);
        assert!(car.wheel_mesh.is_some());
        let paint = car.paint.as_ref().expect("paint set");
        assert_eq!(paint.base_palette.len(), 8);
        assert!(paint.options.len() >= 8);
        assert!(car.material_slots.iter().any(|s| s.paintable));
    }
}

#[test]
fn the_four_cars_are_geometrically_distinct() {
    let pack = racer_cargen::rcp::pack();
    let mut bodies = BTreeSet::new();
    let mut sizes = BTreeSet::new();
    for car in &pack.cars {
        let bytes = &pack.buffers[&car.data.buffers[0].uri];
        bodies.insert(file_digest(bytes));
        let positions = car
            .data
            .read_f32(car.lods[0].mesh.positions, &pack.buffers)
            .unwrap();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        // Wheels touch the ground and the body sits above it.
        assert!(lo[1] >= 0.0, "{}", car.id);
        sizes.insert(format!(
            "{:.2}x{:.2}x{:.2}",
            hi[0] - lo[0],
            hi[1] - lo[1],
            hi[2] - lo[2]
        ));
    }
    assert_eq!(bodies.len(), 4);
    assert_eq!(sizes.len(), 4, "silhouettes must differ: {sizes:?}");
    let ids: BTreeSet<_> = DESIGNS.iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), 4);
}

#[test]
fn validation_reports_a_broken_wheel_mesh() {
    let mut pack = racer_cargen::rcp::pack();
    pack.cars[0].wheel_mesh.as_mut().unwrap().primitives[0].material = 9;
    let err = pack.validate().unwrap_err().to_string();
    assert!(err.contains("wheel_mesh"), "{err}");
}

#[test]
fn gltf_files_are_well_formed() {
    use serde_json::Value;
    let files = generate();
    for d in DESIGNS {
        let text = std::str::from_utf8(&files[&format!("gltf/{}.gltf", d.id)]).unwrap();
        let doc: Value = serde_json::from_str(text).unwrap();
        assert_eq!(doc["asset"]["version"], "2.0");
        let nodes = doc["nodes"].as_array().unwrap();
        let wheels = nodes
            .iter()
            .filter(|n| n["name"].as_str().is_some_and(|s| s.starts_with("wheel_")))
            .count();
        assert_eq!(wheels, 4);
        assert_eq!(
            doc["extensions"]["KHR_materials_variants"]["variants"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        // Buffer: embedded and consistent with its declared length.
        let uri = doc["buffers"][0]["uri"].as_str().unwrap();
        let prefix = "data:application/octet-stream;base64,";
        assert!(uri.starts_with(prefix));
        let encoded = &uri[prefix.len()..];
        assert_eq!(
            encoded.len() / 4 * 3 - encoded.matches('=').count(),
            doc["buffers"][0]["byteLength"].as_u64().unwrap() as usize
        );
        // Every accessor stays inside its view.
        let views = doc["bufferViews"].as_array().unwrap();
        for a in doc["accessors"].as_array().unwrap() {
            let view = &views[a["bufferView"].as_u64().unwrap() as usize];
            let elem = match (
                a["componentType"].as_u64().unwrap(),
                a["type"].as_str().unwrap(),
            ) {
                (5126, "VEC3") => 12,
                (5123, "SCALAR") => 2,
                other => panic!("unexpected accessor {other:?}"),
            };
            assert!(a["count"].as_u64().unwrap() * elem <= view["byteLength"].as_u64().unwrap());
        }
        // The names of the shipped files carry no trademarks; sanity-check ASCII.
        assert!(text.is_ascii());
    }
}
