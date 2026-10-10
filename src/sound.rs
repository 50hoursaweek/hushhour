//! Gentle key sounds and background music, mixed in one thread and played through ALSA.
//!
//! `libasound` is loaded at runtime with `libloading` rather than linked, so the
//! binary builds without ALSA headers and still runs on systems without it (the
//! sound is simply unavailable). On PipeWire and PulseAudio desktops the ALSA
//! `default` device is routed through the sound server.

use std::ffi::{CString, c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::time::{Duration, Instant};

use libloading::Library;
use libloading::os::unix::Symbol;

use crate::keys::Voice as KeyVoice;
use crate::music::{Music, MusicDirs, Source};
use crate::prefs::SoundSet;

pub use crate::keys::Key;

pub const RATE: u32 = 44_100;
const CHANNELS: usize = 2;
/// Frames per write; the blocking write paces the mixer thread (5 ms).
const BLOCK: usize = 220;
/// Requested device latency.
const LATENCY_US: u32 = 20_000;
/// Close the device after this much silence so it is not held open while idle.
const IDLE_CLOSE: Duration = Duration::from_secs(3);

struct Play {
    key: Key,
    set: SoundSet,
    /// 0..=1, as set in the preferences.
    volume: f32,
}

enum Command {
    Key(Play),
    Music(Music),
    MusicVolume(f32),
}

/// Handle owned by the application; dropping it stops the mixer thread.
pub struct Sound {
    tx: Sender<Command>,
}

impl Sound {
    /// Start the mixer thread. If audio turns out to be unavailable, requests are ignored.
    /// `dirs` is where tracks are looked up.
    pub fn start(dirs: MusicDirs) -> Self {
        let (tx, rx) = mpsc::channel::<Command>();
        std::thread::Builder::new()
            .name("inkpad-sound".into())
            .spawn(move || mixer(rx, dirs))
            .expect("spawn sound thread");
        Self { tx }
    }

    pub fn play(&self, key: Key, set: SoundSet, volume: f32) {
        if volume > 0.0 {
            let _ = self.tx.send(Command::Key(Play { key, set, volume }));
        }
    }

    /// Switch the background music, cross-fading from whatever plays now.
    pub fn set_music(&self, music: Music) {
        let _ = self.tx.send(Command::Music(music));
    }

    pub fn set_music_volume(&self, volume: f32) {
        let _ = self.tx.send(Command::MusicVolume(volume));
    }
}

/// Small xorshift generator, enough to vary the sounds and to make noise.
pub struct Rng(u64);

impl Rng {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Uniform in `lo..hi`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// White noise sample in -1..1.
    pub fn noise(&mut self) -> f32 {
        self.range(-1.0, 1.0)
    }
}

/// A music source with its current gain, ramped per block for click-free fades.
struct Layer {
    source: Box<dyn Source>,
    gain: f32,
    target: f32,
    /// Gain change per block while fading in and out.
    in_step: f32,
    out_step: f32,
}

impl Layer {
    fn new(source: Box<dyn Source>, target: f32) -> Self {
        let (fade_in, fade_out) = source.fade_seconds();
        let blocks_per_second = RATE as f32 / BLOCK as f32;
        Self {
            source,
            gain: 0.0,
            target,
            in_step: 1.0 / (fade_in * blocks_per_second),
            out_step: 1.0 / (fade_out * blocks_per_second),
        }
    }

    /// Advance the fade; false once a fade-out has completed.
    fn step(&mut self) -> bool {
        if self.gain < self.target {
            self.gain = (self.gain + self.in_step).min(self.target);
        } else if self.gain > self.target {
            self.gain = (self.gain - self.out_step).max(self.target);
        }
        self.target > 0.0 || self.gain > 0.0
    }
}

fn mixer(rx: mpsc::Receiver<Command>, dirs: MusicDirs) {
    let mut rng = Rng::new();
    // One second of white noise, built once and read from random offsets by key sounds.
    let noise: Vec<f32> = (0..RATE as usize).map(|_| rng.noise()).collect();
    let mut voices: Vec<KeyVoice> = Vec::new();
    let mut block = vec![0f32; BLOCK * CHANNELS];
    let mut music_block = vec![0f32; BLOCK * CHANNELS];
    let mut pcm = vec![0i16; BLOCK * CHANNELS];
    let mut device: Option<Alsa> = None;
    let mut last_activity = Instant::now();
    // The playing layer plus any layers still fading out.
    let mut layers: Vec<Layer> = Vec::new();
    let mut music_volume = 0.5f32;

    loop {
        // Block for work while the device is closed; otherwise the write below paces us.
        let next = if device.is_none() {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => return,
            }
        } else {
            match rx.try_recv() {
                Ok(cmd) => Some(cmd),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        };

        let mut play = None;
        match next {
            Some(Command::Key(p)) => play = Some(p),
            Some(Command::Music(music)) => {
                for layer in &mut layers {
                    layer.target = 0.0;
                }
                if let Some(source) = music.source(&dirs) {
                    layers.push(Layer::new(source, music_volume * music_volume));
                }
                last_activity = Instant::now();
            }
            Some(Command::MusicVolume(v)) => {
                music_volume = v.clamp(0.0, 1.0);
                if let Some(layer) = layers.iter_mut().filter(|l| l.target > 0.0).last() {
                    layer.target = music_volume * music_volume;
                }
            }
            None => {}
        }

        if device.is_none() && !layers.is_empty() {
            device = Alsa::open();
            if device.is_none() {
                layers.clear();
            }
        }

        if let Some(play) = play {
            voices.push(KeyVoice::new(play.set, play.key, play.volume, &mut rng, noise.len()));
            last_activity = Instant::now();
            if device.is_none() {
                device = Alsa::open();
                if device.is_none() {
                    voices.clear();
                    continue;
                }
            }
        }

        let Some(alsa) = device.as_mut() else { continue };
        block.fill(0.0);
        // Music layers, each ramping linearly across the block towards its target gain.
        layers.retain_mut(|layer| {
            let from = layer.gain;
            let alive = layer.step();
            layer.source.fill(&mut music_block);
            let frames = BLOCK as f32;
            for (i, frame) in music_block.chunks(CHANNELS).enumerate() {
                let g = from + (layer.gain - from) * i as f32 / frames;
                block[i * CHANNELS] += frame[0] * g;
                block[i * CHANNELS + 1] += frame[1] * g;
            }
            alive
        });
        // Key sounds, synthesised on the fly, mono, placed in the centre.
        for voice in &mut voices {
            for out in block.chunks_mut(CHANNELS) {
                let s = voice.render(&noise);
                out[0] += s;
                out[1] += s;
            }
        }
        voices.retain(|v| !v.finished());
        for (dst, src) in pcm.iter_mut().zip(&block) {
            *dst = (src.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        }
        if !alsa.write(&pcm) {
            device = None;
            voices.clear();
            layers.clear();
            continue;
        }
        if voices.is_empty() && layers.is_empty() && last_activity.elapsed() > IDLE_CLOSE {
            device = None;
        }
    }
}

// The handful of libasound entry points we use; signatures from <alsa/pcm.h>.
type PcmOpen = unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int, c_int) -> c_int;
type PcmSetParams = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int;
type PcmWritei = unsafe extern "C" fn(*mut c_void, *const c_void, c_ulong) -> c_long;
type PcmRecover = unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int;
type PcmClose = unsafe extern "C" fn(*mut c_void) -> c_int;

const SND_PCM_STREAM_PLAYBACK: c_int = 0;
const SND_PCM_FORMAT_S16_LE: c_int = 2;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

/// An open playback stream on the default ALSA device.
struct Alsa {
    pcm: *mut c_void,
    writei: Symbol<PcmWritei>,
    recover: Symbol<PcmRecover>,
    close: Symbol<PcmClose>,
    /// Keeps the symbols above valid.
    _lib: Library,
}

impl Alsa {
    fn open() -> Option<Self> {
        // SAFETY: loading libasound runs its initialisers; it is a well-behaved system library.
        let lib = ["libasound.so.2", "libasound.so"]
            .iter()
            .find_map(|name| unsafe { Library::new(*name) }.ok())?;
        // SAFETY: the symbol names and signatures match the ALSA C API.
        let (open, set_params, writei, recover, close) = unsafe {
            let open = lib.get::<PcmOpen>(b"snd_pcm_open\0").ok()?.into_raw();
            let set_params = lib.get::<PcmSetParams>(b"snd_pcm_set_params\0").ok()?.into_raw();
            let writei = lib.get::<PcmWritei>(b"snd_pcm_writei\0").ok()?.into_raw();
            let recover = lib.get::<PcmRecover>(b"snd_pcm_recover\0").ok()?.into_raw();
            let close = lib.get::<PcmClose>(b"snd_pcm_close\0").ok()?.into_raw();
            (open, set_params, writei, recover, close)
        };
        let name = CString::new("default").ok()?;
        let mut pcm: *mut c_void = std::ptr::null_mut();
        // SAFETY: plain FFI calls with valid pointers; `pcm` is only used if open succeeded.
        unsafe {
            if open(&mut pcm, name.as_ptr(), SND_PCM_STREAM_PLAYBACK, 0) < 0 || pcm.is_null() {
                return None;
            }
            let err = set_params(
                pcm,
                SND_PCM_FORMAT_S16_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                CHANNELS as c_uint,
                RATE,
                1,
                LATENCY_US,
            );
            if err < 0 {
                close(pcm);
                return None;
            }
        }
        Some(Self { pcm, writei, recover, close, _lib: lib })
    }

    /// Write one block of interleaved stereo, recovering once from an underrun. False means the device is gone.
    fn write(&mut self, samples: &[i16]) -> bool {
        let frames = (samples.len() / CHANNELS) as c_ulong;
        for _ in 0..2 {
            // SAFETY: `pcm` is open and `samples` outlives the call.
            let n = unsafe { (self.writei)(self.pcm, samples.as_ptr().cast(), frames) };
            if n >= 0 {
                return true;
            }
            // SAFETY: recover expects the negative errno returned by writei.
            if unsafe { (self.recover)(self.pcm, n as c_int, 1) } < 0 {
                return false;
            }
        }
        false
    }
}

impl Drop for Alsa {
    fn drop(&mut self) {
        // SAFETY: `pcm` was opened by `open` and is closed exactly once.
        unsafe { (self.close)(self.pcm) };
    }
}

// SAFETY: the stream handle is only ever used from the mixer thread.
unsafe impl Send for Alsa {}
