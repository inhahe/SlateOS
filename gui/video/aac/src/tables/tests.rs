#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "test code, on the tables' own indices and values"
)]

//! The tables FFmpeg computes as the decoder opens are taken from FFmpeg's
//! own build (`tools/ffaac/tables.sh`), so they are its values exactly. What
//! these tests check is the wiring: that each name holds the table its
//! documentation says -- the right window, length, shape parameter and
//! scale -- by working each one out again from its formula in `f64`, where
//! only the last unit may differ.

use super::*;
use core::f64::consts::PI;

/// The modified Bessel function of the first kind and order zero, by its
/// power series: every term positive, so `f64` keeps nearly all its digits.
fn bessel_i0(x: f64) -> f64 {
    let quarter = x * x / 4.0;
    let (mut sum, mut term, mut k) = (1.0, 1.0, 0.0);
    while term > sum * f64::EPSILON / 4.0 {
        k += 1.0;
        term *= quarter / (k * k);
        sum += term;
    }
    sum
}

/// The rising half of the Kaiser-Bessel-derived window of `2n` samples with
/// shape `alpha`, as `kbd_window_init` (`kbdwin.c`) defines it, from 0 to 1.
fn kbd(alpha: f64, n: usize) -> Vec<f64> {
    let alpha2 = 4.0 * (alpha * PI / n as f64).powi(2);
    let half = n / 2;
    let kernel: Vec<f64> =
        (0..=half).map(|i| bessel_i0(((i * (n - i)) as f64 * alpha2).sqrt())).collect();
    // The kernel is symmetric, n + 1 samples long; its last sample, like its
    // first, is I0(0) = 1.
    let total = kernel
        .iter()
        .enumerate()
        .map(|(i, &k)| if i == 0 || i == half { k } else { 2.0 * k })
        .sum::<f64>()
        + 1.0;
    let mut sum = 0.0;
    (0..n)
        .map(|i| {
            sum += kernel[if i <= half { i } else { n - i }];
            (sum / total).sqrt()
        })
        .collect()
}

#[test]
fn the_kaiser_bessel_derived_windows_are_their_formulas_q31() {
    for (table, alpha, n) in [
        (&KBD_LONG_1024[..], 4.0, 1024),
        (&KBD_SHORT_128[..], 6.0, 128),
        (&KBD_LONG_960[..], 4.0, 960),
        (&KBD_SHORT_120[..], 6.0, 120),
    ] {
        assert_eq!(table.len(), n);
        for (i, (&got, want)) in table.iter().zip(kbd(alpha, n)).enumerate() {
            // FFmpeg scales by 2^31 - 1, not 2^31.
            let want = (want * 2_147_483_647.0).round();
            assert!(
                (f64::from(got) - want).abs() <= 1.0,
                "{n}-line window, alpha {alpha}, sample {i}: {got}, not {want}"
            );
        }
    }
}

#[test]
fn the_sine_windows_are_their_formulas_q31() {
    for (table, n) in [
        (&SINE_1024[..], 1024),
        (&SINE_128[..], 128),
        (&SINE_960[..], 960),
        (&SINE_120[..], 120),
        (&SINE_512[..], 512),
    ] {
        assert_eq!(table.len(), n);
        for (i, &got) in table.iter().enumerate() {
            // `sine_window_init_fixed` takes the sine in `float` (`sinf`) of
            // the angle rounded to `float`, then scales it by 2^31 and
            // rounds: so it may be a `float` step off the true sine, and
            // half a unit more for the rounding.
            let angle = ((i as f64 + 0.5) * (PI / (2.0 * n as f64))) as f32;
            let want = f64::from(angle).sin();
            let step = f64::from(want as f32).abs() * f64::from(f32::EPSILON);
            let got = f64::from(got) / 2_147_483_648.0;
            assert!(
                (got - want).abs() <= step + 0.5 / 2_147_483_648.0,
                "{n}-line window, sample {i}: {got}, not {want}"
            );
        }
    }
}

#[test]
fn the_cube_root_table_is_each_number_to_the_four_thirds_q13() {
    assert_eq!(CBRT_TAB_FIXED[0], 0);
    for (i, &got) in CBRT_TAB_FIXED.iter().enumerate() {
        let want = (i as f64).cbrt().powi(4) * 8192.0;
        assert!(
            (f64::from(got) - want.round()).abs() <= 1.0,
            "{i}: {got}, not {want}"
        );
    }
}
