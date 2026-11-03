//! The viewport's wgpu drawing: a background gradient, the models, support tips, the grid and axes, drawn
//! through an egui paint callback into egui's own render pass.

use std::collections::HashMap;
use std::sync::Arc;

use egui_wgpu::wgpu;
use resin_engine::geom::{Affine3, Mesh, Vec3, unit_sphere};
use resin_engine::render::Line;
use wgpu::util::DeviceExt;

/// What the window's render pass carries; the app's native options must ask for the same, and
/// the viewport pipelines are built to match.
pub const DEPTH_BITS: u8 = 24;
pub const MSAA_SAMPLES: u16 = 4;

/// Biggest vertex buffer created for a mesh; larger meshes are split. Kept under wgpu's default
/// `max_buffer_size` (256 MiB) so a big STL never trips a device limit.
const MAX_CHUNK_BYTES: u64 = 128 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    back: [f32; 4],
    grid: [f32; 4],
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshVertex {
    pos: [f32; 3],
    normal: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SphereInstance {
    center_radius: [f32; 4],
    color: [f32; 4],
}

const INSTANCE_STRIDE: u64 = std::mem::size_of::<ModelInstance>() as u64;

/// Triangle subdivisions of the drawn sphere: smooth enough at the size a tip is looked at.
const SPHERE_DETAIL: u32 = 2;

/// A shaded sphere (a support tip), in linear RGBA.
#[derive(Clone, Copy, Debug)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
    pos: [f32; 3],
    color: [f32; 4],
}

/// A model to draw: its mesh as imported (uploaded once per id), where it sits, and its linear
/// RGBA `color`.
pub struct ModelDraw {
    pub id: u64,
    pub mesh: Arc<Mesh>,
    pub transform: Affine3,
    pub color: [f32; 4],
}

/// Per model, as an instance attribute: the placement's rows (3×4), the normals' rows (the
/// cofactor matrix, so a stretched model still shades right) and the colour.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ModelInstance {
    rows: [[f32; 4]; 3],
    normal_rows: [[f32; 4]; 3],
    color: [f32; 4],
}

impl ModelInstance {
    fn new(m: &ModelDraw) -> ModelInstance {
        let a = &m.transform;
        let t: [f64; 3] = a.t.into();
        let rows = [0, 1, 2].map(|i| [a.m[i][0] as f32, a.m[i][1] as f32, a.m[i][2] as f32, t[i] as f32]);
        let normal_rows = a.normal_matrix().map(|r| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0]);
        ModelInstance { rows, normal_rows, color: m.color }
    }
}

/// One frame of the viewport, handed from the UI to the GPU.
pub struct ViewFrame {
    pub view_proj: [[f32; 4]; 4],
    /// Unit vectors: toward the key light, and toward the eye.
    pub light: Vec3,
    pub back: Vec3,
    pub bg_top: [f32; 4],
    pub bg_bottom: [f32; 4],
    pub grid_center: Vec3,
    pub grid_radius: f64,
    pub lines: Vec<Line>,
    pub spheres: Vec<Sphere>,
    pub models: Vec<ModelDraw>,
}

pub struct ViewportCallback(pub ViewFrame);

impl egui_wgpu::CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get_mut::<Renderer>() {
            r.prepare(device, queue, &self.0);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        if let Some(r) = resources.get::<Renderer>() {
            r.paint(pass, &self.0);
        }
    }
}

struct GpuMesh {
    chunks: Vec<(wgpu::Buffer, u32)>,
}

pub struct Renderer {
    srgb_target: bool,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    background: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    line_buffer: Option<(wgpu::Buffer, u64)>,
    line_count: u32,
    sphere: wgpu::RenderPipeline,
    /// The unit sphere's vertices, and how many.
    sphere_mesh: (wgpu::Buffer, u32),
    sphere_buffer: Option<(wgpu::Buffer, u64)>,
    sphere_count: u32,
    /// Uploaded meshes by model id (a model's mesh never changes).
    meshes: HashMap<u64, GpuMesh>,
    /// One [`ModelInstance`] per model this frame, in `ViewFrame::models` order.
    model_instances: Option<(wgpu::Buffer, u64)>,
}

impl Renderer {
    /// Build the pipelines and store them where the paint callback finds them.
    pub fn install(rs: &egui_wgpu::RenderState) {
        let r = Renderer::new(&rs.device, rs.target_format);
        rs.renderer.write().callback_resources.insert(r);
    }

    fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport_frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport_frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport_frame"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let depth_format = egui_wgpu::depth_format_from_bits(DEPTH_BITS, 0).unwrap_or(wgpu::TextureFormat::Depth24Plus);
        let multisample = wgpu::MultisampleState { count: u32::from(MSAA_SAMPLES), mask: !0, alpha_to_coverage_enabled: false };

        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool,
                        depth_compare: wgpu::CompareFunction,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(depth_compare),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: target, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let background =
            pipeline("viewport_background", "bg_vs", "bg_fs", &[], wgpu::PrimitiveTopology::TriangleList, false, wgpu::CompareFunction::Always, None);
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        };
        let model_layouts = [
            Some(mesh_layout.clone()),
            Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_STRIDE,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4],
            }),
        ];
        let mesh = pipeline(
            "viewport_mesh",
            "mesh_vs",
            "mesh_fs",
            &model_layouts,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::CompareFunction::Less,
            None,
        );
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
        };
        let lines = pipeline(
            "viewport_lines",
            "line_vs",
            "line_fs",
            &[Some(line_layout)],
            wgpu::PrimitiveTopology::LineList,
            false,
            wgpu::CompareFunction::LessEqual,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let sphere_layouts = [
            Some(mesh_layout.clone()),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<SphereInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4],
            }),
        ];
        let sphere = pipeline(
            "viewport_sphere",
            "sphere_vs",
            "sphere_fs",
            &sphere_layouts,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::CompareFunction::Less,
            None,
        );
        // Smooth shading: on a unit sphere the normal is the position.
        let sphere_verts: Vec<MeshVertex> = unit_sphere(SPHERE_DETAIL).iter().flatten().map(|&p| MeshVertex { pos: p, normal: p }).collect();
        let sphere_mesh = (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport_sphere_mesh"),
                contents: bytemuck::cast_slice(&sphere_verts),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            sphere_verts.len() as u32,
        );
        Renderer {
            srgb_target: target.is_srgb(),
            uniform,
            bind_group,
            background,
            mesh,
            lines,
            line_buffer: None,
            line_count: 0,
            sphere,
            sphere_mesh,
            sphere_buffer: None,
            sphere_count: 0,
            meshes: HashMap::new(),
            model_instances: None,
        }
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, f: &ViewFrame) {
        let v4 = |v: Vec3, w: f32| [v.x as f32, v.y as f32, v.z as f32, w];
        let u = FrameUniform {
            view_proj: f.view_proj,
            light: v4(f.light, if self.srgb_target { 1.0 } else { 0.0 }),
            back: v4(f.back, 0.0),
            grid: [f.grid_center.x as f32, f.grid_center.y as f32, f.grid_radius as f32, 0.0],
            bg_top: f.bg_top,
            bg_bottom: f.bg_bottom,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));

        self.meshes.retain(|id, _| f.models.iter().any(|m| m.id == *id));
        for m in &f.models {
            self.meshes.entry(m.id).or_insert_with(|| upload_mesh(device, &m.mesh));
        }
        let instances: Vec<ModelInstance> = f.models.iter().map(ModelInstance::new).collect();
        write_growing(device, queue, &mut self.model_instances, "viewport_model_instances", bytemuck::cast_slice(&instances));

        let verts: Vec<LineVertex> = f
            .lines
            .iter()
            .flat_map(|l| [LineVertex { pos: l.a.to_f32(), color: l.color }, LineVertex { pos: l.b.to_f32(), color: l.color }])
            .collect();
        write_growing(device, queue, &mut self.line_buffer, "viewport_lines", bytemuck::cast_slice(&verts));
        self.line_count = verts.len() as u32;

        let instances: Vec<SphereInstance> = f
            .spheres
            .iter()
            .filter(|s| s.center.is_finite() && s.radius.is_finite() && s.radius > 0.0)
            .map(|s| {
                let [x, y, z] = s.center.to_f32();
                SphereInstance { center_radius: [x, y, z, s.radius], color: s.color }
            })
            .collect();
        write_growing(device, queue, &mut self.sphere_buffer, "viewport_spheres", bytemuck::cast_slice(&instances));
        self.sphere_count = instances.len() as u32;
    }

    fn paint(&self, pass: &mut wgpu::RenderPass<'static>, f: &ViewFrame) {
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.background);
        pass.draw(0..3, 0..1);
        if let Some((instances, _)) = &self.model_instances {
            pass.set_pipeline(&self.mesh);
            for (i, m) in f.models.iter().enumerate() {
                let Some(gpu) = self.meshes.get(&m.id) else { continue };
                let at = i as u64 * INSTANCE_STRIDE;
                pass.set_vertex_buffer(1, instances.slice(at..at + INSTANCE_STRIDE));
                for (buffer, count) in &gpu.chunks {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
        }
        if let Some((buffer, _)) = &self.sphere_buffer
            && self.sphere_count > 0
        {
            pass.set_pipeline(&self.sphere);
            pass.set_vertex_buffer(0, self.sphere_mesh.0.slice(..));
            pass.set_vertex_buffer(1, buffer.slice(..));
            pass.draw(0..self.sphere_mesh.1, 0..self.sphere_count);
        }
        if let Some((buffer, _)) = &self.line_buffer
            && self.line_count > 0
        {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..self.line_count, 0..1);
        }
    }
}

/// Write `bytes` to a per-frame vertex buffer, replacing it with a bigger one when they don't fit.
fn write_growing(device: &wgpu::Device, queue: &wgpu::Queue, slot: &mut Option<(wgpu::Buffer, u64)>, label: &str, bytes: &[u8]) {
    let needed = bytes.len() as u64;
    if slot.as_ref().is_none_or(|(_, cap)| *cap < needed) {
        let cap = needed.next_power_of_two().max(4096);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        *slot = Some((buffer, cap));
    }
    if let Some((buffer, _)) = slot
        && needed > 0
    {
        queue.write_buffer(buffer, 0, bytes);
    }
}

/// Flat-shaded vertices (each corner carries its face's normal), split into buffers under
/// [`MAX_CHUNK_BYTES`] and the device's own limit.
fn upload_mesh(device: &wgpu::Device, mesh: &Mesh) -> GpuMesh {
    let per_triangle = 3 * std::mem::size_of::<MeshVertex>() as u64;
    let limit = MAX_CHUNK_BYTES.min(device.limits().max_buffer_size);
    let tris_per_chunk = (limit / per_triangle).max(1) as usize;
    let chunks = mesh
        .triangles
        .chunks(tris_per_chunk)
        .map(|chunk| {
            let verts: Vec<MeshVertex> = chunk
                .iter()
                .flat_map(|t| {
                    let n = Mesh::face_normal(t).unwrap_or(Vec3::Z).to_f32();
                    t.map(|pos| MeshVertex { pos, normal: n })
                })
                .collect();
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport_mesh"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buffer, verts.len() as u32)
        })
        .collect();
    GpuMesh { chunks }
}
