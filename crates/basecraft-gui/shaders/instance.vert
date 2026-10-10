#version 440

// One unit-square corner per vertex; everything else per instance.
layout(location = 0) in vec2 corner;
layout(location = 1) in vec4 top;     // top-left.xy, top-right.xy
layout(location = 2) in vec4 bottom;  // bottom-left.xy, bottom-right.xy
layout(location = 3) in vec4 color;   // straight alpha
layout(location = 4) in vec4 uv;      // u, v, du, dv; du == 0 for solid

layout(location = 0) out vec4 v_color;
layout(location = 1) out vec2 v_uv;
layout(location = 2) out float v_textured;

layout(std140, binding = 0) uniform buf {
    mat4 mvp;
};

void main()
{
    vec2 upper = mix(top.xy, top.zw, corner.x);
    vec2 lower = mix(bottom.xy, bottom.zw, corner.x);
    gl_Position = mvp * vec4(mix(upper, lower, corner.y), 0.0, 1.0);
    v_color = color;
    v_uv = uv.xy + corner * uv.zw;
    v_textured = uv.z > 0.0 ? 1.0 : 0.0;
}
