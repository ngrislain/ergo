pub const VERTEX: &str = r#"#version 300 es
precision highp float;
layout(location=0) in vec3 position;
layout(location=1) in vec3 normal;
layout(location=2) in vec3 face_color;
layout(location=3) in mat4 model;
layout(location=7) in vec4 color;
uniform mat4 view_projection;
uniform mat4 display_scale;
uniform mat4 shadow_matrix;
uniform vec3 eye;
uniform float projection_pixels;
uniform highp int stud_mode;
out vec3 v_normal;
out vec3 v_color;
out vec3 v_position;
out float v_selected;
out vec4 v_shadow;
out vec3 v_local;
out vec3 v_local_normal;
out vec3 v_tangent_x;
out vec3 v_tangent_z;
out float v_stud_geometry;
void main() {
    mat4 physical_model = display_scale * model;
    float stud_pixels=projection_pixels*0.6*length(physical_model[0].xyz)/max(distance(eye,physical_model[3].xyz),0.1);
    v_stud_geometry=stud_mode==3?1.0:(stud_mode==1?smoothstep(1.25,6.0,stud_pixels):0.0);
    vec3 local=position;
    if(stud_mode==1 && local.y>0.0)local.y*=v_stud_geometry;
    vec4 world = physical_model * vec4(local, 1.0);
    v_local=position;
    v_local_normal=normal;
    v_tangent_x=normalize(physical_model[0].xyz);
    v_tangent_z=normalize(physical_model[2].xyz);
    v_position = world.xyz;
    v_normal = normalize(transpose(inverse(mat3(physical_model))) * normal);
    v_color = face_color.x < 0.0 ? color.rgb : face_color;
    v_selected = color.a;
    v_shadow = shadow_matrix * world;
    gl_Position = view_projection * world;
}"#;

