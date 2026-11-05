//! The compositor on the graphics card (wgpu). It draws the same `Scene` as
//! `compositor::compose`, with shaders that port its CPU code step by step, so exports render
//! many times faster and still match the preview. Titles are added on the CPU afterwards, as
//! there. Callers fall back to the CPU when there is no suitable card or anything fails.

use crate::compositor::{ComposeInput, Frame, Scene};
use crate::model3d::PreparedModel;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use wgpu::util::DeviceExt;

/// Sizes of the uniform blocks (vec4s), as in compose.wgsl and model.wgsl.
const SCENE_VEC4S: usize = 41;
const MODEL_VEC4S: usize = 19;

/// Touch indicators the shader takes per frame (more are dropped).
const MAX_INDICATORS: usize = 64;

type Vec4 = [f32; 4];

fn bytes(v: &[Vec4]) -> &[u8] {
    bytemuck::cast_slice(v)
}

struct Target {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

fn target(device: &wgpu::Device, label: &str, w: u32, h: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> Target {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    Target { tex, view }
}

fn upload_rgba(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, w: usize, h: usize, data: &[u8]) -> Target {
    let t = target(device, label, w as u32, h as u32, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    queue.write_texture(
        wgpu::ImageCopyTexture { texture: &t.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        data,
        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w as u32 * 4), rows_per_image: Some(h as u32) },
        wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
    );
    t
}

fn uniform(binding: u32, stages: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: stages,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn texture(binding: u32, sample_type: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}

fn sampler(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

const FLOAT: wgpu::TextureSampleType = wgpu::TextureSampleType::Float { filterable: true };
const UNFILTERED: wgpu::TextureSampleType = wgpu::TextureSampleType::Float { filterable: false };

fn fullscreen_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::BindGroupLayout,
    fragment: &str,
    formats: &[wgpu::TextureFormat],
) -> wgpu::RenderPipeline {
    let targets: Vec<Option<wgpu::ColorTargetState>> = formats.iter().map(|&format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })).collect();
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[layout], push_constant_ranges: &[] });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(&pl),
        vertex: wgpu::VertexState { module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview: None,
        cache: None,
    })
}

fn pass<'a>(enc: &'a mut wgpu::CommandEncoder, view: &'a wgpu::TextureView, depth: Option<&'a wgpu::TextureView>) -> wgpu::RenderPass<'a> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: depth.map(|view| wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

/// The imported model's buffers, materials and pipelines.
struct GpuModel {
    pos: wgpu::Buffer,
    nrm: wgpu::Buffer,
    uv: wgpu::Buffer,
    suv: wgpu::Buffer,
    opaque: wgpu::Buffer,
    /// Index ranges of `opaque` per material key.
    opaque_runs: Vec<(usize, Range<u32>)>,
    /// See-through triangles and their material key, sorted back to front every frame.
    blend_tris: Vec<(usize, usize)>,
    blend: wgpu::Buffer,
    /// One bind group per material key (the materials, then the screen).
    materials: Vec<wgpu::BindGroup>,
    frame: wgpu::Buffer,
    frame_layout: wgpu::BindGroupLayout,
    frame_group: wgpu::BindGroup,
    opaque_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    _textures: Vec<Target>,
    _material_buf: wgpu::Buffer,
}

pub struct GpuCompositor {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    error: Arc<Mutex<Option<String>>>,
    /// For people: "AMD Radeon RX 9070 XT (Vulkan)".
    pub name: String,
    ow: usize,
    oh: usize,
    ss: usize,
    scene: wgpu::Buffer,
    inds: wgpu::Buffer,
    /// The recording at two mip levels, and the previous clip's last frame (dissolves), with
    /// what each one holds.
    videos: [Target; 3],
    video_keys: [(usize, usize, usize); 3],
    video_sampler: wgpu::Sampler,
    model_layer: Target,
    depth: Option<Target>,
    sub: Target,
    out: Target,
    acc: [Target; 2],
    compose_layout: wgpu::BindGroupLayout,
    compose_group: wgpu::BindGroup,
    compose_pipeline: wgpu::RenderPipeline,
    blur_n: wgpu::Buffer,
    blur_groups: [wgpu::BindGroup; 2],
    accumulate_pipeline: wgpu::RenderPipeline,
    resolve_pipeline: wgpu::RenderPipeline,
    model: Option<GpuModel>,
    readback: wgpu::Buffer,
    padded_row: usize,
    /// Titles drawn on the CPU, put over the frame before converting it to YUV.
    overlay: Target,
    finish: wgpu::Buffer,
    finish_group: wgpu::BindGroup,
    luma_pipeline: wgpu::RenderPipeline,
    chroma_pipeline: wgpu::RenderPipeline,
    /// Y, U and V planes, and their padded rows when read back.
    planes: [Target; 3],
    plane_rows: [usize; 2],
    yuv_readback: wgpu::Buffer,
    /// The finished frame for the window (the preview): written as RGBA, read as sRGB.
    display: Option<(Target, wgpu::TextureView)>,
    display_pipeline: wgpu::RenderPipeline,
}

impl GpuCompositor {
    /// Sets up the card for `ow`×`oh` frames with `model` (if the style uses one) and the
    /// model's supersampling `ss`. Fails when there is no hardware adapter.
    pub fn new(ow: usize, oh: usize, ss: usize, model: Option<&PreparedModel>) -> Result<Self, String> {
        let adapter = adapter()?;
        let info = adapter.get_info();
        let max_dim = adapter.limits().max_texture_dimension_2d as usize;
        // As many samples as the card's largest texture allows (4K at 4×4 does not fit).
        let ss = ss.clamp(1, 4).min(max_dim / ow.max(oh).max(1));
        if ss == 0 {
            return Err("El vídeo es demasiado grande para la tarjeta gráfica".into());
        }
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("rec-ording"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits { max_texture_dimension_2d: max_dim as u32, ..wgpu::Limits::default() },
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| format!("No se pudo usar la tarjeta gráfica: {e}"))?;
        Self::build(Arc::new(device), Arc::new(queue), card_name(&info), ow, oh, ss, model)
    }

