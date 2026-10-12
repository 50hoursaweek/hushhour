//! The optional animated background: Waves, Swell or Drift, a slow pattern behind the
//! page at the edge of visibility.
//!
//! Every pattern yields an intensity `k` per pixel (0 = page colour). The pixel colour is
//! `mix(page, text, k * STRENGTH)`, mixed in sRGB, so the pattern is the text colour
//! a few percent into the page colour in every theme. [`intensity`] is the reference for
//! one pixel; [`WGSL`] is the same maths as a fragment shader, which the Linux app runs
//! and a port can translate (to Metal, say) and test against [`intensity`].
//!
//! [`Motion`] holds the clock: it advances with real time, eases to a stop while the user
//! types and resumes after a pause. Platforms tick it at [`FRAME_INTERVAL_MS`], or at
//! [`RESTING_INTERVAL_MS`] while it is at rest, and stop ticking while the window is
//! inactive. The pattern is off by default.

/// The patterns; `Off` is the platform's choice, not a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Waves,
    Swell,
    Drift,
}

impl Pattern {
    pub const ALL: [Self; 3] = [Self::Waves, Self::Swell, Self::Drift];

    /// The pattern's number in the shader's `mode.x` (0 draws nothing).
    pub fn index(self) -> u32 {
        match self {
            Self::Waves => 1,
            Self::Swell => 2,
            Self::Drift => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Waves => "Waves",
            Self::Swell => "Swell",
            Self::Drift => "Drift",
        }
    }
}

/// How far an intensity of 1 moves from the page colour towards the text colour.
pub const STRENGTH: f32 = 0.05;
/// Frame interval while moving: slow motion looks smooth at 15 frames a second.
pub const FRAME_INTERVAL_MS: u64 = 66;
/// Tick interval once typing has brought the pattern to rest: enough to see the pause.
pub const RESTING_INTERVAL_MS: u64 = 250;
/// Typing counts as recent for this long after the last edit.
pub const TYPING_WINDOW_SECS: f32 = 1.4;

/// The animation clock, slowed by typing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// Seconds of animation; the shader's time.
    pub clock: f32,
    /// 0 = moving freely, 1 = brought to a stop by typing.
    pub calm: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self { clock: 7.0, calm: 0.0 }
    }
}

impl Motion {
    /// Move on by `dt` seconds of real time (capped at a quarter second, so a stall or a
    /// long pause does not jump). `since_typed` is the time since the last edit, if any.
    /// The pattern eases to a stop within about a second of typing and back over a few
    /// seconds after.
    pub fn advance(&mut self, dt: f32, since_typed: Option<f32>) {
        let dt = dt.clamp(0.0, 0.25);
        let typing = since_typed.is_some_and(|s| s < TYPING_WINDOW_SECS);
        let (target, rate) = if typing { (1.0, 3.0) } else { (0.0, 0.8) };
        self.calm += (target - self.calm) * (dt * rate).min(1.0);
        self.clock += dt * (1.0 - self.calm);
    }

    /// At rest: the platform may tick at [`RESTING_INTERVAL_MS`] instead.
    pub fn is_resting(&self) -> bool {
        self.calm > 0.99
    }
}

