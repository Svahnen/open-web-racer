struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) @interpolate(flat) texture_layer: u32,
    @location(4) @interpolate(flat) uv_scale: vec2<f32>,
    @location(5) @interpolate(flat) uv_offset: vec2<f32>,
    @location(6) @interpolate(flat) uv_wrap: vec2<u32>,
    // rgb: fog colour, a: fog blend at this vertex
    @location(7) fog: vec4<f32>,
    // 1 when the scene opts in to warm/cool grading (project scenes), else 0
    @location(8) grade: f32,
};

@group(0) @binding(0) var material_texture: texture_2d_array<f32>;
@group(0) @binding(1) var material_sampler: sampler;

struct MirrorCamera {
    position: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    projection: vec4<f32>,
    fog: vec4<f32>,
    fog_range: vec4<f32>,
};

@group(1) @binding(0) var<uniform> mirror_camera: MirrorCamera;

@vertex
fn vs_main(
    @location(0) clip_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) texture_layer: u32,
    @location(5) uv_scale: vec2<f32>,
    @location(6) uv_offset: vec2<f32>,
    @location(7) uv_wrap: vec2<u32>,
) -> VertexOut {
    var out: VertexOut;
    out.clip_position = clip_position;
    out.normal = normal;
    out.uv = uv;
    out.tint = tint;
    out.texture_layer = texture_layer;
    out.uv_scale = uv_scale;
    out.uv_offset = uv_offset;
    out.uv_wrap = uv_wrap;
    out.fog = vec4<f32>(0.0);
    out.grade = 0.0;
    return out;
}

@vertex
fn vs_mirror(
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) texture_layer: u32,
    @location(5) uv_scale: vec2<f32>,
    @location(6) uv_offset: vec2<f32>,
    @location(7) uv_wrap: vec2<u32>,
) -> VertexOut {
    let relative = world_position.xyz - mirror_camera.position.xyz;
    let view_x = dot(relative, mirror_camera.right.xyz);
    let view_y = dot(relative, mirror_camera.up.xyz);
    let view_z = dot(relative, mirror_camera.forward.xyz);
    var out: VertexOut;
    out.clip_position = vec4<f32>(
        view_x * mirror_camera.projection.x / mirror_camera.projection.y,
        view_y * mirror_camera.projection.x,
        mirror_camera.projection.z * view_z - mirror_camera.projection.w,
        view_z,
    );
    out.normal = normal;
    out.uv = uv;
    out.tint = tint;
    out.texture_layer = texture_layer;
    out.uv_scale = uv_scale;
    out.uv_offset = uv_offset;
    out.uv_wrap = uv_wrap;
    let span = max(mirror_camera.fog_range.y - mirror_camera.fog_range.x, 1.0);
    let t = clamp((view_z - mirror_camera.fog_range.x) / span, 0.0, 1.0);
    out.fog = vec4<f32>(mirror_camera.fog.rgb, t * t * (3.0 - 2.0 * t) * mirror_camera.fog.a);
    out.grade = select(0.0, 1.0, mirror_camera.fog.a > 0.0);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return shade_material(in, true, true);
}

@fragment
fn fs_straight_alpha(in: VertexOut) -> @location(0) vec4<f32> {
    return shade_material(in, true, false);
}

@fragment
fn fs_background(in: VertexOut) -> @location(0) vec4<f32> {
    return shade_material(in, false, true);
}

fn shade_material(in: VertexOut, lit: bool, discard_cutout: bool) -> vec4<f32> {
    let light = normalize(vec3<f32>(-0.35, -0.45, 0.82));
    var diffuse = vec3<f32>(1.0);
    if lit {
        let n = normalize(in.normal);
        let ndl = max(dot(n, light), 0.0);
        if in.grade > 0.5 {
            // warm sun, cool sky-lit shade (ground bounce for downward faces)
            let shade = mix(vec3<f32>(0.30, 0.27, 0.24), vec3<f32>(0.31, 0.34, 0.43), 0.5 + 0.5 * n.z);
            let sun = vec3<f32>(1.06, 0.99, 0.90);
            diffuse = shade + (sun - shade) * ndl;
        } else {
            diffuse = vec3<f32>(0.34 + 0.66 * ndl);
        }
    }
    var source_uv = in.uv;
    if in.uv_wrap.x == 0u {
        source_uv.x = clamp(source_uv.x, 0.0, 1.0);
    } else {
        source_uv.x = fract(source_uv.x);
    }
    if in.uv_wrap.y == 0u {
        source_uv.y = clamp(source_uv.y, 0.0, 1.0);
    } else {
        source_uv.y = fract(source_uv.y);
    }
    let atlas_uv = in.uv_offset + source_uv * in.uv_scale;
    let texel = textureSample(material_texture, material_sampler, atlas_uv, i32(in.texture_layer));
    let alpha = in.tint.a * texel.a;
    if discard_cutout && alpha < 0.5 {
        discard;
    }
    let color = in.tint.rgb * texel.rgb * diffuse;
    return vec4<f32>(mix(color, in.fog.rgb, in.fog.a), alpha);
}
