#version 330 core
// Scroll-edge style legibility zone: a progressive blur + dim, strongest at the window edge and
// fading out towards the content, instead of a hard gradient scrim.
in vec2 v_uv;
out vec4 frag;
uniform vec2 u_view;
uniform float u_fromTop;   // 1: strongest at the top edge, 0: at the bottom edge
uniform float u_dim;
uniform float u_opacity;
uniform sampler2D fine;
uniform sampler2D coarse;

void main() {
    // v_uv.y runs top to bottom of the band
    float t = mix(v_uv.y, 1.0 - v_uv.y, u_fromTop); // 1 at the window edge
    float mask = smoothstep(0.0, 1.0, t);
    vec2 uv = gl_FragCoord.xy / u_view;
    vec3 col = mix(texture(fine, uv).rgb, texture(coarse, uv).rgb, mask);
    col *= 1.0 - u_dim * mask;
    float a = mask * 0.92;
    frag = vec4(col * a, a) * u_opacity;
}
