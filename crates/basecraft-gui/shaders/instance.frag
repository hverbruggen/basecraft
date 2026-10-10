#version 440

layout(location = 0) in vec4 v_color;
layout(location = 1) in vec2 v_uv;
layout(location = 2) in float v_textured;

layout(location = 0) out vec4 frag_color;

layout(binding = 1) uniform sampler2D atlas;

void main()
{
    float coverage = v_textured > 0.5 ? texture(atlas, v_uv).a : 1.0;
    float a = v_color.a * coverage;
    // Premultiplied output; the pipeline blends with One, OneMinusSrcAlpha.
    frag_color = vec4(v_color.rgb * a, a);
}