pub const FRAGMENT: &str = r#"#version 300 es
precision highp float;
in vec3 v_normal;
in vec3 v_color;
in vec3 v_position;
in float v_selected;
in vec4 v_shadow;
in vec3 v_local;
in vec3 v_local_normal;
in vec3 v_tangent_x;
in vec3 v_tangent_z;
in float v_stud_geometry;
uniform vec3 eye;
uniform int floor_mode;
uniform int grid;
uniform vec2 fog_limits;
uniform vec3 sun_direction;
uniform vec3 light_position;
uniform vec3 light_forward;
uniform int flashlight;
uniform int point_light;
uniform int studio;
uniform float sky_amount;
uniform highp sampler2DShadow shadow_map;
uniform float shadow_texel;
uniform float ground_stud_range;
uniform float projection_pixels;
uniform highp int stud_mode;
uniform vec2 stud_offset;
out vec4 out_color;
const float PI = 3.14159265359;
vec3 sky_horizon() { return mix(vec3(0.004,0.007,0.016),vec3(0.735,0.829,0.894),sky_amount); }
float visibility(vec3 n, vec3 light) {
    if (v_shadow.w <= 0.0) return 1.0;
    vec3 p = v_shadow.xyz / v_shadow.w * 0.5 + 0.5;
    if (any(lessThan(p,vec3(0.0))) || any(greaterThan(p,vec3(1.0)))) return 1.0;
    float bias = max(0.00025 * (1.0 - max(dot(n,light),0.0)),0.00006);
    float result = 0.0;
    for (int x=-1;x<=1;x++) for (int y=-1;y<=1;y++)
        result += texture(shadow_map,vec3(p.xy+vec2(float(x),float(y))*shadow_texel,p.z-bias));
    return result / 9.0;
}
void main() {
    vec3 n = normalize(v_normal);
    if (!gl_FrontFacing) n = -n;
    vec3 base = max(v_color,vec3(0.0));
    if (floor_mode == 1 && n.y > 0.5) {
        vec2 cell = abs(fract(v_position.xz/4.0-0.5)-0.5)/fwidth(v_position.xz/4.0);
        float line = 1.0-min(min(cell.x,cell.y),1.0);
        base *= 1.0-float(grid)*line*0.035;
        vec2 p = fract(v_position.xz)-0.5;
        float radius = length(p);
        float footprint = max(length(dFdx(v_position.xz)),length(dFdy(v_position.xz)));
        float rim = smoothstep(0.23,0.29,radius)*(1.0-smoothstep(0.29,0.34,radius));
        // Near studs already have real sides and cast shadows: don't shade a
        // second procedural rim beneath them. Tops keep the same albedo as the plate.
        float d=distance(eye,v_position);
        float real_studs = ground_stud_range > 0.0 ? smoothstep(1.25,6.0,projection_pixels*0.6/max(d,0.1))*(1.0-smoothstep(ground_stud_range*0.6,ground_stud_range,d)) : 0.0;
        float detail = (1.0-smoothstep(0.35,1.2,footprint))*(1.0-real_studs);
        n = normalize(vec3(p.x*rim*6.0*detail,1.0,p.y*rim*6.0*detail));
    }
    if(stud_mode>0 && v_local_normal.y>0.8 && abs(v_local.y)<0.001) {
        vec2 p=fract(v_local.xz+stud_offset)-0.5;
        float radius=length(p);
        float aa=max(fwidth(radius),0.015);
        float rim=smoothstep(0.27-aa,0.31,radius)*(1.0-smoothstep(0.31,0.35+aa,radius));
        float footprint=max(length(dFdx(v_local.xz)),length(dFdy(v_local.xz)));
        float detail=(1.0-smoothstep(0.35,1.2,footprint))*(1.0-v_stud_geometry);
        n=normalize(n+(v_tangent_x*p.x+v_tangent_z*p.y)*rim*6.0*detail);
    }
    vec3 albedo = pow(base,vec3(2.2));
    vec3 view = normalize(eye-v_position);
    vec3 light = sun_direction;
    float power = 3.0*(0.35+0.65*max(sun_direction.y,0.0));
    vec3 light_color = mix(vec3(1.0,0.52,0.25),vec3(1.0,0.97,0.9),clamp(sun_direction.y/0.35,0.0,1.0));
    if (flashlight == 1 || point_light == 1) {
        vec3 delta = light_position-v_position;
        float d2 = dot(delta,delta);
        light = delta*inversesqrt(max(d2,0.0001));
        float cone = smoothstep(cos(radians(32.0)),cos(radians(16.0)),dot(-light,light_forward));
        power = point_light == 1 ? min(6.0,4000.0/(d2+200.0)) : cone*min(8.0,8000.0/(d2+100.0));
        light_color = vec3(1.0,0.96,0.87);
    }
    float nl = max(dot(n,light),0.0);
    float nv = max(dot(n,view),0.001);
    vec3 half_delta = light+view;
    vec3 half_vector = half_delta/max(length(half_delta),0.0001);
    float nh = max(dot(n,half_vector),0.0);
    float vh = max(dot(view,half_vector),0.0);
    float roughness = 0.36;
    float a2 = pow(roughness,4.0);
    float denominator = nh*nh*(a2-1.0)+1.0;
    float distribution = a2/max(PI*denominator*denominator,0.0001);
    float k = pow(roughness+1.0,2.0)/8.0;
    float geometry = nv/(nv*(1.0-k)+k)*nl/(nl*(1.0-k)+k);
    vec3 fresnel = vec3(0.04)+(1.0-vec3(0.04))*pow(1.0-vh,5.0);
    vec3 specular = distribution*geometry*fresnel/max(4.0*nv*nl,0.001);
    float hemi = n.y*0.5+0.5;
    vec3 ambient = albedo*mix(vec3(0.008,0.012,0.022),mix(vec3(0.15,0.13,0.11),vec3(0.28,0.34,0.43),hemi),sky_amount);
    vec3 reflection = reflect(-view,n);
    vec3 environment = mix(sky_horizon(),mix(vec3(0.006,0.009,0.02),vec3(0.3,0.5,0.76),sky_amount),clamp(reflection.y,0.0,1.0));
    vec3 ambient_fresnel = vec3(0.04)+(1.0-vec3(0.04))*pow(1.0-nv,5.0);
    ambient += environment*ambient_fresnel*(1.0-roughness)*0.3;
    vec3 radiance = ambient+((1.0-fresnel)*albedo/PI+specular)*light_color*power*nl*visibility(n,light);
    if (studio == 1) {
        // Broad world-fixed fill and rim keep rear/side faces readable while
        // orbiting. The key retains real shadowing and plastic highlights.
        vec3 fill = normalize(vec3(0.8,0.45,-0.6));
        vec3 rim = normalize(vec3(-0.3,0.6,-1.0));
        radiance += albedo*(vec3(0.16)+vec3(0.62,0.70,0.82)*max(dot(n,fill),0.0)
            +vec3(0.28,0.31,0.36)*max(dot(n,rim),0.0));
    }
    vec3 shaded = pow(radiance/(radiance+vec3(1.0)),vec3(1.0/2.2));
    if (v_selected > 0.5) shaded = mix(shaded,vec3(1.0,0.65,0.13),0.52);
    float fog = smoothstep(fog_limits.x,fog_limits.y,distance(eye,v_position));
    out_color = vec4(mix(shaded,pow(sky_horizon(),vec3(1.0/2.2)),fog),1.0);
}"#;

