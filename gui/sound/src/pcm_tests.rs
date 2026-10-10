#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;
use crate::fake::{Call, Fake};

/// `_IOC(dir, 'A', nr, size)`, written out from `asm-generic/ioctl.h`.
fn ioc(dir: u32, nr: u32, size: u32) -> u32 {
    (dir << 30) | (size << 16) | (u32::from(b'A') << 8) | nr
}

/// **The requests are Linux's**: each by `_IOC`'s formula, and the two
/// the kernel asserts in its own self-test by its numbers.
#[test]
fn the_requests_are_linuxs() {
    assert_eq!(IOCTL_HW_PARAMS, ioc(3, 0x11, 608));
    assert_eq!(IOCTL_PREPARE, ioc(0, 0x40, 0));
    assert_eq!(IOCTL_DROP, ioc(0, 0x43, 0));
    assert_eq!(IOCTL_DRAIN, ioc(0, 0x44, 0));
    assert_eq!(IOCTL_STATUS, ioc(2, 0x20, 152));
    // `kernel/src/audio_alsa.rs`: HW_PARAMS == 0xC260_4111 and STATUS
    // "encodes to 0x8098_4120".
    assert_eq!(IOCTL_HW_PARAMS, 0xC260_4111);
    assert_eq!(IOCTL_STATUS, 0x8098_4120);
    assert_eq!(u32::try_from(HW_PARAMS_SIZE).unwrap(), 608);
    assert_eq!(u32::try_from(STATUS_SIZE).unwrap(), 152);
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// **The request asks for the mixer's configuration, at the kernel's
/// offsets**: read back with the layout worked out afresh from `struct
/// snd_pcm_hw_params` -- flags, three masks of 32 bytes, five reserved,
/// twelve intervals of 12 bytes from sample bits, nine reserved, then
/// `rmask` -- not with this module's own constants.
#[test]
fn the_request_asks_for_the_mixers_configuration() {
    let p = hw_params_request();
    assert_eq!(p.len(), 608);
    // Masks: access at 4, format at 36, subformat at 68.
    assert_eq!(u32_at(&p, 4), 1 << 3, "RW_INTERLEAVED");
    assert_eq!(u32_at(&p, 36), 1 << 2, "S16_LE");
    assert_eq!(u32_at(&p, 68), 1, "SUBFORMAT_STD");
    // The reserved masks are untouched.
    assert!(p[100..260].iter().all(|&b| b == 0));
    // Intervals from 260: sample bits, frame bits, channels, rate, ...,
    // buffer size the tenth (the kernel's IV_BUFFER_SIZE = 9).
    let iv = |k: usize| {
        (
            u32_at(&p, 260 + 12 * k),
            u32_at(&p, 264 + 12 * k),
            u32_at(&p, 268 + 12 * k),
        )
    };
    assert_eq!(iv(0), (16, 16, 0b100), "sample bits");
    assert_eq!(iv(1), (32, 32, 0b100), "frame bits");
    assert_eq!(iv(2), (2, 2, 0b100), "channels");
    assert_eq!(iv(3), (48_000, 48_000, 0b100), "rate");
    assert_eq!(iv(5), (1024, 1024, 0b100), "period size");
    assert_eq!(iv(7), (4, 4, 0b100), "periods");
    assert_eq!(iv(9), (4096, 4096, 0b100), "buffer size");
    assert_eq!(iv(10), (16_384, 16_384, 0b100), "buffer bytes");
    assert_eq!(iv(4), (0, u32::MAX, 0), "period time: any");
    assert_eq!(u32_at(&p, 512), u32::MAX, "rmask: settle everything");
    assert!(
        p[516..].iter().all(|&b| b == 0),
        "the reply fields are left nought"
    );
}

/// **A reply in the mixer's configuration is native; one in another is
/// not**, and one too short is not a reply.
#[test]
fn a_reply_is_read_for_what_it_settled() {
    let native = Configured::read(&hw_params_request()).unwrap();
    assert!(native.is_native());
    assert_eq!(native.buffer_frames, RING_FRAMES);

    let mut other = hw_params_request();
    set_fixed(&mut other, IV_RATE, 44_100);
    assert!(
        !Configured::read(&other).unwrap().is_native(),
        "another rate"
    );

    let mut wide = hw_params_request();
    put_u32(&mut wide, 260 + 12 * 2 + 4, 6);
    assert!(
        !Configured::read(&wide).unwrap().is_native(),
        "a range of channels"
    );

    let mut float = hw_params_request();
    set_mask(&mut float, MASK_FORMAT, 14);
    assert_eq!(Configured::read(&float).unwrap().format, Some(14));
    assert!(
        !Configured::read(&float).unwrap().is_native(),
        "float samples"
    );

    let mut two = hw_params_request();
    put_u32(&mut two, 36, (1 << 2) | (1 << 10));
    assert_eq!(
        Configured::read(&two).unwrap().format,
        None,
        "two formats is no one format"
    );

    assert_eq!(Configured::read(&hw_params_request()[..607]), None);
}

/// **A status reply is read at the kernel's offsets**: state at 0, delay
/// at 56, avail at 64 (`SndPcmStatus`, after two timespecs and two
/// pointers).
#[test]
fn a_status_reply_is_read_at_the_kernels_offsets() {
    let mut s = [0u8; 152];
    s[0..4].copy_from_slice(&3u32.to_le_bytes());
    s[56..64].copy_from_slice(&1234i64.to_le_bytes());
    s[64..72].copy_from_slice(&99u64.to_le_bytes());
    assert_eq!(
        Status::read(&s),
        Some(Status {
            state: 3,
            delay: 1234,
            avail: 99
        })
    );
    s[56..64].copy_from_slice(&(-5i64).to_le_bytes());
    assert_eq!(Status::read(&s).unwrap().delay, -5);
    assert_eq!(Status::read(&s[..151]), None);
}

/// `n` frames of a recognisable stereo ramp.
fn ramp(n: usize) -> Vec<i16> {
    (0..n * 2)
        .map(|i| i16::try_from(i % 30_000).unwrap())
        .collect()
}

fn bytes_of(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// A device forwarding to a fake the test keeps, to look at after
/// [`Playback::finish`] has consumed the playback.
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

/// **A sound is configured, prepared, written whole as the ring drains,
/// waited on until it has played, and drained** -- in that order, every
/// byte in order, and the drain only once nothing is queued: closing the
/// device sooner would empty its ring with the sound's end still in it.
#[test]
fn a_sound_is_written_whole_waited_on_and_drained() {
    let samples = ramp(20_000);
    let mut fake = Fake::new();
    let mut playback = Playback::open(Keep(&mut fake)).unwrap();
    playback.write(&samples).unwrap();
    playback.finish().unwrap();
    assert_eq!(fake.written, bytes_of(&samples), "every byte, in order");
    assert!(
        fake.calls.contains(&Call::Write(4096 * 4)),
        "a full ring at a time"
    );
    let ioctls = fake.ioctls();
    assert_eq!(ioctls[..2], [IOCTL_HW_PARAMS, IOCTL_PREPARE]);
    assert_eq!(ioctls.last(), Some(&IOCTL_DRAIN));
    assert!(
        ioctls[2..ioctls.len() - 1]
            .iter()
            .all(|&r| r == IOCTL_STATUS),
        "between the writes and the drain, only looks at how far it has played"
    );
    assert!(
        ioctls.iter().filter(|&&r| r == IOCTL_STATUS).count() > 1,
        "waited"
    );
    assert_eq!(fake.queued, 0, "drained only when nothing was left");
    // Nothing written after the first STATUS.
    let first_status = fake
        .calls
        .iter()
        .position(|c| *c == Call::Ioctl(IOCTL_STATUS))
        .unwrap();
    assert!(
        !fake.calls[first_status..]
            .iter()
            .any(|c| matches!(c, Call::Write(_)))
    );
}

/// **Finishing with nothing queued drains at once.**
#[test]
fn finishing_with_nothing_queued_drains_at_once() {
    let mut fake = Fake::new();
    Playback::open(Keep(&mut fake)).unwrap().finish().unwrap();
    assert_eq!(
        fake.ioctls(),
        [IOCTL_HW_PARAMS, IOCTL_PREPARE, IOCTL_STATUS, IOCTL_DRAIN]
    );
    assert!(!fake.calls.iter().any(|c| matches!(c, Call::Pause(_))));
}

/// **A ring that never drains is given up on** -- within [`STALL_MS`] of
/// its last progress, the queue dropped -- rather than waited on forever,
/// as it would be with no sound card behind the mixer.
#[test]
fn a_ring_that_never_drains_is_given_up_on() {
    let mut fake = Fake::new();
    fake.drains = false;
    let mut playback = Playback::open(fake).unwrap();
    assert_eq!(playback.write(&ramp(10_000)), Err(PcmError::Stalled));
    let paused: u64 = playback
        .sys
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::Pause(ms) => Some(*ms),
            _ => None,
        })
        .sum();
    assert_eq!(
        paused, STALL_MS,
        "waited exactly as long as the stall allows"
    );
    assert_eq!(playback.sys.ioctls().last(), Some(&IOCTL_DROP));

    // And finishing on a queue that does not move.
    let mut fake = Fake::new();
    fake.drains = false;
    fake.queued = 500;
    let playback = Playback::open(fake).unwrap();
    assert_eq!(playback.finish(), Err(PcmError::Stalled));
}