/// The intensity at pixel (`x`, `y`) of a `width` × `height` page, all in physical
/// pixels, at `time` seconds; `scale` is the display scale (strokes are one logical
/// pixel wide). Waves and Drift stay within 0..=1 and 0..=3, Swell within 0..=1.2.
pub fn intensity(pattern: Pattern, x: f32, y: f32, width: f32, height: f32, time: f32, scale: f32) -> f32 {
    let (u, v, t) = (x / width, y / height, time);
    match pattern {
        Pattern::Waves => (0..9)
            .map(|i| {
                let fi = i as f32;
                let line_y = (fi + 1.0) / 10.0 + 0.03 * (u * 5.5 + t * 0.35 + fi * 0.7).sin() + 0.012 * (u * 13.0 - t * 0.22 + fi * 1.9).sin();
                let d = (v - line_y).abs() * height;
                let line = (0.5 * scale + 0.5 - d).clamp(0.0, 1.0);
                line * (0.65 + 0.35 * (fi * 1.7 + t * 0.15).sin())
            })
            .fold(0.0, f32::max),
        Pattern::Swell => {
            let mut k = 0.0;
            let mut below = 0.0;
            for i in 0..4 {
                let fi = i as f32;
                let edge = 0.52 + fi * 0.12 + 0.035 * (u * (3.0 + fi) + t * (0.18 + fi * 0.05) + fi * 2.1).sin() + 0.015 * (u * 9.0 - t * 0.12 + fi).sin();
                let cover = ((v - edge) * height + 0.5).clamp(0.0, 1.0);
                let level = 1.2 * (fi + 1.0) / 4.0;
                k += cover * (level - below);
                below = level;
            }
            k
        }
        Pattern::Drift => {
            let (px, py) = (x / height, y / height);
            3.0 * smoothstep(0.46, 0.62, fbm(px * 2.6 + t * 0.04, py * 2.6 + t * 0.06, t * 0.09))
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash(x: i32, y: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663) ^ (z as u32).wrapping_mul(83_492_791);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as f32 / 4_294_967_295.0
}

fn noise(x: f32, y: f32, z: f32) -> f32 {
    let (xi, yi, zi) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let s = |f: f32| f * f * (3.0 - 2.0 * f);
    let (u, v, w) = (s(x - x.floor()), s(y - y.floor()), s(z - z.floor()));
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx, dy, dz| hash(xi + dx, yi + dy, zi + dz);
    l(
        l(l(c(0, 0, 0), c(1, 0, 0), u), l(c(0, 1, 0), c(1, 1, 0), u), v),
        l(l(c(0, 0, 1), c(1, 0, 1), u), l(c(0, 1, 1), c(1, 1, 1), u), v),
        w,
    )
}

fn fbm(x: f32, y: f32, z: f32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for i in 0..3 {
        sum += amp * noise(x * freq, y * freq, z * freq + i as f32 * 17.0);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// The fragment shader (WGSL). Uniforms, five `vec4<f32>`: page colour, text colour (both
/// sRGB), the page rectangle (x, y, w, h in physical pixels), (time, [`STRENGTH`], corner
/// radius in physical pixels, scale) and (pattern index, 1 if the target is sRGB). Draw a
/// full-screen triangle (three vertices, no buffers) with premultiplied-alpha blending.
pub const WGSL: &str = r#"
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

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1280.0;
    const H: f32 = 800.0;

    #[test]
    fn waves_draw_thin_lines() {
        let t = 7.0;
        // On the first line at the left edge, and halfway between two lines.
        let line_y = (0.1 + 0.03 * (t * 0.35f32).sin() + 0.012 * (-t * 0.22f32).sin()) * H;
        assert!(intensity(Pattern::Waves, 0.0, line_y, W, H, t, 1.0) > 0.3);
        assert_eq!(intensity(Pattern::Waves, 0.0, line_y + 40.0, W, H, t, 1.0), 0.0);
    }

    #[test]
    fn swell_rises_from_the_bottom() {
        assert_eq!(intensity(Pattern::Swell, 300.0, 10.0, W, H, 7.0, 1.0), 0.0);
        assert!((intensity(Pattern::Swell, 300.0, H - 1.0, W, H, 7.0, 1.0) - 1.2).abs() < 1e-4);
    }

    #[test]
    fn drift_stays_in_range_and_varies() {
        let samples: Vec<f32> = (0..400).map(|i| intensity(Pattern::Drift, (i % 20) as f32 * 64.0, (i / 20) as f32 * 40.0, W, H, 7.0, 1.0)).collect();
        assert!(samples.iter().all(|k| (0.0..=3.0).contains(k)));
        assert!(samples.contains(&0.0) && samples.iter().any(|&k| k > 1.0), "patches with clear ground between them");
    }

    #[test]
    fn motion_rests_while_typing_and_resumes() {
        let mut m = Motion::default();
        m.advance(0.2, None);
        assert!((m.clock - 7.2).abs() < 1e-5);
        // Typing steadily: at rest within about a second and a half.
        for _ in 0..24 {
            m.advance(0.066, Some(0.1));
        }
        assert!(m.is_resting());
        let c = m.clock;
        m.advance(0.066, Some(1.2));
        assert!(m.clock - c < 0.01);
        for _ in 0..70 {
            m.advance(0.066, Some(5.0));
        }
        assert!(!m.is_resting() && m.calm < 0.05);
    }
}
