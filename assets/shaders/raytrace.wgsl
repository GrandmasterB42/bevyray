
// Since post processing is a fullscreen effect, we use the fullscreen vertex shader provided by bevy.
// This will import a vertex shader that renders a single fullscreen triangle.
//
// A fullscreen triangle is a single triangle that covers the entire screen.
// The box in the top left in that diagram is the screen. The 4 x are the corner of the screen
//
// Y axis
//  1 |  x-----x......
//  0 |  |  s  |  . ´
// -1 |  x_____x´
// -2 |  :  .´
// -3 |  :´
//    +---------------  X axis
//      -1  0  1  2  3
//
// As you can see, the triangle ends up bigger than the screen.
//
// You don't need to worry about this too much since bevy will compute the correct UVs for you.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var depth_sampler: sampler;
struct RayTraceLevel {
    level: u32,
}
@group(0) @binding(4) var<uniform> settings: RayTraceLevel;

struct RayTraceResult {
    color: vec4<f32>,
    depth: f32,
}

@group(0) @binding(5) var<uniform> camera: Camera;

struct Camera {
    // 0 -> perspective; 1 -> orthographic
    projection_type: u32,
    near: f32,
    far: f32,
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let depth_sample = textureSample(depth_texture, depth_sampler, in.uv);
    // Skip Raytracing
    if settings.level == 0 {
        return vec4<f32>(
            textureSample(screen_texture, texture_sampler, in.uv).rgba,
        );
    }


    let raytrace_result = RayTraceResult(
        vec4<f32>(
            1.0,
            0.0,
            0.0,
            1.0,
        ),
        camera.far - 0.1,
    );
        
    // combine option
    if settings.level == 1 {
        let depth_sample = 1.0 - textureSample(depth_texture, depth_sampler, in.uv);

        var rasterized_depth: f32 = 0.0;
        if camera.projection_type == 0 {
            rasterized_depth = (camera.near * camera.far) / (camera.far - depth_sample * (camera.far - camera.near));
        } else {
            rasterized_depth = camera.near + depth_sample * (camera.far - camera.near);
        }


        if rasterized_depth < raytrace_result.depth {
            return textureSample(screen_texture, texture_sampler, in.uv);
        } else {
            return raytrace_result.color;
        }
    }

    return raytrace_result.color;
}