#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use super::*;

/// **Every sound renders as the mixer's stream** -- two equal channels at
/// 48 kHz, between a twentieth of a second and a second and a half -- and
/// **peaks at -6 dBFS**, not past it.
#[test]
fn every_sound_renders_at_its_peak() {
    for sound in BuiltIn::ALL {
        let a = sound.render();
        assert_eq!((a.rate, a.channels), (48_000, 2), "{sound:?}");
        let ms = a.duration_ms();
        assert!((50..=1500).contains(&ms), "{sound:?}: {ms} ms");
        assert!(
            a.samples.chunks_exact(2).all(|f| f[0] == f[1]),
            "{sound:?}: both sides alike"
        );
        let peak = a.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= PEAK + 1e-6, "{sound:?}: {peak}");
        assert!(peak > PEAK * 0.9, "{sound:?}: {peak}");
    }
}

/// **Every sound starts and ends quietly**, so none clicks: the first
/// sample is near nought (a three-millisecond rise), the last nearer still
/// (the fade), and no tone jumps between neighbouring samples by more than
/// a high note's steepest slope allows.
#[test]
fn every_sound_starts_and_ends_quietly() {
    for sound in BuiltIn::ALL {
        let a = sound.render();
        let left: Vec<f32> = a.samples.iter().step_by(2).copied().collect();
        assert!(left[0].abs() < 0.02, "{sound:?} starts at {}", left[0]);
        assert!(
            left[left.len() - 1].abs() < 0.005,
            "{sound:?} ends at {}",
            left[left.len() - 1]
        );
        if !matches!(sound, BuiltIn::TrashEmpty | BuiltIn::ScreenCapture) {
            let steepest = left
                .windows(2)
                .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
            assert!(steepest < 0.25, "{sound:?}: a step of {steepest}");
        }
    }
}

/// **A sound renders the same every time** -- the noise is seeded.
#[test]
fn a_sound_renders_the_same_every_time() {
    for sound in [
        BuiltIn::Message,
        BuiltIn::TrashEmpty,
        BuiltIn::ScreenCapture,
    ] {
        assert_eq!(sound.render(), sound.render(), "{sound:?}");
    }
}

/// **Every sound answers to its own name, and to the names the
/// specification gives events that sound alike.**
#[test]
fn every_sound_answers_to_its_names() {
    for sound in BuiltIn::ALL {
        assert_eq!(BuiltIn::for_event(sound.name()), Some(sound));
    }
    assert_eq!(BuiltIn::for_event("message"), Some(BuiltIn::Message));
    assert_eq!(BuiltIn::for_event("bell-terminal"), Some(BuiltIn::Bell));
    assert_eq!(BuiltIn::for_event("system-ready"), Some(BuiltIn::Login));
    assert_eq!(
        BuiltIn::for_event(" camera-shutter "),
        Some(BuiltIn::ScreenCapture)
    );
}

/// **No two sounds are alike**: each event is told apart by ear.
#[test]
fn no_two_sounds_are_alike() {
    let rendered: Vec<(BuiltIn, Audio)> = BuiltIn::ALL.iter().map(|&s| (s, s.render())).collect();
    for (i, (a, audio_a)) in rendered.iter().enumerate() {
        for (b, audio_b) in &rendered[i + 1..] {
            assert_ne!(audio_a.samples, audio_b.samples, "{a:?} and {b:?}");
        }
    }
    assert_eq!(
        BuiltIn::for_event("network-connectivity-error"),
        Some(BuiltIn::NetworkLost),
        "a connection that failed is one lost"
    );
}

/// **An event with no sound of its own sounds as its name cut at the last
/// hyphen**, and so on -- the specification's fallback -- and one no cut
/// of which has a sound has none.
#[test]
fn an_event_falls_back_through_its_name() {
    assert_eq!(
        BuiltIn::for_event("dialog-error-serious"),
        Some(BuiltIn::Error)
    );
    assert_eq!(BuiltIn::for_event("complete-copy"), Some(BuiltIn::Complete));
    assert_eq!(
        BuiltIn::for_event("complete-media-burn-test"),
        Some(BuiltIn::Complete)
    );
    assert_eq!(BuiltIn::for_event("window-attention"), None);
    assert_eq!(
        BuiltIn::for_event("dialog"),
        None,
        "not shortened past what was asked"
    );
    assert_eq!(BuiltIn::for_event(""), None);
    assert_eq!(BuiltIn::for_event("-"), None);
}
