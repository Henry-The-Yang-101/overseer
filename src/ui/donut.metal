#include <metal_stdlib>
using namespace metal;
struct Params {
    uint width, height;
    float aspect, scale, sin_a, cos_a, sin_b, cos_b;
};
struct SurfacePoint { float4 position; float4 normal; };
float3 rotate(float3 v, constant Params &p) {
    float y = v.y * p.cos_a - v.z * p.sin_a;
    return float3(v.x * p.cos_b - y * p.sin_b,
                  v.x * p.sin_b + y * p.cos_b,
                  v.y * p.sin_a + v.z * p.cos_a);
}
kernel void draw_donut(constant Params &p [[buffer(0)]],
                       device const SurfacePoint *points [[buffer(1)]],
                       device atomic_uint *output [[buffer(2)]],
                       constant uint &samples [[buffer(3)]],
                       uint i [[thread_position_in_grid]]) {
    if (i >= samples) return;
    float3 v = rotate(points[i].position.xyz, p);
    float inverse_depth = 1.f / (6.f + v.z);
    int x = int(floor(float(p.width) / 2.f + p.scale * v.x * inverse_depth));
    int y = int(floor(float(p.height) / 2.f - p.scale * v.y * inverse_depth / p.aspect));
    if (x < 0 || y < 0 || x >= int(p.width) || y >= int(p.height)) return;
    float3 n = rotate(points[i].normal.xyz, p);
    float luminance = clamp((n.y - n.z) * 0.70710678118f, 0.f, 1.f);
    uint shade = uint(round(luminance * 11.f)) + 1;
    uint sample = (uint(inverse_depth * 1000000.f) << 8) | shade;
    atomic_fetch_max_explicit(&output[uint(y) * p.width + uint(x)], sample, memory_order_relaxed);
}
