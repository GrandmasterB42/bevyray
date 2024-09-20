// https://github.com/gnikoloff/webgpu-raytracer/blob/3bdad829c536b530ba98c396dc11d08002427b41/src/shaders/utils/utils.ts

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

fn randomVec3InUnitSphere(state: ptr<private, u32>) -> vec3<f32> {
    var p: vec3<f32>;
    loop {
        p = 2.0 * vec3<f32>(rngNextFloat(state), rngNextFloat(state), rngNextFloat(state)) - vec3<f32>(1.0);
        if dot(p, p) <= 1.0 {
            break;
        }
    }
    return p;
}

fn randomUnitVec3(rngState: ptr<private, u32>) -> vec3<f32> {
    return (randomVec3InUnitSphere(rngState));
}