//! wgpu renderer for the live viewer and for 3D snapshots.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use lamina_model::Side;
use lamina_scene::theme;
use lamina_scene::{Scene2D, Scene3D, SceneLayer};
use lamina_step::Mesh;
use wgpu::util::DeviceExt;

use crate::raster::{View, appearance};
use crate::tess::{BoardVertex, Tris, board_slab};

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MSAA: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    view2d: [f32; 4],
    params: [f32; 4],
    bg_top: [f32; 4],
    bg_bot: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct V2 {
    pos: [f32; 2],
    color: [f32; 4],
    depth: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VM {
    pos: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

fn lin(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn rgba_lin(c: theme::Rgba) -> [f32; 4] {
    [lin(c[0]), lin(c[1]), lin(c[2]), c[3] as f32 / 255.0]
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    ubuf: wgpu::Buffer,
    ubg: wgpu::BindGroup,
    tex_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipe_bg: wgpu::RenderPipeline,
    pipe_2d: wgpu::RenderPipeline,
    pipe_mesh: wgpu::RenderPipeline,
    pipe_board: wgpu::RenderPipeline,
}

/// An offscreen render target: multisampled colour and depth, resolved into
/// a texture that can be shown or read back.
pub struct Target {
    pub width: u32,
    pub height: u32,
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    pub resolve: wgpu::Texture,
    pub resolve_view: wgpu::TextureView,
}

/// 2D layers uploaded to the GPU.
pub struct Gpu2D {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    /// (layer, index start, index count)
    pub ranges: Vec<(SceneLayer, u32, u32)>,
}

/// Board and parts uploaded to the GPU.
pub struct Gpu3D {
    board_vb: wgpu::Buffer,
    board_ib: wgpu::Buffer,
    board_count: u32,
    board_bg: wgpu::BindGroup,
    mesh_vb: Option<wgpu::Buffer>,
    mesh_count: u32,
    /// Board and parts extent: (min, max).
    pub bounds: ([f32; 3], [f32; 3]),
    /// The load problems of each instance (refdes, message).
    pub problems: Vec<(String, String)>,
}

/// An orbit camera around a target point. Z is up.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub target: [f32; 3],
    /// Degrees around Z.
    pub yaw: f32,
    /// Degrees above the board plane.
    pub pitch: f32,
    pub distance: f32,
    pub fov: f32,
}

impl Camera {
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        Vec3::from(self.target) + Vec3::new(cp * cy, cp * sy, sp) * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let eye = self.eye();
        let up = if self.pitch.abs() > 89.0 {
            Vec3::new(
                -self.yaw.to_radians().cos(),
                -self.yaw.to_radians().sin(),
                0.0,
            )
        } else {
            Vec3::Z
        };
        let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::from(self.target), up);
        let near = (self.distance * 0.01).max(0.05);
        let far = self.distance * 20.0 + 500.0;
        glam::camera::rh::proj::directx::perspective(
            self.fov.to_radians(),
            aspect.max(1e-3),
            near,
            far,
        ) * view
    }

    /// The point on the plane at height `z` under a point of the view. `ndc`
    /// is the point in -1..1 on both axes, with y up. `None` if the ray of the
    /// point does not meet the plane in front of the camera.
    pub fn hit_plane(&self, aspect: f32, ndc: [f32; 2], z: f32) -> Option<[f32; 3]> {
        let inv = self.view_proj(aspect).inverse();
        let a = inv.project_point3(Vec3::new(ndc[0], ndc[1], 0.0));
        let b = inv.project_point3(Vec3::new(ndc[0], ndc[1], 1.0));
        let d = b - a;
        if d.z.abs() < 1e-9 {
            return None;
        }
        let t = (z - a.z) / d.z;
        (t > 0.0).then(|| (a + d * t).into())
    }

    /// Move the camera nearer (`factor` below 1) or farther, so that the point
    /// `at` stays at the same place in the view.
    pub fn zoom_about(&mut self, at: [f32; 3], factor: f32) {
        let dist = (self.distance * factor).clamp(2.0, 5000.0);
        let k = dist / self.distance;
        for (t, a) in self.target.iter_mut().zip(at) {
            *t = a + (*t - a) * k;
        }
        self.distance = dist;
    }

    /// Look straight down at the top of the board, or straight up at its
    /// bottom, with +Y up in the view. From the bottom, +X points to the left,
    /// as when you turn the board over. The target and the distance stay.
    /// Both pitches are inside the orbit range of the viewer (-89 to 89.9),
    /// so that the first orbit drag does not turn the view over.
    pub fn look_straight(&mut self, from_top: bool) {
        if from_top {
            self.yaw = -90.0;
            self.pitch = 89.9;
        } else {
            self.yaw = 90.0;
            self.pitch = -89.0;
        }
    }

    /// Fit a box in a square view, seen from a three-quarter angle.
    pub fn frame(lo: [f32; 3], hi: [f32; 3]) -> Camera {
        Camera::frame_aspect(lo, hi, 1.0)
    }

    /// Fit a box in a view with `aspect` = width / height. In a tall view, the
    /// narrower horizontal field is the limit.
    pub fn frame_aspect(lo: [f32; 3], hi: [f32; 3], aspect: f32) -> Camera {
        let c = [
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ];
        let r = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2))
            .sqrt()
            / 2.0;
        let fov = 30.0f32;
        let half_v = fov.to_radians() / 2.0;
        let half_h = (half_v.tan() * aspect.max(0.05)).atan();
        Camera {
            target: c,
            yaw: -62.0,
            pitch: 38.0,
            distance: (r / half_v.min(half_h).sin()).max(5.0) * 1.05,
            fov,
        }
    }
}

