#version 330 core
// Liquid Glass container pass: up to six rounded shapes, smooth-unioned into one surface,
// rendered with one backdrop sample per pixel. Refraction bends the blurred backdrop along the
// SDF gradient (strongest at the rim), then saturation/lift, tint, dimming, specular rim,
// top sheen, press shimmer and a soft two-layer shadow. All lengths are physical pixels.
in vec2 v_uv;
out vec4 frag;

uniform vec2 u_view;        // render target size in px
uniform vec2 u_origin;      // top-left of this quad in the window, px (y down)
uniform vec2 u_size;        // quad size in px
uniform float u_dpi;
uniform float u_opacity;
uniform vec4 shape0; uniform vec4 shape1; uniform vec4 shape2; uniform vec4 shape3; uniform vec4 shape4; uniform vec4 shape5; // x, y, w, h (w = 0: unused)
uniform vec4 radiiA;        // radius of shapes 0..3
uniform vec4 radiiB;        // radius 4, radius 5, smooth-union k, shadow opacity
uniform vec4 glowA;         // hover/focus brightening per shape 0..3
uniform vec4 glowB;         // glow 4, glow 5, shimmer position (<0: off), frost (reduce transparency)
uniform vec4 optics;        // blur level (-1 sharp, 0 fine, 1 coarse, 2 heavy), refraction px, saturation, lift
uniform vec4 tint;          // straight rgba
uniform vec4 light;         // light direction x, y; dimming; rim strength
uniform vec4 frostColor;    // straight rgba
uniform vec4 rimParams;     // rim width px, rim high, rim low, border alpha
uniform sampler2D sharp;
uniform sampler2D fine;
uniform sampler2D coarse;
uniform sampler2D heavy;

float sdRoundBox(vec2 p, vec4 s, float r) {
    if (s.z <= 0.0) return 1e5;
    vec2 h = s.zw * 0.5;
    r = min(r, min(h.x, h.y));
    vec2 q = abs(p - s.xy - h) - h + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

float smin(float a, float b, float k) {
    if (k <= 0.0) return min(a, b);
    float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

float sceneSdf(vec2 p) {
    float k = radiiB.z;
    float d = sdRoundBox(p, shape0, radiiA.x);
    d = smin(d, sdRoundBox(p, shape1, radiiA.y), k);
    d = smin(d, sdRoundBox(p, shape2, radiiA.z), k);
    d = smin(d, sdRoundBox(p, shape3, radiiA.w), k);
    d = smin(d, sdRoundBox(p, shape4, radiiB.x), k);
    d = smin(d, sdRoundBox(p, shape5, radiiB.y), k);
    return d;
}

float glowAt(vec2 p) {
    float g = 0.0;
    g = max(g, glowA.x * step(sdRoundBox(p, shape0, radiiA.x), 0.0));
    g = max(g, glowA.y * step(sdRoundBox(p, shape1, radiiA.y), 0.0));
    g = max(g, glowA.z * step(sdRoundBox(p, shape2, radiiA.z), 0.0));
    g = max(g, glowA.w * step(sdRoundBox(p, shape3, radiiA.w), 0.0));
    g = max(g, glowB.x * step(sdRoundBox(p, shape4, radiiB.x), 0.0));
    g = max(g, glowB.y * step(sdRoundBox(p, shape5, radiiB.y), 0.0));
    return g;
}

vec3 backdrop(vec2 px) {
    vec2 w = u_origin + px;
    vec2 uv = vec2(w.x, u_view.y - w.y) / u_view;
    float b = optics.x;
    if (b < 0.0) return mix(texture(fine, uv).rgb, texture(sharp, uv).rgb, -b);
    if (b <= 1.0) return mix(texture(fine, uv).rgb, texture(coarse, uv).rgb, b);
    return mix(texture(coarse, uv).rgb, texture(heavy, uv).rgb, b - 1.0);
}

void main() {
    float D = u_dpi;
    vec2 p = v_uv * u_size;
    float d = sceneSdf(p);

    // soft floating shadow (large, offset down) + small contact shadow
    float ds = sceneSdf(p - vec2(0.0, 8.0 * D));
    float big = exp(-max(ds, 0.0) * max(ds, 0.0) / (2.0 * 16.0 * 16.0 * D * D)) * smoothstep(-24.0 * D, 6.0 * D, ds);
    float contact = exp(-max(d, 0.0) * max(d, 0.0) / (2.0 * 2.5 * 2.5 * D * D)) * 0.5;
    float shadow = radiiB.w * clamp(big * 0.8 + contact, 0.0, 1.0);

    float inside = clamp(0.5 - d, 0.0, 1.0);
    if (inside <= 0.0 && shadow < 0.002) {
        frag = vec4(0.0);
        return;
    }

    // outward normal from the SDF gradient
    const float e = 1.0;
    vec2 g = vec2(sceneSdf(p + vec2(e, 0.0)) - sceneSdf(p - vec2(e, 0.0)),
                  sceneSdf(p + vec2(0.0, e)) - sceneSdf(p - vec2(0.0, e)));
    vec2 n = length(g) > 1e-4 ? normalize(g) : vec2(0.0, -1.0);

    // lensing: the backdrop is pulled from further inside near the rim (a lens, not just a blur)
    float rimBand = max(optics.y * 2.2, 1.0);
    float depth = clamp(-d / rimBand, 0.0, 1.0);
    float bend = (1.0 - depth) * (1.0 - depth);
    vec2 offset = -n * optics.y * bend;
    vec3 col = backdrop(p + offset);
    col.r = mix(col.r, backdrop(p + offset * 1.08).r, 0.5 * bend); // faint dispersion at the edge

    float glow = glowAt(p);
    float luma = dot(col, vec3(0.2126, 0.7152, 0.0722));
    col = mix(vec3(luma), col, optics.z);
    col += optics.w + glow * 0.07;
    col = mix(col, tint.rgb, tint.a);
    col = mix(col, frostColor.rgb, glowB.w * frostColor.a);
    col *= 1.0 - light.z;

    // specular: inner rim, brightest facing the light, faint glint opposite, plus top sheen
    vec2 L = normalize(light.xy);
    float facing = 0.5 + 0.5 * dot(n, L);
    float rimA = mix(rimParams.z, rimParams.y, facing * facing) + 0.22 * pow(max(-dot(n, L), 0.0), 6.0);
    float rim = 1.0 - smoothstep(0.0, rimParams.x + 0.75, -d);
    float border = (1.0 - smoothstep(0.0, 1.5, abs(d + 0.5))) * rimParams.w;
    col = mix(col, vec3(1.0), clamp(max(rim * rimA, border) * light.w, 0.0, 1.0));
    float sheen = max(-n.y, 0.0) * (1.0 - clamp(-d / (18.0 * D), 0.0, 1.0));
    col += sheen * 0.05 * light.w;

    if (glowB.z >= 0.0) { // press shimmer sweeping diagonally
        float x = (p.x + p.y * 0.6) / (u_size.x + u_size.y * 0.6);
        col += exp(-pow((x - glowB.z) * 10.0, 2.0)) * 0.14;
    }

    vec4 glass = vec4(clamp(col, 0.0, 1.0) * inside, inside);
    frag = (glass + vec4(0.0, 0.0, 0.0, shadow) * (1.0 - inside)) * u_opacity;
}
