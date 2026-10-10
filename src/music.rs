//! Background soundscapes for focus: generated ambiences that loop forever,
//! binaural beats, and the user's own tracks decoded from files.
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

use crate::sound::{RATE, Rng};

const SR: f32 = RATE as f32;

/// What plays in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Music {
    Off,
    Rain,
    Night,
    Train,
    Breeze,
    BinauralCalm,
    BinauralFocus,
    /// A track shipped with the app, by file name in the bundled music directory.
    Bundled(String),
    /// A file in the user's music directory, by file name.
    File(String),
}

impl Music {
    pub const BUILT_IN: [Self; 7] = [
        Self::Off,
        Self::Rain,
        Self::Night,
        Self::Train,
        Self::Breeze,
        Self::BinauralCalm,
        Self::BinauralFocus,
    ];

    pub fn label(&self) -> String {
        match self {
            Self::Off => "Off".into(),
            Self::Rain => "Rainfall".into(),
            Self::Night => "Cricket night".into(),
            Self::Train => "Night train".into(),
            Self::Breeze => "Open window".into(),
            Self::BinauralCalm => "Alpha waves, 10 Hz (headphones)".into(),
            Self::BinauralFocus => "Beta waves, 15 Hz (headphones)".into(),
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
            Self::BinauralCalm => Box::new(Binaural::new(200.0, 10.0)),
            Self::BinauralFocus => Box::new(Binaural::new(220.0, 15.0)),
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

/// The bundled tracks: `../share/inkpad/music` relative to the executable (the AppImage
/// layout), else `assets/music` in the source tree for development runs.
fn bundled_music_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidates = [
        exe.parent()?.join("../share/inkpad/music"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/music"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

impl fmt::Display for Music {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// Where the user's own tracks live.
pub fn music_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("inkpad").join("music"))
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
}

// ---------------------------------------------------------------------------
// Building blocks

/// One-pole low-pass filter.
#[derive(Clone, Copy)]
struct LowPass {
    a: f32,
    y: f32,
}

impl LowPass {
    fn new(cutoff_hz: f32) -> Self {
        Self { a: coefficient(cutoff_hz), y: 0.0 }
    }

    fn set(&mut self, cutoff_hz: f32) {
        self.a = coefficient(cutoff_hz);
    }

    fn run(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

fn coefficient(cutoff_hz: f32) -> f32 {
    1.0 - (-TAU * cutoff_hz / SR).exp()
}

/// Band-pass as the difference of two one-pole filters.
#[derive(Clone, Copy)]
struct BandPass {
    low: LowPass,
    high: LowPass,
}

impl BandPass {
    fn new(low_hz: f32, high_hz: f32) -> Self {
        Self { low: LowPass::new(high_hz), high: LowPass::new(low_hz) }
    }

    fn run(&mut self, x: f32) -> f32 {
        let wide = self.low.run(x);
        wide - self.high.run(wide)
    }
}

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
// Binaural beats: a slightly different tone in each ear over a soft noise bed

struct Binaural {
    rng: Rng,
    phase: [f32; 2],
    freq: [f32; 2],
    bed: [LowPass; 2],
}

impl Binaural {
    fn new(carrier_hz: f32, beat_hz: f32) -> Self {
        Self {
            rng: Rng::new(),
            phase: [0.0; 2],
            freq: [carrier_hz, carrier_hz + beat_hz],
            bed: [LowPass::new(600.0), LowPass::new(600.0)],
        }
    }
}

impl Source for Binaural {
    fn fill(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            for ch in 0..2 {
                self.phase[ch] = (self.phase[ch] + self.freq[ch] / SR) % 1.0;
                let tone = (self.phase[ch] * TAU).sin() * 0.16;
                let bed = self.bed[ch].run(self.rng.noise()) * 0.05;
                frame[ch] = tone + bed;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// A file from the music directory, looped

pub struct FileSource {
    path: PathBuf,
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    channels: usize,
    /// Source frames per output frame; 1.0 when the file is at the output rate.
    rate: f32,
    /// Decoded, interleaved stereo at the file's rate, waiting to be resampled.
    pending: VecDeque<[f32; 2]>,
    position: f32,
    last: [f32; 2],
}

impl FileSource {
    pub fn open(path: &Path) -> Option<Self> {
        let (reader, decoder, track_id, channels, sample_rate) = Self::open_streams(path)?;
        Some(Self {
            path: path.to_owned(),
            reader,
            decoder,
            track_id,
            channels,
            rate: sample_rate as f32 / SR,
            pending: VecDeque::new(),
            position: 0.0,
            last: [0.0; 2],
        })
    }

    #[allow(clippy::type_complexity)]
    fn open_streams(path: &Path) -> Option<(Box<dyn FormatReader>, Box<dyn Decoder>, u32, usize, u32)> {
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

    /// Decode packets until at least `frames` are pending; loop at the end of the file.
    fn refill(&mut self, frames: usize) {
        let mut restarted = false;
        while self.pending.len() < frames {
            let packet = match self.reader.next_packet() {
                Ok(p) => p,
                Err(_) => {
                    if restarted {
                        return;
                    }
                    // End of file (or a read error): reopen from the start to loop.
                    let Some((reader, decoder, track_id, channels, rate)) = Self::open_streams(&self.path) else {
                        return;
                    };
                    self.reader = reader;
                    self.decoder = decoder;
                    self.track_id = track_id;
                    self.channels = channels;
                    self.rate = rate as f32 / SR;
                    restarted = true;
                    continue;
                }
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let Ok(decoded) = self.decoder.decode(&packet) else { continue };
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            buf.copy_interleaved_ref(decoded);
            for frame in buf.samples().chunks(self.channels) {
                let l = frame.first().copied().unwrap_or(0.0);
                let r = frame.get(1).copied().unwrap_or(l);
                self.pending.push_back([l, r]);
            }
        }
    }
}

impl Source for FileSource {
    fn fill(&mut self, out: &mut [f32]) {
        let needed = (out.len() / 2) as f32 * self.rate + 2.0;
        self.refill(needed as usize + 1);
        for frame in out.chunks_mut(2) {
            // Linear interpolation between the last consumed frame and the next pending one.
            while self.position >= 1.0 {
                match self.pending.pop_front() {
                    Some(next) => self.last = next,
                    None => break,
                }
                self.position -= 1.0;
            }
            let next = self.pending.front().copied().unwrap_or(self.last);
            let t = self.position;
            frame[0] = self.last[0] + (next[0] - self.last[0]) * t;
            frame[1] = self.last[1] + (next[1] - self.last[1]) * t;
            self.position += self.rate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let dir = std::env::temp_dir().join(format!("inkpad-music-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 22_050, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..11_025 {
            // 0.5 s of a 220 Hz tone at half scale.
            let v = (i as f32 * 220.0 * TAU / 22_050.0).sin() * 0.5;
            writer.write_sample((v * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();

        let mut source = FileSource::open(&path).expect("open wav");
        // Pull 2 s at 44.1 kHz: four passes over the file.
        let mut buf = vec![0f32; 2 * RATE as usize * 2];
        for chunk in buf.chunks_mut(882) {
            source.fill(chunk);
        }
        let rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
        assert!((rms - 0.5 / 2f32.sqrt()).abs() < 0.03, "rms {rms}");
        // Both channels carry the mono signal.
        assert!(buf.chunks(2).all(|f| (f[0] - f[1]).abs() < 1e-6));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn labels_drop_extensions() {
        assert_eq!(Music::File("lofi.mp3".into()).label(), "lofi");
        assert_eq!(Music::Bundled("Meditations.ogg".into()).label(), "Meditations");
        assert_eq!(Music::Off.label(), "Off");
    }
}
