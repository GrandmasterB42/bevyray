
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
@group(0) @binding(4) var<uniform> settings: RaytraceLevel;
struct RaytraceLevel {
    level: u32,
}
@group(0) @binding(5) var<uniform> camera: Camera;
struct Camera {
    random_seed: u32,
    sample_count: u32,
    // 0 -> perspective; 1 -> orthographic
    projection_type: u32,
    near: f32,
    far: f32,
    fov: f32,
    aspect: f32,
    height: u32,
    position: vec3<f32>,
    direction: vec3<f32>,
    up: vec3<f32>,
}

@group(1) @binding(0) var<storage, read_write> geometry_buffer: array<Sphere>;
struct Sphere {
    position: vec3<f32>,
    radius: f32,
    material_id: u32,
}

@group(1) @binding(1) var<storage, read_write> material_buffer: array<Material>;
struct Material {
    base_color: vec3<f32>,
}

var<private> rng_state: u32;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    rng_state = camera.random_seed;
    // Skip Raytracing
    if settings.level == 0 {
        return textureSample(screen_texture, texture_sampler, in.uv);
    }

    let raytrace_result = trace_multisampled(in.uv, &rng_state);
        
    // combine option
    if settings.level == 1 || settings.level == 2 {
        // 0 is at far plane, 1 at near plane
        let depth = textureSample(depth_texture, depth_sampler, in.uv);

        var raytraced_depth = raytrace_result.depth;
        if raytraced_depth > camera.far {
            raytraced_depth = -1.0;
        } else {
            raytraced_depth = camera.near / raytraced_depth;
        }

        if depth > raytraced_depth {
            return textureSample(screen_texture, texture_sampler, in.uv);
        } else {
            return raytrace_result.color;
        }
    }

    return raytrace_result.color;
}

struct Ray {
    origin: vec3<f32>,
    direction: vec3<f32>,
}

fn ray_at(ray: Ray, t: f32) -> vec3<f32> {
    return ray.origin + t * ray.direction;
}

struct RaytraceResult {
    color: vec4<f32>,
    depth: f32,
}

fn random_ray_from_uv(uv: vec2<f32>, state: ptr<private, u32>) -> Ray {
    let rand_square = vec2<f32>(rngNextFloat(state) - 0.5, rngNextFloat(state) - 0.5);
    let height = f32(camera.height);
    let width = f32(camera.height) * camera.aspect;
    let delta_u = (1.0 / width) * rand_square.x;
    let delta_v = (1.0 / height) * rand_square.y;

    let ndc_x = (uv.x * 2.0 - 1.0) + delta_u;
    let ndc_y = (1.0 - uv.y * 2.0) + delta_v;

    let right = cross(camera.direction, camera.up);

    let scale = tan(camera.fov * 0.5);

    let ray_direction = normalize(camera.direction + (ndc_x * camera.aspect * scale * right) + (ndc_y * scale * camera.up));

    return Ray(camera.position, ray_direction);
}

// default camera is at 0.0, 0.0, 5.0, looking at 0 with up as Y | Pass this as uniform data
fn trace_multisampled(uv: vec2<f32>, state: ptr<private, u32>) -> RaytraceResult {
    var total_result: RaytraceResult = RaytraceResult(vec4<f32>(0.0, 0.0, 0.0, 0.0), 0.0);
    for (var sample_index: u32 = 0; sample_index < camera.sample_count; sample_index++) {
        let ray = random_ray_from_uv(uv, state);
        let sample_result = raytrace(ray);

        total_result.color += sample_result.color;
        total_result.depth += sample_result.depth;
    }

    let averaged_color = total_result.color.rgba / (f32(camera.sample_count));
    let averaged_depth = total_result.depth / f32(camera.sample_count);
    return RaytraceResult(averaged_color, averaged_depth);
}

fn raytrace(ray: Ray) -> RaytraceResult {
    var fallback_far: f32;
    if settings.level == 1 {
        fallback_far = camera.far + 10.0;
    } else {
        fallback_far = camera.far - 1.0;
    }

    var closest = RaytraceResult(vec4<f32>(0.0, 0.0, 0.0, 0.0), 3.40282e+38);

    for (var geometry_index: u32 = 0; geometry_index < arrayLength(&geometry_buffer); geometry_index++) {
        let sphere = geometry_buffer[geometry_index];

        // get the hit position
        let hit_distance = hit_sphere(sphere, ray);
        if hit_distance != -1.0 && hit_distance > 0.0 {
            let hit_position = ray_at(ray, hit_distance);
            let normal = normalize(hit_position - sphere.position);

            if dot(ray.direction, normal) > 0.0 {
                // inside the sphere
            } else {
                // outside the sphere
            }

            let this_hit = RaytraceResult(vec4<f32>(0.5 * (vec3<f32>(normal.x, normal.y, normal.z) + vec3<f32>(1.0, 1.0, 1.0)), 1.0), hit_distance);

            if this_hit.depth < closest.depth {
                closest = this_hit;
            }
            

            //let material = material_buffer[sphere.material_id];
            //return RaytraceResult(
            //    vec4<f32>(
            //        material.base_color, 1.0,
            //    ),
            //    0.0,
            //);
        }
    }

    if closest.depth == 3.40282e+38 {
        return RaytraceResult(
            vec4<f32>(
                background_gradient(ray),
                1.0,
            ),
            fallback_far,
        );
    }
    return closest;
}


fn background_gradient(ray: Ray) -> vec3<f32> {
    let unit: vec3<f32> = normalize(ray.direction);
    let a: f32 = 0.5 * (unit.y + 1.0);
    let color: vec3<f32> = (1.0 - a) * vec3<f32>(1.0, 1.0, 1.0) + a * vec3<f32>(0.0, 0.0, 1.0);
    return color;
}

fn hit_sphere(sphere: Sphere, ray: Ray) -> f32 {
    let oc: vec3<f32> = sphere.position - ray.origin;
    let a = dot(ray.direction, ray.direction);
    let h = dot(ray.direction, oc);
    let c = dot(oc, oc) - sphere.radius * sphere.radius;
    let discriminant = h * h - a * c;

    if discriminant < 0.0 {
        return -1.0;
    } else {
        return (h - sqrt(discriminant)) / a;
    }
}


// https://github.com/gnikoloff/webgpu-raytracer/blob/3bdad829c536b530ba98c396dc11d08002427b41/src/shaders/utils/utils.ts#L10

fn rngNextFloat(state: ptr<private, u32>) -> f32 {
    rngNextInt(state);
    return f32(*state) / f32(0xffffffffu);
}

fn rngNextInt(state: ptr<private, u32>) {
    // PCG random number generator
    // Based on https://www.shadertoy.com/view/XlGcRh

    let oldState = *state + 747796405u + 2891336453u;
    let word = ((oldState >> ((oldState >> 28u) + 4u)) ^ oldState) * 277803737u;
    *state = (word >> 22u) ^ word;
}

fn randInRange(min: f32, max: f32, state: ptr<private, u32>) -> f32 {
    return min + rngNextFloat(state) * (max - min);
}