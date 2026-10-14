#version 330 core
// Slider knob while it is touched: a small glass lens that magnifies and bends the track under it.
// The track under the knob is described analytically (a horizontal bar, accent left of the knob).
in vec2 v_uv;
out vec4 frag;
uniform float u_opacity;
uniform float u_radius;     // knob radius in px
uniform float magnify;      // 1 = none
uniform float strength;     // 0 flat knob .. 1 full lens
uniform float halfThick;    // half the track thickness in px
uniform vec4 accent;        // straight rgb, the filled part of the track
uniform vec4 trackColor;    // straight rgba
uniform vec4 knobColor;     // straight rgba of the resting knob
uniform vec4 light;         // light direction x, y; rim strength; white tint

vec4 track(vec2 q) {        // q in px from the knob centre -> premultiplied colour
    float band = clamp(halfThick - abs(q.y) + 0.5, 0.0, 1.0);
    // the filled part ends in a round cap (centre one half-thickness left of the knob), not a straight cut
    vec2 cap = vec2(max(q.x + halfThick, 0.0), q.y);
    float fill = clamp(halfThick - length(cap) + 0.5, 0.0, 1.0);
    vec4 base = vec4(trackColor.rgb * trackColor.a, trackColor.a) * band;
    return vec4(accent.rgb, 1.0) * fill + base * (1.0 - fill);
}

void main() {
    vec2 p = v_uv * 2.0 - 1.0;  // -1..1
    float r = length(p);
    float aa = fwidth(r) * 1.5;
    float inside = 1.0 - smoothstep(1.0 - aa, 1.0, r);
    if (inside <= 0.0) { frag = vec4(0.0); return; }
    // barrel lens: magnified centre, compressed rim
    vec2 q = p / magnify * (1.0 + 0.35 * r * r);
    vec4 seen = track(q * u_radius);
    vec3 glass = mix(vec3(0.92), seen.rgb / max(seen.a, 1e-3), seen.a * 0.85);
    glass = mix(glass, vec3(1.0), light.w);
    vec2 n = r > 1e-4 ? p / r : vec2(0.0, -1.0);
    float facing = 0.5 + 0.5 * dot(n, normalize(light.xy));
    float rim = smoothstep(0.78, 1.0, r) * mix(0.1, 0.8, facing * facing) * light.z;
    glass = mix(glass, vec3(1.0), rim);
    vec3 col = mix(knobColor.rgb, glass, strength);
    float a = mix(knobColor.a, 0.96, strength) * inside;
    frag = vec4(col * a, a) * u_opacity;
}
