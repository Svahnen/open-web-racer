use super::*;
use crate::ai::*;
use crate::audio::*;
use crate::buffer::{BufferBuilder, ElementKind};
use crate::car::*;
use crate::common::*;
use crate::hud::*;
use crate::track::*;
use crate::vehicle::*;

fn quad(b: &mut BufferBuilder) -> Mesh {
    let positions = b.add_f32(
        ElementKind::Vec3,
        &[0., 0., 0., 1., 0., 0., 1., 0., -1., 0., 0., -1.],
    );
    let normals = b.add_f32(
        ElementKind::Vec3,
        &[0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0.],
    );
    let uv = b.add_f32(ElementKind::Vec2, &[0., 0., 1., 0., 1., 1., 0., 1.]);
    let indices = b.add_u16(&[0, 1, 2, 0, 2, 3]);
    let tri_surface = b.add_u8(&[0, 0]);
    Mesh {
        name: "quad".into(),
        positions,
        normals: Some(normals),
        uv: Some(uv),
        colors: None,
        indices,
        primitives: vec![Primitive {
            material: 0,
            first_index: 0,
            index_count: 6,
        }],
        triangle_surface: Some(tri_surface),
    }
}

fn texture(b: &mut BufferBuilder, name: &str) -> Texture {
    let pixels = b.add_rgba8(&[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 9, 9, 9, 0]);
    Texture {
        name: name.into(),
        width: 2,
        height: 2,
        pixels,
        wrap_u: WrapMode::Repeat,
        wrap_v: WrapMode::Clamp,
        smooth: false,
    }
}

fn material(texture: Option<u32>) -> Material {
    Material {
        name: "m".into(),
        texture,
        base_color: [1.0; 4],
        alpha_mode: AlphaMode::Cutout,
        alpha_cutoff: 0.5,
        double_sided: false,
        unlit: false,
        animation: None,
    }
}