#[cfg(test)]
mod camera_tests {
    use super::*;

    #[test]
    fn zoom_about_keeps_the_point_in_place() {
        let mut cam = Camera::frame([0.0, 0.0, 0.0], [40.0, 30.0, 1.6]);
        let aspect = 1.5;
        let at = cam.hit_plane(aspect, [0.4, -0.3], 1.6).unwrap();
        let before = cam.view_proj(aspect).project_point3(Vec3::from(at));
        cam.zoom_about(at, 0.5);
        let after = cam.view_proj(aspect).project_point3(Vec3::from(at));
        assert!((before.x - after.x).abs() < 1e-4 && (before.y - after.y).abs() < 1e-4);
        assert!((before.x - 0.4).abs() < 1e-3 && (before.y + 0.3).abs() < 1e-3);
    }

    #[test]
    fn the_bottom_view_has_y_up_and_x_to_the_left() {
        let mut cam = Camera::frame([0.0, 0.0, 0.0], [40.0, 30.0, 1.6]);
        cam.look_straight(false);
        let vp = cam.view_proj(1.0);
        let at = |x: f32, y: f32| vp.project_point3(Vec3::new(x, y, 0.0));
        assert!(at(20.0, 25.0).y > at(20.0, 5.0).y, "+Y is up");
        assert!(at(35.0, 15.0).x < at(5.0, 15.0).x, "+X is to the left");
        // A small orbit does not turn the view over.
        cam.pitch = (cam.pitch + 0.3).clamp(-89.0, 89.9);
        let vp = cam.view_proj(1.0);
        let at = |x: f32, y: f32| vp.project_point3(Vec3::new(x, y, 0.0));
        assert!(at(20.0, 25.0).y > at(20.0, 5.0).y);
    }

    #[test]
    fn a_straight_view_has_y_up() {
        let mut cam = Camera::frame([0.0, 0.0, 0.0], [40.0, 30.0, 1.6]);
        cam.look_straight(true);
        let vp = cam.view_proj(1.0);
        let up = vp.project_point3(Vec3::new(20.0, 25.0, 1.6));
        let down = vp.project_point3(Vec3::new(20.0, 5.0, 1.6));
        assert!(up.y > down.y);
    }
}

/// Mesh files, loaded one time for each path.
#[derive(Default)]
pub struct MeshCache {
    meshes: HashMap<PathBuf, Result<Arc<Mesh>, String>>,
}

