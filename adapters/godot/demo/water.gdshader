shader_type spatial;
render_mode blend_mix, cull_disabled, depth_draw_opaque;

uniform sampler2D ripple1 : hint_normal, repeat_enable;
uniform sampler2D ripple2 : hint_normal, repeat_enable;
uniform sampler2D depth_tex : hint_depth_texture;
uniform sampler2D screen_tex : hint_screen_texture;
uniform vec4 shallow : source_color = vec4(0.25, 0.75, 0.80, 1.0);
uniform vec4 deep : source_color = vec4(0.02, 0.12, 0.35, 1.0);
uniform vec4 foam_col : source_color = vec4(0.9, 0.95, 0.95, 1.0);
uniform float time_scale = 1.0;

void vertex() {
	UV = UV; // world xz meters from the mesh
}

void fragment() {
	float t = TIME * time_scale;
	// two scrolling normal maps
	vec3 n1 = texture(ripple1, UV * 0.05 + vec2(t * 0.02, t * 0.013)).xyz * 2.0 - 1.0;
	vec3 n2 = texture(ripple2, UV * 0.11 - vec2(t * 0.031, t * 0.017)).xyz * 2.0 - 1.0;
	// river flow-map: two-phase blend along UV2 flow
	vec2 flow = UV2;
	float ph = fract(t * 0.5);
	vec3 nf = texture(ripple1, UV * 0.08 + flow * ph).xyz * 2.0 - 1.0;
	vec3 nf2 = texture(ripple1, UV * 0.08 + flow * fract(ph + 0.5)).xyz * 2.0 - 1.0;
	vec3 nflow = mix(nf, nf2, abs(ph * 2.0 - 1.0));
	vec3 nrm = normalize(n1 + n2 + nflow * step(0.001, length(flow)));
	NORMAL_MAP = nrm * 0.5 + 0.5;
	NORMAL_MAP_DEPTH = 0.35;

	// scene depth for shore blending / foam
	vec2 suv = SCREEN_UV;
	float scene_d = texture(depth_tex, suv).r;
	vec3 ndc = vec3(suv * 2.0 - 1.0, scene_d);
	vec4 vp = INV_PROJECTION_MATRIX * vec4(ndc, 1.0);
	float scene_z = -vp.z / vp.w;
	float frag_z = -VERTEX.z;
	float dz = scene_z - frag_z; // >0 when terrain is below the surface
	// robust depth fade: max of the scene-depth fade and the core's
	// precomputed depth01 (COLOR.r) — open sea is deep regardless.
	float deep01 = max(clamp(dz / 8.0, 0.0, 1.0), COLOR.r);
	float afade = max(clamp(dz / 6.0, 0.0, 1.0), COLOR.r);

	// depth colour + kind tint
	vec3 col = mix(shallow.rgb, deep.rgb, deep01);
	float kind = COLOR.g;
	if (kind < 0.25) { col *= vec3(0.7, 0.85, 1.2); }        // sea: deeper blue
	else if (kind < 0.75) { col *= vec3(0.9, 1.1, 0.95); }   // lake: greener
	else { col = mix(col, vec3(0.3, 0.85, 0.8), 0.35); }     // river: teal

	// shoreline foam where the terrain is within ~0.6 m of the surface
	float foam = (1.0 - smoothstep(0.0, 0.6, dz)) * (1.0 - deep01);
	foam *= 0.6 + 0.4 * sin(t * 3.0 + UV.x * 0.9 + UV.y * 0.7);
	col = mix(col, foam_col.rgb, clamp(foam, 0.0, 1.0) * 0.7);

	// refraction: the screen-texture mix is a shallow-water effect;
	// deep water is pure water colour (the bed must not show through).
	vec2 ruv = suv + nrm.xz * 0.04;
	vec3 behind = texture(screen_tex, ruv).rgb;
	col = mix(behind, col, 0.65 + 0.35 * deep01);

	ALBEDO = col;
	ALPHA = mix(0.35, 1.0, afade);
	// keep specular low at grazing angles: the grey horizon sky was
	// washing the surface out; albedo depth colour must dominate.
	ROUGHNESS = 0.25;
	SPECULAR = 0.3;
	METALLIC = 0.0;
}