fn synthetic_pack() -> Pack {
    // Track
    let mut tb = BufferBuilder::new("buffers/track.bin");
    let chunk = quad(&mut tb);
    let prop = quad(&mut tb);
    let t0 = texture(&mut tb, "a");
    let t1 = texture(&mut tb, "b");
    let (table, uri, bytes) = tb.finish();
    let mut animated = material(Some(0));
    animated.alpha_mode = AlphaMode::Blend;
    animated.animation = Some(TextureAnimation {
        frames: vec![0, 1],
        rate_hz: 4.0,
        looping: true,
    });
    let seg = |x: f32| CenterlineSegment {
        position: [x, 0., 0.],
        forward: [1., 0., 0.],
        up: [0., 1., 0.],
        length_m: 10.0,
        left_width_m: 4.0,
        right_width_m: 4.0,
        surface: 0,
        lanes: None,
    };
    let track = Track {
        id: "loop".into(),
        name: "Test Loop".into(),
        data: table,
        chunks: vec![TrackChunk {
            mesh: chunk,
            bounds: Some([[0., 0., -1.], [1., 0., 0.]]),
        }],
        materials: vec![animated],
        textures: vec![t0, t1],
        prop_meshes: vec![prop],
        props: vec![PropInstance {
            mesh: 0,
            transform: Transform::default(),
            kind: PropKind::Animated,
            animation_rate_hz: Some(2.0),
            trigger_radius_m: None,
        }],
        sky: Some(Sky {
            backdrop: SkyBackdrop::Panorama { strips: vec![0, 1] },
            horizon: HorizonParams {
                height_fraction: 0.4,
                parallax: 1.0,
                haze_color: Some([0.7, 0.8, 0.9, 1.0]),
                haze_distance_m: Some(500.0),
            },
            fill_above: [0.1, 0.2, 0.5, 1.0],
            fill_below: [0.2, 0.2, 0.2, 1.0],
        }),
        centerline: vec![seg(0.0), seg(10.0)],
        closed: false,
        walls: vec![WallPolyline {
            points: vec![[0., 0., 4.], [10., 0., 4.]],
            height_m: 1.0,
            side: WallSide::Left,
            closed: false,
        }],
        surfaces: vec![SurfaceType {
            name: "road".into(),
            friction_class: FrictionClass::Paved,
            friction: 1.0,
            rolling_resistance: 0.015,
        }],
        spawn_slots: vec![SpawnSlot {
            position: [1., 0., 0.],
            heading_rad: 0.0,
        }],
        checkpoints: vec![Gate {
            position: [0., 0., 0.],
            normal: [1., 0., 0.],
            width_m: 8.0,
        }],
        variants: VariantFlags {
            allow_mirror: true,
            allow_reverse: true,
        },
        default_laps: 3,
        ai: None,
    };
    let mut buffers = BufferBytes::new();
    buffers.insert(uri, bytes);

    // Car
    let mut cb = BufferBuilder::new("buffers/car.bin");
    let body = quad(&mut cb);
    let tex = texture(&mut cb, "paint");
    let idx = cb.add_u8(&[0, 1, 1, 255]);
    let (ctable, curi, cbytes) = cb.finish();
    buffers.insert(curi, cbytes);
    let car = Car {
        id: "hatch".into(),
        name: "Hatch".into(),
        data: ctable,
        lods: vec![CarLod {
            mesh: body,
            max_distance_m: None,
        }],
        material_slots: vec![MaterialSlot {
            name: "body".into(),
            material: material(Some(0)),
            paintable: true,
        }],
        textures: vec![tex],
        wheel_mesh: None,
        paint: Some(PaintSet {
            texture: 0,
            indexed_pixels: idx,
            base_palette: vec![[200, 0, 0, 255], [10, 10, 10, 255]],
            options: vec![
                PaintOption {
                    name: "blue".into(),
                    scheme: PaintScheme::Recolour {
                        entries: vec![PaletteEntry {
                            index: 0,
                            color: [0, 0, 200, 255],
                        }],
                    },
                },
                PaintOption {
                    name: "mono".into(),
                    scheme: PaintScheme::FullPalette {
                        colors: vec![[255; 4], [0, 0, 0, 255]],
                    },
                },
            ],
        }),
        wheels: vec![WheelAnchor {
            name: "fl".into(),
            position: [0.8, 0.3, -1.2],
            radius_m: 0.3,
            width_m: 0.2,
            steered: true,
            driven: false,
        }],
        lights: vec![LightAnchor {
            kind: LightKind::Head,
            position: [0.5, 0.6, -2.0],
            direction: [0., 0., -1.],
            color: [1., 1., 0.9, 1.],
        }],
        cameras: vec![CameraAnchor {
            name: "chase".into(),
            position: [0., 2., 5.],
            forward: [0., 0., -1.],
            fov_deg: 70.0,
        }],
        mirrors: vec![MirrorAnchor {
            name: "rear".into(),
            position: [0., 1.1, -0.3],
            forward: [0., 0., 1.],
            width_m: 0.25,
            height_m: 0.08,
        }],
        collision: CollisionBox {
            half_extents_m: [0.9, 0.7, 2.1],
            center: [0., 0.7, 0.],
        },
    };

    // Vehicle
    let vehicle = VehicleDef {
        id: "hatch-v".into(),
        name: "Hatch".into(),
        car: Some("hatch".into()),
        ai: None,
        classic: None,
        arcade: Some(ArcadeVehicleParams {
            mass_kg: 1200.0,
            wheelbase_m: 2.5,
            track_width_m: 1.5,
            wheel_radius_m: 0.3,
            peak_torque_nm: 250.0,
            idle_rpm: 900.0,
            redline_rpm: 6500.0,
            gear_ratios: vec![3.5, 2.2, 1.5, 1.1, 0.9],
            reverse_ratio: 3.2,
            final_drive: 3.9,
            drivetrain_efficiency: 0.9,
            max_brake_force_n: 12000.0,
            drag_area_m2: 0.65,
            downforce_area_m2: 0.0,
            rolling_resistance: 0.015,
            tyre_grip: 1.0,
            max_steer_angle_rad: 0.5,
            steer_falloff_speed_mps: 40.0,
            max_speed_mps: 55.0,
            front_drive_bias: 1.0,
            steer_rate_per_s: 3.0,
            steer_return_per_s: 5.0,
            lateral_grip_per_s: 4.0,
        }),
        engine_sound: Some(EngineSynthParams {
            idle_rpm: 900.0,
            redline_rpm: 6500.0,
            cylinders: 4,
            harmonics: vec![1.0, 0.5, 0.3],
            filter_hz_idle: 600.0,
            filter_hz_redline: 3200.0,
            noise_gain: 0.1,
            gain_idle: 0.3,
            gain_redline: 0.6,
        }),
        model: Some(ExternalModel {
            id: "vendor.example".into(),
            params: serde_json::json!({"k": [1, 2, 3]}),
        }),
    };

    // Audio
    let mut ab = BufferBuilder::new("buffers/audio.bin");
    let samples = ab.add_i16(&[0, 1000, -1000, 500, -500, 0, 100, -100]);
    let (atable, auri, abytes) = ab.finish();
    buffers.insert(auri, abytes);
    let curve = |y0: f32, y1: f32| Curve {
        points: vec![[800.0, y0], [6500.0, y1]],
    };
    let audio = AudioSet {
        data: atable,
        clips: vec![Clip {
            id: "engine".into(),
            sample_rate: 22050,
            channels: 1,
            format: SampleFormat::Pcm16,
            samples,
            loop_points: Some(LoopPoints {
                start_frame: 2,
                end_frame: 8,
            }),
        }],
        engine: Some(EngineSoundParams {
            idle_rpm: 800.0,
            max_rpm: 6500.0,
            layers: vec![
                EngineLayer {
                    clip: 0,
                    throttle: ThrottleState::On,
                    reference_rpm: 3000.0,
                    gain_curve: curve(0.2, 1.0),
                    pitch_curve: curve(0.5, 2.0),
                },
                EngineLayer {
                    clip: 0,
                    throttle: ThrottleState::Off,
                    reference_rpm: 3000.0,
                    gain_curve: curve(0.1, 0.6),
                    pitch_curve: curve(0.5, 2.0),
                },
            ],
        }),
        cues: vec![EventCue {
            kind: CueKind::Skid,
            clip: 0,
            gain: 0.7,
            pitch: 1.0,
            cooldown_s: 0.1,
        }],
    };

    // HUD
    let mut hb = BufferBuilder::new("buffers/hud.bin");
    let atlas = {
        let pixels = hb.add_rgba8(&vec![255u8; 16 * 16 * 4]);
        Texture {
            name: "atlas".into(),
            width: 16,
            height: 16,
            pixels,
            wrap_u: WrapMode::Clamp,
            wrap_v: WrapMode::Clamp,
            smooth: false,
        }
    };
    let (htable, huri, hbytes) = hb.finish();
    buffers.insert(huri, hbytes);
    let rect = |x| AtlasRect {
        x,
        y: 0,
        w: 4,
        h: 8,
    };
    let hud = Hud {
        data: htable,
        atlas,
        rects: [
            ("d0".to_string(), rect(0)),
            ("d1".to_string(), rect(4)),
            (
                "dial".to_string(),
                AtlasRect {
                    x: 0,
                    y: 8,
                    w: 16,
                    h: 8,
                },
            ),
        ]
        .into_iter()
        .collect(),
        reference_resolution: [640, 480],
        layout: vec![
            HudElement {
                id: "speed".into(),
                anchor: Anchor::BottomRight,
                offset: [-10.0, -10.0],
                scale: 1.0,
                content: ElementContent::Number {
                    strip: 0,
                    source: "speed".into(),
                    digits: 3,
                },
            },
            HudElement {
                id: "rev".into(),
                anchor: Anchor::BottomLeft,
                offset: [10.0, -10.0],
                scale: 1.0,
                content: ElementContent::Gauge { gauge: 0 },
            },
        ],
        gauges: vec![Gauge {
            id: "rev".into(),
            style: GaugeStyle::Needle,
            source: "engine_rpm".into(),
            min: 0.0,
            max: 7000.0,
            rect: "dial".into(),
            angle_min_deg: -120.0,
            angle_max_deg: 120.0,
            pivot: [8.0, 4.0],
        }],
        digit_strips: vec![DigitStrip {
            id: "digits".into(),
            glyphs: "01".into(),
            rects: vec!["d0".into(), "d1".into()],
            spacing_px: 1.0,
        }],
    };

    Pack {
        manifest: Some(Manifest {
            schema: RCP_SCHEMA_ID.into(),
            id: "synthetic-test".into(),
            name: "Synthetic Test Pack".into(),
            version: Some("1.0.0".into()),
            license: "CC0-1.0".into(),
            author: "Test".into(),
            description: None,
            documents: DocumentIndex::default(),
        }),
        tracks: vec![track],
        cars: vec![car],
        vehicles: vec![vehicle],
        audio: vec![audio],
        hud: vec![hud],
        ai: Vec::new(),
        buffers,
    }
}

