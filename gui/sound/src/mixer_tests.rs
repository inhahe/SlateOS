#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use super::*;

/// `_IOC(dir, 'U', nr, size)`, written out from `asm-generic/ioctl.h`.
fn ioc(dir: u32, nr: u32, size: usize) -> u32 {
    (dir << 30) | (u32::try_from(size).unwrap() << 16) | (u32::from(b'U') << 8) | nr
}

/// **The requests are Linux's**: each by `_IOC`'s formula, and each the
/// number the kernel asserts in its own self-test.
#[test]
fn the_requests_are_linuxs() {
    assert_eq!(IOCTL_ELEM_INFO, ioc(3, 0x11, ELEM_INFO_SIZE));
    assert_eq!(IOCTL_ELEM_READ, ioc(3, 0x12, ELEM_VALUE_SIZE));
    assert_eq!(IOCTL_ELEM_WRITE, ioc(3, 0x13, ELEM_VALUE_SIZE));
    // `kernel/src/audio_alsa_ctl.rs`, `self_test`.
    assert_eq!(IOCTL_ELEM_INFO, 0xC110_5511);
    assert_eq!(IOCTL_ELEM_READ, 0xC4C8_5512);
    assert_eq!(IOCTL_ELEM_WRITE, 0xC4C8_5513);
}

/// **The payloads are laid out as `asound.h` lays them out on x86-64**,
/// worked out afresh from the fields: an id is four 4-byte fields, a
/// 44-byte name and an index (64 bytes); an info is the id, then type,
/// access, count and owner, then a union of `long`s, so 8-aligned (80); a
/// value is the id and the `indirect` word, then a union of `long`s (72).
#[test]
fn the_payloads_are_laid_out_as_asound_h() {
    assert_eq!(at::ID_NAME, 4 * 4);
    assert_eq!(at::ID_INDEX, at::ID_NAME + NAME_BYTES);
    assert_eq!(ELEM_ID_SIZE, at::ID_INDEX + 4);
    assert_eq!(at::INFO_TYPE, ELEM_ID_SIZE);
    assert_eq!(at::INFO_ACCESS, at::INFO_TYPE + 4);
    assert_eq!(at::INFO_COUNT, at::INFO_ACCESS + 4);
    let owner_end = at::INFO_COUNT + 4 + 4;
    assert_eq!(at::INFO_MIN, owner_end.next_multiple_of(8));
    assert_eq!(at::INFO_MAX, at::INFO_MIN + 8);
    // The union is 128 bytes, then `dimen` (8) and 56 reserved.
    assert_eq!(ELEM_INFO_SIZE, at::INFO_MIN + 128 + 8 + 56);
    assert_eq!(at::VALUE_VALUES, (ELEM_ID_SIZE + 4).next_multiple_of(8));
    // 128 `long`s, then 128 reserved bytes.
    assert_eq!(ELEM_VALUE_SIZE, at::VALUE_VALUES + MAX_VALUES * 8 + 128);
}

/// A card that keeps every payload it is handed, and answers as
/// [`Simulated`] does.
struct Recording {
    card: Simulated,
    payloads: Vec<(u32, Vec<u8>)>,
}

impl ControlSys for Recording {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        self.payloads.push((request, payload.to_vec()));
        self.card.ioctl(request, payload)
    }
}

/// **The master is asked for by name, as a mixer control** -- number 0,
/// the mixer interface, the name NUL-ended, device, subdevice and index
/// nought -- in a payload of the size the request says; and read and
/// written by the number the card answered with.
#[test]
fn the_master_is_asked_for_by_name_then_by_number() {
    let mut rec = Recording {
        card: Simulated::new(40, false),
        payloads: Vec::new(),
    };
    let mut master = Master::open(&mut rec).unwrap();
    master.set_level(60).unwrap();
    let (request, info) = &rec.payloads[0];
    assert_eq!(*request, IOCTL_ELEM_INFO);
    assert_eq!(info.len(), ELEM_INFO_SIZE);
    assert_eq!(get_u32(info, at::ID_NUMID), Some(0));
    assert_eq!(get_i32(info, at::ID_IFACE), Some(2));
    assert_eq!(get_u32(info, at::ID_DEVICE), Some(0));
    assert_eq!(get_u32(info, at::ID_SUBDEVICE), Some(0));
    assert_eq!(get_u32(info, at::ID_INDEX), Some(0));
    assert_eq!(
        &info[at::ID_NAME..at::ID_NAME + NAME_BYTES][..=VOLUME_NAME.len()],
        b"Master Playback Volume\0"
    );
    assert_eq!(rec.payloads[1].0, IOCTL_ELEM_INFO, "then the switch");
    assert_eq!(id_name(&rec.payloads[1].1), SWITCH_NAME.as_bytes());
    let (request, value) = rec.payloads.last().unwrap();
    assert_eq!(*request, IOCTL_ELEM_WRITE);
    assert_eq!(value.len(), ELEM_VALUE_SIZE);
    assert_eq!(get_u32(value, at::ID_NUMID), Some(1), "by its number");
    assert_eq!(get_i64(value, at::VALUE_VALUES), Some(60));
}

