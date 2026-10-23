// Splat texturing for generator 2: R grass, G sand, B rock, A snow,
// sampled on UV2 (texel-aligned). A subtle value-noise variation in
// world meters breaks up the flat blends.
shader_type spatial;

uniform sampler2D splat : filter_linear, repeat_disable;
uniform vec3 grass : source_color = vec3(0.35, 0.55, 0.28);
uniform vec3 sand : source_color = vec3(0.87, 0.79, 0.56);
uniform vec3 rock : source_color = vec3(0.45, 0.43, 0.40);
uniform vec3 snow : source_color = vec3(0.92, 0.93, 0.95);

float vnoise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	float a = fract(sin(dot(i, vec2(127.1, 311.7))) * 43758.5453);
	float b = fract(sin(dot(i + vec2(1, 0), vec2(127.1, 311.7))) * 43758.5453);
	float c = fract(sin(dot(i + vec2(0, 1), vec2(127.1, 311.7))) * 43758.5453);
	float d = fract(sin(dot(i + vec2(1, 1), vec2(127.1, 311.7))) * 43758.5453);
	return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

void fragment() {
	vec4 w = texture(splat, UV2);
	vec3 col = grass * w.r + sand * w.g + rock * w.b + snow * w.a;
	// World-space noise: UV holds world meters.
	float n = vnoise(UV * 0.35) * 0.6 + vnoise(UV * 0.08) * 0.4;
	col *= 0.92 + 0.16 * n;
	ALBEDO = col;
	ROUGHNESS = 1.0;
}