    /// Like `new`, on a device that already exists: the window's, for the preview.
    pub fn on_device(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, name: String, ow: usize, oh: usize, ss: usize, model: Option<&PreparedModel>) -> Result<Self, String> {
        let max_dim = device.limits().max_texture_dimension_2d as usize;
        let ss = ss.clamp(1, 4).min(max_dim / ow.max(oh).max(1));
        if ss == 0 {
            return Err("La vista previa es demasiado grande para la tarjeta gráfica".into());
        }
        Self::build(device, queue, name, ow, oh, ss, model)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, name: String, ow: usize, oh: usize, ss: usize, model: Option<&PreparedModel>) -> Result<Self, String> {
        let error = Arc::new(Mutex::new(None));
        let sink = error.clone();
        device.on_uncaptured_error(Box::new(move |e| {
            sink.lock().unwrap().get_or_insert_with(|| e.to_string());
        }));

        let (w, h) = (ow as u32, oh as u32);
        use wgpu::TextureUsages as U;
        let render = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING;

        let scene = device.create_buffer(&wgpu::BufferDescriptor { label: Some("scene"), size: (SCENE_VEC4S * 16) as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let inds = device.create_buffer(&wgpu::BufferDescriptor { label: Some("touches"), size: (MAX_INDICATORS * 48) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let videos = [0, 1, 2].map(|_| upload_rgba(&device, &queue, "video", 1, 1, &[18, 18, 20, 255]));
        let video_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let (mw, mh) = if model.is_some() { (w * ss as u32, h * ss as u32) } else { (1, 1) };
        let model_layer = target(&device, "model layer", mw, mh, wgpu::TextureFormat::Rgba16Float, render);
        let depth = model.map(|_| target(&device, "depth", mw, mh, wgpu::TextureFormat::Depth32Float, U::RENDER_ATTACHMENT));
        let sub = target(&device, "subframe", w, h, wgpu::TextureFormat::Rgba8Unorm, render);
        let out = target(&device, "output", w, h, wgpu::TextureFormat::Rgba8Unorm, render | U::COPY_SRC);
        let acc = [
            target(&device, "blur a", w, h, wgpu::TextureFormat::Rgba32Uint, render),
            target(&device, "blur b", w, h, wgpu::TextureFormat::Rgba32Uint, render),
        ];

        let compose_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("compose"), source: wgpu::ShaderSource::Wgsl(include_str!("compose.wgsl").into()) });
        let compose_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compose"),
            entries: &[
                uniform(0, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                texture(2, FLOAT),
                sampler(3),
                texture(4, UNFILTERED),
                texture(5, FLOAT),
                texture(6, FLOAT),
            ],
        });
        let compose_pipeline = fullscreen_pipeline(&device, &compose_module, &compose_layout, "fs", &[wgpu::TextureFormat::Rgba8Unorm]);

        let blur_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blur"), source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()) });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur"),
            entries: &[texture(0, UNFILTERED), texture(1, wgpu::TextureSampleType::Uint), uniform(2, wgpu::ShaderStages::FRAGMENT)],
        });
        let blur_n = device.create_buffer(&wgpu::BufferDescriptor { label: Some("blur n"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        // Group `i` reads sums from acc[i].
        let blur_group = |i: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &blur_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sub.view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&acc[i].view) },
                    wgpu::BindGroupEntry { binding: 2, resource: blur_n.as_entire_binding() },
                ],
            })
        };
        let blur_groups = [blur_group(0), blur_group(1)];
        let accumulate_pipeline = fullscreen_pipeline(&device, &blur_module, &blur_layout, "accumulate", &[wgpu::TextureFormat::Rgba32Uint]);
        let resolve_pipeline = fullscreen_pipeline(&device, &blur_module, &blur_layout, "resolve", &[wgpu::TextureFormat::Rgba8Unorm]);

        let model = model.map(|m| GpuModel::new(&device, &queue, m, &videos, &video_sampler));
        let compose_group = Self::compose_group(&device, &compose_layout, &scene, &inds, &videos, &video_sampler, &model_layer);

        let padded_row = (ow * 4).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded_row * oh) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // YUV for the encoder.
        let overlay = target(&device, "titles", w, h, wgpu::TextureFormat::Rgba8Unorm, U::TEXTURE_BINDING | U::COPY_DST);
        let finish_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("finish"), source: wgpu::ShaderSource::Wgsl(include_str!("finish.wgsl").into()) });
        let finish_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("finish"),
            entries: &[texture(0, UNFILTERED), texture(1, UNFILTERED), uniform(2, wgpu::ShaderStages::FRAGMENT)],
        });
        let finish = device.create_buffer(&wgpu::BufferDescriptor { label: Some("finish"), size: 48, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let finish_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("finish"),
            layout: &finish_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&out.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&overlay.view) },
                wgpu::BindGroupEntry { binding: 2, resource: finish.as_entire_binding() },
            ],
        });
        let r8 = wgpu::TextureFormat::R8Unorm;
        let luma_pipeline = fullscreen_pipeline(&device, &finish_module, &finish_layout, "luma", &[r8]);
        let chroma_pipeline = fullscreen_pipeline(&device, &finish_module, &finish_layout, "chroma", &[r8, r8]);
        let display_pipeline = fullscreen_pipeline(&device, &finish_module, &finish_layout, "rgba", &[wgpu::TextureFormat::Rgba8Unorm]);
        let planes = [
            target(&device, "y", w, h, r8, U::RENDER_ATTACHMENT | U::COPY_SRC),
            target(&device, "u", w / 2, h / 2, r8, U::RENDER_ATTACHMENT | U::COPY_SRC),
            target(&device, "v", w / 2, h / 2, r8, U::RENDER_ATTACHMENT | U::COPY_SRC),
        ];
        let plane_rows = [ow.div_ceil(256) * 256, (ow / 2).div_ceil(256) * 256];
        let yuv_readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv readback"),
            size: (plane_rows[0] * oh + 2 * plane_rows[1] * (oh / 2)) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let gpu = GpuCompositor {
            device,
            queue,
            error,
            name,
            ow,
            oh,
            ss,
            scene,
            inds,
            videos,
            video_keys: [(0, 1, 1); 3],
            video_sampler,
            model_layer,
            depth,
            sub,
            out,
            acc,
            compose_layout,
            compose_group,
            compose_pipeline,
            blur_n,
            blur_groups,
            accumulate_pipeline,
            resolve_pipeline,
            model,
            readback,
            padded_row,
            overlay,
            finish,
            finish_group,
            luma_pipeline,
            chroma_pipeline,
            planes,
            plane_rows,
            yuv_readback,
            display: None,
            display_pipeline,
        };
        gpu.check()?;
        Ok(gpu)
    }

    fn compose_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        scene: &wgpu::Buffer,
        inds: &wgpu::Buffer,
        videos: &[Target; 3],
        video_sampler: &wgpu::Sampler,
        model_layer: &Target,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compose"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: scene.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: inds.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&videos[0].view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(video_sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&model_layer.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&videos[1].view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&videos[2].view) },
            ],
        })
    }

    fn check(&self) -> Result<(), String> {
        match self.error.lock().unwrap().take() {
            Some(e) => Err(format!("Error de la tarjeta gráfica: {e}")),
            None => Ok(()),
        }
    }

    /// Puts the recording's frames the scene uses on the card (both mip levels, and the previous
    /// clip's during a dissolve), unless they are already there.
    fn upload_video(&mut self, frames: [Option<&Frame>; 3]) {
        let mut recreated = false;
        for (k, frame) in frames.into_iter().enumerate() {
            let Some(f) = frame else { continue };
            let key = (f.data.as_ptr() as usize, f.w, f.h);
            if key == self.video_keys[k] {
                continue;
            }
            if (f.w, f.h) != (self.video_keys[k].1, self.video_keys[k].2) {
                self.videos[k] = target(&self.device, "video", f.w as u32, f.h as u32, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
                recreated = true;
            }
            self.queue.write_texture(
                wgpu::ImageCopyTexture { texture: &self.videos[k].tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &f.data,
                wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(f.w as u32 * 4), rows_per_image: Some(f.h as u32) },
                wgpu::Extent3d { width: f.w as u32, height: f.h as u32, depth_or_array_layers: 1 },
            );
            self.video_keys[k] = key;
        }
        if recreated {
            self.compose_group = Self::compose_group(&self.device, &self.compose_layout, &self.scene, &self.inds, &self.videos, &self.video_sampler, &self.model_layer);
            if let Some(m) = &mut self.model {
                m.frame_group = GpuModel::frame_group(&self.device, &m.frame_layout, &m.frame, &self.videos, &self.video_sampler);
            }
        }
    }

    /// Records the passes that draw `sc` into `view`.
    fn draw(&mut self, enc: &mut wgpu::CommandEncoder, sc: &Scene, view_out: bool) {
        // The second level only matters when blending; the previous clip only during a dissolve.
        self.upload_video([sc.mip, sc.mip2.filter(|_| sc.mip_mix > 0.0), sc.from.map(|f| f.0)]);
        let has_mesh = self.model.is_some() && sc.model.is_some();
        if has_mesh {
            let model = self.model.as_ref().unwrap();
            model.write_frame(&self.queue, sc);
            model.write_pose(&self.queue, sc);
            let blend_runs = model.sort_blend(&self.queue, sc);
            let mut p = pass(enc, &self.model_layer.view, self.depth.as_ref().map(|d| &d.view));
            p.set_bind_group(0, &model.frame_group, &[]);
            for (i, b) in [&model.pos, &model.nrm, &model.uv, &model.suv].into_iter().enumerate() {
                p.set_vertex_buffer(i as u32, b.slice(..));
            }
            p.set_pipeline(&model.opaque_pipeline);
            p.set_index_buffer(model.opaque.slice(..), wgpu::IndexFormat::Uint32);
            for (key, range) in &model.opaque_runs {
                p.set_bind_group(1, &model.materials[*key], &[]);
                p.draw_indexed(range.clone(), 0, 0..1);
            }
            if !blend_runs.is_empty() {
                p.set_pipeline(&model.blend_pipeline);
                p.set_index_buffer(model.blend.slice(..), wgpu::IndexFormat::Uint32);
                for (key, range) in &blend_runs {
                    p.set_bind_group(1, &model.materials[*key], &[]);
                    p.draw_indexed(range.clone(), 0, 0..1);
                }
            }
        }
        self.queue.write_buffer(&self.scene, 0, bytes(&scene_uniform(sc, self.ss, has_mesh)));
        let inds = indicator_data(sc);
        if !inds.is_empty() {
            self.queue.write_buffer(&self.inds, 0, bytes(&inds));
        }
        let view = if view_out { &self.out.view } else { &self.sub.view };
        let mut p = pass(enc, view, None);
        p.set_pipeline(&self.compose_pipeline);
        p.set_bind_group(0, &self.compose_group, &[]);
        p.draw(0..3, 0..1);
    }

    /// Renders the frame into `out` (RGBA8) like `compositor::compose`: the scene (averaged
    /// over the shutter when it moves fast), then the titles.
    #[cfg(test)]
    pub fn compose(&mut self, out: &mut [u8], inp: &ComposeInput) -> Result<(), String> {
        self.compose_frame(out, inp)?;
        crate::text::draw(out, self.ow, self.oh, inp.style, inp.texts);
        Ok(())
    }

    /// Like `compositor::compose_frame`: everything but the titles.
    pub fn compose_frame(&mut self, out: &mut [u8], inp: &ComposeInput) -> Result<(), String> {
        let (ow, oh) = (self.ow, self.oh);
        if out.len() != ow * oh * 4 {
            return Err("Tamaño de imagen inesperado".into());
        }
        self.render(inp);
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture { texture: &self.out.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyBuffer {
                buffer: &self.readback,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(self.padded_row as u32), rows_per_image: Some(oh as u32) },
            },
            wgpu::Extent3d { width: ow as u32, height: oh as u32, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let row = self.padded_row;
        self.download(&self.readback, |data| unpad(data, row, ow * 4, oh, out))
    }

    /// The finished frame for the encoder: everything plus the titles (`overlay`), as YUV 4:2:0
    /// (BT.709, TV range) in `out` (`ow * oh * 3 / 2` bytes: Y, then U, then V).
    /// `post`: vignette, fades and grain over everything.
    pub fn compose_yuv(&mut self, out: &mut [u8], inp: &ComposeInput, overlay: Option<&crate::text::Overlay>, post: &crate::post::Post) -> Result<(), String> {
        let (ow, oh) = (self.ow, self.oh);
        if out.len() != ow * oh * 3 / 2 {
            return Err("Tamaño de imagen inesperado".into());
        }
        self.render(inp);
        self.write_finish(overlay, post);
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut p = pass(&mut enc, &self.planes[0].view, None);
            p.set_pipeline(&self.luma_pipeline);
            p.set_bind_group(0, &self.finish_group, &[]);
            p.draw(0..3, 0..1);
        }
        {
            let attachment = |view| {
                Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })
            };
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("chroma"),
                color_attachments: &[attachment(&self.planes[1].view), attachment(&self.planes[2].view)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            p.set_pipeline(&self.chroma_pipeline);
            p.set_bind_group(0, &self.finish_group, &[]);
            p.draw(0..3, 0..1);
        }
        let [yr, cr] = self.plane_rows;
        let offsets = [0, yr * oh, yr * oh + cr * (oh / 2)];
        for (k, plane) in self.planes.iter().enumerate() {
            let (w, h, row) = if k == 0 { (ow, oh, yr) } else { (ow / 2, oh / 2, cr) };
            enc.copy_texture_to_buffer(
                wgpu::ImageCopyTexture { texture: &plane.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::ImageCopyBuffer {
                    buffer: &self.yuv_readback,
                    layout: wgpu::ImageDataLayout { offset: offsets[k] as u64, bytes_per_row: Some(row as u32), rows_per_image: Some(h as u32) },
                },
                wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
            );
        }
        self.queue.submit([enc.finish()]);
        let ysize = ow * oh;
        let csize = (ow / 2) * (oh / 2);
        self.download(&self.yuv_readback, |data| {
            let (y, rest) = out.split_at_mut(ysize);
            let (u, v) = rest.split_at_mut(csize);
            unpad(&data[offsets[0]..], yr, ow, oh, y);
            unpad(&data[offsets[1]..], cr, ow / 2, oh / 2, u);
            unpad(&data[offsets[2]..], cr, ow / 2, oh / 2, v);
        })
    }

    /// Puts the titles (`overlay`) and the finishing for finish.wgsl on the card.
    fn write_finish(&mut self, overlay: Option<&crate::text::Overlay>, post: &crate::post::Post) {
        let rect = match overlay {
            Some(o) => {
                self.queue.write_texture(
                    wgpu::ImageCopyTexture { texture: &self.overlay.tex, mip_level: 0, origin: wgpu::Origin3d { x: o.x as u32, y: o.y as u32, z: 0 }, aspect: wgpu::TextureAspect::All },
                    &o.data,
                    wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(o.w as u32 * 4), rows_per_image: Some(o.h as u32) },
                    wgpu::Extent3d { width: o.w as u32, height: o.h as u32, depth_or_array_layers: 1 },
                );
                [o.x as f32, o.y as f32, (o.x + o.w) as f32, (o.y + o.h) as f32]
            }
            None => [0.0; 4],
        };
        let fin = [rect, [post.vignette, post.grain, post.fade, post.frame as f32], [self.ow as f32, self.oh as f32, 0.0, 0.0]];
        self.queue.write_buffer(&self.finish, 0, bytes(&fin));
    }

    /// The finished frame (titles and finishing included) drawn into a texture for the window,
    /// with nothing read back. Returns the view to show; it reads as sRGB, like egui's own
    /// textures, so the colours come out as they are.
    pub fn compose_display(&mut self, inp: &ComposeInput, overlay: Option<&crate::text::Overlay>, post: &crate::post::Post) -> Result<&wgpu::TextureView, String> {
        self.render(inp);
        self.write_finish(overlay, post);
        if self.display.is_none() {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("display"),
                size: wgpu::Extent3d { width: self.ow as u32, height: self.oh as u32, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
            });
            let view = tex.create_view(&Default::default());
            let srgb = tex.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8UnormSrgb), ..Default::default() });
            self.display = Some((Target { tex, view }, srgb));
        }
        let (target, _) = self.display.as_ref().unwrap();
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut p = pass(&mut enc, &target.view, None);
            p.set_pipeline(&self.display_pipeline);
            p.set_bind_group(0, &self.finish_group, &[]);
            p.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
        self.check()?;
        Ok(&self.display.as_ref().unwrap().1)
    }

    /// Draws the frame (all subframes, for motion blur) into `out`.
    fn render(&mut self, inp: &ComposeInput) {
        let (ow, oh) = (self.ow, self.oh);
        let subs: Vec<ComposeInput> = if inp.blur.len() < 2 {
            vec![*inp]
        } else {
            inp.blur.iter().map(|&(time, cam, pose)| ComposeInput { cam, pose, time, since_start: time, ..*inp }).collect()
        };
        let n = subs.len();
        if n == 1 {
            let sc = Scene::new(ow, oh, &subs[0]);
            let mut enc = self.device.create_command_encoder(&Default::default());
            self.draw(&mut enc, &sc, true);
            self.queue.submit([enc.finish()]);
            return;
        }
        self.queue.write_buffer(&self.blur_n, 0, bytemuck::cast_slice(&[n as u32, 0, 0, 0]));
        for (i, sub) in subs.iter().enumerate() {
            let sc = Scene::new(ow, oh, sub);
            let mut enc = self.device.create_command_encoder(&Default::default());
            if i == 0 {
                drop(pass(&mut enc, &self.acc[0].view, None));
            }
            self.draw(&mut enc, &sc, false);
            // Add the subframe: read the sums from acc[i % 2], write them to the other one.
            let (src, dst) = (i % 2, (i + 1) % 2);
            let mut p = pass(&mut enc, &self.acc[dst].view, None);
            p.set_pipeline(&self.accumulate_pipeline);
            p.set_bind_group(0, &self.blur_groups[src], &[]);
            p.draw(0..3, 0..1);
            drop(p);
            self.queue.submit([enc.finish()]);
        }
        let mut enc = self.device.create_command_encoder(&Default::default());
        let mut p = pass(&mut enc, &self.out.view, None);
        p.set_pipeline(&self.resolve_pipeline);
        p.set_bind_group(0, &self.blur_groups[n % 2], &[]);
        p.draw(0..3, 0..1);
        drop(p);
        self.queue.submit([enc.finish()]);
    }

    /// Waits for `buf` (filled by the last submission) and hands its bytes to `f`.
    fn download(&self, buf: &wgpu::Buffer, f: impl FnOnce(&[u8])) -> Result<(), String> {
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv().map_err(|e| e.to_string())?.map_err(|e| format!("No se pudo leer la imagen de la tarjeta: {e}"))?;
        f(&slice.get_mapped_range());
        buf.unmap();
        self.check()
    }
}