/// **SlateOS's card: its level read and set, its switch turned**, the level
/// kept through a mute.
#[test]
fn slateos_card_is_read_set_and_muted() {
    let mut card = Simulated::new(40, false);
    let mut master = Master::open(&mut card).unwrap();
    assert!(master.has_switch());
    assert_eq!(master.level(), Ok(40));
    assert_eq!(master.muted(), Ok(false));
    master.set_level(75).unwrap();
    master.set_muted(true).unwrap();
    assert_eq!(master.muted(), Ok(true));
    assert_eq!(master.level(), Ok(75), "a mute keeps the level");
    master.set_level(200).unwrap();
    assert_eq!(master.level(), Ok(100), "more than all is all");
    assert_eq!(card.volume, [100]);
    assert_eq!(card.switch, Some(vec![0]), "off is muted");
}

/// **A card of two channels over another range** -- a Linux host's, 0 to
/// 87 -- is read as the channels' mean share of its range, set on both
/// channels alike, and a level set reads back as itself, not as the
/// nearest share the card's steps make of it.
#[test]
fn a_card_of_two_channels_over_another_range() {
    let mut card = Simulated {
        volume: vec![0, 87],
        range: (0, 87),
        switch: Some(vec![1, 1]),
        ..Simulated::new(0, false)
    };
    let mut master = Master::open(&mut card).unwrap();
    assert_eq!(master.level(), Ok(50), "the mean, 43.5 of 87");
    master.set_level(50).unwrap();
    assert_eq!(master.level(), Ok(50), "not 51, which 44 of 87 is");
    master.set_level(33).unwrap();
    master.set_muted(true).unwrap();
    assert_eq!(card.volume, [29, 29], "33 of 100 is 28.71 of 87");
    assert_eq!(card.switch, Some(vec![0, 0]));
    // Changed by someone else: read as the share it is.
    card.volume = vec![44, 44];
    let mut master = Master::open(&mut card).unwrap();
    assert_eq!(master.level(), Ok(51));
}

/// **A range need not start at nought.**
#[test]
fn a_range_need_not_start_at_nought() {
    let mut card = Simulated {
        volume: vec![0],
        range: (-50, 50),
        ..Simulated::new(0, false)
    };
    let mut master = Master::open(&mut card).unwrap();
    assert_eq!(master.level(), Ok(50));
    master.set_level(25).unwrap();
    master.set_level(0).unwrap();
    assert_eq!(master.level(), Ok(0));
    assert_eq!(card.volume, [-50]);
}

/// **A switch on in any channel is sounding**: muted is off in every one.
#[test]
fn a_switch_on_in_any_channel_is_sounding() {
    let mut card = Simulated {
        volume: vec![10, 10],
        switch: Some(vec![0, 1]),
        ..Simulated::new(0, false)
    };
    let mut master = Master::open(&mut card).unwrap();
    assert_eq!(master.muted(), Ok(false));
    master.set_muted(false).unwrap();
    assert_eq!(card.switch, Some(vec![1, 1]));
}

/// **A card with no switch is muted by its volume**: to the bottom, and
/// back to where it was -- or to where it was moved to while muted.
#[test]
fn a_card_with_no_switch_is_muted_by_its_volume() {
    let mut card = Simulated {
        switch: None,
        ..Simulated::new(60, false)
    };
    let mut master = Master::open(&mut card).unwrap();
    assert!(!master.has_switch());
    master.set_muted(true).unwrap();
    assert_eq!(master.muted(), Ok(true));
    assert_eq!(master.level(), Ok(60), "the level to go back to");
    master.set_muted(true).unwrap();
    master.set_level(80).unwrap();
    assert_eq!(master.level(), Ok(80));
    let silent = master.into_inner().volume.clone();
    assert_eq!(silent, [0], "silent while muted, whatever the level");
    let mut master = Master::open(&mut card).unwrap();
    // A fresh `Master` knows of no mute: the card is at the bottom.
    assert_eq!(master.level(), Ok(0));
    master.set_level(80).unwrap();
    master.set_muted(true).unwrap();
    master.set_level(30).unwrap();
    master.set_muted(false).unwrap();
    assert_eq!(master.muted(), Ok(false));
    assert_eq!(card.volume, [30]);
}

