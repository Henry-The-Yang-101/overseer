#include <metal_stdlib>
using namespace metal;
struct Params { uint width; uint height; float phase; float aspect; };
float4 rotate(float4 p, uint a, uint b, float angle) {
    float s = sin(angle), c = cos(angle);
    float x = p[a], y = p[b];
    p[a] = x * c - y * s; p[b] = x * s + y * c;
    return p;
}
kernel void project(constant Params &p [[buffer(0)]], device float4 *vertices [[buffer(1)]], uint i [[thread_position_in_grid]]) {
    if (i != 0) return;
    float angle = p.phase * 6.28318530718f;
    float2 low = float2(INFINITY), high = float2(-INFINITY);
    for (uint vtx = 0; vtx < 16; ++vtx) {
        float4 v;
        for (uint axis = 0; axis < 4; ++axis) v[axis] = (vtx & (1 << axis)) ? 1.f : -1.f;
        v = rotate(v, 0, 3, angle);
        v.xyz *= 2.f / (3.f - v.w);
        v = rotate(v, 0, 2, 0.55f);
        v = rotate(v, 1, 2, 0.28f);
        v.xy *= 5.f / (6.f - v.z);
        vertices[vtx] = v;
        low = min(low, v.xy);
        high = max(high, v.xy);
    }
    float2 extent = high - low;
    float2 available = max(float2(float(p.width) / 2.f - 2.f,
                                  float(p.height) * p.aspect / 4.f - 1.5f), 1.f);
    float2 fit = min(available / (extent + 0.14f), (available - 0.96f) / extent);
    float scale = min(fit.x, fit.y);
    vertices[16] = float4((low + high) / 2.f, scale, max(0.07f, 0.48f / scale));
}

kernel void shade(constant Params &p [[buffer(0)]], device const float4 *vertices [[buffer(1)]], device float *output [[buffer(2)]], uint i [[thread_position_in_grid]]) {
    if (i >= p.width * p.height) return;
    float4 camera = vertices[16];
    float scale = camera.z, radius = camera.w;
    float2 pixel = camera.xy + float2((float(i % p.width) + 0.5f - float(p.width) / 2.f) / (2.f * scale),
                         -(float(i / p.width) + 0.5f - float(p.height) / 2.f) * p.aspect / (4.f * scale));
    float depth = -INFINITY, light = 0.f;
    for (uint vtx = 0; vtx < 16; ++vtx) {
        for (uint axis = 0; axis < 4; ++axis) {
            if (vtx & (1 << axis)) continue;
            float3 a = vertices[vtx].xyz, b = vertices[vtx | (1 << axis)].xyz;
            float2 d = b.xy - a.xy;
            float t = clamp(dot(pixel - a.xy, d) / max(dot(d, d), 0.000001f), 0.f, 1.f);
            float2 n = (pixel - a.xy - t * d) / radius;
            float d2 = dot(n, n);
            if (d2 > 1.f) continue;
            float nz = sqrt(1.f - d2);
            float z = mix(a.z, b.z, t) + radius * nz;
            if (z > depth) {
                depth = z;
                float diffuse = clamp(0.35f - 0.25f * n.x + 0.3f * n.y + 0.55f * nz, 0.18f, 1.f);
                light = diffuse * clamp(0.65f + 0.16f * z, 0.3f, 1.f);
            }
        }
    }
    output[i] = light;
}