/// Copies `rows` rows of `width` bytes, `stride` apart in `src`, into `dst`.
fn unpad(src: &[u8], stride: usize, width: usize, rows: usize, dst: &mut [u8]) {
    if stride == width {
        dst[..width * rows].copy_from_slice(&src[..width * rows]);
    } else {
        for (y, d) in dst.chunks_mut(width).take(rows).enumerate() {
            d.copy_from_slice(&src[y * stride..y * stride + width]);
        }
    }
}

/// The fastest graphics card (not a software renderer).
fn adapter() -> Result<wgpu::Adapter, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::PRIMARY, ..Default::default() });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or("No hay ninguna tarjeta gráfica compatible")?;
    if is_software(&adapter.get_info()) {
        return Err("Solo hay un adaptador por software".into());
    }
    Ok(adapter)
}

/// A software renderer: slower than our own CPU compositor. Windows' WARP ("Microsoft Basic
/// Render Driver") does not always say it is one, so it is also told apart by its vendor.
pub fn is_software(info: &wgpu::AdapterInfo) -> bool {
    info.device_type == wgpu::DeviceType::Cpu || info.vendor == 0x1414
}

/// "AMD Radeon RX 9070 XT (RADV GFX1201)" → "AMD Radeon RX 9070 XT".
fn card_name(info: &wgpu::AdapterInfo) -> String {
    info.name.split(" (").next().unwrap_or(&info.name).trim().to_string()
}

