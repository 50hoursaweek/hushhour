//! Background soundscapes for focus: generated ambiences that loop forever,
//! and the user's own tracks decoded from files.
//!
//! Everything renders stereo interleaved f32 at [`crate::sound::RATE`].

use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::dsp::{BandPass, LowPass, SR};
use crate::sound::Rng;

/// What plays in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Music {
    Off,
    Rain,
    Night,
    Train,
    Breeze,
    Drone,
    /// A track shipped with the app, by file name in the bundled music directory.
    Bundled(String),
    /// A file in the user's music directory, by file name.
    File(String),
}

impl Music {
    pub const BUILT_IN: [Self; 6] = [Self::Off, Self::Drone, Self::Rain, Self::Night, Self::Train, Self::Breeze];

    pub fn label(&self) -> String {
        match self {
            Self::Off => "Off".into(),
            Self::Rain => "Rainfall".into(),
            Self::Night => "Cricket night".into(),
            Self::Train => "Night train".into(),
            Self::Breeze => "Open window".into(),
            Self::Drone => "Focus drone".into(),
            Self::Bundled(name) | Self::File(name) => {
                name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name).to_owned()
            }
        }
    }

    /// Build the source for this choice; `None` for `Off` or an unreadable file.
    pub fn source(&self, dirs: &MusicDirs) -> Option<Box<dyn Source>> {
        Some(match self {
            Self::Off => return None,
            Self::Rain => Box::new(Rain::new()),
            Self::Night => Box::new(Night::new()),
            Self::Train => Box::new(Train::new()),
            Self::Breeze => Box::new(Breeze::new()),
            Self::Drone => Box::new(Drone::new()),
            Self::Bundled(name) => Box::new(FileSource::open(&dirs.bundled.as_ref()?.join(name))?),
            Self::File(name) => Box::new(FileSource::open(&dirs.user.join(name))?),
        })
    }
}

/// Where tracks are looked up.
#[derive(Debug, Clone)]
pub struct MusicDirs {
    /// Tracks shipped with the app, if found next to the executable or in the source tree.
    pub bundled: Option<PathBuf>,
    /// The user's own tracks.
    pub user: PathBuf,
}

impl MusicDirs {
    pub fn locate() -> Self {
        Self { bundled: bundled_music_dir(), user: music_dir().unwrap_or_else(|| PathBuf::from("music")) }
    }

    /// All tracks for the picker: bundled first, then the user's.
    pub fn choices(&self) -> Vec<Music> {
        let mut out: Vec<Music> =
            self.bundled.as_deref().map(scan_music_dir).unwrap_or_default().into_iter().map(Music::Bundled).collect();
        out.extend(scan_music_dir(&self.user).into_iter().map(Music::File));
        out
    }
}

