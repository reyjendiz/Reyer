#version 330 core
// Batched 2D geometry: rounded rectangles, shadows, glyphs and images, all in physical pixels (y down).
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_local;   // px from the rect centre
layout(location = 2) in vec2 a_half;    // half size of the rect in px
layout(location = 3) in vec4 a_params;  // radius, border width, shadow sigma, mode
layout(location = 4) in vec4 a_fill;    // premultiplied
layout(location = 5) in vec4 a_border;  // premultiplied
layout(location = 6) in vec2 a_uv;
uniform vec2 u_view;

out vec2 v_local;
out vec2 v_half;
out vec4 v_params;
out vec4 v_fill;
out vec4 v_border;
out vec2 v_uv;

void main() {
    v_local = a_local;
    v_half = a_half;
    v_params = a_params;
    v_fill = a_fill;
    v_border = a_border;
    v_uv = a_uv;
    gl_Position = vec4(a_pos.x / u_view.x * 2.0 - 1.0, 1.0 - a_pos.y / u_view.y * 2.0, 0.0, 1.0);
}
