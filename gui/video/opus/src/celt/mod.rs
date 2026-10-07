//! CELT (RFC 6716 §4.3): Opus's transform layer, for music and for the
//! higher frequencies of a hybrid frame -- band energies, then each band's
//! shape as a PVQ codeword, an inverse MDCT, and a pitch post-filter.
//!
//! This is libopus 1.5.2's fixed-point decoder (`FIXED_POINT`), translated:
//! integer arithmetic throughout, so that the samples it decodes are the
//! same on every machine and the same as libopus's.

pub(crate) mod bands;
pub(crate) mod cwrs;
pub(crate) mod decoder;
pub(crate) mod energy;
pub(crate) mod kiss_fft;
pub(crate) mod lpc;
pub(crate) mod mathops;
pub(crate) mod mdct;
pub(crate) mod mode;
pub(crate) mod pitch;
pub(crate) mod pvq_table;
pub(crate) mod rate;
pub(crate) mod tables;
pub(crate) mod vq;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod reference {
    //! CELT's functions held to libopus's own: each over every value of a
    //! 16-bit domain, or hundreds of thousands of inputs drawn at random, its
    //! results digested and the digest compared with the one a C program
    //! makes of libopus 1.5.2's fixed-point build's results over the same
    //! inputs -- `tools/mathops.c` for the math, `tools/functions.c` for the
    //! LPC analysis and pitch search the concealment runs on.

    use super::bands::{bitexact_cos, bitexact_log2tan};
    use super::lpc::{autocorr, lpc};
    use super::mathops::{cos_norm, exp2, exp2_frac, frac_div32, isqrt32, rcp, rsqrt_norm, sqrt};
    use super::pitch::{pitch_downsample, pitch_search};
    use super::tables::WINDOW120;
    use crate::testutil::{Digest, Xorshift, signal};

    /// `tools/mathops.c`'s output: each function, how many results, their
    /// FNV-1a.
    const LIBOPUS: [(&str, usize, u64); 10] = [
        ("exp2", 65536, 0x505a_968c_357c_6153),
        ("exp2_frac", 65536, 0x199a_f9c6_68fc_2445),
        ("bitexact_cos", 65536, 0x3800_7ce7_925e_aa2d),
        ("cos_norm", 151_072, 0x5056_0ac0_bb73_9a9c),
        ("rsqrt_norm", 49152, 0x7c80_2b23_3c08_aa79),
        ("sqrt", 265_536, 0xf440_5019_506b_f7bd),
        ("rcp", 259_107, 0x7d6d_38b3_65da_bd4d),
        ("frac_div32", 99703, 0x8aa3_d49c_cc30_6b50),
        ("isqrt32", 193_793, 0xf1c9_49fa_b337_b0c7),
        ("bitexact_log2tan", 200_000, 0xe402_09bf_5e48_4147),
    ];

    /// A positive value of any magnitude: 31 random bits shifted right 0 to
    /// 30 (`tools/mathops.c`'s `any_magnitude`).
    fn any_magnitude(rng: &mut Xorshift) -> i32 {
        let bits = rng.next();
        let shift = rng.next();
        (bits & 0x7fff_ffff) as i32 >> (shift % 31)
    }

