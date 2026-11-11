//! The viewport's wgpu drawing: a background gradient, the models, the supports, the grid and axes,
//! then the joint handles over everything, drawn through an egui paint callback into egui's own
//! render pass. In Slice edition the solids are cut at the shown layer and the cut is capped with
//! the layer's image; the 2D layer view draws that image alone, from the same textures.

use std::collections::HashMap;
use std::sync::Arc;

use egui_wgpu::wgpu;
use resin_engine::geom::{Affine3, Mesh, Vec3, unit_sphere};
use resin_engine::render::Line;
use wgpu::util::DeviceExt;

use crate::layer_image::{ImageKey, LayerImage, TileRect};
use crate::support_routes::{Look, SupportMesh};

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
    /// x, y: keep what lies between these heights; z: 1 when that applies; w: 1 dims the layer.
    clip: [f32; 4],
    /// The cap's fill (linear RGB), and its outline or the layer view's pixel grid.
    cap: [f32; 4],
    edge: [f32; 4],
    /// Faces looking down are tinted toward this (linear RGB).
    overhang: [f32; 4],
}

/// A corner of a layer quad: where it is (scene mm for the cap, image mm for the 2D view), its
/// texture coordinates and its image pixel.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerVertex {
    pos: [f32; 3],
    uv: [f32; 2],
    px: [f32; 2],
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

/// The opacity of a joint handle where the scene is in front of it: there, so it can be grabbed,
/// and faded, so it doesn't read as in front.
const HIDDEN_HANDLE_ALPHA: f32 = 0.35;

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

/// A model to draw: its mesh as imported (uploaded once per id), where it sits, its linear RGB
/// `color`, and whether its faces looking down are tinted in the frame's overhang colour.
pub struct ModelDraw {
    pub id: u64,
    pub mesh: Arc<Mesh>,
    pub transform: Affine3,
    pub color: [f32; 4],
    pub overhang: bool,
}

/// Per model, as an instance attribute: the placement's rows (3×4), the normals' rows (the
/// cofactor matrix, so a stretched model still shades right) and the colour, whose alpha is how
/// much faces looking down are tinted (the solids are opaque, so it is free).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ModelInstance {
    rows: [[f32; 4]; 3],
    normal_rows: [[f32; 4]; 3],
    color: [f32; 4],
}

impl ModelInstance {
    fn new(m: &ModelDraw) -> ModelInstance {
        let [r, g, b, _] = m.color;
        ModelInstance::placed(&m.transform, [r, g, b, f32::from(u8::from(m.overhang))])
    }
    fn placed(a: &Affine3, color: [f32; 4]) -> ModelInstance {
        let t: [f64; 3] = a.t.into();
        let rows = [0, 1, 2].map(|i| [a.m[i][0] as f32, a.m[i][1] as f32, a.m[i][2] as f32, t[i] as f32]);
        let normal_rows = a.normal_matrix().map(|r| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0]);
        ModelInstance { rows, normal_rows, color }
    }
}

/// The heights between which the solids are drawn, mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub below: f32,
    pub above: f32,
}

/// The shown layer's image laid on the cut at height `z`: its lit pixels in `color` with an
/// `edge` outline, faded when `dim`.
pub struct Cap {
    pub image: Arc<LayerImage>,
    pub z: f32,
    pub color: [f32; 4],
    pub edge: [f32; 4],
    pub dim: bool,
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
    /// Joint handles: drawn after everything, faded where the scene hides them.
    pub handles: Vec<Sphere>,
    pub models: Vec<ModelDraw>,
    /// The supports' solids, already in the scene, and the colour of each [`Look`].
    pub supports: Option<Arc<SupportMesh>>,
    pub support_colors: [[f32; 4]; 3],
    /// The tint of the models' faces looking down, for those drawn with [`ModelDraw::overhang`].
    pub overhang: [f32; 4],
    /// In Slice edition with a job: what is kept of the solids, and the cap on the cut.
    pub clip: Option<Clip>,
    pub cap: Option<Cap>,
}

