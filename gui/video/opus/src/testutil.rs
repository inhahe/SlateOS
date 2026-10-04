//! What the unit tests that hold functions to libopus share with the C
//! programs that digested libopus's results (`tools/mathops.c`,
//! `tools/functions.c`): their generator, drawn in the same order; their
//! digest; and their signals, made of integers so that both languages make
//! the same ones.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "test helpers mirroring C's casts: each wrap is the C program's"
)]

/// The C programs' xorshift generator: 32 bits a draw, as `rnd()`.
pub(crate) struct Xorshift(pub(crate) u64);

impl Xorshift {
    pub(crate) fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }

    /// A draw below `n`, as `rnd() % n`.
    pub(crate) fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    /// 16 random bits as a signed sample shifted right by `shift`, as
    /// `(short)(rnd() & 0xffff) >> shift`.
    pub(crate) fn sample(&mut self, shift: u32) -> i32 {
        i32::from((self.next() & 0xffff) as u16 as i16) >> shift
    }
}

/// Results digested as the C programs digest them: FNV-1a, 64 bits, over
/// each as 4 little-endian bytes.
pub(crate) struct Digest {
    pub(crate) hash: u64,
    pub(crate) count: usize,
}

impl Digest {
    pub(crate) fn new() -> Self {
        Self {
            hash: 0xCBF2_9CE4_8422_2325,
            count: 0,
        }
    }

    pub(crate) fn add(&mut self, v: i32) {
        for b in v.to_le_bytes() {
            self.hash = (self.hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
        }
        self.count += 1;
    }

    /// The digest against libopus's: `Err` saying both if they differ.
    pub(crate) fn check(&self, name: &str, count: usize, hash: u64) -> Result<(), String> {
        if (self.count, self.hash) == (count, hash) {
            Ok(())
        } else {
            Err(format!(
                "{name}: {} results {:016x}, libopus {count} {hash:016x}",
                self.count, self.hash
            ))
        }
    }
}

/// `tone`: an integer oscillator, `y[n] = 2c y[n-1] / 2^14 - y[n-2]`
/// clipped to 16 bits.
pub(crate) fn tone(out: &mut [i32], c: i32, amp: i32) {
    let (mut y2, mut y1) = (0i32, amp);
    for v in out {
        let y = (((2 * c * y1) >> 14) - y2).clamp(-32768, 32767);
        *v = y;
        y2 = y1;
        y1 = y;
    }
}

/// `signal`: noise, a tone, two tones and noise, silence, resonant noise,
/// impulses, a square wave, or the Nyquist tone.
pub(crate) fn signal(rng: &mut Xorshift, out: &mut [i32]) {
    match rng.below(8) {
        0 => {
            let shift = rng.below(16);
            for v in out.iter_mut() {
                *v = rng.sample(shift);
            }
        }
        1 => {
            let c = rng.below(16384) as i32;
            let amp = rng.below(32768) as i32;
            tone(out, c, amp);
        }
        2 => {
            let c1 = rng.below(16384) as i32;
            let c2 = rng.below(16384) as i32;
            let mut other = vec![0i32; out.len()];
            tone(out, c1, 12000);
            tone(&mut other, c2, 8000);
            let shift = rng.below(16);
            for (v, o) in out.iter_mut().zip(&other) {
                let noise = rng.sample(shift + 4);
                // C's division truncates toward zero, as Rust's does.
                *v = (*v / 2 + o / 2 + noise).clamp(-32768, 32767);
            }
        }
        3 => out.fill(0),
        4 => {
            // Quiet noise through three resonators at one frequency, poles
            // just inside the unit circle.
            let cosw = i64::from(rng.below(16384));
            let r = 16383 - i64::from(rng.below(64));
            let (a1, a2) = ((2 * r * cosw) >> 14, (r * r) >> 14);
            let mut y = [[0i64; 2]; 3];
            for o in out.iter_mut() {
                let mut v = i64::from(rng.sample(8));
                for st in &mut y {
                    let w = (v + ((a1 * st[0]) >> 14) - ((a2 * st[1]) >> 14))
                        .clamp(-(1 << 24), 1 << 24);
                    st[1] = st[0];
                    st[0] = w;
                    v = w;
                }
                *o = (v >> 8).clamp(-32768, 32767) as i32;
            }
        }
        5 => {
            let period = 20 + rng.below(300) as usize;
            let amp = rng.below(32768) as i32;
            for (i, o) in out.iter_mut().enumerate() {
                *o = if i % period == 0 { amp } else { 0 };
            }
        }
        6 => {
            let half = 1 + rng.below(100) as usize;
            let amp = rng.below(32768) as i32;
            for (i, o) in out.iter_mut().enumerate() {
                *o = if (i / half) & 1 != 0 { amp } else { -amp };
            }
        }
        _ => {
            let amp = rng.below(32768) as i32;
            for (i, o) in out.iter_mut().enumerate() {
                *o = if i & 1 != 0 { amp } else { -amp };
            }
        }
    }
}