impl MeshCache {
    pub fn get(&mut self, p: &std::path::Path) -> Result<Arc<Mesh>, String> {
        self.meshes
            .entry(p.to_path_buf())
            .or_insert_with(|| lamina_step::load(p).map(Arc::new))
            .clone()
    }

    pub fn forget(&mut self, p: &std::path::Path) {
        self.meshes.remove(p);
    }
}

impl Gpu {
    /// A device without a window, for snapshots.
    pub fn headless() -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        }))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: true,
                compatible_surface: None,
                ..Default::default()
            }))
        })
        .map_err(|e| format!("no GPU adapter was found: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lamina"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("cannot get a GPU device: {e}"))?;
        Ok(Gpu::new(device, queue))
    }

    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lamina shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ubgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let ubg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms"),
            layout: &ubgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ubuf.as_entire_binding(),
            }],
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("board textures"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("board"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let layout_u = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("u"),
            bind_group_layouts: &[Some(&ubgl)],
            immediate_size: 0,
        });
        let layout_board = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("board"),
            bind_group_layouts: &[Some(&ubgl), Some(&tex_bgl)],
            immediate_size: 0,
        });
        let depth = |write: bool, cmp: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(cmp),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        let ms = wgpu::MultisampleState {
            count: MSAA,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let target = |blend: Option<wgpu::BlendState>| {
            [Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })]
        };
        let opaque = target(None);
        let blended = target(Some(wgpu::BlendState::ALPHA_BLENDING));

        let pipe = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    depth_state: Option<wgpu::DepthStencilState>,
                    targets: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: depth_state,
                multisample: ms,
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipe_bg = pipe(
            "bg",
            &layout_u,
            "vs_bg",
            "fs_bg",
            &[],
            depth(false, wgpu::CompareFunction::Always),
            &opaque,
        );
        let v2_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32];
        let pipe_2d = pipe(
            "2d",
            &layout_u,
            "vs_2d",
            "fs_2d",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<V2>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &v2_attrs,
            })],
            depth(true, wgpu::CompareFunction::Less),
            &blended,
        );
        let vm_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
        let pipe_mesh = pipe(
            "mesh",
            &layout_u,
            "vs_mesh",
            "fs_mesh",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<VM>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vm_attrs,
            })],
            depth(true, wgpu::CompareFunction::Less),
            &opaque,
        );
        let vb_attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32];
        let pipe_board = pipe(
            "board",
            &layout_board,
            "vs_board",
            "fs_board",
            &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<BoardVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vb_attrs,
            })],
            depth(true, wgpu::CompareFunction::Less),
            &opaque,
        );
        Gpu {
            device,
            queue,
            ubuf,
            ubg,
            tex_bgl,
            sampler,
            pipe_bg,
            pipe_2d,
            pipe_mesh,
            pipe_board,
        }
    }

    pub fn target(&self, width: u32, height: u32) -> Target {
        let (w, h) = (width.max(1), height.max(1));
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let mk = |label, format, samples, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = mk(
            "msaa",
            COLOR_FORMAT,
            MSAA,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth = mk(
            "depth",
            DEPTH_FORMAT,
            MSAA,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let resolve = mk(
            "resolve",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        Target {
            width: w,
            height: h,
            msaa: msaa.create_view(&Default::default()),
            depth: depth.create_view(&Default::default()),
            resolve_view: resolve.create_view(&Default::default()),
            resolve,
        }
    }

    /// Upload 2D layers. Each layer gets its own depth, so that overlaps in a
    /// layer blend one time.
    pub fn upload_2d(&self, s: &Scene2D, px_per_mm: f64, active_copper: u8) -> Gpu2D {
        /// Pour fills are drawn at this share of the layer colour.
        const POUR_ALPHA: f32 = lamina_scene::theme::POUR_ALPHA;
        let mut verts: Vec<V2> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let mut ranges = Vec::new();
        let n = s.layers.len().max(1) as f32;
        for (li, l) in s.layers.iter().enumerate() {
            let base_color = rgba_lin(theme::layer_color(l.layer, s.copper_layers));
            // Copper that is not on the active layer is dimmer.
            let layer_k = match l.layer {
                SceneLayer::Copper(i) if i != active_copper => 0.38,
                _ => 1.0,
            };
            let depth = 0.9 - 0.8 * (li as f32 / n);
            let start = idx.len() as u32;
            // Pours have their own depth slot, just below the pads and tracks
            // of the layer. They are also lighter.
            for (part, items, k) in [
                (0, &l.items[..l.pour], POUR_ALPHA),
                (1, &l.items[l.pour..], 1.0),
            ] {
                let mut tris = Tris::default();
                for g in items {
                    tris.add(g, px_per_mm);
                }
                let mut color = base_color;
                color[3] *= k * layer_k;
                let d = depth + if part == 0 { 0.4 / n } else { 0.0 };
                let base = verts.len() as u32;
                verts.extend(tris.pos.iter().map(|p| V2 {
                    pos: *p,
                    color,
                    depth: d,
                }));
                idx.extend(tris.idx.iter().map(|i| base + i));
            }
            ranges.push((l.layer, start, idx.len() as u32 - start));
        }
        if verts.is_empty() {
            verts.push(V2 {
                pos: [0.0, 0.0],
                color: [0.0; 4],
                depth: 1.0,
            });
            idx.extend_from_slice(&[0, 0, 0]);
        }
        Gpu2D {
            vb: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("2d vb"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            ib: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("2d ib"),
                    contents: bytemuck::cast_slice(&idx),
                    usage: wgpu::BufferUsages::INDEX,
                }),
            ranges,
        }
    }

    fn texture(&self, pm: &tiny_skia::Pixmap, label: &str) -> wgpu::TextureView {
        let size = wgpu::Extent3d {
            width: pm.width(),
            height: pm.height(),
            depth_or_array_layers: 1,
        };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // tiny-skia keeps premultiplied alpha. The board is opaque in its outline.
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pm.data(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * pm.width()),
                rows_per_image: Some(pm.height()),
            },
            size,
        );
        tex.create_view(&Default::default())
    }

    /// Upload the board slab with its surface textures, and all part meshes.
    pub fn upload_3d(&self, s3: &Scene3D, s2: &Scene2D, cache: &mut MeshCache) -> Gpu3D {
        let bb = s3.bbox;
        let (vb, ib) = board_slab(&s3.outline, &s3.holes, s3.thickness, bb);
        let max_dim = bb.width().max(bb.height()).max(1.0);
        let ppm = (24.0f64).min(4096.0 / max_dim);
        let tw = (bb.width() * ppm).ceil().max(1.0) as u32;
        let th = (bb.height() * ppm).ceil().max(1.0) as u32;
        let view = View::exact(bb, tw, th);
        let top = appearance(s2, Side::Top, view, None)
            .unwrap_or_else(|| tiny_skia::Pixmap::new(1, 1).unwrap());
        let bot = appearance(s2, Side::Bottom, view, None)
            .unwrap_or_else(|| tiny_skia::Pixmap::new(1, 1).unwrap());
        let tv = self.texture(&top, "board top");
        let bv = self.texture(&bot, "board bottom");
        let board_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("board"),
            layout: &self.tex_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&tv),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&bv),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut verts: Vec<VM> = Vec::new();
        let mut problems = Vec::new();
        let mut lo = [bb.min.x as f32, bb.min.y as f32, 0.0f32];
        let mut hi = [bb.max.x as f32, bb.max.y as f32, s3.thickness as f32];
        for inst in &s3.instances {
            let m = Mat4::from_cols_array(&inst.transform);
            let nm = Mat3::from_mat4(m).inverse().transpose();
            let mut push = |p: [f32; 3], n: [f32; 3], c: [f32; 3]| {
                let w = m.transform_point3(Vec3::from(p));
                let nn = (nm * Vec3::from(n)).normalize_or_zero();
                for k in 0..3 {
                    lo[k] = lo[k].min(w[k]);
                    hi[k] = hi[k].max(w[k]);
                }
                verts.push(VM {
                    pos: w.into(),
                    normal: nn.into(),
                    color: c,
                });
            };
            let mesh = match &inst.model {
                Some(p) => match cache.get(p) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        problems.push((inst.refdes.clone(), e));
                        None
                    }
                },
                None => None,
            };
            match mesh {
                Some(mesh) => {
                    for part in &mesh.parts {
                        for (p, n) in part.positions.iter().zip(&part.normals) {
                            push(*p, *n, part.color);
                        }
                    }
                }
                None => {
                    if let Some((a, b)) = inst.placeholder {
                        for (p, n) in box_tris(a, b) {
                            push(p, n, [0.08, 0.08, 0.09]);
                        }
                    }
                }
            }
        }
        let mesh_count = verts.len() as u32;
        let mesh_vb = (!verts.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("meshes"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let (board_vb, board_ib, board_count) = if vb.is_empty() {
            let v = [BoardVertex {
                pos: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
                uv: [0.0; 2],
                face: 0.0,
            }];
            (
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("board vb"),
                        contents: bytemuck::cast_slice(&v),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("board ib"),
                        contents: bytemuck::cast_slice(&[0u32, 0, 0]),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                0,
            )
        } else {
            (
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("board vb"),
                        contents: bytemuck::cast_slice(&vb),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("board ib"),
                        contents: bytemuck::cast_slice(&ib),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                ib.len() as u32,
            )
        };
        Gpu3D {
            board_vb,
            board_ib,
            board_count,
            board_bg,
            mesh_vb,
            mesh_count,
            bounds: (lo, hi),
            problems,
        }
    }

    fn write_uniforms(&self, u: &Uniforms) {
        self.queue
            .write_buffer(&self.ubuf, 0, bytemuck::bytes_of(u));
    }

    fn base_uniforms() -> Uniforms {
        Uniforms {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            eye: [0.0; 4],
            view2d: [1.0, 1.0, 0.0, 0.0],
            params: [0.0; 4],
            bg_top: [0.050, 0.058, 0.075, 1.0],
            bg_bot: [0.010, 0.011, 0.014, 1.0],
        }
    }

    /// Draw 2D layers. `center` and `px_per_mm` set the view. `visible`
    /// filters the layers.
    pub fn draw_2d(
        &self,
        t: &Target,
        s: &Gpu2D,
        center: [f64; 2],
        px_per_mm: f64,
        visible: &dyn Fn(SceneLayer) -> bool,
    ) {
        let mut u = Self::base_uniforms();
        let sx = (2.0 * px_per_mm / t.width as f64) as f32;
        let sy = (2.0 * px_per_mm / t.height as f64) as f32;
        u.view2d = [sx, sy, -(center[0] as f32) * sx, -(center[1] as f32) * sy];
        self.write_uniforms(&u);
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut rp = self.begin(&mut enc, t, [0.035, 0.040, 0.050]);
            rp.set_bind_group(0, &self.ubg, &[]);
            rp.set_pipeline(&self.pipe_bg);
            rp.draw(0..3, 0..1);
            rp.set_pipeline(&self.pipe_2d);
            rp.set_vertex_buffer(0, s.vb.slice(..));
            rp.set_index_buffer(s.ib.slice(..), wgpu::IndexFormat::Uint32);
            for (layer, start, count) in &s.ranges {
                if *count > 0 && visible(*layer) {
                    rp.draw_indexed(*start..start + count, 0, 0..1);
                }
            }
        }
        self.queue.submit([enc.finish()]);
    }

    pub fn draw_3d(&self, t: &Target, s: &Gpu3D, cam: &Camera) {
        self.draw_3d_with(t, s, cam, true);
    }

    /// Draw the 3D view. With `models` false, only the board shows, without
    /// the part bodies.
    pub fn draw_3d_with(&self, t: &Target, s: &Gpu3D, cam: &Camera, models: bool) {
        let mut u = Self::base_uniforms();
        u.view_proj = cam
            .view_proj(t.width as f32 / t.height as f32)
            .to_cols_array_2d();
        let e = cam.eye();
        u.eye = [e.x, e.y, e.z, 1.0];
        // Put the light on the side of the camera, so that the bottom is not in shadow.
        u.params[0] = if e.z < cam.target[2] { -1.0 } else { 1.0 };
        u.bg_top = [0.085, 0.095, 0.120, 1.0];
        u.bg_bot = [0.012, 0.013, 0.017, 1.0];
        self.write_uniforms(&u);
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut rp = self.begin(&mut enc, t, [0.0, 0.0, 0.0]);
            rp.set_bind_group(0, &self.ubg, &[]);
            rp.set_pipeline(&self.pipe_bg);
            rp.draw(0..3, 0..1);
            if s.board_count > 0 {
                rp.set_pipeline(&self.pipe_board);
                rp.set_bind_group(1, &s.board_bg, &[]);
                rp.set_vertex_buffer(0, s.board_vb.slice(..));
                rp.set_index_buffer(s.board_ib.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..s.board_count, 0, 0..1);
            }
            if let (true, Some(vb)) = (models, &s.mesh_vb) {
                rp.set_pipeline(&self.pipe_mesh);
                rp.set_vertex_buffer(0, vb.slice(..));
                rp.draw(0..s.mesh_count, 0..1);
            }
        }
        self.queue.submit([enc.finish()]);
    }

    fn begin<'e>(
        &self,
        enc: &'e mut wgpu::CommandEncoder,
        t: &Target,
        clear: [f64; 3],
    ) -> wgpu::RenderPass<'e> {
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lamina"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &t.msaa,
                depth_slice: None,
                resolve_target: Some(&t.resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear[0],
                        g: clear[1],
                        b: clear[2],
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &t.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Copy the resolved image to the CPU as RGBA8.
    pub fn read_rgba(&self, t: &Target) -> Result<Vec<u8>, String> {
        let row = 4 * t.width;
        let padded = row.div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * t.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(t.height),
                },
            },
            wgpu::Extent3d {
                width: t.width,
                height: t.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("the GPU poll failed: {e}"))?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("the GPU readback failed: {e}"))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| format!("the GPU readback failed: {e}"))?;
        let mut out = Vec::with_capacity((row * t.height) as usize);
        for y in 0..t.height {
            let s = (y * padded) as usize;
            out.extend_from_slice(&data[s..s + row as usize]);
        }
        drop(data);
        buf.unmap();
        Ok(out)
    }
}

