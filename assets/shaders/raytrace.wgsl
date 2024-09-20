
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

#import "shaders/random.wgsl"::{rngNextFloat, randomUnitVec3}

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
    random_seed: f32,
    sample_count: u32,
    bounce_count: u32,
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
    // Doubles as diffuse albedo for non-metallic, specular for metallic and a mix for everything in between
    base_color: vec3<f32>,
    // 0.0 for dielectric materials, 1.0 for metallic
    metallic: f32,
    // 0.0 -> very glossy
    roughness: f32, // "Fuzzy Reflection"
    // Specular intensity for non-metals
    reflectance: f32, // unused for now
    // Index of refraction
    ior: f32,
    // transmission through a material via refraction
    specular_transmission: f32
}

const PI: f32 = 3.141592653589793;
const INF: f32 = 3.40282e+38;

var<private> rng_state: u32;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    rng_state = u32((camera.random_seed * 10000.0) * (in.uv.x * 402.0) * (in.uv.y * 31.5)) ;
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
            return vec4<f32>(raytrace_result.color, 1.0);
        }
    }

    return vec4<f32>(raytrace_result.color, 1.0);
}

struct Ray {
    origin: vec3<f32>,
    direction: vec3<f32>,
}

fn ray_at(ray: Ray, t: f32) -> vec3<f32> {
    return ray.origin + t * ray.direction;
}

struct RaytraceResult {
    color: vec3<f32>,
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
    var total_result: RaytraceResult = RaytraceResult(vec3<f32>(0.0, 0.0, 0.0), 0.0);
    for (var sample_index: u32 = 0; sample_index < camera.sample_count; sample_index++) {
        let ray = random_ray_from_uv(uv, state);
        let sample_result = raytrace(ray, state);

        total_result.color += sample_result.color;
        total_result.depth += sample_result.depth;
    }

    let averaged_color = total_result.color.rgb / (f32(camera.sample_count));
    let averaged_depth = total_result.depth / f32(camera.sample_count);
    return RaytraceResult(averaged_color, averaged_depth);
}

fn raytrace(base_ray: Ray, state: ptr<private, u32>) -> RaytraceResult {
    var ray = base_ray;

    var fallback_far: f32;
    if settings.level == 1 {
        fallback_far = camera.far + 10.0;
    } else {
        fallback_far = camera.far - 1.0;
    }

    var first_depth: f32 = INF;
    var ray_color: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);
    var lightSourceColor: vec3<f32> = vec3<f32>(0.0, 0.0, 0.0);

    var bounce_count: u32 = 0;
    for (; bounce_count <= camera.bounce_count; bounce_count++) {
        let hit = raycast(ray);

        // Setting the depth for depth buffer comparison, this might have to be a early return at some point
        if bounce_count == 0 {
            first_depth = hit.distance;
        }

        // The background
        if hit.distance == INF {
            lightSourceColor = background_gradient(ray);
            break;
        }

        let material = material_buffer[hit.object];

        var attenuation: vec3<f32>;
        let absorbed = scatter(material, &ray, &attenuation, hit, state);

        // rays getting absorbed
        if absorbed {
            break;
        }

        ray_color *= attenuation;
    }

    // A extra bounce could be added -> the background break wasn't hit
    if bounce_count == camera.bounce_count + 1 {
        ray_color = vec3<f32>(0.0, 0.0, 0.0);
    }

    if first_depth == INF {
        first_depth = fallback_far;
    }

    return RaytraceResult(linear_to_gamma_Vec3(ray_color * lightSourceColor), first_depth);
}

fn linear_to_gamma_Vec3(in: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(sqrt(in.x), sqrt(in.y), sqrt(in.z));
}

