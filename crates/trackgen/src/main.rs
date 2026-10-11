//! `racer-trackgen`: export generated demo tracks as RCP pack directories.
//!
//! Usage: `racer-trackgen <track-id|all> <out-dir> [--map] [--pack]`
//!
//! `--pack` writes `<out-dir>/<track-id>/` with `pack.json`, `tracks/*.json`
//! and `buffers/*.bin`; `--map` writes a top-down layout preview PNG.

use racer_trackgen::{builtin_specs, find_spec, generate_files, layout_map, png};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: racer-trackgen <track-id|all> <out-dir> [--map] [--pack]");
        std::process::exit(2);
    }
    let specs = if args[0] == "all" {
        builtin_specs()
    } else {
        match find_spec(&args[0]) {
            Some(s) => vec![s],
            None => {
                eprintln!("unknown track {:?}", args[0]);
                std::process::exit(2);
            }
        }
    };
    let out = PathBuf::from(&args[1]);
    let want_map = args.iter().any(|a| a == "--map");
    let want_pack = args.iter().any(|a| a == "--pack");
    for spec in specs {
        let road = racer_trackgen::build_road(&spec);
        let st = road.stats();
        println!(
            "{}: length {:.0} m, min radius {:.0} m, max grade {:.1}%, max bank {:.1} deg, elevation range {:.1} m, clearance {:.0} m",
            spec.id,
            st.length_m,
            st.min_radius_m,
            st.max_grade * 100.0,
            st.max_bank_deg,
            st.elevation_range_m,
            st.min_clearance_m
        );
        std::fs::create_dir_all(&out).expect("create output directory");
        if want_map {
            let map = png::encode_rgba(512, 512, &layout_map(&road, 512));
            std::fs::write(out.join(format!("{}-map.png", spec.id)), map).expect("write map");
        }
        if want_pack {
            let started = std::time::Instant::now();
            let files = generate_files(&spec);
            let mut total = 0usize;
            for (path, bytes) in &files {
                let dest = out.join(&spec.id).join(path);
                std::fs::create_dir_all(dest.parent().expect("parent")).expect("create dirs");
                std::fs::write(&dest, bytes).expect("write pack file");
                total += bytes.len();
                println!("  {path}: {} bytes", bytes.len());
            }
            println!(
                "  pack total {:.2} MiB in {:.1} s",
                total as f64 / 1048576.0,
                started.elapsed().as_secs_f64()
            );
        }
    }
}