fn messages(p: &Pack) -> String {
    p.validate().unwrap_err().to_string()
}

#[test]
fn synthetic_pack_validates() {
    synthetic_pack().validate().unwrap();
}

#[test]
fn round_trip_through_files() {
    let pack = synthetic_pack();
    let files = pack.to_files().unwrap();
    assert!(files.contains_key("pack.json"));
    assert!(files.contains_key("tracks/loop.json"));
    let back = Pack::from_files(&files).unwrap();
    back.validate().unwrap();
    let mut expected = pack.clone();
    expected.manifest.as_mut().unwrap().documents =
        back.manifest.as_ref().unwrap().documents.clone();
    assert_eq!(back, expected);
}

#[test]
fn typed_accessors_read_back_values() {
    let pack = synthetic_pack();
    let t = &pack.tracks[0];
    let pos = t
        .data
        .read_f32(t.chunks[0].mesh.positions, &pack.buffers)
        .unwrap();
    assert_eq!(pos.len(), 12);
    assert_eq!(pos[3], 1.0);
    let idx = t
        .data
        .read_u32(t.chunks[0].mesh.indices, &pack.buffers)
        .unwrap();
    assert_eq!(idx, vec![0, 1, 2, 0, 2, 3]);
    let px = t
        .data
        .read_f32(t.textures[0].pixels, &pack.buffers)
        .unwrap();
    assert_eq!(px[0], 1.0);
    let a = &pack.audio[0];
    let s = a.data.read_i16(a.clips[0].samples, &pack.buffers).unwrap();
    assert_eq!(s[2], -1000);
    assert!(
        a.data
            .read_i16(a.clips[0].samples, &BufferBytes::new())
            .is_err()
    );
}