// returns wether the ray was absorbed
fn scatter(material: Material, scattered: ptr<function, Ray>, attenuation: ptr<function, vec3<f32>>, hit: HitInfo, state: ptr<private, u32>) -> bool {
    if rngNextFloat(state) < material.metallic {
        // metallic interaction
        
        // reflection and roughness 
        let reflected = normalize(reflect((*scattered).direction, hit.normal)) + (material.roughness * randomUnitVec3(state));

        // setting return values
        *scattered = Ray(hit.position, reflected);
        *attenuation = material.base_color;

        // Discard below surface
        return dot((*scattered).direction, hit.normal) < 0;
    } else {
        // non-metallic interaction

        if rngNextFloat(state) < material.specular_transmission {
            // Specular transmission

            var ri: f32;
            if hit.front_face {
                // inside
                ri = 1.0 / material.ior;
            } else {
                // outside
                ri = material.ior;
            }

            let unit_direction = normalize((*scattered).direction);

            let cos_theta = min(dot(-unit_direction, hit.normal), 1.0);
            let sin_theta = sqrt(1.0 - cos_theta * cos_theta);

            let cannot_refract = ri * sin_theta > 1.0;
            var direction: vec3<f32>;

            if cannot_refract || reflectance(cos_theta, ri) > rngNextFloat(state) {
                direction = reflect(unit_direction, hit.normal);
            } else {
                direction = refract(unit_direction, hit.normal, ri);
            }

            // setting return values
            *scattered = Ray(hit.position, direction);
            *attenuation = vec3<f32>(1.0, 1.0, 1.0);

            // A refrected ray always continues on
            return false;
        } else {
            // normal diffuse

            // diffuse and roughness
            var scatter_direction = hit.normal + randomUnitVec3(state) + (material.roughness * randomUnitVec3(state));

            if vec3_near_zero(scatter_direction) {
                scatter_direction = hit.normal;
            }

            // setting return values
            *scattered = Ray(hit.position, scatter_direction);
            *attenuation = material.base_color;

            // Discard below surface
            return dot((*scattered).direction, hit.normal) < 0;
        }
    }
}

struct HitInfo {
    distance: f32,
    position: vec3<f32>,
    normal: vec3<f32>,
    object: u32,
    front_face: bool,
}

fn raycast(ray: Ray) -> HitInfo {
    var closest = HitInfo(INF, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 0.0), 0, true);
    for (var geometry_index: u32 = 0; geometry_index < arrayLength(&geometry_buffer); geometry_index++) {
        let sphere = geometry_buffer[geometry_index];

        let hit_distance = hit_sphere(sphere, ray);
        if hit_distance != -1.0 && hit_distance > 0.0001 {
            if hit_distance < closest.distance {
                let hit_position = ray_at(ray, hit_distance);
                let normal = normalize(hit_position - sphere.position);

                closest = HitInfo(hit_distance, hit_position, normal, geometry_index, dot(ray.direction, normal) < 0.0);
            }
        }
    }

    return closest;
}

fn background_gradient(ray: Ray) -> vec3<f32> {
    let unit: vec3<f32> = normalize(ray.direction);
    let a: f32 = 0.5 * (unit.y + 1.0);
    let color: vec3<f32> = (1.0 - a) * vec3<f32>(1.0, 1.0, 1.0) + a * vec3<f32>(0.5, 0.7, 1.0);
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
    }

    return (h - sqrt(discriminant)) / a;
}

fn reflect(vector: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    return vector - 2 * dot(vector, normal) * normal;
}

fn refract(vector: vec3<f32>, normal: vec3<f32>, etai_over_etat: f32) -> vec3<f32> {
    let cos_theta = min(dot(-vector, normal), 1.0);
    let r_out_perp = etai_over_etat * (vector + cos_theta * normal);
    let r_out_parallel = -sqrt(abs(1.0 - dot(r_out_perp, r_out_perp))) * normal;
    return r_out_perp + r_out_parallel;
}

fn reflectance(cosine: f32, refraction_index: f32) -> f32 {
    // Use Schlick's approximation for reflectance.
    var r0 = (1.0 - refraction_index) / (1.0 + refraction_index);
    r0 = r0 * r0;
    return r0 + (1.0 - r0) * pow((1.0 - cosine), 5.0);
}

fn vec3_near_zero(vector: vec3<f32>) -> bool {
    let s = 1e-8;
    return abs(vector.x) < s && abs(vector.y) < s && abs(vector.z) < s;
}