pub const SHADOW_VERTEX: &str = r#"#version 300 es
precision highp float;
layout(location=0) in vec3 position;
layout(location=3) in mat4 model;
uniform mat4 display_scale;
uniform mat4 shadow_matrix;
uniform vec3 eye;
uniform float projection_pixels;
uniform highp int stud_mode;
void main() {
    mat4 physical_model=display_scale*model;
    vec3 local=position;
    if(stud_mode==1 && local.y>0.0) {
        float px=projection_pixels*0.6*length(physical_model[0].xyz)/max(distance(eye,physical_model[3].xyz),0.1);
        local.y*=smoothstep(1.25,6.0,px);
    }
    gl_Position=shadow_matrix*physical_model*vec4(local,1.0);
}
"#;
pub const SHADOW_FRAGMENT: &str = r#"#version 300 es
precision highp float;
void main() {}
"#;

pub const SKY_VERTEX: &str = r#"#version 300 es
out vec2 v_clip;
void main() {
    vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));
    v_clip=p*2.0-1.0;
    gl_Position=vec4(v_clip,1.0,1.0);
}"#;
pub const SKY_FRAGMENT: &str = r#"#version 300 es
precision highp float;
in vec2 v_clip;
uniform mat4 inverse_view_projection;
uniform vec3 eye;
uniform vec3 sun_direction;
uniform float sky_amount;
out vec4 out_color;
void main() {
    vec4 world=inverse_view_projection*vec4(v_clip,1.0,1.0);
    vec3 ray=normalize(world.xyz/world.w-eye);
    vec3 horizon=mix(vec3(0.004,0.007,0.016),vec3(0.735,0.829,0.894),sky_amount);
    vec3 zenith=mix(vec3(0.001,0.002,0.008),vec3(0.3,0.5,0.76),sky_amount);
    vec3 color=mix(horizon,zenith,pow(clamp(ray.y,0.0,1.0),0.5));
    if(sun_direction.y>0.0) {
        float cosine=dot(ray,sun_direction);
        float disk=smoothstep(cos(0.0055),cos(0.00465),cosine);
        float glow=pow(max(cosine,0.0),512.0)*0.15;
        color+=vec3(1.0,0.8,0.5)*(disk*4.0+glow);
    }
    out_color=vec4(pow(clamp(color,0.0,1.0),vec3(1.0/2.2)),1.0);
}"#;
