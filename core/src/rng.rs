//! A small random source for the sounds; not for anything that needs real randomness.

/// Small xorshift generator, enough to vary the sounds and to make noise.
pub struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

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
