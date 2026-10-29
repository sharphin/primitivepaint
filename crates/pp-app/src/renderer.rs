use std::sync::Arc;

use pp_core::{Document, Rect, Rgba8};
use winit::window::Window;

use crate::ui::theme;
use crate::view::ViewTransform;

/// キャンバスのまわりの色。配色（ライト・ダーク）に合わせる。
fn workspace_color() -> wgpu::Color {
    let c = theme::colors().workspace;
    let channel = |v: u8| v as f64 / 255.0;
    wgpu::Color { r: channel(c.r), g: channel(c.g), b: channel(c.b), a: 1.0 }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    linear: [f32; 4],
    pan: [f32; 2],
    screen_size: [f32; 2],
    canvas_size: [f32; 2],
    _pad: [f32; 2],
}

/// 画面に貼る 1 枚の画像（キャンバスや UI）。テクスチャと表示位置を持つ。
struct TexturedQuad {
    texture: wgpu::Texture,
    view_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

impl TexturedQuad {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        label: &str,
        size: (u32, u32),
    ) -> Self {
        // ドキュメントは sRGB の値をそのまま持つので、テクスチャも非 sRGB で扱う
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&Default::default());
        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: view_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&texture_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        TexturedQuad { texture, view_buffer, bind_group, size }
    }

    /// テクスチャの `region` を `pixels`（region の幅 × 高さ）で置き換える。
    fn write(&self, queue: &wgpu::Queue, region: Rect, pixels: &[Rgba8]) {
        let (w, h) = (region.width(), region.height());
        let bytes: &[u8] = unsafe {
            // Rgba8 は repr(C) の u8 × 4
            std::slice::from_raw_parts(pixels.as_ptr().cast(), pixels.len() * 4)
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: region.x0 as u32, y: region.y0 as u32, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    fn set_transform(&self, queue: &wgpu::Queue, view: &ViewTransform, screen: (u32, u32)) {
        let uniform = ViewUniform {
            linear: view.linear(),
            pan: view.pan,
            screen_size: [screen.0 as f32, screen.1 as f32],
            canvas_size: [self.size.0 as f32, self.size.1 as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&uniform));
    }
}

/// wgpu でキャンバスと UI を表示する。合成は CPU、表示と拡大縮小・回転は GPU が受け持つ。
/// 描画のたびに変更された範囲だけをテクスチャへ転送する。
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    ui_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    canvas: TexturedQuad,
    /// 画面全体を覆う UI の画像。キャンバスの区画は透明。
    ui: TexturedQuad,
    /// 合成結果の一時バッファ（毎回確保しないよう使い回す）。
    scratch: Vec<Rgba8>,
}

impl Renderer {
    pub fn new(window: Arc<Window>, canvas_size: (u32, u32)) -> Self {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).expect("サーフェスを作成できません");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("GPU アダプタが見つかりません");
        log::info!("adapter: {:?}", adapter.get_info());

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("primitivepaint"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("GPU デバイスを作成できません");

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            // ペン入力の遅延を減らすため、対応していれば Mailbox を使う
            present_mode: if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::AutoVsync
            },
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("canvas"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let canvas = TexturedQuad::new(&device, &bind_group_layout, &sampler, "canvas", canvas_size);
        let ui = TexturedQuad::new(&device, &bind_group_layout, &sampler, "ui", (config.width, config.height));

        let shader = device.create_shader_module(wgpu::include_wgsl!("canvas.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            ..Default::default()
        });
        let make_pipeline = |entry_point: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline("fs_main", None);
        let ui_pipeline = make_pipeline("fs_ui", Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING));

        Renderer {
            surface,
            device,
            queue,
            config,
            pipeline,
            ui_pipeline,
            bind_group_layout,
            sampler,
            canvas,
            ui,
            scratch: Vec::new(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.ui = TexturedQuad::new(&self.device, &self.bind_group_layout, &self.sampler, "ui", (width, height));
    }

    /// キャンバスの大きさが変わったとき（別の大きさの作品を開いたとき）に、テクスチャを作り直す。
    pub fn set_canvas_size(&mut self, size: (u32, u32)) {
        self.canvas = TexturedQuad::new(&self.device, &self.bind_group_layout, &self.sampler, "canvas", size);
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// ドキュメントの `dirty` を合成し直してテクスチャへ転送する。
    pub fn upload_canvas(&mut self, doc: &Document, dirty: Rect) {
        let region = dirty.intersect(&doc.bounds());
        if region.is_empty() {
            return;
        }
        self.scratch.resize((region.width() * region.height()) as usize, Rgba8::default());
        doc.composite(region, &mut self.scratch);
        self.canvas.write(&self.queue, region, &self.scratch);
    }

    /// UI 画像の `region` を `pixels`（region の幅 × 高さ）で置き換える。
    pub fn upload_ui(&mut self, region: Rect, pixels: &[Rgba8]) {
        let r = region.intersect(&Rect::from_size(self.ui.size.0, self.ui.size.1));
        if !r.is_empty() && r == region {
            self.ui.write(&self.queue, r, pixels);
        }
    }

    /// キャンバスを `view` で描き、その上に UI を等倍で重ねる。
    pub fn render(&mut self, view: &ViewTransform) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                self.surface.configure(&self.device, &self.config);
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };

        let screen = self.surface_size();
        self.canvas.set_transform(&self.queue, view, screen);
        let ui_view = ViewTransform { zoom: 1.0, rotation: 0.0, pan: [0.0, 0.0] };
        self.ui.set_transform(&self.queue, &ui_view, screen);

        let target = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(workspace_color()), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.canvas.bind_group, &[]);
            pass.draw(0..6, 0..1);
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_bind_group(0, &self.ui.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }
}

#[cfg(test)]
mod tests {
    use wgpu::naga;

    /// GPU がなくても、シェーダーの文法と型の誤りを検出する。
    #[test]
    fn canvas_shader_is_valid() {
        let module = naga::front::wgsl::parse_str(include_str!("canvas.wgsl")).expect("WGSL の構文エラー");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("WGSL の検証エラー");
    }
}
