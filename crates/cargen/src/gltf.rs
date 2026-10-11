//! glTF 2.0 export (`.gltf` with an embedded base64 buffer).
//!
//! Node layout: `car` (rotated half a turn so the nose faces +Z as glTF
//! expects) with children `body` and `wheel_fl|fr|rl|rr`. The wheels are
//! separate nodes sharing one mesh; wheels on the left are rotated half a turn
//! so the outer face points outwards. Paint colour variants use the
//! `KHR_materials_variants` extension on the `paint` and `accent` primitives.

use crate::design::Design;
use crate::geom::Role;
use crate::mesh::{Geometry, Tri, sorted_by_role, to_rcp, triangulate};
use crate::palette::{FIXED, PAINTS, linear, palette};
use serde_json::{Value, json};

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

struct Builder {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn view(&mut self, data: &[u8], target: u32) -> usize {
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        self.views.push(json!({
            "buffer": 0,
            "byteOffset": self.bytes.len(),
            "byteLength": data.len(),
            "target": target,
        }));
        self.bytes.extend_from_slice(data);
        self.views.len() - 1
    }

    fn f32s(&mut self, values: &[f32], kind: &str, with_bounds: bool) -> usize {
        let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&raw, 34962);
        let mut acc = json!({
            "bufferView": view,
            "componentType": 5126,
            "count": values.len() / 3,
            "type": kind,
        });
        if with_bounds {
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for v in values.chunks_exact(3) {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            acc["min"] = json!(lo);
            acc["max"] = json!(hi);
        }
        self.accessors.push(acc);
        self.accessors.len() - 1
    }

    fn indices(&mut self, values: &[u16]) -> usize {
        let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&raw, 34963);
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": 5123,
            "count": values.len(),
            "type": "SCALAR",
        }));
        self.accessors.len() - 1
    }
}

/// Triangles of one role as separate geometry.
fn role_geometry(tris: &[Tri], role: Role) -> Option<Geometry> {
    let own: Vec<Tri> = tris.iter().copied().filter(|t| t.role == role).collect();
    (!own.is_empty()).then(|| Geometry::from_tris(&own))
}

pub fn export(design: &Design) -> String {
    let body_tris = sorted_by_role(&triangulate(&design.body_faces()));
    let wheel_tris = sorted_by_role(&triangulate(&design.wheel_faces()));
    let mut b = Builder {
        bytes: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
    };

    // Materials: one per role (default paint), then paint/accent per scheme.
    let default_pal = palette(&PAINTS[design.default_paint]);
    let mat = |name: String, c: [u8; 4], rough: f64, emissive: bool| {
        let mut m = json!({
            "name": name,
            "pbrMetallicRoughness": {
                "baseColorFactor": [linear(c[0]), linear(c[1]), linear(c[2]), 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": rough,
            },
        });
        if emissive {
            m["emissiveFactor"] =
                json!([linear(c[0]) * 0.6, linear(c[1]) * 0.6, linear(c[2]) * 0.6]);
        }
        m
    };
    let mut materials: Vec<Value> = Role::ALL
        .iter()
        .map(|r| {
            let rough = match r {
                Role::Glass => 0.15,
                Role::Paint | Role::Accent => 0.45,
                _ => 0.85,
            };
            mat(
                r.name().to_string(),
                default_pal[*r as usize],
                rough,
                matches!(r, Role::LampFront | Role::LampRear),
            )
        })
        .collect();
    let mut variant_materials = Vec::new();
    for scheme in &PAINTS {
        let paint_idx = materials.len();
        materials.push(mat(
            format!("paint.{}", scheme.id),
            scheme.body,
            0.45,
            false,
        ));
        let accent_idx = materials.len();
        materials.push(mat(
            format!("accent.{}", scheme.id),
            scheme.accent,
            0.45,
            false,
        ));
        variant_materials.push((paint_idx, accent_idx));
    }
    let _ = FIXED;

    let mut meshes = Vec::new();
    for (name, tris, is_body) in [("body", &body_tris, true), ("wheel", &wheel_tris, false)] {
        let mut prims = Vec::new();
        for role in Role::ALL {
            let Some(g) = role_geometry(tris, role) else {
                continue;
            };
            let pos = b.f32s(&g.positions, "VEC3", true);
            let nor = b.f32s(&g.normals, "VEC3", false);
            let idx = b.indices(&g.indices);
            let mut prim = json!({
                "attributes": { "POSITION": pos, "NORMAL": nor },
                "indices": idx,
                "material": role as usize,
            });
            if is_body && matches!(role, Role::Paint | Role::Accent) {
                let mappings: Vec<Value> = variant_materials
                    .iter()
                    .enumerate()
                    .map(|(i, (p, a))| {
                        json!({
                            "material": if role == Role::Paint { *p } else { *a },
                            "variants": [i],
                        })
                    })
                    .collect();
                prim["extensions"] = json!({ "KHR_materials_variants": { "mappings": mappings } });
            }
            prims.push(prim);
        }
        meshes.push(json!({ "name": name, "primitives": prims }));
    }

    let mut nodes = vec![
        json!({
            "name": "car",
            "rotation": [0.0, 1.0, 0.0, 0.0],
            "children": [1, 2, 3, 4, 5],
            "extras": {
                "wheelbase_m": design.wheelbase,
                "paint_slots": ["paint", "accent"],
                "default_variant": PAINTS[design.default_paint].id,
            },
        }),
        json!({ "name": "body", "mesh": 0 }),
    ];
    for (name, centre) in design.wheel_centres() {
        let rcp = to_rcp(centre);
        let mut node = json!({
            "name": name,
            "mesh": 1,
            "translation": [rcp[0], rcp[1], rcp[2]],
            "extras": {
                "steered": name.starts_with("wheel_f"),
                "driven": if name.starts_with("wheel_f") { design.wheel.front_driven } else { design.wheel.rear_driven },
                "radius_m": design.wheel.radius,
                "width_m": design.wheel.width,
            },
        });
        if rcp[0] < 0.0 {
            node["rotation"] = json!([0.0, 1.0, 0.0, 0.0]);
        }
        nodes.push(node);
    }

    let doc = json!({
        "asset": {
            "version": "2.0",
            "generator": "racer-cargen",
            "copyright": "Project-owned generated content, Apache-2.0",
        },
        "extensionsUsed": ["KHR_materials_variants"],
        "extensions": {
            "KHR_materials_variants": {
                "variants": PAINTS.iter().map(|p| json!({ "name": p.id })).collect::<Vec<_>>(),
            },
        },
        "scene": 0,
        "scenes": [{ "name": design.name, "nodes": [0] }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "buffers": [{
            "byteLength": b.bytes.len(),
            "uri": format!("data:application/octet-stream;base64,{}", base64(&b.bytes)),
        }],
        "bufferViews": b.views,
        "accessors": b.accessors,
    });
    let mut text = serde_json::to_string_pretty(&doc).expect("glTF serializes");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
    }
}
