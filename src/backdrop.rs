//! The animated background behind the page: runs the shader from
//! `hushhour_core::backdrop` (where the patterns, strength and motion are defined) in an
//! iced shader widget.
//!
//! The shader only runs with the wgpu renderer; under the software fallback the widget
//! draws nothing and the page keeps its plain colour.

use std::borrow::Cow;

use iced::wgpu;
use iced::widget::shader::{self, Viewport};
use iced::{Color, Rectangle, mouse};

pub use hushhour_core::backdrop::{FRAME_INTERVAL_MS, Motion, RESTING_INTERVAL_MS};
use hushhour_core::backdrop::{Pattern, STRENGTH, WGSL};

/// The shader program for the current pattern, colours and time.
#[derive(Debug, Clone, Copy)]
pub struct Program {
    pub pattern: Pattern,
    pub background: Color,
    pub text: Color,
    /// Seconds of animation; it only advances while the pattern is moving.
    pub time: f32,
    /// Logical pixels; the window's rounded corners stay transparent.
    pub radius: f32,
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
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let p = &self.0;
        let scale = viewport.scale_factor();
        let pattern = p.pattern.index() as f32;
        let (bg, fg) = (p.background, p.text);
        let values: [f32; UNIFORM_FLOATS] = [
            bg.r, bg.g, bg.b, 1.0,
            fg.r, fg.g, fg.b, 1.0,
            bounds.x * scale, bounds.y * scale, bounds.width * scale, bounds.height * scale,
            p.time, STRENGTH, p.radius * scale, scale,
            pattern, if pipeline.srgb { 1.0 } else { 0.0 }, 0.0, 0.0,
        ];
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&pipeline.uniforms, 0, &bytes);
    }

    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &pipeline.bind_group, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

const UNIFORM_FLOATS: usize = 20;

pub struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// The target stores linear colour, so the shader converts from sRGB before writing.
    srgb: bool,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hushhour backdrop"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(WGSL)),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hushhour backdrop"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hushhour backdrop"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hushhour backdrop"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hushhour backdrop"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
        });
        Self { pipeline, uniforms, bind_group, srgb: format.is_srgb() }
    }
}