/// **A card that refuses says so** -- as a native program's `ioctl` on a
/// sound device answers on SlateOS today -- and one with no master volume
/// is told apart from it.
#[test]
fn a_card_that_refuses_or_has_no_master_says_which() {
    let refusing = Simulated {
        refuse: Some(Errno::ENOTTY),
        ..Simulated::new(50, false)
    };
    let err = Master::open(refusing).unwrap_err();
    assert_eq!(err, MixerError::Refused(Errno::ENOTTY));
    assert!(err.to_string().contains("refused"), "{err}");

    let empty = Simulated {
        refuse: Some(Errno::ENOENT),
        ..Simulated::new(50, false)
    };
    assert_eq!(Master::open(empty).unwrap_err(), MixerError::NoMaster);
    assert_eq!(
        MixerError::NoMaster.to_string(),
        "the sound card has no master volume"
    );
}

/// A card whose answer to `ELEM_INFO` is set by the test.
struct Answering(fn(&mut [u8]));

impl std::fmt::Debug for Answering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Answering")
    }
}

impl ControlSys for Answering {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        if request != IOCTL_ELEM_INFO || id_name(payload) != VOLUME_NAME.as_bytes() {
            return Err(Errno::ENOENT);
        }
        (self.0)(payload);
        Ok(())
    }
}

/// An answer for a usable master volume, 0 to 100, one channel.
fn usable(payload: &mut [u8]) {
    put_u32(payload, at::ID_NUMID, 1);
    put_i32(payload, at::INFO_TYPE, TYPE_INTEGER);
    put_u32(payload, at::INFO_ACCESS, ACCESS_READ | ACCESS_WRITE);
    put_u32(payload, at::INFO_COUNT, 1);
    put_i64(payload, at::INFO_MAX, 100);
}

/// **A master that is not a number this can set is no master**: one of
/// another type, one that cannot be written, one of no values or more than
/// a payload holds, one over no range, one the card numbers nought.
#[test]
fn a_master_this_cannot_set_is_no_master() {
    assert!(Master::open(Answering(usable)).is_ok(), "the control");
    let unusable: [fn(&mut [u8]); 6] = [
        |p| {
            usable(p);
            put_i32(p, at::INFO_TYPE, TYPE_BOOLEAN);
        },
        |p| {
            usable(p);
            put_u32(p, at::INFO_ACCESS, ACCESS_READ);
        },
        |p| {
            usable(p);
            put_u32(p, at::INFO_COUNT, 0);
        },
        |p| {
            usable(p);
            put_u32(p, at::INFO_COUNT, 129);
        },
        |p| {
            usable(p);
            put_i64(p, at::INFO_MAX, 0);
        },
        |p| {
            usable(p);
            put_u32(p, at::ID_NUMID, 0);
        },
    ];
    for (i, answer) in unusable.into_iter().enumerate() {
        assert_eq!(
            Master::open(Answering(answer)).unwrap_err(),
            MixerError::NoMaster,
            "answer {i}"
        );
    }
    // With no switch to be found, it mutes by its volume.
    assert!(!Master::open(Answering(usable)).unwrap().has_switch());
}

/// **A level and a value convert to the nearest**, either way, and a value
/// outside its range is at its end.
#[test]
fn levels_and_values_convert_to_the_nearest() {
    assert_eq!(percent_of_sum(0, 1, 0, 100), 0);
    assert_eq!(percent_of_sum(100, 1, 0, 100), 100);
    assert_eq!(percent_of_sum(-5, 1, 0, 100), 0);
    assert_eq!(percent_of_sum(150, 1, 0, 100), 100);
    assert_eq!(percent_of_sum(44, 1, 0, 87), 51);
    assert_eq!(percent_of_sum(5, 1, 5, 5), 0, "a range of nothing");
    assert_eq!(raw_of(50, 0, 87), 44, "43.5 rounds up");
    assert_eq!(raw_of(0, -10, 10), -10);
    assert_eq!(raw_of(100, -10, 10), 10);
    assert_eq!(raw_of(50, 3, 3), 3);
    assert_eq!(raw_of(50, i64::MIN, i64::MAX), 0, "no overflow");
    assert_eq!(percent_of_sum(0, 1, i64::MIN, i64::MAX), 50);
}

/// **On a system this does not open the device on, there is none**: no
/// call made, and the reason is the system's.
#[test]
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn there_is_no_device_on_another_system() {
    const { assert!(!DEVICE_SUPPORTED) };
    assert_eq!(open_master().unwrap_err(), MixerError::Open(Errno::ENOSYS));
}
