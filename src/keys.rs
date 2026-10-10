//! Key sounds synthesised per press: nothing is recorded or stored.
//!
//! A press is a handful of layers (noise bursts, tones, a filter sweep), each
//! with its own delay, decay and gain, rendered sample by sample in the mixer.
//! Filter frequencies vary ±10 % and gains ±20 % per press so runs of keys
//! do not sound mechanical.

use std::f32::consts::TAU;

use crate::dsp::{BandPass, LowPass, SR, decay};
use crate::prefs::SoundSet;
use crate::sound::Rng;

/// What kind of key was pressed; each gets a slightly different sound.
#[derive(Debug, Clone, Copy)]
pub enum Key {
    Character,
    Enter,
    Backspace,
}

enum Kind {
    /// White noise through a band-pass, read from the shared noise table at `pos`.
    Noise { pos: usize, filter: BandPass },
    /// Noise through a low-pass that sweeps from one cutoff to another over `sweep` seconds.
    Sweep { pos: usize, filter: LowPass, from_hz: f32, to_hz: f32, sweep: f32 },
    /// A sine or triangle tone.
    Tone { phase: f32, freq_hz: f32, triangle: bool },
}

struct Layer {
    kind: Kind,
    /// Seconds before this layer starts.
    delay: f32,
    /// Decay time constant in seconds; the layer ends after about five of them.
    tau: f32,
    gain: f32,
    /// Samples rendered so far, counting from the end of the delay.
    i: usize,
}

impl Layer {
    fn length(&self) -> usize {
        (self.tau * 5.0 * SR) as usize
    }

    fn render(&mut self, noise: &[f32]) -> f32 {
        let i = self.i;
        self.i += 1;
        let env = decay(i, self.tau) * self.gain;
        match &mut self.kind {
            Kind::Noise { pos, filter } => {
                *pos = (*pos + 1) % noise.len();
                filter.run(noise[*pos]) * env
            }
            Kind::Sweep { pos, filter, from_hz, to_hz, sweep } => {
                let t = (i as f32 / (*sweep * SR)).min(1.0);
                filter.set(*from_hz + (*to_hz - *from_hz) * t);
                *pos = (*pos + 1) % noise.len();
                filter.run(noise[*pos]) * env
            }
            Kind::Tone { phase, freq_hz, triangle } => {
                *phase = (*phase + *freq_hz / SR) % 1.0;
                let s = if *triangle { 1.0 - 4.0 * (*phase - 0.5).abs() } else { (*phase * TAU).sin() };
                s * env
            }
        }
    }
}

/// One key press being played.
pub struct Voice {
    layers: Vec<Layer>,
    /// Samples since the press.
    age: usize,
}

impl Voice {
    /// Build the layers for a press of `key` in `set`.
    pub fn new(set: SoundSet, key: Key, volume: f32, rng: &mut Rng, noise_len: usize) -> Self {
        let mut b = Builder { rng, noise_len, layers: Vec::new(), master: 0.22 * volume * volume };
        match set {
            SoundSet::Soft => b.soft(key),
            SoundSet::Typewriter => b.typewriter(key),
        }
        Self { layers: b.layers, age: 0 }
    }

    pub fn finished(&self) -> bool {
        self.layers.is_empty()
    }

    /// Next sample, mono.
    pub fn render(&mut self, noise: &[f32]) -> f32 {
        let t = self.age as f32 / SR;
        self.age += 1;
        let mut out = 0.0;
        self.layers.retain_mut(|layer| {
            if t < layer.delay {
                return true;
            }
            out += layer.render(noise);
            layer.i < layer.length()
        });
        out
    }
}

struct Builder<'a> {
    rng: &'a mut Rng,
    noise_len: usize,
    layers: Vec<Layer>,
    master: f32,
}

impl Builder<'_> {
    /// ±10 % on a frequency.
    fn hz(&mut self, hz: f32) -> f32 {
        hz * self.rng.range(0.9, 1.1)
    }

    /// ±20 % on a gain, times the master level.
    fn gain(&mut self, g: f32) -> f32 {
        g * self.rng.range(0.8, 1.2) * self.master
    }

    fn pos(&mut self) -> usize {
        (self.rng.next_u64() % self.noise_len as u64) as usize
    }

    fn noise(&mut self, centre_hz: f32, tau: f32, gain: f32, delay: f32) {
        let centre = self.hz(centre_hz);
        let layer = Layer {
            kind: Kind::Noise { pos: self.pos(), filter: BandPass::around(centre) },
            delay,
            tau,
            gain: self.gain(gain),
            i: 0,
        };
        self.layers.push(layer);
    }

    fn tone(&mut self, freq_hz: f32, triangle: bool, tau: f32, gain: f32, delay: f32) {
        let freq_hz = self.hz(freq_hz);
        let layer = Layer { kind: Kind::Tone { phase: 0.0, freq_hz, triangle }, delay, tau, gain: self.gain(gain), i: 0 };
        self.layers.push(layer);
    }

    fn sweep(&mut self, from_hz: f32, to_hz: f32, sweep: f32, tau: f32, gain: f32, delay: f32) {
        let layer = Layer {
            kind: Kind::Sweep { pos: self.pos(), filter: LowPass::new(from_hz), from_hz: self.hz(from_hz), to_hz: self.hz(to_hz), sweep },
            delay,
            tau,
            gain: self.gain(gain),
            i: 0,
        };
        self.layers.push(layer);
    }

    /// ~40 ms: a band-passed noise burst around 1.5 kHz and a faint 150 Hz thock.
    fn soft(&mut self, key: Key) {
        let (centre, gain) = match key {
            Key::Character => (1500.0, 1.0),
            Key::Enter => (1200.0, 1.1),
            Key::Backspace => (1000.0, 0.8),
        };
        self.noise(centre, 0.008, gain, 0.0);
        self.tone(150.0, false, 0.007, 0.6, 0.0);
    }

    /// ~120 ms: a clack, a body resonance and a second tick; Enter rings a bell
    /// and slides the carriage, Backspace is duller.
    fn typewriter(&mut self, key: Key) {
        match key {
            Key::Enter => {
                self.noise(3000.0, 0.003, 0.7, 0.0);
                self.tone(2500.0, false, 0.06, 0.35, 0.0);
                self.sweep(2000.0, 500.0, 0.2, 0.08, 0.5, 0.04);
            }
            Key::Character | Key::Backspace => {
                let dull = matches!(key, Key::Backspace);
                let centre = if dull { 2000.0 } else { 3000.0 };
                let gain = if dull { 0.8 } else { 1.0 };
                self.noise(centre, 0.003, gain, 0.0);
                self.tone(500.0, true, 0.012, 0.35, 0.0);
                self.noise(centre, 0.003, 0.5 * gain, 0.025);
            }
        }
    }
}
