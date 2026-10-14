#version 330 core
in vec2 v_local;
in vec2 v_half;
in vec4 v_params;
in vec4 v_fill;
in vec4 v_border;
in vec2 v_uv;
uniform sampler2D u_tex;
out vec4 frag;

float sdRoundBox(vec2 p, vec2 h, float r) {
    r = min(r, min(h.x, h.y));
    vec2 q = abs(p) - h + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    int mode = int(v_params.w + 0.5);
    if (mode == 1) {                       // glyph or icon: alpha mask from the atlas
        frag = v_fill * texture(u_tex, v_uv).a;
        return;
    }
    float d = sdRoundBox(v_local, v_half, v_params.x);
    if (mode == 3) {                       // soft shadow of a rounded rectangle
        float sigma = max(v_params.z, 0.5);
        float c = 1.0 - smoothstep(-sigma, sigma, d);
        frag = v_fill * (c * c);
        return;
    }
    float cov = clamp(0.5 - d, 0.0, 1.0);
    if (mode == 2) {                       // image, premultiplied, clipped to the rounded rectangle
        vec4 t = texture(u_tex, v_uv);
        frag = t * (v_fill.a * cov);
        return;
    }
    float bw = v_params.y;
    if (bw > 0.0) {
        float inner = clamp(0.5 - (d + bw), 0.0, 1.0);
        frag = v_border * (cov - inner) + v_fill * inner;
    } else {
        frag = v_fill * cov;
    }
}
