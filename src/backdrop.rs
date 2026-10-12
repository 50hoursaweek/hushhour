//! The animated background behind the page: runs the shader from
//! `hushhour_core::backdrop` (where the patterns, strength and motion are defined) in an
//! iced shader widget.
//!
//! The pattern is drawn into a cached texture only when it changes (the clock moved, or the
//! size or colours changed); every other frame, such as one drawn for a keystroke while the
//! pattern is at rest, only copies that texture onto the page.
//!
//! The shader only runs with the wgpu renderer; under the software fallback the widget
//! draws nothing and the page keeps its plain colour.

use std::borrow::Cow;

use iced::wgpu;
use iced::widget::shader::{self, Viewport};
use iced::{Color, Rectangle, mouse};

pub use hushhour_core::backdrop::{FRAME_INTERVAL_MS, MAX_SECONDARY_FADE, Motion, RESTING_INTERVAL_MS};
use hushhour_core::backdrop::{Pattern, STRENGTH, WGSL};

/// The shader program for the current pattern, colours and time.
#[derive(Debug, Clone, Copy)]
pub struct Program {
    pub pattern: Pattern,
    pub background: Color,
    pub text: Color,
    /// Seconds of animation; it stands still while the pattern is at rest.
    pub time: f32,
}

impl<Message> shader::Program<Message> for Program {
    type State = ();
    type Primitive = Primitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive(*self)
    }
}

#[derive(Debug)]
pub struct Primitive(Program);

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let p = &self.0;
        let scale = viewport.scale_factor();
        let width = (bounds.width * scale).round().max(1.0) as u32;
        let height = (bounds.height * scale).round().max(1.0) as u32;
        if pipeline.cache.as_ref().is_none_or(|c| c.size != (width, height)) {
            pipeline.cache = Some(Cache::new(device, pipeline, width, height));
            pipeline.drawn = None;
        }

        // The pattern is drawn in the cache's own coordinates, from its top-left corner.
        let (bg, fg) = (p.background, p.text);
        let values: [f32; UNIFORM_FLOATS] = [
            bg.r, bg.g, bg.b, 1.0,
            fg.r, fg.g, fg.b, 1.0,
            0.0, 0.0, width as f32, height as f32,
            p.time, STRENGTH, 0.0, scale,
            p.pattern.index() as f32, if pipeline.srgb { 1.0 } else { 0.0 }, 0.0, 0.0,
        ];
        let key = values.map(f32::to_bits);
        pipeline.dirty = pipeline.drawn != Some(key);
        if pipeline.dirty {
            queue.write_buffer(&pipeline.pattern_uniforms, 0, &to_bytes(&values));
            pipeline.drawn = Some(key);
        }
        let origin = [(bounds.x * scale).round(), (bounds.y * scale).round(), 0.0, 0.0];
        queue.write_buffer(&pipeline.blit_uniforms, 0, &to_bytes(&origin));
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        // A changed pattern needs its own pass into the cache first: see `render`.
        if pipeline.dirty {
            return false;
        }
        let Some(cache) = &pipeline.cache else { return true };
        pipeline.blit(pass, cache);
        true
    }

    fn render(
        &self,
        pipeline: &Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let Some(cache) = &pipeline.cache else { return };
        {
            let mut pass = begin(encoder, &cache.view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&pipeline.pattern);
            pass.set_bind_group(0, &pipeline.pattern_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let mut pass = begin(encoder, target, wgpu::LoadOp::Load);
        pass.set_scissor_rect(clip_bounds.x, clip_bounds.y, clip_bounds.width, clip_bounds.height);
        pipeline.blit(&mut pass, cache);
    }
}

const UNIFORM_FLOATS: usize = 20;

fn to_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_ne_bytes()).collect()
}

fn begin<'a>(encoder: &'a mut wgpu::CommandEncoder, view: &'a wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("hushhour backdrop"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

/// The last pattern drawn, the size of the page in physical pixels.
struct Cache {
    size: (u32, u32),
    view: wgpu::TextureView,
    blit_bind_group: wgpu::BindGroup,
}

impl Cache {
    fn new(device: &wgpu::Device, pipeline: &Pipeline, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hushhour backdrop"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: pipeline.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blit_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hushhour backdrop"),
            layout: &pipeline.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: pipeline.blit_uniforms.as_entire_binding() },
            ],
        });
        Self { size: (width, height), view, blit_bind_group }
    }
}

pub struct Pipeline {
    pattern: wgpu::RenderPipeline,
    pattern_uniforms: wgpu::Buffer,
    pattern_bind_group: wgpu::BindGroup,
    blit: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    /// The page's top-left corner in the target, physical pixels.
    blit_uniforms: wgpu::Buffer,
    cache: Option<Cache>,
    /// The uniforms the cache was last drawn with.
    drawn: Option<[u32; UNIFORM_FLOATS]>,
    /// This frame's pattern differs from the cache.
    dirty: bool,
    format: wgpu::TextureFormat,
    /// The target stores linear colour, so the shader converts from sRGB before writing.
    srgb: bool,
}

impl Pipeline {
    fn blit(&self, pass: &mut wgpu::RenderPass<'_>, cache: &Cache) {
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &cache.blit_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn render_pipeline(
    device: &wgpu::Device,
    source: &'static str,
    layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("hushhour backdrop"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("hushhour backdrop"),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("hushhour backdrop"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let buffer = |size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("hushhour backdrop"),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let pattern_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hushhour backdrop"),
            entries: &[uniform_entry(0)],
        });
        let pattern_uniforms = buffer((UNIFORM_FLOATS * 4) as u64);
        let pattern_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hushhour backdrop"),
            layout: &pattern_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: pattern_uniforms.as_entire_binding() }],
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hushhour backdrop"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                uniform_entry(1),
            ],
        });
        Self {
            pattern: render_pipeline(device, WGSL, &pattern_layout, format),
            pattern_uniforms,
            pattern_bind_group,
            blit: render_pipeline(device, BLIT, &blit_layout, format),
            blit_layout,
            blit_uniforms: buffer(16),
            cache: None,
            drawn: None,
            dirty: true,
            format,
            srgb: format.is_srgb(),
        }
    }
}

/// Copies the cached pattern onto the page, pixel for pixel (it is already premultiplied).
const BLIT: &str = r#"
struct Blit {
    origin: vec4<f32>,
}

@group(0) @binding(0) var cache: texture_2d<f32>;
@group(0) @binding(1) var<uniform> blit: Blit;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(cache));
    let p = clamp(vec2<i32>(floor(pos.xy - blit.origin.xy)), vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(cache, p, 0);
}
"#;
