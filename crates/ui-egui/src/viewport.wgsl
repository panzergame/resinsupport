struct Frame {
    view_proj: mat4x4<f32>,
    // xyz: unit direction toward the light; w: 1 when the target wants sRGB-encoded output.
    light: vec4<f32>,
    // xyz: unit direction from the target toward the eye.
    back: vec4<f32>,
    // xy: grid centre, z: grid radius (lines fade toward it).
    grid: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

fn encode(c: vec3<f32>) -> vec3<f32> {
    if frame.light.w > 0.5 {
        return c;
    }
    // The target stores what it gets: encode to sRGB here.
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// ---- background: one triangle covering the viewport ----

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn bg_vs(@builtin(vertex_index) i: u32) -> BgOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: BgOut;
    out.pos = vec4<f32>(uv * 2.0 - 1.0, 1.0, 1.0);
    out.t = uv.y * 0.5;
    return out;
}

@fragment
fn bg_fs(in: BgOut) -> @location(0) vec4<f32> {
    let c = mix(frame.bg_bottom.rgb, frame.bg_top.rgb, clamp(in.t * 2.0, 0.0, 1.0));
    return vec4<f32>(encode(c), 1.0);
}

// ---- model ----

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // Per model (instance step): the placement's rows (xyz linear, w translation), the
    // normals' rows, the colour.
    @location(2) row0: vec4<f32>,
    @location(3) row1: vec4<f32>,
    @location(4) row2: vec4<f32>,
    @location(5) nrow0: vec4<f32>,
    @location(6) nrow1: vec4<f32>,
    @location(7) nrow2: vec4<f32>,
    @location(8) color: vec4<f32>,
};

struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn mesh_vs(v: MeshIn) -> MeshOut {
    var out: MeshOut;
    let p = vec4<f32>(v.pos, 1.0);
    let world = vec3<f32>(dot(v.row0, p), dot(v.row1, p), dot(v.row2, p));
    out.pos = frame.view_proj * vec4<f32>(world, 1.0);
    out.normal = vec3<f32>(dot(v.nrow0.xyz, v.normal), dot(v.nrow1.xyz, v.normal), dot(v.nrow2.xyz, v.normal));
    out.color = v.color.rgb;
    return out;
}

@fragment
fn mesh_fs(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    // Faces wound the wrong way (an inside-out STL) show as a darker red-tinted shade.
    var base = in.color;
    if !front {
        n = -n;
        base = mix(base, vec3<f32>(0.55, 0.12, 0.12), 0.7);
    }
    let key = max(dot(n, frame.light.xyz), 0.0);
    let fill = max(dot(n, frame.back.xyz), 0.0);
    let c = base * (0.18 + 0.62 * key + 0.25 * fill);
    return vec4<f32>(encode(c), 1.0);
}

// ---- support tips: a unit sphere, instanced ----

struct SphereIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // xyz: centre, w: radius.
    @location(2) center_radius: vec4<f32>,
    @location(3) color: vec4<f32>,
};

struct SphereOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn sphere_vs(v: SphereIn) -> SphereOut {
    var out: SphereOut;
    let world = v.center_radius.xyz + v.pos * v.center_radius.w;
    out.pos = frame.view_proj * vec4<f32>(world, 1.0);
    out.normal = v.normal;
    out.color = v.color;
    return out;
}

fn sphere_shade(in: SphereOut) -> vec3<f32> {
    let n = normalize(in.normal);
    let key = max(dot(n, frame.light.xyz), 0.0);
    let fill = max(dot(n, frame.back.xyz), 0.0);
    return encode(in.color.rgb * (0.25 + 0.55 * key + 0.3 * fill));
}

@fragment
fn sphere_fs(in: SphereOut) -> @location(0) vec4<f32> {
    return vec4<f32>(sphere_shade(in), 1.0);
}

// Joint handles, drawn over everything: only the near side, so a handle without depth writes
// doesn't show its far side through itself; the instance's alpha fades the part behind the scene.
@fragment
fn handle_fs(in: SphereOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if !front {
        discard;
    }
    return vec4<f32>(sphere_shade(in), in.color.a);
}

// ---- lines: grid and axes ----

struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
};

@vertex
fn line_vs(v: LineIn) -> LineOut {
    var out: LineOut;
    out.pos = frame.view_proj * vec4<f32>(v.pos, 1.0);
    out.color = v.color;
    out.world = v.pos;
    return out;
}

@fragment
fn line_fs(in: LineOut) -> @location(0) vec4<f32> {
    let r = max(frame.grid.z, 1e-6);
    let d = length(in.world.xy - frame.grid.xy) / r;
    let fade = 1.0 - smoothstep(0.55, 1.0, d);
    return vec4<f32>(encode(in.color.rgb), in.color.a * fade);
}