#[test]
fn json_schemas_cover_all_documents() {
    let s = json_schemas();
    assert_eq!(s.len(), 7);
    for (name, v) in &s {
        assert!(v.get("$schema").is_some(), "{name}");
        assert!(
            v.get("definitions").is_some() || v.get("properties").is_some(),
            "{name}"
        );
    }
    assert!(s["track"]["properties"].get("checkpoints").is_some());
}

#[test]
fn rejects_wrong_schema_version() {
    let mut p = synthetic_pack();
    p.manifest.as_mut().unwrap().schema = "rcp/2".into();
    assert!(messages(&p).contains("manifest.schema"));
    let files = p.to_files().unwrap();
    assert!(matches!(
        Pack::from_files(&files),
        Err(PackError::Invalid(_))
    ));
}

#[test]
fn rejects_buffer_range_overflow() {
    let mut p = synthetic_pack();
    p.tracks[0].data.accessors[0].count = 1000;
    let m = messages(&p);
    assert!(m.contains("exceed view length"), "{m}");
}

#[test]
fn rejects_missing_or_short_buffer_file() {
    let mut p = synthetic_pack();
    p.buffers.remove("buffers/track.bin");
    assert!(messages(&p).contains("missing from the pack"));
    let mut p = synthetic_pack();
    p.buffers.get_mut("buffers/track.bin").unwrap().pop();
    assert!(messages(&p).contains("does not match file size"));
}

#[test]
fn rejects_path_traversal_uri() {
    let mut p = synthetic_pack();
    p.tracks[0].data.buffers[0].uri = "../evil.bin".into();
    assert!(messages(&p).contains("relative forward-slash path"));
}

#[test]
fn rejects_index_beyond_vertices_and_bad_counts() {
    let mut p = synthetic_pack();
    let idx = p.tracks[0].chunks[0].mesh.indices;
    let view = p.tracks[0].data.accessors[idx as usize].view as usize;
    let off = p.tracks[0].data.views[view].byte_offset as usize;
    p.buffers.get_mut("buffers/track.bin").unwrap()[off] = 9;
    assert!(messages(&p).contains("references a vertex beyond"));
    let mut p = synthetic_pack();
    p.tracks[0].chunks[0].mesh.primitives[0].index_count = 9;
    assert!(messages(&p).contains("exceeds the 6 indices"));
    let mut p = synthetic_pack();
    p.tracks[0].chunks[0].mesh.primitives[0].material = 5;
    assert!(messages(&p).contains("material index 5 out of range"));
}