/// One frame of the 2D layer view: the image, `view_proj` from image millimetres (from its
/// top-left corner, Y down) to the view, the background, the pixel grid's colour.
pub struct LayerFrame {
    pub image: Arc<LayerImage>,
    pub view_proj: [[f32; 4]; 4],
    pub bg_top: [f32; 4],
    pub bg_bottom: [f32; 4],
    pub grid: [f32; 4],
    pub dim: bool,
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

pub struct LayerViewCallback(pub LayerFrame);

impl egui_wgpu::CallbackTrait for LayerViewCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get_mut::<Renderer>() {
            r.prepare_layer_view(device, queue, &self.0);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        if let Some(r) = resources.get::<Renderer>() {
            r.paint_layer_view(pass);
        }
    }
}

struct GpuMesh {
    chunks: Vec<(wgpu::Buffer, u32)>,
}

/// One tile of the shown layer on the GPU, with a bind group for each way it is sampled.
struct GpuTile {
    rect: TileRect,
    texture: wgpu::Texture,
    cap_group: wgpu::BindGroup,
    flat_group: wgpu::BindGroup,
}

/// The layer whose tiles are uploaded.
struct GpuLayer {
    key: ImageKey,
    tiles: Vec<GpuTile>,
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
    /// Handles where the scene hides them, then where it doesn't.
    handle_hidden: wgpu::RenderPipeline,
    handle_visible: wgpu::RenderPipeline,
    handle_buffer: Option<(wgpu::Buffer, u64)>,
    handle_count: u32,
    /// Uploaded meshes by model id (a model's mesh never changes).
    meshes: HashMap<u64, GpuMesh>,
    /// One [`ModelInstance`] per model this frame, in `ViewFrame::models` order, then one per
    /// support [`Look`].
    model_instances: Option<(wgpu::Buffer, u64)>,
    /// The supports' solids, and the ones uploaded there: uploaded again only when they change.
    support_buffer: Option<(wgpu::Buffer, u64)>,
    supports_uploaded: Option<Arc<SupportMesh>>,
    layer_layout: wgpu::BindGroupLayout,
    /// The cap blends between texels; the 2D view shows each pixel as a square up close.
    cap_sampler: wgpu::Sampler,
    flat_sampler: wgpu::Sampler,
    cap: wgpu::RenderPipeline,
    flat: wgpu::RenderPipeline,
    layer: Option<GpuLayer>,
    /// This frame's cap quads (one per tile; none without a cap), and the 2D view's.
    cap_quads: Option<(wgpu::Buffer, u64)>,
    cap_count: u32,
    flat_quads: Option<(wgpu::Buffer, u64)>,
    /// The 2D view's own frame uniform, so it never shares one with the 3D view.
    flat_uniform: wgpu::Buffer,
    flat_bind_group: wgpu::BindGroup,
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
        let flat_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer_view_frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let flat_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer_view_frame"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: flat_uniform.as_entire_binding() }],
        });
        let layer_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer_tile"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layer_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layer"),
            bind_group_layouts: &[Some(&layout), Some(&layer_layout)],
            immediate_size: 0,
        });
        let sampler = |label: &str, mag: wgpu::FilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                mag_filter: mag,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        let cap_sampler = sampler("layer_cap", wgpu::FilterMode::Linear);
        let flat_sampler = sampler("layer_flat", wgpu::FilterMode::Nearest);
        let depth_format = egui_wgpu::depth_format_from_bits(DEPTH_BITS, 0).unwrap_or(wgpu::TextureFormat::Depth24Plus);
        let multisample = wgpu::MultisampleState { count: u32::from(MSAA_SAMPLES), mask: !0, alpha_to_coverage_enabled: false };

        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        vs: &str,
                        fs: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool,
                        depth_compare: wgpu::CompareFunction,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
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

        let background = pipeline(
            "viewport_background",
            &pipeline_layout,
            "bg_vs",
            "bg_fs",
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            false,
            wgpu::CompareFunction::Always,
            None,
        );
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
            &pipeline_layout,
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
            &pipeline_layout,
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
            &pipeline_layout,
            "sphere_vs",
            "sphere_fs",
            &sphere_layouts,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::CompareFunction::Less,
            None,
        );
        // Hidden first: once the visible pass writes its depth, the far test would pass on it too.
        let handle_hidden = pipeline(
            "viewport_handle_hidden",
            &pipeline_layout,
            "sphere_vs",
            "handle_fs",
            &sphere_layouts,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            wgpu::CompareFunction::Greater,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let handle_visible = pipeline(
            "viewport_handle_visible",
            &pipeline_layout,
            "sphere_vs",
            "handle_fs",
            &sphere_layouts,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::CompareFunction::LessEqual,
            None,
        );
        let layer_layouts = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LayerVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x2],
        })];
        let cap = pipeline(
            "layer_cap",
            &layer_pipeline_layout,
            "layer_vs",
            "cap_fs",
            &layer_layouts,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::CompareFunction::Less,
            None,
        );
        let flat = pipeline(
            "layer_flat",
            &layer_pipeline_layout,
            "layer_vs",
            "flat_fs",
            &layer_layouts,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            wgpu::CompareFunction::Always,
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
            handle_hidden,
            handle_visible,
            handle_buffer: None,
            handle_count: 0,
            meshes: HashMap::new(),
            model_instances: None,
            support_buffer: None,
            supports_uploaded: None,
            layer_layout,
            cap_sampler,
            flat_sampler,
            cap,
            flat,
            layer: None,
            cap_quads: None,
            cap_count: 0,
            flat_quads: None,
            flat_uniform,
            flat_bind_group,
        }
    }

    /// Upload `image`'s tiles and mipmaps unless they are there; textures of the same sizes are
    /// written again rather than made anew, as the slider steps through a job.
    fn upload_layer(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &LayerImage) {
        if self.layer.as_ref().is_some_and(|l| l.key == image.key) {
            return;
        }
        let same_shape = self.layer.as_ref().is_some_and(|l| {
            l.tiles.len() == image.tiles.len()
                && l.tiles.iter().zip(&image.tiles).all(|(g, t)| g.rect == t.rect && g.texture.mip_level_count() as usize == t.mips.len() + 1)
        });
        if !same_shape {
            let tiles = image.tiles.iter().map(|t| self.new_tile(device, t.rect, t.mips.len() as u32 + 1)).collect();
            self.layer = Some(GpuLayer { key: image.key, tiles });
        }
        let Some(layer) = &mut self.layer else { return };
        layer.key = image.key;
        let stride = image.plate.width;
        for (gpu, tile) in layer.tiles.iter().zip(&image.tiles) {
            let r = tile.rect;
            let start = r.y as usize * stride as usize + r.x as usize;
            let Some(first) = image.pixels.get(start..) else { continue };
            write_level(queue, &gpu.texture, 0, first, stride, r.width, r.height);
            for (level, (w, h, data)) in tile.mips.iter().enumerate() {
                write_level(queue, &gpu.texture, level as u32 + 1, data, *w, *w, *h);
            }
        }
    }

    fn new_tile(&self, device: &wgpu::Device, rect: TileRect, levels: u32) -> GpuTile {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer_tile"),
            size: wgpu::Extent3d { width: rect.width.max(1), height: rect.height.max(1), depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = |sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("layer_tile"),
                layout: &self.layer_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };
        GpuTile { rect, cap_group: group(&self.cap_sampler), flat_group: group(&self.flat_sampler), texture }
    }

    fn prepare_layer_view(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, f: &LayerFrame) {
        let u = FrameUniform {
            view_proj: f.view_proj,
            light: [0.0, 0.0, 1.0, if self.srgb_target { 1.0 } else { 0.0 }],
            back: [0.0, 0.0, 1.0, 0.0],
            grid: [0.0; 4],
            bg_top: f.bg_top,
            bg_bottom: f.bg_bottom,
            clip: [0.0, 0.0, 0.0, if f.dim { 1.0 } else { 0.0 }],
            cap: [0.0; 4],
            edge: f.grid,
            overhang: [0.0; 4],
        };
        queue.write_buffer(&self.flat_uniform, 0, bytemuck::bytes_of(&u));
        self.upload_layer(device, queue, &f.image);
        let (kx, ky) = f.image.plate.pitch();
        let verts = layer_quads(&f.image, |px, py| [(px * kx) as f32, (py * ky) as f32, 0.0]);
        write_growing(device, queue, &mut self.flat_quads, "layer_flat_quads", bytemuck::cast_slice(&verts));
    }

    fn paint_layer_view(&self, pass: &mut wgpu::RenderPass<'static>) {
        pass.set_bind_group(0, &self.flat_bind_group, &[]);
        pass.set_pipeline(&self.background);
        pass.draw(0..3, 0..1);
        let (Some(layer), Some((quads, _))) = (&self.layer, &self.flat_quads) else { return };
        pass.set_pipeline(&self.flat);
        pass.set_vertex_buffer(0, quads.slice(..));
        for (i, tile) in layer.tiles.iter().enumerate() {
            let first = i as u32 * 6;
            pass.set_bind_group(1, &tile.flat_group, &[]);
            pass.draw(first..first + 6, 0..1);
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
            clip: match f.clip {
                Some(c) => [c.below, c.above, 1.0, f32::from(u8::from(f.cap.as_ref().is_some_and(|c| c.dim)))],
                None => [0.0; 4],
            },
            cap: f.cap.as_ref().map_or([0.0; 4], |c| c.color),
            edge: f.cap.as_ref().map_or([0.0; 4], |c| c.edge),
            overhang: f.overhang,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        self.cap_count = 0;
        if let Some(cap) = &f.cap {
            self.upload_layer(device, queue, &cap.image);
            let plate = cap.image.plate;
            let verts = layer_quads(&cap.image, |px, py| {
                let (x, y) = plate.to_plate(px, py);
                [x as f32, y as f32, cap.z]
            });
            write_growing(device, queue, &mut self.cap_quads, "layer_cap_quads", bytemuck::cast_slice(&verts));
            self.cap_count = cap.image.tiles.len() as u32;
        }

        self.meshes.retain(|id, _| f.models.iter().any(|m| m.id == *id));
        for m in &f.models {
            self.meshes.entry(m.id).or_insert_with(|| upload_mesh(device, &m.mesh));
        }
        let mut instances: Vec<ModelInstance> = f.models.iter().map(ModelInstance::new).collect();
        instances.extend(f.support_colors.map(|[r, g, b, _]| ModelInstance::placed(&Affine3::IDENTITY, [r, g, b, 0.0])));
        write_growing(device, queue, &mut self.model_instances, "viewport_model_instances", bytemuck::cast_slice(&instances));

        let fresh = match (&f.supports, &self.supports_uploaded) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (a, b) => a.is_some() != b.is_some(),
        };
        if fresh {
            let verts: Vec<MeshVertex> =
                f.supports.iter().flat_map(|m| m.vertices.iter()).map(|v| MeshVertex { pos: v.pos, normal: v.normal }).collect();
            write_growing(device, queue, &mut self.support_buffer, "viewport_supports", bytemuck::cast_slice(&verts));
            self.supports_uploaded = f.supports.clone();
        }

        let verts: Vec<LineVertex> = f
            .lines
            .iter()
            .flat_map(|l| [LineVertex { pos: l.a.to_f32(), color: l.color }, LineVertex { pos: l.b.to_f32(), color: l.color }])
            .collect();
        write_growing(device, queue, &mut self.line_buffer, "viewport_lines", bytemuck::cast_slice(&verts));
        self.line_count = verts.len() as u32;

        let instances = sphere_instances(&f.spheres, None);
        write_growing(device, queue, &mut self.sphere_buffer, "viewport_spheres", bytemuck::cast_slice(&instances));
        self.sphere_count = instances.len() as u32;
        // The visible pass draws without blending, so the alpha only fades the hidden one.
        let instances = sphere_instances(&f.handles, Some(HIDDEN_HANDLE_ALPHA));
        write_growing(device, queue, &mut self.handle_buffer, "viewport_handles", bytemuck::cast_slice(&instances));
        self.handle_count = instances.len() as u32;
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
            if let (Some(mesh), Some((buffer, _))) = (&self.supports_uploaded, &self.support_buffer) {
                for look in Look::ALL {
                    let Some(range) = mesh.ranges.get(look.index()).filter(|r| !r.is_empty()) else { continue };
                    let at = (f.models.len() + look.index()) as u64 * INSTANCE_STRIDE;
                    pass.set_vertex_buffer(1, instances.slice(at..at + INSTANCE_STRIDE));
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(range.clone(), 0..1);
                }
            }
        }
        if let (Some(layer), Some((quads, _))) = (&self.layer, &self.cap_quads)
            && self.cap_count > 0
        {
            pass.set_pipeline(&self.cap);
            pass.set_vertex_buffer(0, quads.slice(..));
            for (i, tile) in layer.tiles.iter().enumerate().take(self.cap_count as usize) {
                let first = i as u32 * 6;
                pass.set_bind_group(1, &tile.cap_group, &[]);
                pass.draw(first..first + 6, 0..1);
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
        if let Some((buffer, _)) = &self.handle_buffer
            && self.handle_count > 0
        {
            for pipeline in [&self.handle_hidden, &self.handle_visible] {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, self.sphere_mesh.0.slice(..));
                pass.set_vertex_buffer(1, buffer.slice(..));
                pass.draw(0..self.sphere_mesh.1, 0..self.handle_count);
            }
        }
    }
}

