//! The animated background behind the page: Waves, Swell or Drift, drawn by one small
//! fragment shader. Every pattern is the text colour mixed into the page colour by a few
//! percent, so it stays near the edge of what can be seen in every theme.
//!
//! The shader only runs with the wgpu renderer; under the software fallback the widget
//! draws nothing and the page keeps its plain colour.

use std::borrow::Cow;

use iced::wgpu;
use iced::widget::shader::{self, Viewport};
use iced::{Color, Rectangle, mouse};

use crate::prefs::Backdrop;

/// How far the pattern's strongest marks move from the page colour towards the text colour.
pub const STRENGTH: f32 = 0.05;

/// The shader program for the current pattern, colours and time.
#[derive(Debug, Clone, Copy)]
pub struct Program {
    pub pattern: Backdrop,
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
        let pattern = match p.pattern {
            Backdrop::Off => 0.0,
            Backdrop::Waves => 1.0,
            Backdrop::Swell => 2.0,
            Backdrop::Drift => 3.0,
        };
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
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
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

/// The patterns follow the design sketch: positions are fractions of the page, so the
/// picture keeps its shape at any window size, and strokes are one logical pixel wide.
const SHADER: &str = r#"
struct Params {
    bg: vec4<f32>,
    fg: vec4<f32>,
    rect: vec4<f32>,   // origin x, y and size w, h in physical pixels
    misc: vec4<f32>,   // time, strength, corner radius (physical), scale factor
    mode: vec4<f32>,   // pattern, target is sRGB
}

@group(0) @binding(0) var<uniform> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn hash(x: i32, y: i32, z: i32) -> f32 {
    var h: u32 = (bitcast<u32>(x) * 73856093u) ^ (bitcast<u32>(y) * 19349663u) ^ (bitcast<u32>(z) * 83492791u);
    h = (h ^ (h >> 13u)) * 1274126177u;
    return f32(h ^ (h >> 16u)) / 4294967295.0;
}

fn noise(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = mix(hash(i.x, i.y, i.z), hash(i.x + 1, i.y, i.z), s.x);
    let b = mix(hash(i.x, i.y + 1, i.z), hash(i.x + 1, i.y + 1, i.z), s.x);
    let c = mix(hash(i.x, i.y, i.z + 1), hash(i.x + 1, i.y, i.z + 1), s.x);
    let d = mix(hash(i.x, i.y + 1, i.z + 1), hash(i.x + 1, i.y + 1, i.z + 1), s.x);
    return mix(mix(a, b, s.y), mix(c, d, s.y), s.z);
}

fn fbm(p: vec3<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var freq = 1.0;
    var norm = 0.0;
    for (var i = 0; i < 3; i++) {
        sum += amp * noise(vec3<f32>(p.xy * freq, p.z * freq + f32(i) * 17.0));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    return sum / norm;
}

fn waves(u: f32, v: f32, h: f32, t: f32, scale: f32) -> f32 {
    var k = 0.0;
    for (var i = 0; i < 9; i++) {
        let fi = f32(i);
        let y = (fi + 1.0) / 10.0 + 0.03 * sin(u * 5.5 + t * 0.35 + fi * 0.7) + 0.012 * sin(u * 13.0 - t * 0.22 + fi * 1.9);
        let d = abs(v - y) * h;
        let line = clamp(0.5 * scale + 0.5 - d, 0.0, 1.0);
        k = max(k, line * (0.65 + 0.35 * sin(fi * 1.7 + t * 0.15)));
    }
    return k;
}

fn swell(u: f32, v: f32, h: f32, t: f32) -> f32 {
    var k = 0.0;
    var below = 0.0;
    for (var i = 0; i < 4; i++) {
        let fi = f32(i);
        let y = 0.52 + fi * 0.12 + 0.035 * sin(u * (3.0 + fi) + t * (0.18 + fi * 0.05) + fi * 2.1) + 0.015 * sin(u * 9.0 - t * 0.12 + fi);
        let cover = clamp((v - y) * h + 0.5, 0.0, 1.0);
        let level = 1.2 * (fi + 1.0) / 4.0;
        k += cover * (level - below);
        below = level;
    }
    return k;
}

fn drift(x: f32, y: f32, t: f32) -> f32 {
    return 3.0 * smoothstep(0.46, 0.62, fbm(vec3<f32>(x * 2.6 + t * 0.04, y * 2.6 + t * 0.06, t * 0.09)));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = params.rect.zw;
    let p = pos.xy - params.rect.xy;
    let t = params.misc.x;
    let u = p.x / size.x;
    let v = p.y / size.y;

    var k = 0.0;
    let mode = i32(params.mode.x + 0.5);
    if mode == 1 {
        k = waves(u, v, size.y, t, params.misc.w);
    } else if mode == 2 {
        k = swell(u, v, size.y, t);
    } else if mode == 3 {
        k = drift(p.x / size.y, p.y / size.y, t);
    }
    var color = mix(params.bg.rgb, params.fg.rgb, clamp(k * params.misc.y, 0.0, 1.0));
    if params.mode.y > 0.5 {
        color = to_linear(color);
    }

    // Rounded page corners: outside them the window stays transparent.
    let r = params.misc.z;
    let q = abs(p - size * 0.5) - (size * 0.5 - vec2<f32>(r));
    let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let alpha = clamp(0.5 - dist, 0.0, 1.0);
    return vec4<f32>(color * alpha, alpha);
}
"#;