#[test]
fn rejects_bad_track_content() {
    let mut p = synthetic_pack();
    p.tracks[0].checkpoints.clear();
    p.tracks[0].spawn_slots.clear();
    p.tracks[0].centerline[0].surface = 3;
    p.tracks[0].props[0].animation_rate_hz = None;
    let m = messages(&p);
    assert!(m.contains("checkpoints"), "{m}");
    assert!(m.contains("spawn slot"), "{m}");
    assert!(m.contains("surface index 3"), "{m}");
    assert!(m.contains("animation_rate_hz"), "{m}");
}

#[test]
fn rejects_bad_texture_and_animation() {
    let mut p = synthetic_pack();
    p.tracks[0].textures[0].width = 3;
    p.tracks[0].materials[0].animation.as_mut().unwrap().frames = vec![0, 7];
    let m = messages(&p);
    assert!(m.contains("count 4, expected 6"), "{m}");
    assert!(m.contains("texture index 7"), "{m}");
}

#[test]
fn rejects_bad_car() {
    let mut p = synthetic_pack();
    p.cars[0].paint.as_mut().unwrap().options[1] = PaintOption {
        name: "short".into(),
        scheme: PaintScheme::FullPalette {
            colors: vec![[0; 4]],
        },
    };
    p.cars[0].wheels.clear();
    p.cars[0].collision.half_extents_m[1] = 0.0;
    let m = messages(&p);
    assert!(m.contains("full palette has 1 entries"), "{m}");
    assert!(m.contains("wheel anchor"), "{m}");
    assert!(m.contains("half_extents_m[1]"), "{m}");
}

#[test]
fn rejects_bad_vehicle_audio_hud() {
    let mut p = synthetic_pack();
    p.vehicles[0].arcade.as_mut().unwrap().redline_rpm = 100.0;
    p.vehicles[0].car = Some("nope".into());
    p.audio[0].clips[0].loop_points = Some(LoopPoints {
        start_frame: 4,
        end_frame: 99,
    });
    p.audio[0].cues[0].clip = 4;
    p.hud[0].rects.get_mut("d0").unwrap().w = 100;
    p.hud[0].gauges[0].rect = "missing".into();
    p.hud[0].digit_strips[0].rects.pop();
    let m = messages(&p);
    for needle in [
        "redline_rpm must exceed idle_rpm",
        "unknown car",
        "loop 4..99",
        "clip index 4",
        "exceeds the 16x16 atlas",
        "unknown rect \"missing\"",
        "2 glyphs but 1 rects",
    ] {
        assert!(m.contains(needle), "missing {needle:?} in {m}");
    }
}

#[test]
fn vehicle_needs_some_model() {
    let mut p = synthetic_pack();
    p.vehicles[0].arcade = None;
    p.vehicles[0].model = None;
    assert!(messages(&p).contains("needs `arcade` or `classic` parameters"));
}

#[test]
fn curve_eval_interpolates_and_clamps() {
    let c = Curve {
        points: vec![[0.0, 0.0], [10.0, 1.0]],
    };
    assert_eq!(c.eval(-5.0), 0.0);
    assert!((c.eval(5.0) - 0.5).abs() < 1e-6);
    assert_eq!(c.eval(50.0), 1.0);
}

#[test]
fn manifest_rejects_bad_id_and_unknown_fields() {
    let mut p = synthetic_pack();
    p.manifest.as_mut().unwrap().id = "Bad Id".into();
    assert!(messages(&p).contains("manifest.id"));
    let json = r#"{"schema":"rcp/1","id":"x","name":"n","license":"MIT","author":"a","bogus":1}"#;
    assert!(serde_json::from_str::<Manifest>(json).is_err());
}