static CARD: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The graphics card that can draw exports, if any (looks the first time).
pub fn detect() -> Option<String> {
    CARD.get_or_init(|| adapter().ok().map(|a| card_name(&a.get_info()))).clone()
}

/// What `detect` found, or `None` while it has not run yet.
pub fn detected() -> Option<Option<String>> {
    CARD.get().cloned()
}

/// Packs the scene for compose.wgsl (same order as its `Scene` struct).
fn scene_uniform(sc: &Scene, ss: usize, has_mesh: bool) -> Vec<Vec4> {
    let l = &sc.layout;
    let r = &sc.rig;
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    let rr = |r: &crate::compositor::RRect| [r.cx, r.cy, r.hx, r.hy];
    let btn = |i: usize| sc.buttons.get(i).map(|r| (rr(r), [r.r, 1.0, 0.0, 0.0])).unwrap_or(([0.0; 4], [0.0; 4]));
    let (btn0, btn0b) = btn(0);
    let (btn1, btn1b) = btn(1);
    let hole = sc.hole.map(|(x, y, r)| [x, y, r, 1.0]).unwrap_or([0.0; 4]);
    let (bar, barb) = sc.camera_bar.as_ref().map(|r| (rr(r), [r.r, 1.0, 0.0, 0.0])).unwrap_or(([0.0; 4], [0.0; 4]));
    let lens = |i: usize| sc.lenses.get(i).map(|&(x, y, r)| [x, y, r, 1.0]).unwrap_or([0.0; 4]);
    let row = |i: usize| [sc.rot[i][0], sc.rot[i][1], sc.rot[i][2], 0.0];
    vec![
        [sc.cw, sc.ch, sc.ccx, sc.ccy],
        [sc.tx, sc.ty, sc.inv_s, sc.dev_scale],
        [sc.lcx, sc.lcy, sc.dx, sc.dy],
        [sc.focal, sc.depth, sc.tilt, sc.turn],
        row(0),
        row(1),
        row(2),
        [sc.eye[0], sc.eye[1], sc.eye[2], 0.0],
        [sc.face_n[0], sc.face_n[1], sc.face_n[2], sc.sheen_amount],
        v3(r.key, sc.rim),
        v3(r.half, sc.sc_short),
        [sc.bg_a[0], sc.bg_a[1], sc.bg_a[2], sc.half_extent],
        [sc.bg_b[0], sc.bg_b[1], sc.bg_b[2], sc.diag],
        [sc.gdx, sc.gdy, sc.body_top, sc.body_h],
        rr(&sc.body),
        rr(&sc.screen),
        [sc.body.r, sc.screen.r, sc.shadow_alpha, sc.shadow_blur],
        [sc.shadow_body.cy, sc.contact.cy, sc.shadow_body.cx, 0.0],
        [l.screen.x0, l.screen.y0, l.screen.w(), l.screen.h()],
        [sc.metal[0], sc.metal[1], sc.metal[2], 0.0],
        [b(sc.angled), b(sc.mesh_free), b(l.framed), b(sc.tablet)],
        [b(l.portrait), b(sc.curved), b(sc.mip.is_some()), b(has_mesh)],
        [ss as f32, sc.inds.len().min(MAX_INDICATORS) as f32, 0.0, 0.0],
        btn0,
        btn0b,
        btn1,
        btn1b,
        hole,
        bar,
        barb,
        lens(0),
        lens(1),
        v3(r.key_col, 0.0),
        v3(r.fill, 0.0),
        v3(r.fill_col, 0.0),
        v3(r.rim, 0.0),
        v3(r.rim_col, 0.0),
        v3(r.amb, 0.0),
        v3(r.rim_half, r.spec),
        v3(sc.face_col, sc.mip_mix),
        [sc.from.map(|f| f.1).unwrap_or(0.0), 0.0, 0.0, 0.0],
    ]
}