/// The bundled tracks: `../share/hushhour/music` relative to the executable (the AppImage
/// layout), else `assets/music` in the source tree for development runs.
fn bundled_music_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidates = [
        exe.parent()?.join("../share/hushhour/music"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/music"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

impl fmt::Display for Music {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// Where the user's own tracks live: `$XDG_DATA_HOME/hushhour/music`. A directory left
/// from when the app was called Inkpad is used while the new one does not exist.
pub fn music_dir() -> Option<PathBuf> {
    let base = dirs::data_dir()?;
    let new = base.join("hushhour").join("music");
    let old = base.join("inkpad").join("music");
    Some(if !new.is_dir() && old.is_dir() { old } else { new })
}

/// File names of playable tracks in the music directory, sorted.
pub fn scan_music_dir(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            let ext = n.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            matches!(ext.as_str(), "mp3" | "ogg" | "oga" | "flac" | "wav")
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// A stereo audio generator. `fill` writes interleaved L/R frames.
pub trait Source: Send {
    fn fill(&mut self, out: &mut [f32]);

    /// Fade-in and fade-out times in seconds when this source starts or stops.
    fn fade_seconds(&self) -> (f32, f32) {
        (1.5, 1.5)
    }

    /// True once the source can produce nothing more (a file that failed); the
    /// mixer then drops it.
    fn finished(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Building blocks

/// Slow sine oscillator for modulation, in 0..=1.
struct Lfo {
    phase: f32,
    step: f32,
}

impl Lfo {
    fn new(rng: &mut Rng, period_s: f32) -> Self {
        Self { phase: rng.range(0.0, 1.0), step: 1.0 / (period_s * SR) }
    }

    fn next(&mut self) -> f32 {
        self.phase = (self.phase + self.step) % 1.0;
        0.5 + 0.5 * (self.phase * TAU).sin()
    }
}

/// A short decaying noise burst, used for drops, clacks and flutters.
struct Burst {
    filter: LowPass,
    env: f32,
    decay: f32,
    gain: f32,
    pan: f32,
}

impl Burst {
    fn new(cutoff_hz: f32, tau_s: f32, gain: f32, pan: f32) -> Self {
        Self { filter: LowPass::new(cutoff_hz), env: 1.0, decay: (-1.0 / (tau_s * SR)).exp(), gain, pan }
    }

    fn alive(&self) -> bool {
        self.env > 0.001
    }

    /// Add one frame of this burst to `l`/`r`.
    fn run(&mut self, rng: &mut Rng, l: &mut f32, r: &mut f32) {
        let s = self.filter.run(rng.noise()) * self.env * self.gain;
        self.env *= self.decay;
        *l += s * (1.0 - self.pan).sqrt();
        *r += s * self.pan.sqrt();
    }
}

// ---------------------------------------------------------------------------
// Rainfall with an occasional soft gong

struct Rain {
    rng: Rng,
    bed: [BandPass; 2],
    drops: Vec<Burst>,
    gong: Gong,
    until_gong: f32,
}

struct Gong {
    /// (phase, frequency, amplitude, decay per sample)
    partials: Vec<(f32, f32, f32, f32)>,
    attack: f32,
}

impl Gong {
    fn strike(rng: &mut Rng) -> Self {
        let base = rng.range(170.0, 215.0);
        let partials = [1.0, 1.58, 2.19, 3.02, 4.31, 5.6]
            .iter()
            .zip([1.0, 0.6, 0.45, 0.3, 0.18, 0.1])
            .zip([11.0, 8.0, 6.0, 4.5, 3.0, 2.0])
            .map(|((ratio, amp), tau): ((&f32, f32), f32)| (0.0, base * ratio, amp * 0.11, (-1.0 / (tau * SR)).exp()))
            .collect();
        Self { partials, attack: 0.0 }
    }

    fn run(&mut self) -> f32 {
        self.attack = (self.attack + 1.0 / (0.04 * SR)).min(1.0);
        let mut s = 0.0;
        for (phase, freq, amp, decay) in &mut self.partials {
            *phase = (*phase + *freq / SR) % 1.0;
            s += (*phase * TAU).sin() * *amp;
            *amp *= *decay;
        }
        s * self.attack
    }

    fn alive(&self) -> bool {
        self.partials.iter().any(|(_, _, amp, _)| *amp > 0.0002)
    }
}

impl Rain {
    fn new() -> Self {
        let mut rng = Rng::new();
        Self {
            bed: [BandPass::new(250.0, 1400.0), BandPass::new(250.0, 1400.0)],
            drops: Vec::new(),
            gong: Gong { partials: Vec::new(), attack: 0.0 },
            until_gong: rng.range(20.0, 50.0) * SR,
            rng,
        }
    }
}

impl Source for Rain {
    fn fill(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            let mut l = self.bed[0].run(self.rng.noise()) * 0.35;
            let mut r = self.bed[1].run(self.rng.noise()) * 0.35;
            // About 30 audible drops per second, spread across the stereo field.
            if self.rng.range(0.0, 1.0) < 30.0 / SR && self.drops.len() < 24 {
                let cutoff = self.rng.range(1800.0, 6000.0);
                let gain = self.rng.range(0.06, 0.2);
                self.drops.push(Burst::new(cutoff, self.rng.range(0.008, 0.03), gain, self.rng.range(0.0, 1.0)));
            }
            for drop in &mut self.drops {
                drop.run(&mut self.rng, &mut l, &mut r);
            }
            self.drops.retain(Burst::alive);
            self.until_gong -= 1.0;
            if self.until_gong <= 0.0 {
                self.gong = Gong::strike(&mut self.rng);
                self.until_gong = self.rng.range(45.0, 110.0) * SR;
            }
            if self.gong.alive() {
                let g = self.gong.run();
                l += g * 0.9;
                r += g;
            }
            frame[0] = l;
            frame[1] = r;
        }
    }
}

// ---------------------------------------------------------------------------
// Cricket night: crickets over a faint warm breeze

struct Cricket {
    carrier_hz: f32,
    chirp_hz: f32,
    phase: f32,
    chirp_phase: f32,
    pan: f32,
    gain: f32,
    /// Remaining frames in the current burst (singing) or rest (silent).
    remaining: f32,
    singing: bool,
    env: f32,
}

impl Cricket {
    fn new(rng: &mut Rng) -> Self {
        Self {
            carrier_hz: rng.range(3600.0, 5200.0),
            chirp_hz: rng.range(18.0, 34.0),
            phase: 0.0,
            chirp_phase: rng.range(0.0, 1.0),
            pan: rng.range(0.1, 0.9),
            gain: rng.range(0.012, 0.03),
            remaining: rng.range(0.5, 4.0) * SR,
            singing: false,
            env: 0.0,
        }
    }

    fn run(&mut self, rng: &mut Rng) -> (f32, f32) {
        self.remaining -= 1.0;
        if self.remaining <= 0.0 {
            self.singing = !self.singing;
            self.remaining = if self.singing { rng.range(0.6, 2.5) } else { rng.range(0.8, 5.0) } * SR;
        }
        let target = if self.singing { 1.0 } else { 0.0 };
        self.env += (target - self.env) * 0.0008;
        self.phase = (self.phase + self.carrier_hz / SR) % 1.0;
        self.chirp_phase = (self.chirp_phase + self.chirp_hz / SR) % 1.0;
        // Each chirp is a sharp pulse of the carrier.
        let pulse = (0.5 + 0.5 * (self.chirp_phase * TAU).sin()).powi(6);
        let s = (self.phase * TAU).sin() * pulse * self.env * self.gain;
        (s * (1.0 - self.pan).sqrt(), s * self.pan.sqrt())
    }
}

struct Night {
    rng: Rng,
    crickets: Vec<Cricket>,
    wind: [LowPass; 2],
    brown: [f32; 2],
    swell: Lfo,
}

impl Night {
    fn new() -> Self {
        let mut rng = Rng::new();
        let crickets = (0..6).map(|_| Cricket::new(&mut rng)).collect();
        Self {
            crickets,
            wind: [LowPass::new(220.0), LowPass::new(220.0)],
            brown: [0.0; 2],
            swell: Lfo::new(&mut rng, 13.0),
            rng,
        }
    }
}

impl Source for Night {
    fn fill(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            let swell = 0.5 + 0.5 * self.swell.next();
            let mut lr = [0.0f32; 2];
            for (ch, sample) in lr.iter_mut().enumerate() {
                // Leaky integration of white noise gives a brown "wind" rumble.
                self.brown[ch] = (self.brown[ch] + 0.02 * self.rng.noise()) * 0.995;
                *sample = self.wind[ch].run(self.brown[ch]) * 0.9 * swell;
            }
            for cricket in &mut self.crickets {
                let (l, r) = cricket.run(&mut self.rng);
                lr[0] += l;
                lr[1] += r;
            }
            frame[0] = lr[0];
            frame[1] = lr[1];
        }
    }
}

// ---------------------------------------------------------------------------
// Night train: rumble, hiss and the clickety-clack of the rails

struct Train {
    rng: Rng,
    rumble: [LowPass; 2],
    brown: [f32; 2],
    hiss: [LowPass; 2],
    sway: Lfo,
    /// Frames until the next joint; the pattern is two bogies per period.
    until_clack: f32,
    period: f32,
    step: usize,
    clacks: Vec<Burst>,
}

impl Train {
    fn new() -> Self {
        let mut rng = Rng::new();
        Self {
            rumble: [LowPass::new(140.0), LowPass::new(140.0)],
            brown: [0.0; 2],
            hiss: [LowPass::new(2500.0), LowPass::new(2500.0)],
            sway: Lfo::new(&mut rng, 7.0),
            until_clack: SR,
            period: 1.5 * SR,
            step: 0,
            clacks: Vec::new(),
            rng,
        }
    }
}

impl Source for Train {
    fn fill(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            let sway = 0.75 + 0.25 * self.sway.next();
            let mut lr = [0.0f32; 2];
            for (ch, sample) in lr.iter_mut().enumerate() {
                self.brown[ch] = (self.brown[ch] + 0.03 * self.rng.noise()) * 0.993;
                *sample = self.rumble[ch].run(self.brown[ch]) * 1.6 * sway + self.hiss[ch].run(self.rng.noise()) * 0.025;
            }
            self.until_clack -= 1.0;
            if self.until_clack <= 0.0 {
                // Steps: front bogie (two wheels), gap, rear bogie (two wheels), long gap.
                let gaps = [0.11, 0.5, 0.11, 0.28];
                let gap = gaps[self.step % 4] * self.period / 1.0;
                self.until_clack = gap;
                if self.step % 4 == 0 {
                    self.period *= self.rng.range(0.985, 1.015);
                    self.period = self.period.clamp(1.3 * SR, 1.7 * SR);
                }
                let pan = if self.step % 2 == 0 { 0.4 } else { 0.6 };
                let gain = self.rng.range(0.25, 0.45);
                self.clacks.push(Burst::new(self.rng.range(500.0, 900.0), 0.022, gain, pan));
                self.step += 1;
            }
            let [l, r] = &mut lr;
            for clack in &mut self.clacks {
                clack.run(&mut self.rng, l, r);
            }
            self.clacks.retain(Burst::alive);
            frame[0] = lr[0];
            frame[1] = lr[1];
        }
    }
}

// ---------------------------------------------------------------------------
// Open window: a breeze with the curtains stirring

struct Breeze {
    rng: Rng,
    bands: [BandPass; 2],
    cutoff: [Lfo; 2],
    swell: [Lfo; 2],
    until_flutter: f32,
    flutters: Vec<Burst>,
    flutter_left: u32,
    flutter_gap: f32,
}

impl Breeze {
    fn new() -> Self {
        let mut rng = Rng::new();
        Self {
            bands: [BandPass::new(120.0, 900.0), BandPass::new(120.0, 900.0)],
            cutoff: [Lfo::new(&mut rng, 11.0), Lfo::new(&mut rng, 17.0)],
            swell: [Lfo::new(&mut rng, 9.0), Lfo::new(&mut rng, 13.0)],
            until_flutter: rng.range(2.0, 6.0) * SR,
            flutters: Vec::new(),
            flutter_left: 0,
            flutter_gap: 0.0,
            rng,
        }
    }
}

impl Source for Breeze {
    fn fill(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            let mut lr = [0.0f32; 2];
            for (ch, sample) in lr.iter_mut().enumerate() {
                let cutoff = 400.0 + 1100.0 * self.cutoff[ch].next();
                self.bands[ch].low.set(cutoff);
                let swell = self.swell[ch].next().powi(2);
                *sample = self.bands[ch].run(self.rng.noise()) * (0.12 + 0.5 * swell);
            }
            // Fabric flutter: a few soft taps in quick succession, now and then.
            self.until_flutter -= 1.0;
            if self.until_flutter <= 0.0 {
                if self.flutter_left == 0 {
                    self.flutter_left = self.rng.range(3.0, 7.0) as u32;
                    self.flutter_gap = self.rng.range(0.05, 0.12) * SR;
                }
                let pan = self.rng.range(0.3, 0.7);
                self.flutters.push(Burst::new(self.rng.range(300.0, 700.0), 0.02, self.rng.range(0.05, 0.1), pan));
                self.flutter_left -= 1;
                self.until_flutter =
                    if self.flutter_left > 0 { self.flutter_gap } else { self.rng.range(3.0, 9.0) * SR };
            }
            let [l, r] = &mut lr;
            for flutter in &mut self.flutters {
                flutter.run(&mut self.rng, l, r);
            }
            self.flutters.retain(Burst::alive);
            frame[0] = lr[0];
            frame[1] = lr[1];
        }
    }
}

// ---------------------------------------------------------------------------
// Focus drone: a breathing pad on a low major chord, through a soft reverb

/// One pad voice: two oscillator slots cross-faded so the pitch can move and return.
struct DroneVoice {
    base_hz: f32,
    freq: [f32; 2],
    phase: [f32; 2],
    /// Weight of the second slot, gliding towards `target` over about three seconds.
    blend: f32,
    target: f32,
    triangle: bool,
    lfo: Lfo,
}

impl DroneVoice {
    fn new(rng: &mut Rng, base_hz: f32, triangle: bool) -> Self {
        // ±5 cents of detune keeps the chord alive.
        let hz = base_hz * 2f32.powf(rng.range(-5.0, 5.0) / 1200.0);
        let period = 1.0 / rng.range(0.05, 0.15);
        Self {
            base_hz: hz,
            freq: [hz, hz],
            phase: [rng.range(0.0, 1.0), rng.range(0.0, 1.0)],
            blend: 0.0,
            target: 0.0,
            triangle,
            lfo: Lfo::new(rng, period),
        }
    }

    fn run(&mut self) -> f32 {
        let step = 1.0 / (3.0 * SR);
        if self.blend < self.target {
            self.blend = (self.blend + step).min(self.target);
        } else if self.blend > self.target {
            self.blend = (self.blend - step).max(self.target);
        }
        let mut s = 0.0;
        for (slot, weight) in [(0usize, 1.0 - self.blend), (1, self.blend)] {
            if weight <= 0.0 {
                continue;
            }
            self.phase[slot] = (self.phase[slot] + self.freq[slot] / SR) % 1.0;
            let p = self.phase[slot];
            let osc = if self.triangle { 1.0 - 4.0 * (p - 0.5).abs() } else { (p * TAU).sin() };
            s += osc * weight;
        }
        s * (0.55 + 0.45 * self.lfo.next())
    }
}

/// A Schroeder reverb: parallel damped combs into two all-passes, per channel.
struct Reverb {
    combs: Vec<(Vec<f32>, usize, f32)>,
    allpasses: Vec<(Vec<f32>, usize)>,
}

impl Reverb {
    /// `offset` shifts the delay lengths so the two channels decorrelate.
    fn new(offset: usize) -> Self {
        let combs = [1116, 1188, 1277, 1356].iter().map(|d| (vec![0.0; d + offset], 0, 0.0)).collect();
        let allpasses = [556, 441].iter().map(|d| (vec![0.0; d + offset], 0)).collect();
        Self { combs, allpasses }
    }

    fn run(&mut self, x: f32) -> f32 {
        let mut out = 0.0;
        for (line, pos, damp) in &mut self.combs {
            let y = line[*pos];
            // Damping: high frequencies die faster, like a real room. Feedback 0.94 gives ~3 s.
            *damp += 0.3 * (y - *damp);
            line[*pos] = x + *damp * 0.94;
            *pos = (*pos + 1) % line.len();
            out += y;
        }
        let mut y = out * 0.25;
        for (line, pos) in &mut self.allpasses {
            let delayed = line[*pos];
            let v = y + delayed * 0.5;
            line[*pos] = v;
            *pos = (*pos + 1) % line.len();
            y = delayed - v * 0.5;
        }
        y
    }
}

struct Drone {
    rng: Rng,
    voices: Vec<DroneVoice>,
    brown: f32,
    brown_lp: LowPass,
    /// Two cascaded one-pole filters per channel, with their own states, for a steeper roll-off.
    master_lp: [[LowPass; 2]; 2],
    reverb: [Reverb; 2],
    /// Samples until the next pitch move; `moving` is the voice currently away from its root.
    until_move: f32,
    moving: Option<usize>,
}

impl Drone {
    fn new() -> Self {
        let mut rng = Rng::new();
        // C2 major: C2, E2, G2, C3.
        let chord = [65.41, 82.41, 98.0, 130.81];
        let voices = chord.iter().enumerate().map(|(i, hz)| DroneVoice::new(&mut rng, *hz, i % 2 == 0)).collect();
        Self {
            voices,
            brown: 0.0,
            brown_lp: LowPass::new(400.0),
            master_lp: [[LowPass::new(1200.0), LowPass::new(1200.0)], [LowPass::new(1200.0), LowPass::new(1200.0)]],
            reverb: [Reverb::new(0), Reverb::new(23)],
            until_move: rng.range(20.0, 40.0) * SR,
            moving: None,
            rng,
        }
    }
}

impl Source for Drone {
    fn fill(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            self.until_move -= 1.0;
            if self.until_move <= 0.0 {
                self.until_move = self.rng.range(20.0, 40.0) * SR;
                match self.moving.take() {
                    // Bring the moved voice back to its root.
                    Some(i) => self.voices[i].target = 0.0,
                    None => {
                        let i = (self.rng.next_u64() % self.voices.len() as u64) as usize;
                        let ratio = [6.0 / 5.0, 5.0 / 4.0, 3.0 / 2.0][(self.rng.next_u64() % 3) as usize];
                        let voice = &mut self.voices[i];
                        voice.freq[1] = voice.base_hz * ratio;
                        voice.target = 1.0;
                        self.moving = Some(i);
                    }
                }
            }
            let mut pad = 0.0;
            for voice in &mut self.voices {
                pad += voice.run() * 0.12;
            }
            self.brown = (self.brown + 0.02 * self.rng.noise()) * 0.995;
            // Brown noise at about 5 % of the pad level.
            let noise = self.brown_lp.run(self.brown) * 0.025;
            let dry = pad + noise;
            for ch in 0..2 {
                let once = self.master_lp[ch][0].run(dry);
                let filtered = self.master_lp[ch][1].run(once);
                let wet = self.reverb[ch].run(filtered);
                frame[ch] = (filtered * 0.6 + wet * 0.4) * 0.8;
            }
        }
    }

    fn fade_seconds(&self) -> (f32, f32) {
        (3.0, 2.0)
    }
}


// ---------------------------------------------------------------------------
// A file from the music directory, looped

/// -12 dB, applied to every file track.
const FILE_GAIN: f32 = 0.25;

/// Frames per chunk handed from the decoder thread to the mixer (about 90 ms at 44.1 kHz).
const CHUNK_FRAMES: usize = 4096;
/// Chunks the decoder may run ahead; bounds memory and keeps a track switch quick.
const CHUNK_QUEUE: usize = 8;
/// Decoded frames the mixer keeps ready on its side of the channel. Chunks are received
/// only while fewer than this are pending, so the channel's bound stays the decoder's
/// bound and the mixer never holds more than this plus one chunk. It covers over 40 ms of
/// output for any source rate up to 192 kHz, many times one 5 ms mixer block.
const PENDING_FRAMES: usize = 2 * CHUNK_FRAMES;
/// Consecutive undecodable packets before a file is given up on (not reset by looping).
const MAX_DECODE_ERRORS: usize = 100;
/// Passes over the file that yield no audio at all before it is given up on.
const MAX_EMPTY_PASSES: usize = 2;

enum Decoded {
    /// The file was probed: its sample rate, so the mixer can resample.
    Opened { sample_rate: u32 },
    Frames(Vec<[f32; 2]>),
    /// The file could not be read or decoded any further.
    Failed,
}

/// A file from the music directory, looped. Everything that touches the file,
/// probing included, runs on a decoder thread that hands bounded chunks to the
/// mixer; the mixer never blocks on it and plays silence until audio is ready.
/// Dropping the source cancels the thread.
pub struct FileSource {
    rx: std::sync::mpsc::Receiver<Decoded>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Set by the worker when it gives up, so failure is seen even behind queued frames.
    worker_failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Source frames per output frame; `None` until the decoder has probed the file.
    rate: Option<f32>,
    /// Decoded, interleaved stereo at the file's rate, waiting to be resampled.
    pending: VecDeque<[f32; 2]>,
    position: f32,
    last: [f32; 2],
    failed: bool,
}

impl FileSource {
    /// Returns at once; the decoder thread does the probing. A file that turns
    /// out to be unreadable makes the source `finished()` instead.
    pub fn open(path: &Path) -> Option<Self> {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Decoded>(CHUNK_QUEUE);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let failed_flag = worker_failed.clone();
        let path = path.to_owned();
        std::thread::Builder::new()
            .name("hushhour-decode".into())
            .spawn(move || decode_worker(&path, tx, &worker_cancel, &failed_flag))
            .ok()?;
        Some(Self {
            rx,
            cancel,
            worker_failed,
            rate: None,
            pending: VecDeque::new(),
            position: 0.0,
            last: [0.0; 2],
            failed: false,
        })
    }

    #[allow(clippy::type_complexity)]
    fn open_streams(path: &Path) -> Option<(Box<dyn FormatReader>, Box<dyn Decoder>, u32, usize, u32)> {
        if !std::fs::metadata(path).ok()?.is_file() {
            return None;
        }
        let file = File::open(path).ok()?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let probed = symphonia::default::get_probe()
            .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
            .ok()?;
        let reader = probed.format;
        let (track_id, params) = {
            let track = reader.default_track()?;
            (track.id, track.codec_params.clone())
        };
        let channels = params.channels?.count();
        let sample_rate = params.sample_rate?;
        let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).ok()?;
        Some((reader, decoder, track_id, channels, sample_rate))
    }

    /// Take what the decoder has ready, without waiting for it and without hoarding it.
    /// Chunks are received only while fewer than `PENDING_FRAMES` are pending, so the
    /// bounded channel keeps throttling the decoder however fast it runs, and at most
    /// `CHUNK_QUEUE + 1` messages are taken per call even if the decoder refills the
    /// channel meanwhile. Failure is signalled through a flag as well as the channel, so
    /// it is seen at once even when frames are queued ahead of it.
    fn pull(&mut self) {
        if self.failed {
            return;
        }
        if self.worker_failed.load(std::sync::atomic::Ordering::Relaxed) {
            self.fail();
            return;
        }
        let mut received = 0;
        while self.pending.len() < PENDING_FRAMES && received <= CHUNK_QUEUE {
            received += 1;
            match self.rx.try_recv() {
                Ok(Decoded::Opened { sample_rate }) => self.rate = Some(sample_rate as f32 / SR),
                Ok(Decoded::Frames(chunk)) => self.pending.extend(chunk),
                Ok(Decoded::Failed) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.fail();
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
            }
        }
    }

    /// Policy on failure: whatever was buffered is discarded and the source ends now.
    fn fail(&mut self) {
        self.failed = true;
        self.pending.clear();
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for FileSource {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Decode `path` over and over (the track loops), sending chunks until cancelled
/// or until the file proves unusable. Every packet and every pass checks the
/// cancellation flag, and a pass that produces no audio counts towards giving up.
/// Giving up raises `failed` before the `Failed` message, so the mixer learns of it
/// without first consuming whatever frames are still queued.
fn decode_worker(
    path: &Path,
    tx: std::sync::mpsc::SyncSender<Decoded>,
    cancel: &std::sync::atomic::AtomicBool,
    failed: &std::sync::atomic::AtomicBool,
) {
    use std::sync::atomic::Ordering;
    let cancelled = || cancel.load(Ordering::Relaxed);
    let give_up = || {
        failed.store(true, Ordering::Relaxed);
        let _ = tx.send(Decoded::Failed);
    };
    let mut chunk: Vec<[f32; 2]> = Vec::with_capacity(CHUNK_FRAMES);
    let mut buf: Option<SampleBuffer<f32>> = None;
    let mut errors = 0usize;
    let mut empty_passes = 0usize;
    let mut announced = false;
    loop {
        if cancelled() {
            return;
        }
        let Some((mut reader, mut decoder, track_id, channels, sample_rate)) = FileSource::open_streams(path) else {
            give_up();
            return;
        };
        if !announced {
            announced = true;
            if tx.send(Decoded::Opened { sample_rate }).is_err() {
                return;
            }
        }
        let mut frames_this_pass = 0usize;
        loop {
            if cancelled() {
                return;
            }
            let packet = match reader.next_packet() {
                Ok(p) => p,
                // End of file (symphonia reports it as an error) or a read failure:
                // either way this pass is over.
                Err(_) => break,
            };
            if packet.track_id() != track_id {
                continue;
            }
            let decoded = match decoder.decode(&packet) {
                Ok(d) => {
                    errors = 0;
                    d
                }
                Err(_) => {
                    errors += 1;
                    if errors > MAX_DECODE_ERRORS {
                        give_up();
                        return;
                    }
                    continue;
                }
            };
            let spec = *decoded.spec();
            let capacity = decoded.capacity() as u64;
            let sample_buf = match &mut buf {
                Some(b) if b.capacity() >= capacity as usize * channels => b,
                _ => buf.insert(SampleBuffer::<f32>::new(capacity, spec)),
            };
            sample_buf.copy_interleaved_ref(decoded);
            for frame in sample_buf.samples().chunks(channels) {
                let l = frame.first().copied().unwrap_or(0.0);
                let r = frame.get(1).copied().unwrap_or(l);
                chunk.push([l, r]);
                frames_this_pass += 1;
                if chunk.len() >= CHUNK_FRAMES {
                    // Blocks while the queue is full: that is the pacing. A dropped
                    // receiver ends the thread here; the flag covers the quiet stretches.
                    if tx.send(Decoded::Frames(std::mem::replace(&mut chunk, Vec::with_capacity(CHUNK_FRAMES)))).is_err() {
                        return;
                    }
                }
            }
        }
        if !chunk.is_empty() && tx.send(Decoded::Frames(std::mem::take(&mut chunk))).is_err() {
            return;
        }
        if frames_this_pass == 0 {
            // An empty or broken file: do not spin on it.
            empty_passes += 1;
            if empty_passes >= MAX_EMPTY_PASSES {
                give_up();
                return;
            }
        } else {
            empty_passes = 0;
        }
    }
}

impl Source for FileSource {
    fn fill(&mut self, out: &mut [f32]) {
        let rate = match self.rate {
            Some(rate) => rate,
            None => {
                // Not probed yet (or never will be): silence while we wait.
                self.pull();
                out.fill(0.0);
                return;
            }
        };
        self.pull();
        for frame in out.chunks_mut(2) {
            // Linear interpolation between the last consumed frame and the next pending one.
            let mut starved = false;
            while self.position >= 1.0 {
                match self.pending.pop_front() {
                    Some(next) => self.last = next,
                    None => {
                        starved = true;
                        break;
                    }
                }
                self.position -= 1.0;
            }
            if starved || self.failed {
                // Nothing ready: silence, and a clean restart of the interpolation.
                self.position = 0.0;
                self.last = [0.0; 2];
                frame[0] = 0.0;
                frame[1] = 0.0;
                continue;
            }
            let next = self.pending.front().copied().unwrap_or(self.last);
            let t = self.position;
            // Files are usually mastered loud (around -11 LUFS); pull them down 12 dB so
            // they sit at the level of the generated soundscapes on the same volume setting.
            frame[0] = (self.last[0] + (next[0] - self.last[0]) * t) * FILE_GAIN;
            frame[1] = (self.last[1] + (next[1] - self.last[1]) * t) * FILE_GAIN;
            self.position += rate;
        }
    }

    fn finished(&self) -> bool {
        self.failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::RATE;

    /// Every generator must produce a sane, finite signal at a modest level.
    #[test]
    fn generators_are_finite_and_moderate() {
        let dirs = MusicDirs { bundled: None, user: PathBuf::from("/nonexistent") };
        for music in &Music::BUILT_IN[1..] {
            let mut source = music.source(&dirs).expect("built-in source");
            let mut buf = vec![0f32; 2 * RATE as usize * 3];
            source.fill(&mut buf);
            let rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
            let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
            assert!(buf.iter().all(|s| s.is_finite()), "{music:?} produced NaN");
            assert!(peak <= 1.0, "{music:?} clips: peak {peak}");
            assert!((0.01..0.5).contains(&rms), "{music:?} level off: rms {rms}, peak {peak}");
        }
    }

    /// A short file loops seamlessly and is resampled to the output rate.
    #[test]
    fn file_source_loops_and_resamples() {
        let dir = std::env::temp_dir().join(format!("hushhour-music-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        // 0.5 s of a 220 Hz tone at half scale, as a minimal 16-bit mono WAV.
        let samples: Vec<i16> =
            (0..11_025).map(|i| ((i as f32 * 220.0 * TAU / 22_050.0).sin() * 0.5 * i16::MAX as f32) as i16).collect();
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&22_050u32.to_le_bytes());
        bytes.extend_from_slice(&(22_050u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for s in &samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();

        let mut source = FileSource::open(&path).expect("open wav");
        // Probing and decoding happen on a worker thread; give it a moment to fill the queue.
        std::thread::sleep(std::time::Duration::from_millis(300));
        // Pull 2 s at 44.1 kHz: four passes over the file.
        let mut buf = vec![0f32; 2 * RATE as usize * 2];
        for chunk in buf.chunks_mut(882) {
            source.fill(chunk);
            // Mimic real-time consumption so the decoder keeps ahead.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!source.finished());
        let rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
        assert!((rms - 0.5 / 2f32.sqrt() * FILE_GAIN).abs() < 0.01, "rms {rms}");
        // Both channels carry the mono signal.
        assert!(buf.chunks(2).all(|f| (f[0] - f[1]).abs() < 1e-6));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Audit F06: an empty WAV must make the worker give up (and a dropped source must
    /// cancel the thread), instead of reopening forever.
    #[test]
    fn empty_file_fails_and_a_dropped_source_cancels() {
        let dir = std::env::temp_dir().join(format!("hushhour-music-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.wav");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&36u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&(44_100u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();

        let mut source = FileSource::open(&path).expect("spawn");
        let mut buf = vec![0f32; 882];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !source.finished() && std::time::Instant::now() < deadline {
            source.fill(&mut buf);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(source.finished(), "an empty file must end the source");
        assert!(buf.iter().all(|s| *s == 0.0));

        // Dropping a source sets its cancellation flag; the worker sees it on its next check.
        let source = FileSource::open(&path).expect("spawn");
        let flag = source.cancel.clone();
        drop(source);
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A source fed by a test channel instead of a decoder thread.
    fn fed_source(capacity: usize) -> (std::sync::mpsc::SyncSender<Decoded>, FileSource) {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Decoded>(capacity);
        let source = FileSource {
            rx,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            worker_failed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            rate: Some(1.0),
            pending: VecDeque::new(),
            position: 0.0,
            last: [0.0; 2],
            failed: false,
        };
        (tx, source)
    }

    /// Audit F07: failure discards buffered frames and the source finishes at once,
    /// whether it arrives through the channel or, behind queued frames, through the flag.
    #[test]
    fn failure_discards_pending_frames_and_finishes() {
        let (tx, mut source) = fed_source(4);
        tx.send(Decoded::Frames(vec![[0.5, 0.5]; 100])).unwrap();
        tx.send(Decoded::Failed).unwrap();
        let mut out = vec![0f32; 2 * 50];
        source.fill(&mut out);
        assert!(source.finished());
        assert!(source.pending.is_empty());
        assert!(out.iter().all(|s| *s == 0.0));

        // Frames queued well past what one pull takes, then the worker gives up: the flag
        // makes the failure visible on the next block, not after the queue has drained.
        let (tx, mut source) = fed_source(16);
        for _ in 0..16 {
            tx.send(Decoded::Frames(vec![[0.5, 0.5]; CHUNK_FRAMES])).unwrap();
        }
        source.fill(&mut out);
        assert!(!source.finished());
        source.worker_failed.store(true, std::sync::atomic::Ordering::Relaxed);
        source.fill(&mut out);
        assert!(source.finished());
        assert!(source.pending.is_empty());
        assert!(out.iter().all(|s| *s == 0.0));
    }

    /// Verification V02: a decoder that runs ahead cannot make the mixer hoard audio.
    /// One fill takes a bounded number of chunks and leaves the rest in the channel, and
    /// under a producer that never sleeps the pending frames stay under the stated bound
    /// while audio keeps flowing.
    #[test]
    fn pending_frames_stay_bounded_under_a_fast_decoder() {
        // Deterministic half: a full channel, no producer.
        let (tx, mut source) = fed_source(64);
        for _ in 0..64 {
            tx.send(Decoded::Frames(vec![[0.1, 0.1]; CHUNK_FRAMES])).unwrap();
        }
        let mut out = vec![0f32; 2 * 220];
        source.fill(&mut out);
        // Two chunks were received; one block was played (the interpolator holds one frame back).
        assert!((PENDING_FRAMES - 220..=PENDING_FRAMES).contains(&source.pending.len()), "pending {}", source.pending.len());
        // Exactly two slots were freed: the channel is still throttling.
        assert!(tx.try_send(Decoded::Frames(Vec::new())).is_ok());
        assert!(tx.try_send(Decoded::Frames(Vec::new())).is_ok());
        assert!(matches!(tx.try_send(Decoded::Frames(Vec::new())), Err(std::sync::mpsc::TrySendError::Full(_))));

        // Racing half: the producer refills the channel as fast as it can.
        let (tx, mut source) = fed_source(CHUNK_QUEUE);
        let producer = std::thread::spawn(move || {
            while tx.send(Decoded::Frames(vec![[0.1, 0.1]; CHUNK_FRAMES])).is_ok() {}
        });
        let mut max_pending = 0;
        for _ in 0..500 {
            source.fill(&mut out);
            max_pending = max_pending.max(source.pending.len());
            assert!(
                source.pending.len() <= PENDING_FRAMES + CHUNK_FRAMES,
                "pending {} exceeds the bound",
                source.pending.len()
            );
        }
        assert!(max_pending >= PENDING_FRAMES - 220, "the mixer did keep audio ready");
        assert!(out.iter().all(|s| (s - 0.1 * FILE_GAIN).abs() < 1e-6), "audio kept flowing");
        drop(source);
        producer.join().unwrap();
    }

    #[test]
    fn labels_drop_extensions() {
        assert_eq!(Music::File("lofi.mp3".into()).label(), "lofi");
        assert_eq!(Music::Bundled("Meditations.ogg".into()).label(), "Meditations");
        assert_eq!(Music::Off.label(), "Off");
    }
}