fn ai_set() -> AiSet {
    let personality = |name: &str, speed: f32| Personality {
        name: name.into(),
        speed_factor: [speed * 0.95, speed],
        accel_factor: [0.9, 1.0],
        handling_adjust: 1.0,
        blocker_look_ahead_m: 40.0,
        lane_change_speed_limit_mps: 60.0,
        steer_look_ahead_per_mps: 0.5,
        steer_look_ahead_min_rows: 2,
        steer_look_ahead_max_rows: 12,
        lane_slack_m: [-0.5, 0.0, 0.0, 0.5],
        slack_redraw_one_in: 40,
        speed_glue: vec![1.0; GLUE_CURVE_LEN],
        accel_glue: vec![1.0; GLUE_CURVE_LEN],
        aggression: None,
    };
    let gain = GainSet {
        lateral_pos_to_vel: 1.0,
        max_lateral_vel_mps: 5.0,
        lateral_vel_to_accel: 4.0,
        heading_to_yaw_rate: 2.0,
        max_yaw_rate_rad_s: 2.5,
        yaw_rate_to_accel: 8.0,
        low_speed_mps: 6.0,
        low_speed_lateral_factor: 0.5,
        low_speed_yaw_factor: 0.4,
    };
    AiSet {
        id: "default".into(),
        name: "Default drivers".into(),
        personalities: vec![personality("steady", 1.0), personality("quick", 1.02)],
        catch_up: CatchUp {
            speed_scale: 1.0,
            accel_scale: 1.0,
            step_segments: 15,
            fade_in_segments: [[0; 4]; 3],
            clamp_range: [0.6, 1.3],
            clamp_within_segments: 20,
            start_boost_segments: 20,
            start_boost_max: 0.25,
        },
        lane_weights: LaneWeights {
            absent_now: 1000.0,
            absent_ahead: 400.0,
            absent_near: 600.0,
            shoulder: 200.0,
            edge: 300.0,
            adjacent_car: 500.0,
            blocker_speed: 20.0,
            obstacle: 30.0,
            hint: 40.0,
            block_human: 25.0,
        },
        gains: Gains {
            normal: gain.clone(),
            recovery: gain,
        },
    }
}

#[test]
fn ai_content_validates_and_round_trips() {
    let mut p = synthetic_pack();
    p.ai.push(ai_set());
    p.tracks[0].centerline[0].lanes = Some(SegmentLanes {
        left_count: 1,
        right_count: 2,
        left_width_m: 3.0,
        right_width_m: 3.0,
        availability_mask: Some(lane_mask_bit(6) | lane_mask_bit(7) | lane_mask_bit(8)),
        preferred_lane: Some(7),
    });
    p.tracks[0].ai = Some(TrackAi {
        default_speed_mps: 40.0,
        speed_profile: Some(SpeedProfile {
            segments_per_entry: 2,
            forward_mps: vec![50],
            reverse_mps: None,
        }),
        grid: Some(GridLayout {
            start_segment: 0,
            spacing_segments: 5,
        }),
        late_pace: vec![0.98, 1.0],
        late_pace_after_segments: 100,
        pace_trim: Some(PaceTrim {
            after_segments: 300,
            factor: 0.97,
        }),
    });
    p.vehicles[0].ai = Some(VehicleAi {
        speed_cap_mps: 70.0,
        acceleration_mps2: vec![8.0, 7.5, 7.0],
        brake_mps2: 15.0,
        lateral_accel_cap_mps2: 14.0,
        yaw_accel_cap_rad_s2: 12.0,
        misalignment: Some(Misalignment {
            start_deg: 3.0,
            full_deg: 10.0,
            max_slowdown: 0.2,
        }),
        shift_pause_updates: 0,
    });
    p.validate().unwrap();
    let files = p.to_files().unwrap();
    assert!(files.contains_key("ai/default.json"));
    let back = Pack::from_files(&files).unwrap();
    back.validate().unwrap();
    assert_eq!(back.ai, p.ai);
    assert_eq!(back.tracks[0].ai, p.tracks[0].ai);
}

#[test]
fn ai_content_rejects_bad_values() {
    let mut p = synthetic_pack();
    let mut set = ai_set();
    set.personalities[0].speed_glue.pop();
    set.personalities[1].slack_redraw_one_in = 0;
    set.catch_up.step_segments = 0;
    set.lane_weights.edge = -1.0;
    p.ai.push(set);
    p.tracks[0].centerline[0].lanes = Some(SegmentLanes {
        left_count: 0,
        right_count: 0,
        left_width_m: 3.0,
        right_width_m: 3.0,
        availability_mask: Some(0x8001),
        preferred_lane: Some(14),
    });
    p.tracks[0].ai = Some(TrackAi {
        default_speed_mps: 40.0,
        speed_profile: Some(SpeedProfile {
            segments_per_entry: 1,
            forward_mps: vec![50],
            reverse_mps: None,
        }),
        grid: Some(GridLayout {
            start_segment: 9,
            spacing_segments: 0,
        }),
        late_pace: vec![1.0, 1.0, 1.0],
        late_pace_after_segments: 100,
        pace_trim: None,
    });
    let m = messages(&p);
    for needle in [
        "speed_glue",
        "slack_redraw_one_in",
        "step_segments",
        "lane_weights.edge",
        "at least one driving lane",
        "availability_mask",
        "preferred_lane",
        "forward_mps",
        "spacing_segments",
        "start_segment",
        "late_pace",
    ] {
        assert!(m.contains(needle), "{needle} missing in {m}");
    }
}