fn box_tris(a: [f32; 3], b: [f32; 3]) -> Vec<([f32; 3], [f32; 3])> {
    let c = |x: bool, y: bool, z: bool| {
        [
            if x { b[0] } else { a[0] },
            if y { b[1] } else { a[1] },
            if z { b[2] } else { a[2] },
        ]
    };
    type Corner = [bool; 3];
    let faces: [(Corner, Corner, Corner, Corner, [f32; 3]); 6] = [
        (
            [false, false, true],
            [true, false, true],
            [true, true, true],
            [false, true, true],
            [0.0, 0.0, 1.0],
        ),
        (
            [false, false, false],
            [false, true, false],
            [true, true, false],
            [true, false, false],
            [0.0, 0.0, -1.0],
        ),
        (
            [false, false, false],
            [true, false, false],
            [true, false, true],
            [false, false, true],
            [0.0, -1.0, 0.0],
        ),
        (
            [false, true, false],
            [false, true, true],
            [true, true, true],
            [true, true, false],
            [0.0, 1.0, 0.0],
        ),
        (
            [false, false, false],
            [false, false, true],
            [false, true, true],
            [false, true, false],
            [-1.0, 0.0, 0.0],
        ),
        (
            [true, false, false],
            [true, true, false],
            [true, true, true],
            [true, false, true],
            [1.0, 0.0, 0.0],
        ),
    ];
    let mut out = Vec::new();
    for (p, q, r, s, n) in faces {
        let v = [
            c(p[0], p[1], p[2]),
            c(q[0], q[1], q[2]),
            c(r[0], r[1], r[2]),
            c(s[0], s[1], s[2]),
        ];
        for i in [0, 1, 2, 0, 2, 3] {
            out.push((v[i], n));
        }
    }
    out
}
