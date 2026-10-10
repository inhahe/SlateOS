#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use crate::fake::Fake;
use scratchdir::ScratchDir;

/// A device forwarding to a fake the test keeps, to look at afterwards.
struct Keep<'a>(&'a mut Fake);

impl PcmSys for Keep<'_> {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        self.0.ioctl(request, payload)
    }
    fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno> {
        self.0.write(bytes)
    }
    fn pause(&mut self, ms: u64) {
        self.0.pause(ms);
    }
}

/// A small WAV file: `samples` of 16-bit mono at `rate`.
fn wav_file(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let mut f = b"RIFF".to_vec();
    f.extend_from_slice(&u32::try_from(36 + data.len()).unwrap().to_le_bytes());
    f.extend_from_slice(b"WAVEfmt ");
    f.extend_from_slice(&16u32.to_le_bytes());
    f.extend_from_slice(&1u16.to_le_bytes());
    f.extend_from_slice(&1u16.to_le_bytes());
    f.extend_from_slice(&rate.to_le_bytes());
    f.extend_from_slice(&(rate * 2).to_le_bytes());
    f.extend_from_slice(&2u16.to_le_bytes());
    f.extend_from_slice(&16u16.to_le_bytes());
    f.extend_from_slice(b"data");
    f.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    f.extend_from_slice(&data);
    f
}

/// **A sound renders as the mixer plays it, at its volume**: a built-in
/// at half volume is half the level, and a file is read, decoded and
/// converted the same way.
#[test]
fn a_sound_renders_at_its_volume() {
    let full = render(&Sound::BuiltIn(BuiltIn::Information), 1.0).unwrap();
    let half = render(&Sound::BuiltIn(BuiltIn::Information), 0.5).unwrap();
    assert_eq!(full.len(), half.len());
    assert_eq!(full.len() % 2, 0);
    let peak = |s: &[i16]| s.iter().map(|v| i32::from(*v).abs()).max().unwrap();
    let (pf, ph) = (peak(&full), peak(&half));
    assert!((ph * 2 - pf).abs() <= 4, "{pf} and {ph}");

    let dir = ScratchDir::new("sound-render");
    let path = dir.path("tone.wav");
    std::fs::write(&path, wav_file(48_000, &[8192; 480])).unwrap();
    let samples = render(&Sound::File(path), 1.0).unwrap();
    assert_eq!(
        samples.len(),
        960,
        "480 frames, stereo, at the mixer's own rate"
    );
    assert!(
        samples[100..900]
            .iter()
            .all(|&s| (8190..=8194).contains(&s))
    );
}

/// **A sound plays through the device whole**: what the fake took is what
/// rendering made, configured first and drained last.
#[test]
fn a_sound_plays_through_the_device_whole() {
    let sound = Sound::BuiltIn(BuiltIn::Complete);
    let mut fake = Fake::new();
    play_on(Keep(&mut fake), &sound, 0.8).unwrap();
    let expected: Vec<u8> = render(&sound, 0.8)
        .unwrap()
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    assert_eq!(fake.written, expected);
    assert_eq!(fake.ioctls().first(), Some(&pcm::IOCTL_HW_PARAMS));
    assert_eq!(fake.ioctls().last(), Some(&pcm::IOCTL_DRAIN));
    assert_eq!(fake.queued, 0);
}

/// **No volume, no sound -- and no device**: a volume of nought, below it
/// or not a number opens nothing.
#[test]
fn no_volume_opens_nothing() {
    for volume in [0.0, -1.0, f32::NAN] {
        let mut fake = Fake::new();
        play_on(Keep(&mut fake), &Sound::BuiltIn(BuiltIn::Bell), volume).unwrap();
        assert!(fake.calls.is_empty(), "{volume}: {:?}", fake.calls);
        assert!(
            play(Sound::BuiltIn(BuiltIn::Bell), volume).is_ok(),
            "nothing to do is done"
        );
    }
}

/// **A file past the limit is refused before it is read into memory**, one
/// at the limit is read, and one that is not there says so.
#[test]
fn a_file_past_the_limit_is_refused() {
    let dir = ScratchDir::new("sound-limit");
    let path = dir.path("big.wav");
    std::fs::write(&path, vec![0u8; 101]).unwrap();
    assert!(matches!(read_at_most(&path, 100), Err(PlayError::TooLarge)));
    assert_eq!(read_at_most(&path, 101).unwrap().len(), 101);
    assert!(matches!(
        read_file(&dir.path("missing.wav")),
        Err(PlayError::Read(std::io::ErrorKind::NotFound))
    ));
    // A file that is not a sound is refused by the decoder.
    std::fs::write(&path, b"not a sound").unwrap();
    assert!(matches!(
        render(&Sound::File(path), 1.0),
        Err(PlayError::Decode(DecodeError::Unrecognised))
    ));
}

/// **At most so many sounds play at once**: the places are taken until
/// none is left, and one given back is there to take again.
#[test]
fn at_most_so_many_sounds_play_at_once() {
    let places: Vec<Place> = (0..MAX_PLAYING).map(|_| Place::take().unwrap()).collect();
    assert!(Place::take().is_none(), "all taken");
    drop(places);
    let again = Place::take();
    assert!(again.is_some(), "given back");
}

/// **Where there is no device, playing says so at once**, without starting
/// a thread to fail on.
#[test]
fn where_there_is_no_device_playing_says_so() {
    if !pcm::DEVICE_SUPPORTED {
        assert!(matches!(
            play(Sound::BuiltIn(BuiltIn::Bell), 1.0),
            Err(PlayError::Unsupported)
        ));
        assert!(matches!(
            pcm::open_device(),
            Err(PcmError::Open(Errno::ENOSYS))
        ));
    }
}

/// **A sound's length and duration** are its frames at its rate.
#[test]
fn a_sounds_length_is_its_frames_at_its_rate() {
    let a = Audio {
        rate: 8000,
        channels: 2,
        samples: vec![0.0; 2 * 4000],
        order: ChannelOrder::Wav,
    };
    assert_eq!(a.frames(), 4000);
    assert_eq!(a.duration_ms(), 500);
    let none = Audio {
        rate: 0,
        channels: 0,
        samples: vec![],
        order: ChannelOrder::Wav,
    };
    assert_eq!((none.frames(), none.duration_ms()), (0, 0));
}