#[test]
fn lane_mask_bits_follow_the_slot_rule() {
    assert_eq!(lane_mask_bit(-2), 0x8000);
    assert_eq!(lane_mask_bit(0), 0x4000);
    assert_eq!(lane_mask_bit(14), 1);
    assert_eq!(lane_mask_bit(15), 1);
}

#[test]
fn rejects_bad_arcade_dynamics_and_engine_sound() {
    let mut p = synthetic_pack();
    {
        let a = p.vehicles[0].arcade.as_mut().unwrap();
        a.steer_rate_per_s = 0.0;
        a.lateral_grip_per_s = 99.0;
        a.max_speed_mps = 90.0;
    }
    {
        let e = p.vehicles[0].engine_sound.as_mut().unwrap();
        e.cylinders = 0;
        e.harmonics = vec![];
        e.gain_idle = 2.0;
        e.redline_rpm = 100.0;
    }
    let m = messages(&p);
    for needle in [
        "steer_rate_per_s",
        "lateral_grip_per_s",
        "top gear reaches only",
        "cylinders must be 1..=16",
        "1 to 8 harmonics",
        "engine_sound.gain_idle",
        "engine_sound: redline_rpm",
    ] {
        assert!(m.contains(needle), "missing {needle:?} in {m}");
    }
}

#[test]
fn legacy_vehicle_json_without_new_fields_still_parses() {
    let p = synthetic_pack();
    let mut v = serde_json::to_value(&p.vehicles[0]).unwrap();
    let a = v["arcade"].as_object_mut().unwrap();
    for k in [
        "steer_rate_per_s",
        "steer_return_per_s",
        "lateral_grip_per_s",
    ] {
        a.remove(k);
    }
    v.as_object_mut().unwrap().remove("engine_sound");
    let parsed: VehicleDef = serde_json::from_value(v).unwrap();
    let a = parsed.arcade.unwrap();
    assert_eq!(a.steer_rate_per_s, 3.0);
    assert_eq!(a.lateral_grip_per_s, 4.0);
    assert!(parsed.engine_sound.is_none());
}

fn classic_params() -> ClassicVehicleParams {
    ClassicVehicleParams {
        upshift_mps: vec![27.4, 39.9, 52.3, 62.3],
        downshift_mps: vec![22.7, 33.1, 44.5, 53.2],
        top_speed_mps: 80.0,
        accel_scale: 1.0,
        coast_scale: 1.0,
        brake_scale: 1.0,
        shift_seconds: 0.2755,
        reverse_speed_mps: 29.4,
        steer_gain: 18.3,
        grip: 3.3,
        low_speed_yaw_rate: 1.73,
        yaw_boost_speed_mps: 68.6,
    }
}

#[test]
fn classic_params_validate_and_round_trip() {
    let mut p = synthetic_pack();
    p.vehicles[0].classic = Some(classic_params());
    p.validate().unwrap();
    let json = serde_json::to_string(&p.vehicles[0]).unwrap();
    let back: VehicleDef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.classic, p.vehicles[0].classic);
    // A classic block alone satisfies the "some model" rule.
    p.vehicles[0].arcade = None;
    p.validate().unwrap();
}

#[test]
fn rejects_bad_classic_params() {
    let mut p = synthetic_pack();
    let mut c = classic_params();
    c.downshift_mps[1] = 60.0;
    c.top_speed_mps = 50.0;
    c.grip = 0.0;
    c.yaw_boost_speed_mps = 5.0;
    p.vehicles[0].classic = Some(c);
    let m = messages(&p);
    for needle in [
        "below its upshift speed",
        "classic.top_speed_mps",
        "classic.grip",
        "classic.yaw_boost_speed_mps",
    ] {
        assert!(m.contains(needle), "missing {needle:?} in {m}");
    }
    let mut c = classic_params();
    c.downshift_mps.pop();
    p.vehicles[0].classic = Some(c);
    assert!(messages(&p).contains("upshift_mps needs 1 to 7 entries"));
}
