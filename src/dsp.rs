//! Small building blocks shared by the key sounds and the soundscapes.

use std::f32::consts::TAU;

use crate::sound::RATE;

pub const SR: f32 = RATE as f32;

/// One-pole low-pass filter.
#[derive(Clone, Copy)]
pub struct LowPass {
    a: f32,
    y: f32,
}

impl LowPass {
    pub fn new(cutoff_hz: f32) -> Self {
        Self { a: coefficient(cutoff_hz), y: 0.0 }
    }

    pub fn set(&mut self, cutoff_hz: f32) {
        self.a = coefficient(cutoff_hz);
    }

    pub fn run(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

pub fn coefficient(cutoff_hz: f32) -> f32 {
    1.0 - (-TAU * cutoff_hz / SR).exp()
}

/// Band-pass as the difference of two one-pole filters.
#[derive(Clone, Copy)]
pub struct BandPass {
    pub low: LowPass,
    high: LowPass,
}

impl BandPass {
    pub fn new(low_hz: f32, high_hz: f32) -> Self {
        Self { low: LowPass::new(high_hz), high: LowPass::new(low_hz) }
    }

    /// Centred on `centre_hz`, about one octave wide.
    pub fn around(centre_hz: f32) -> Self {
        Self::new(centre_hz / 1.4, centre_hz * 1.4)
    }

    pub fn run(&mut self, x: f32) -> f32 {
        let wide = self.low.run(x);
        wide - self.high.run(wide)
    }
}

/// A decaying exponential envelope in samples: 1 at the start, 1/e after `tau_s`.
pub fn decay(i: usize, tau_s: f32) -> f32 {
    (-(i as f32) / (tau_s * SR)).exp()
}
