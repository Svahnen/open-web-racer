struct MirrorVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var mirror_texture: texture_2d<f32>;
@group(0) @binding(1) var mirror_sampler: sampler;

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
) -> MirrorVertex {
    var out: MirrorVertex;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_image(in: MirrorVertex) -> @location(0) vec4<f32> {
    return textureSample(mirror_texture, mirror_sampler, in.uv);
}

@fragment
fn fs_frame(in: MirrorVertex) -> @location(0) vec4<f32> {
    return vec4<f32>(0.03, 0.05, 0.06, 1.0);
}