    #[test]
    fn mathops_agree_with_libopus() {
        let mut rng = Xorshift(0x9E37_79B9_7F4A_7C15);
        let mut ours: Vec<(&str, Digest)> = Vec::new();
        let mut d = Digest::new();
        for i in -32768..32768 {
            d.add(exp2(i));
        }
        ours.push(("exp2", d));
        let mut d = Digest::new();
        for i in -32768..32768 {
            d.add(exp2_frac(i));
        }
        ours.push(("exp2_frac", d));
        let mut d = Digest::new();
        for i in -32768..32768 {
            d.add(bitexact_cos(i));
        }
        ours.push(("bitexact_cos", d));
        let mut d = Digest::new();
        for i in 0..131_072 {
            d.add(cos_norm(i));
        }
        for _ in 0..20000 {
            d.add(cos_norm(rng.next() as i32));
        }
        ours.push(("cos_norm", d));
        let mut d = Digest::new();
        for i in 16384..65536 {
            d.add(rsqrt_norm(i));
        }
        ours.push(("rsqrt_norm", d));
        let mut d = Digest::new();
        for i in 0..65536 {
            d.add(sqrt(i));
        }
        for _ in 0..200_000 {
            d.add(sqrt(any_magnitude(&mut rng)));
        }
        ours.push(("sqrt", d));
        let mut d = Digest::new();
        for i in 1..65536 {
            d.add(rcp(i));
        }
        for _ in 0..200_000 {
            let x = any_magnitude(&mut rng);
            if x > 0 {
                d.add(rcp(x));
            }
        }
        ours.push(("rcp", d));
        let mut d = Digest::new();
        for _ in 0..200_000 {
            let mut a = any_magnitude(&mut rng);
            let b = any_magnitude(&mut rng);
            if rng.next() & 1 != 0 {
                a = -a;
            }
            if b > 0 && a < b && a > -b {
                d.add(frac_div32(a, b));
            }
        }
        ours.push(("frac_div32", d));
        let mut d = Digest::new();
        for _ in 0..200_000 {
            let v = rng.next();
            let shift = rng.next();
            let v = v >> (shift % 32);
            if v != 0 {
                d.add(isqrt32(v) as i32);
            }
        }
        ours.push(("isqrt32", d));
        let mut d = Digest::new();
        for _ in 0..200_000 {
            let a = 1 + (rng.next() % 32767) as i32;
            let b = 1 + (rng.next() % 32767) as i32;
            d.add(bitexact_log2tan(a, b));
        }
        ours.push(("bitexact_log2tan", d));

        let mut differ = Vec::new();
        for ((name, d), (their_name, count, hash)) in ours.iter().zip(LIBOPUS) {
            assert_eq!(*name, their_name);
            if let Err(e) = d.check(name, count, hash) {
                differ.push(e);
            }
        }
        assert!(
            differ.is_empty(),
            "the fixed-point math differs from libopus's: {differ:#?}"
        );
    }

    /// The concealment's LPC analysis: `_celt_autocorr` of signals of every
    /// kind (noise, tones, silence), then `_celt_lpc` of it -- and of
    /// autocorrelations that need not be any signal's, as a damaged stream's
    /// could hand it. Tones drive the coefficients past 16 bits, into the
    /// bandwidth expansion and its last resort, A(z) = 1.
    #[test]
    fn lpc_agrees_with_libopus() {
        let mut rng = Xorshift(0x9E37_79B9_7F4A_7C15);
        let mut d = Digest::new();
        let mut x = vec![0i32; 1024];
        let mut ac = [0i32; 25];
        let mut out = [0i32; 24];
        for i in 0..20000 {
            if i % 2 == 0 {
                signal(&mut rng, &mut x);
                let shift = autocorr(&x, &mut ac, &WINDOW120, 120, 24, 1024);
                d.add(shift);
                for &a in &ac {
                    d.add(a);
                }
            } else {
                let bits0 = rng.next();
                let shift0 = rng.below(31);
                ac[0] = (bits0 & 0x7fff_ffff) as i32 >> shift0;
                for a in &mut ac[1..] {
                    let bits = rng.next();
                    let shift = rng.below(31);
                    *a = (bits & 0x7fff_ffff) as i32 >> shift;
                    if rng.next() & 1 != 0 {
                        *a = -*a;
                    }
                }
            }
            // The fallback leaves all but the first coefficient as they were.
            for (k, o) in out.iter_mut().enumerate() {
                *o = k as i32 * 1000 - 12000;
            }
            lpc(&mut out, &ac, 24);
            for &o in &out {
                d.add(o);
            }
        }
        d.check("celt_lpc", 740_000, 0xcee6_b06f_4e49_6698).unwrap();
    }

    /// The concealment's pitch search: `pitch_downsample` of one or two
    /// channels of every kind of signal at every level, then `pitch_search`
    /// over the concealment's lags -- loud ones taking the search's
    /// down-shift.
    #[test]
    fn pitch_agrees_with_libopus() {
        let mut rng = Xorshift(0xD1B5_4A32_D192_ED03);
        let mut d = Digest::new();
        let (mut a, mut b) = (vec![0i32; 2048], vec![0i32; 2048]);
        let mut lp = vec![0i32; 1024];
        for _ in 0..1500 {
            let channels = 1 + rng.below(2) as usize;
            let shift = rng.below(13);
            signal(&mut rng, &mut a);
            signal(&mut rng, &mut b);
            let ch0: Vec<i32> = a.iter().map(|&v| v << shift).collect();
            let ch1: Vec<i32> = b.iter().map(|&v| v << shift).collect();
            let chans: [&[i32]; 2] = [&ch0, &ch1];
            pitch_downsample(&chans[..channels], &mut lp, 2048);
            for &v in &lp {
                d.add(v);
            }
            d.add(pitch_search(&lp[360..], &lp, 2048 - 720, 720 - 100));
        }
        d.check("celt_pitch", 1_537_500, 0x943f_644a_7dc9_39ef)
            .unwrap();
    }
}
