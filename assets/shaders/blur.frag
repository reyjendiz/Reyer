#version 330 core
// One direction of a separable 9-tap gaussian, using linear filtering (5 fetches).
in vec2 v_uv;
uniform sampler2D u_src;
uniform vec2 u_dir;   // texel step (uv) times spread
out vec4 frag;

void main() {
    vec4 c = texture(u_src, v_uv) * 0.2270270270;
    c += texture(u_src, v_uv + u_dir * 1.3846153846) * 0.3162162162;
    c += texture(u_src, v_uv - u_dir * 1.3846153846) * 0.3162162162;
    c += texture(u_src, v_uv + u_dir * 3.2307692308) * 0.0702702703;
    c += texture(u_src, v_uv - u_dir * 3.2307692308) * 0.0702702703;
    frag = c;
}