/// **A refused configuration plays nothing**, and **a device that settles
/// on another configuration is refused** rather than fed the mixer's.
#[test]
fn a_configuration_refused_or_not_native_plays_nothing() {
    let mut fake = Fake::new();
    fake.configure = Err(Errno::EINVAL);
    assert_eq!(
        Playback::open(fake).map(|_| ()),
        Err(PcmError::Configure(Errno::EINVAL))
    );

    let mut other = hw_params_request();
    set_fixed(&mut other, IV_RATE, 44_100);
    let mut fake = Fake::new();
    fake.configure = Ok(other.to_vec());
    match Playback::open(fake) {
        Err(PcmError::NotNative(c)) => assert_eq!(c.rate, (44_100, 44_100)),
        other => panic!("{other:?}"),
    }
}

/// **An interrupted write is tried again; a refused one stops the
/// sound**; and a write the ring takes only part of continues from where
/// it stopped, even mid-frame.
#[test]
fn writes_are_retried_resumed_and_refused() {
    let samples = ramp(100);
    let mut fake = Fake::new();
    fake.refuse_write = Some(Errno::EINTR);
    let mut playback = Playback::open(fake).unwrap();
    playback.write(&samples).unwrap();
    assert_eq!(playback.sys.written, bytes_of(&samples));

    let mut fake = Fake::new();
    fake.refuse_write = Some(Errno::EBADF);
    let mut playback = Playback::open(fake).unwrap();
    assert_eq!(playback.write(&samples), Err(PcmError::Write(Errno::EBADF)));

    let mut fake = Fake::new();
    fake.largest_write = 7;
    let mut playback = Playback::open(fake).unwrap();
    playback.write(&samples).unwrap();
    assert_eq!(
        playback.sys.written,
        bytes_of(&samples),
        "seven bytes at a time, in order"
    );
}

/// **Nothing to write writes nothing** and waits for nothing.
#[test]
fn nothing_to_write_writes_nothing() {
    let mut playback = Playback::open(Fake::new()).unwrap();
    playback.write(&[]).unwrap();
    assert!(playback.sys.written.is_empty());
    assert!(
        !playback
            .sys
            .calls
            .iter()
            .any(|c| matches!(c, Call::Write(_) | Call::Pause(_)))
    );
}