/// Two triangles per tile of `image`, each corner placed by `at` from its image pixel position.
fn layer_quads(image: &LayerImage, at: impl Fn(f64, f64) -> [f32; 3]) -> Vec<LayerVertex> {
    image
        .tiles
        .iter()
        .flat_map(|t| {
            let r = t.rect;
            let (x0, y0) = (f64::from(r.x), f64::from(r.y));
            let (x1, y1) = (x0 + f64::from(r.width), y0 + f64::from(r.height));
            let corner = |x: f64, y: f64, u: f32, v: f32| LayerVertex { pos: at(x, y), uv: [u, v], px: [x as f32, y as f32] };
            let (a, b, c, d) = (corner(x0, y0, 0.0, 0.0), corner(x1, y0, 1.0, 0.0), corner(x1, y1, 1.0, 1.0), corner(x0, y1, 0.0, 1.0));
            [a, b, c, a, c, d]
        })
        .collect()
}

/// Write one mip `level` of `texture`, `width × height` texels read from `data` in rows of
/// `stride` bytes.
fn write_level(queue: &wgpu::Queue, texture: &wgpu::Texture, level: u32, data: &[u8], stride: u32, width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(height) },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
}

/// The drawable spheres as instances, their alpha replaced by `alpha` if given.
fn sphere_instances(spheres: &[Sphere], alpha: Option<f32>) -> Vec<SphereInstance> {
    spheres
        .iter()
        .filter(|s| s.center.is_finite() && s.radius.is_finite() && s.radius > 0.0)
        .map(|s| {
            let [x, y, z] = s.center.to_f32();
            let [r, g, b, a] = s.color;
            SphereInstance { center_radius: [x, y, z, s.radius], color: [r, g, b, alpha.unwrap_or(a)] }
        })
        .collect()
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
