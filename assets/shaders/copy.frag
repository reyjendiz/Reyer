#version 330 core
// Plain texture copy with an optional colour grade (used for the letterbox "background extension").
in vec2 v_uv;
uniform sampler2D u_src;
uniform vec4 u_rect;       // source sub-rectangle in uv: x, y, w, h
uniform vec4 u_grade;      // brightness offset, saturation, 0, 0
out vec4 frag;

void main() {
    vec4 c = texture(u_src, u_rect.xy + v_uv * u_rect.zw);
    float luma = dot(c.rgb, vec3(0.2126, 0.7152, 0.0722));
    c.rgb = mix(vec3(luma), c.rgb, u_grade.y) + u_grade.x;
    frag = vec4(clamp(c.rgb, 0.0, 1.0), 1.0);
}