fn v3(v: [f32; 3], w: f32) -> Vec4 {
    [v[0], v[1], v[2], w]
}

fn indicator_data(sc: &Scene) -> Vec<Vec4> {
    sc.inds
        .iter()
        .take(MAX_INDICATORS)
        .flat_map(|i| {
            let (tx, ty, ta) = i.trail.unwrap_or((0.0, 0.0, 0.0));
            [[i.x, i.y, i.r, i.alpha], [i.ring_r, i.ring_alpha, if i.trail.is_some() { 1.0 } else { 0.0 }, ta], [tx, ty, 0.0, 0.0]]
        })
        .collect()
}

impl GpuModel {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, m: &PreparedModel, videos: &[Target; 3], video_sampler: &wgpu::Sampler) -> Self {
        let vbuf = |label: &str, data: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: data, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST })
        };
        let pos = vbuf("positions", bytemuck::cast_slice(&m.pos));
        let nrm = vbuf("normals", bytemuck::cast_slice(&m.nrm));
        let uv = vbuf("uv", bytemuck::cast_slice(m.uv.as_slice()));
        let suv = vbuf("screen uv", bytemuck::cast_slice(&m.suv));

        // Material keys: each material, then one for the screen.
        let screen_key = m.materials.len();
        let key = |ti: usize| if m.tri_screen[ti] { screen_key } else { m.tri_mat[ti] as usize };
        let mut opaque_tris: Vec<(usize, usize)> = Vec::new();
        let mut blend_tris = Vec::new();
        for ti in 0..m.tris.len() {
            let k = key(ti);
            if k != screen_key && m.materials[k].blend {
                blend_tris.push((ti, k));
            } else {
                opaque_tris.push((ti, k));
            }
        }
        opaque_tris.sort_by_key(|&(_, k)| k);
        let mut indices: Vec<u32> = Vec::with_capacity(opaque_tris.len() * 3);
        let mut opaque_runs: Vec<(usize, Range<u32>)> = Vec::new();
        for &(ti, k) in &opaque_tris {
            let start = indices.len() as u32;
            indices.extend_from_slice(&m.tris[ti]);
            match opaque_runs.last_mut() {
                Some((rk, r)) if *rk == k => r.end = start + 3,
                _ => opaque_runs.push((k, start..start + 3)),
            }
        }
        let opaque = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("opaque"),
            contents: bytemuck::cast_slice(if indices.is_empty() { &[0u32; 3][..] } else { &indices }),
            usage: wgpu::BufferUsages::INDEX,
        });
        let blend = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("see-through"),
            size: ((blend_tris.len().max(1) * 3) * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Materials: uniforms at aligned offsets, textures (or a white pixel).
        let align = device.limits().min_uniform_buffer_offset_alignment as usize;
        let stride = 48usize.div_ceil(align) * align;
        let mut mat_bytes = vec![0u8; stride * (screen_key + 1)];
        for k in 0..=screen_key {
            let v: [Vec4; 3] = if k == screen_key {
                [[0.0; 4], [0.0, 1.0, -1.0, 4.0], [0.0; 4]]
            } else {
                let mt = &m.materials[k];
                let has_tex = mt.texture.is_some_and(|i| i < m.textures.len());
                let flags = (if has_tex { 1 } else { 0 }) | (if mt.blend { 2 } else { 0 });
                [
                    [mt.base[0], mt.base[1], mt.base[2], mt.alpha],
                    [mt.metallic, mt.roughness, mt.cutoff.unwrap_or(-1.0), flags as f32],
                    [mt.emissive[0], mt.emissive[1], mt.emissive[2], 0.0],
                ]
            };
            mat_bytes[k * stride..k * stride + 48].copy_from_slice(bytemuck::cast_slice(&v));
        }
        let material_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("materials"), contents: &mat_bytes, usage: wgpu::BufferUsages::UNIFORM });
        let mut textures: Vec<Target> = m.textures.iter().map(|t| upload_rgba(device, queue, "material", t.w, t.h, &t.data)).collect();
        textures.push(upload_rgba(device, queue, "white", 1, 1, &[255; 4]));
        let white = textures.len() - 1;
        let tex_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[uniform(0, both), texture(1, FLOAT), sampler(2)],
        });
        let materials = (0..=screen_key)
            .map(|k| {
                let t = if k < screen_key { m.materials[k].texture.filter(|&i| i < m.textures.len()).unwrap_or(white) } else { white };
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &material_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &material_buf, offset: (k * stride) as u64, size: wgpu::BufferSize::new(48) }),
                        },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&textures[t].view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&tex_sampler) },
                    ],
                })
            })
            .collect();

        let frame = device.create_buffer(&wgpu::BufferDescriptor { label: Some("model frame"), size: (MODEL_VEC4S * 16) as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model frame"),
            entries: &[uniform(0, both), texture(1, FLOAT), sampler(2), texture(3, FLOAT), texture(4, FLOAT)],
        });
        let frame_group = Self::frame_group(device, &frame_layout, &frame, videos, video_sampler);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("model"), source: wgpu::ShaderSource::Wgsl(include_str!("model.wgsl").into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[&frame_layout, &material_layout], push_constant_ranges: &[] });
        let attr = |loc: u32, format: wgpu::VertexFormat| [wgpu::VertexAttribute { format, offset: 0, shader_location: loc }];
        let (a0, a1, a2, a3) = (attr(0, wgpu::VertexFormat::Float32x3), attr(1, wgpu::VertexFormat::Float32x3), attr(2, wgpu::VertexFormat::Float32x2), attr(3, wgpu::VertexFormat::Float32x2));
        let buffers = [
            wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &a0 },
            wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &a1 },
            wgpu::VertexBufferLayout { array_stride: 8, step_mode: wgpu::VertexStepMode::Vertex, attributes: &a2 },
            wgpu::VertexBufferLayout { array_stride: 8, step_mode: wgpu::VertexStepMode::Vertex, attributes: &a3 },
        ];
        // Premultiplied "over", as the CPU rasterizer blends.
        let over = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        };
        let pipeline = |write_depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("model"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &buffers },
                // No culling (the CPU draws both sides); its "front" is clockwise here.
                primitive: wgpu::PrimitiveState { front_face: wgpu::FrontFace::Cw, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: write_depth,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: Some(over), write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview: None,
                cache: None,
            })
        };
        GpuModel {
            pos,
            nrm,
            uv,
            suv,
            opaque,
            opaque_runs,
            blend_tris,
            blend,
            materials,
            frame,
            frame_group,
            frame_layout,
            opaque_pipeline: pipeline(true),
            blend_pipeline: pipeline(false),
            _textures: textures,
            _material_buf: material_buf,
        }
    }

    fn frame_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, frame: &wgpu::Buffer, videos: &[Target; 3], video_sampler: &wgpu::Sampler) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model frame"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&videos[0].view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(video_sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&videos[1].view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&videos[2].view) },
            ],
        })
    }

    fn write_frame(&self, queue: &wgpu::Queue, sc: &Scene) {
        let (l, r) = (&sc.layout, &sc.rot);
        let g = &sc.rig;
        let v: [Vec4; MODEL_VEC4S] = [
            [r[0][0], r[0][1], r[0][2], sc.mox],
            [r[1][0], r[1][1], r[1][2], sc.moy],
            [r[2][0], r[2][1], r[2][2], sc.unit],
            [sc.focal, sc.nearest, sc.dev_scale, sc.s],
            [sc.lcx + sc.dx, sc.lcy + sc.dy, sc.ccx, sc.ccy],
            [sc.tx, sc.ty, sc.cw, sc.ch],
            v3(g.key, 0.0),
            v3(g.key_col, 0.0),
            v3(g.fill, 0.0),
            v3(g.fill_col, 0.0),
            v3(g.rim, 0.0),
            v3(g.rim_col, 0.0),
            v3(g.top, 0.0),
            v3(g.bottom, 0.0),
            v3(g.half, 0.0),
            v3(g.rim_half, g.spec),
            [l.screen.x0, l.screen.y0, l.screen.w(), l.screen.h()],
            [l.video.x0, l.video.y0, l.video.w(), l.video.h()],
            [if sc.mip.is_some() { 1.0 } else { 0.0 }, sc.mip_mix, sc.from.map(|f| f.1).unwrap_or(0.0), 0.0],
        ];
        queue.write_buffer(&self.frame, 0, bytes(&v));
    }

    /// Animated models: this instant's vertices.
    fn write_pose(&self, queue: &wgpu::Queue, sc: &Scene) {
        if let Some(p) = &sc.pose {
            queue.write_buffer(&self.pos, 0, bytemuck::cast_slice(&p.pos));
            queue.write_buffer(&self.nrm, 0, bytemuck::cast_slice(&p.nrm));
        }
    }

    /// Orders the see-through triangles back to front for this frame, as the CPU does.
    fn sort_blend(&self, queue: &wgpu::Queue, sc: &Scene) -> Vec<(usize, Range<u32>)> {
        if self.blend_tris.is_empty() {
            return Vec::new();
        }
        let m = sc.model.expect("model");
        let pos = sc.pose.as_ref().map(|p| &p.pos).unwrap_or(&m.pos);
        let iw = |i: u32| sc.place(pos[i as usize]).2;
        let mut order: Vec<(f32, usize, usize)> = self.blend_tris.iter().map(|&(ti, k)| (m.tris[ti].iter().map(|&i| iw(i)).sum(), ti, k)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut indices = Vec::with_capacity(order.len() * 3);
        let mut runs: Vec<(usize, Range<u32>)> = Vec::new();
        for (_, ti, k) in order {
            let start = indices.len() as u32;
            indices.extend_from_slice(&m.tris[ti]);
            match runs.last_mut() {
                Some((rk, r)) if *rk == k => r.end = start + 3,
                _ => runs.push((k, start..start + 3)),
            }
        }
        queue.write_buffer(&self.blend, 0, bytemuck::cast_slice(&indices));
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::{compose, Mips};
    use crate::model::{CustomModel, FrameStyle, Style, Touch, TouchKind};
    use crate::timeline::{CameraValue, PoseValue};
    use std::time::Instant;

    /// A recording with gradients, stripes and fine detail, so sampling differences show.
    fn recording(w: usize, h: usize) -> Mips {
        let mut data = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                data[i] = (x * 255 / w) as u8;
                data[i + 1] = (y * 255 / h) as u8;
                data[i + 2] = if (x / 24 + y / 24) % 2 == 0 { 230 } else { 30 };
                data[i + 3] = 255;
            }
        }
        Mips::build(Frame { w, h, data })
    }

    fn input<'a>(style: &'a Style, mips: &'a Mips, touches: &'a [Touch], model: Option<&'a PreparedModel>) -> ComposeInput<'a> {
        ComposeInput {
            style,
            screen: Some(mips),
            video_w: 1080.0,
            video_h: 2400.0,
            cam: CameraValue::REST,
            pose: PoseValue::REST,
            touches,
            time: 2.0,
            since_start: 2.0,
            model,
            supersample: 2,
            texts: &[],
            blur: &[],
            from: None,
        }
    }

    /// Mean difference per channel, and the share of pixels off by more than 8.
    fn diff(a: &[u8], b: &[u8]) -> (f64, f64) {
        let (mut sum, mut big) = (0u64, 0usize);
        for (pa, pb) in a.chunks(4).zip(b.chunks(4)) {
            let d = (0..3).map(|k| (pa[k] as i32 - pb[k] as i32).unsigned_abs()).max().unwrap();
            sum += (0..3).map(|k| (pa[k] as i32 - pb[k] as i32).unsigned_abs() as u64).sum::<u64>();
            if d > 8 {
                big += 1;
            }
        }
        let n = (a.len() / 4) as f64;
        (sum as f64 / (n * 3.0), big as f64 / n)
    }

    fn check(gpu: &mut GpuCompositor, name: &str, inp: &ComposeInput, w: usize, h: usize) {
        let (mut cpu, mut gpx) = (vec![0u8; w * h * 4], vec![0u8; w * h * 4]);
        compose(&mut cpu, w, h, inp);
        gpu.compose(&mut gpx, inp).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (mean, big) = diff(&cpu, &gpx);
        println!("{name}: mean {mean:.3}, off by > 8: {:.3} %", big * 100.0);
        if let Ok(dir) = std::env::var("REC_GPU_DUMP") {
            for (tag, px) in [("cpu", &cpu), ("gpu", &gpx)] {
                let mut f = Vec::new();
                f.extend_from_slice(format!("P6\n{w} {h}\n255\n").as_bytes());
                for p in px.chunks(4) {
                    f.extend_from_slice(&p[..3]);
                }
                std::fs::write(format!("{dir}/{name}-{tag}.ppm"), f).unwrap();
            }
        }
        assert!(mean < 0.6 && big < 0.004, "{name}: mean {mean}, off by > 8 in {:.2} % of pixels", big * 100.0);
    }

    /// The GPU draws what the CPU draws. Skipped without a graphics card.
    #[test]
    fn matches_the_cpu() {
        let (w, h) = (640, 360);
        let mut gpu = match GpuCompositor::new(w, h, 2, None) {
            Ok(g) => g,
            Err(e) => return eprintln!("sin tarjeta gráfica, se omite: {e}"),
        };
        println!("{}", gpu.name);
        let mips = recording(540, 1200);
        let tap = [
            Touch { time: 1.95, duration: 0.1, x: 0.4, y: 0.5, end_x: 0.4, end_y: 0.5, kind: TouchKind::Tap },
            Touch { time: 1.8, duration: 0.5, x: 0.2, y: 0.8, end_x: 0.7, end_y: 0.3, kind: TouchKind::Swipe },
        ];
        let base = Style::default();
        check(&mut gpu, "flat", &input(&base, &mips, &tap, None), w, h);

        let turned = Style { turn: 32.0, tilt: 12.0, roll: -4.0, straighten_on_zoom: false, ..base.clone() };
        check(&mut gpu, "turned", &input(&turned, &mips, &tap, None), w, h);
        let back = Style { turn: 160.0, tilt: 10.0, straighten_on_zoom: false, ..base.clone() };
        check(&mut gpu, "back", &input(&back, &mips, &[], None), w, h);

        let curved = Style { frame: FrameStyle::Curved, turn: -20.0, straighten_on_zoom: false, ..base.clone() };
        let mut zoomed = input(&curved, &mips, &tap, None);
        zoomed.cam = CameraValue { zoom: 2.0, x: 0.5, y: 0.3, ..CameraValue::REST };
        check(&mut gpu, "curved-zoom", &zoomed, w, h);

        let tablet = Style { frame: FrameStyle::Tablet, tilt: -15.0, straighten_on_zoom: false, ..base.clone() };
        let mut wide = input(&tablet, &mips, &[], None);
        wide.video_w = 2400.0;
        wide.video_h = 1080.0;
        check(&mut gpu, "tablet", &wide, w, h);

        let none = Style { frame: FrameStyle::None, ..base.clone() };
        check(&mut gpu, "frameless", &input(&none, &mips, &tap, None), w, h);

        // Every lighting style, turned and from behind, with the light moved round.
        for light in crate::model::LightStyle::ALL {
            let lit = Style { turn: 28.0, tilt: 10.0, straighten_on_zoom: false, lighting: crate::model::Lighting { style: light, angle: 20.0, intensity: 1.2, glare: 1.5 }, ..base.clone() };
            check(&mut gpu, &format!("light-{light:?}"), &input(&lit, &mips, &[], None), w, h);
            let behind = Style { turn: 150.0, ..lit.clone() };
            check(&mut gpu, &format!("light-{light:?}-back"), &input(&behind, &mips, &[], None), w, h);
            let flat = Style { turn: 0.0, tilt: 0.0, ..lit };
            check(&mut gpu, &format!("light-{light:?}-flat"), &input(&flat, &mips, &[], None), w, h);
        }

        // Mip blending off and on, and a dissolve from another clip.
        let other = recording(400, 900);
        for aa in crate::model::Antialias::ALL {
            let st = Style { finish: crate::model::Finish { antialias: aa, ..Default::default() }, ..base.clone() };
            let mut z = input(&st, &mips, &[], None);
            z.cam = CameraValue { zoom: 1.3, x: 0.4, y: 0.6, ..CameraValue::REST };
            check(&mut gpu, &format!("aa-{aa:?}"), &z, w, h);
        }
        let mut dissolve = input(&base, &mips, &[], None);
        dissolve.from = Some((&other, 0.4));
        check(&mut gpu, "dissolve", &dissolve, w, h);

        // Motion blur over a quick turn.
        let blur: Vec<(f32, CameraValue, PoseValue)> = (0..5).map(|i| (2.0 + i as f32 * 0.002, CameraValue::REST, PoseValue { turn: 60.0 + i as f32 * 3.0, ..PoseValue::REST })).collect();
        let mut moving = input(&base, &mips, &[], None);
        moving.blur = &blur;
        check(&mut gpu, "blur", &moving, w, h);
    }

    #[test]
    fn matches_the_cpu_with_models() {
        let (w, h) = (640, 360);
        let Some(entry) = crate::library::list().into_iter().find(|m| m.name == "Moderno") else {
            return eprintln!("falta el modelo incluido «Moderno»");
        };
        let src = crate::model3d::load(&entry.path).expect("model");
        let model = crate::model3d::prepare(&src, src.guess.expect("screen"), 0).expect("prepare");
        let mut gpu = match GpuCompositor::new(w, h, 2, Some(&model)) {
            Ok(g) => g,
            Err(e) => return eprintln!("sin tarjeta gráfica, se omite: {e}"),
        };
        let mips = recording(540, 1200);
        let custom = Style { frame: FrameStyle::Custom, model: Some(CustomModel { path: entry.path.clone(), ..Default::default() }), straighten_on_zoom: false, ..Style::default() };
        check(&mut gpu, "model", &input(&custom, &mips, &[], Some(&model)), w, h);
        let turned = Style { turn: 35.0, tilt: 10.0, ..custom.clone() };
        check(&mut gpu, "model-turned", &input(&turned, &mips, &[], Some(&model)), w, h);
        let back = Style { turn: 180.0, ..custom.clone() };
        check(&mut gpu, "model-back", &input(&back, &mips, &[], Some(&model)), w, h);
        for light in crate::model::LightStyle::ALL {
            let lit = Style { turn: 30.0, lighting: crate::model::Lighting { style: light, angle: -30.0, intensity: 1.1, glare: 1.0 }, ..custom.clone() };
            check(&mut gpu, &format!("model-light-{light:?}"), &input(&lit, &mips, &[], Some(&model)), w, h);
        }
        let mut zoomed = input(&custom, &mips, &[], Some(&model));
        zoomed.cam = CameraValue { zoom: 1.8, x: 0.5, y: 0.25, ..CameraValue::REST };
        check(&mut gpu, "model-zoom", &zoomed, w, h);
    }

    /// BT.709 TV-range 4:2:0 of an RGBA frame, as finish.wgsl computes it.
    fn yuv(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            [rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32]
        };
        let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
        let mut out: Vec<u8> = (0..w * h).map(|i| q(16.0 + luma(px(i % w, i / w)) * 219.0 / 255.0)).collect();
        let (mut u, mut v) = (Vec::new(), Vec::new());
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                let c = [px(2 * x, 2 * y), px(2 * x + 1, 2 * y), px(2 * x, 2 * y + 1), px(2 * x + 1, 2 * y + 1)];
                let m = [0, 1, 2].map(|k| c.iter().map(|p| p[k]).sum::<f32>() / 4.0);
                let l = luma(m);
                u.push(q(128.0 + (m[2] - l) / 1.8556 * 224.0 / 255.0));
                v.push(q(128.0 + (m[0] - l) / 1.5748 * 224.0 / 255.0));
            }
        }
        out.extend(u);
        out.extend(v);
        out
    }

    /// The encoder's YUV, titles included, matches the CPU frame converted the same way.
    #[test]
    fn yuv_with_titles_matches_the_cpu() {
        use crate::model::{TextClip, TextColor, TextPlace};
        let (w, h) = (640, 360);
        let mut gpu = match GpuCompositor::new(w, h, 2, None) {
            Ok(g) => g,
            Err(e) => return eprintln!("sin tarjeta gráfica, se omite: {e}"),
        };
        let mips = recording(540, 1200);
        let style = Style { turn: 20.0, straighten_on_zoom: false, ..Style::default() };
        let a = TextClip { title: "Camina.".into(), subtitle: "Tus pasos te mueven.".into(), color: TextColor::Light, ..TextClip::default() };
        let b = TextClip { title: "Sobre la grabación".into(), place: TextPlace::Center, color: TextColor::Dark, ..TextClip::default() };
        let shown = [crate::text::Shown { clip: &a, local: 1.5 }, crate::text::Shown { clip: &b, local: 1.5 }];
        let blur: Vec<(f32, CameraValue, PoseValue)> = (0..4).map(|i| (2.0, CameraValue::REST, PoseValue { turn: 10.0 + i as f32 * 4.0, ..PoseValue::REST })).collect();
        let finished = crate::post::Post { vignette: 0.6, grain: 0.5, fade: 0.7, frame: 12 };
        for (name, texts, blur, post) in [
            ("plain", &[][..], &[][..], crate::post::Post::NONE),
            ("titles", &shown[..], &[][..], crate::post::Post::NONE),
            ("titles-blur", &shown[..], &blur[..], crate::post::Post::NONE),
            ("finish", &shown[..], &[][..], finished),
        ] {
            let mut inp = input(&style, &mips, &[], None);
            inp.texts = texts;
            inp.blur = blur;
            let mut cpu = vec![0u8; w * h * 4];
            compose(&mut cpu, w, h, &inp);
            crate::post::apply(&mut cpu, w, h, &post);
            let expected = yuv(&cpu, w, h);
            let mut got = vec![0u8; w * h * 3 / 2];
            let overlay = crate::text::overlay(w, h, &style, texts);
            gpu.compose_yuv(&mut got, &inp, overlay.as_ref(), &post).unwrap();
            let diffs: Vec<u32> = expected.iter().zip(&got).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs()).collect();
            let mean = diffs.iter().map(|&d| d as f64).sum::<f64>() / diffs.len() as f64;
            let big = diffs.iter().filter(|&&d| d > 4).count() as f64 / diffs.len() as f64;
            println!("{name}: mean {mean:.3}, off by > 4: {:.3} %", big * 100.0);
            assert!(mean < 0.3 && big < 0.002, "{name}: mean {mean}, {:.2} % off by > 4", big * 100.0);
        }
    }

    /// The preview's texture (titles and finishing on the card) matches the CPU frame.
    #[test]
    fn display_matches_the_cpu() {
        use crate::model::{TextClip, TextColor};
        let (w, h) = (640, 360);
        let mut gpu = match GpuCompositor::new(w, h, 2, None) {
            Ok(g) => g,
            Err(e) => return eprintln!("sin tarjeta gráfica, se omite: {e}"),
        };
        let mips = recording(540, 1200);
        let style = Style { turn: 20.0, straighten_on_zoom: false, ..Style::default() };
        let a = TextClip { title: "Camina.".into(), subtitle: "Tus pasos te mueven.".into(), color: TextColor::Light, ..TextClip::default() };
        let shown = [crate::text::Shown { clip: &a, local: 1.5 }];
        let post = crate::post::Post { vignette: 0.6, grain: 0.5, fade: 0.8, frame: 3 };
        let mut inp = input(&style, &mips, &[], None);
        inp.texts = &shown;
        let mut cpu = vec![0u8; w * h * 4];
        compose(&mut cpu, w, h, &inp);
        crate::post::apply(&mut cpu, w, h, &post);
        let overlay = crate::text::overlay(w, h, &style, &shown);
        gpu.compose_display(&inp, overlay.as_ref(), &post).unwrap();
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture { texture: &gpu.display.as_ref().unwrap().0.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::ImageCopyBuffer { buffer: &gpu.readback, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(gpu.padded_row as u32), rows_per_image: Some(h as u32) } },
            wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        let mut got = vec![0u8; w * h * 4];
        let row = gpu.padded_row;
        gpu.download(&gpu.readback, |d| unpad(d, row, w * 4, h, &mut got)).unwrap();
        let diffs: Vec<u32> = cpu.iter().zip(&got).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs()).collect();
        let mean = diffs.iter().map(|&d| d as f64).sum::<f64>() / diffs.len() as f64;
        let big = diffs.iter().filter(|&&d| d > 4).count() as f64 / diffs.len() as f64;
        println!("display: mean {mean:.3}, off by > 4: {:.3} %", big * 100.0);
        assert!(mean < 0.4 && big < 0.003, "mean {mean}, {:.2} % off by > 4", big * 100.0);
    }

    /// Time per 1080p frame, CPU against GPU: cargo test gpu_speed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn gpu_speed() {
        let (w, h) = (1920, 1080);
        let mips = recording(1080, 2400);
        let model_path = std::env::var("REC_TEST_MODEL").ok();
        let prepared = model_path.as_ref().map(|p| {
            let src = crate::model3d::load(p).expect("model");
            crate::model3d::prepare(&src, src.guess.expect("screen"), 0).expect("prepare")
        });
        let style = match &model_path {
            Some(p) => Style { frame: FrameStyle::Custom, model: Some(CustomModel { path: p.clone(), ..Default::default() }), turn: 25.0, ..Style::default() },
            None => Style { turn: 25.0, ..Style::default() },
        };
        let inp = input(&style, &mips, &[], prepared.as_ref());
        let mut out = vec![0u8; w * h * 4];
        let t = Instant::now();
        for _ in 0..20 {
            compose(&mut out, w, h, &inp);
        }
        let cpu = t.elapsed().as_secs_f64() / 20.0;
        let mut gpu = GpuCompositor::new(w, h, 2, prepared.as_ref()).expect("gpu");
        gpu.compose(&mut out, &inp).unwrap();
        let t = Instant::now();
        for _ in 0..20 {
            gpu.compose(&mut out, &inp).unwrap();
        }
        let g = t.elapsed().as_secs_f64() / 20.0;
        println!("CPU {:.1} ms/fotograma, GPU {:.1} ms/fotograma ({:.1}×)", cpu * 1000.0, g * 1000.0, cpu / g);
    }
}
