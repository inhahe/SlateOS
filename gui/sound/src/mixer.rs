//! The sound card's master volume and its mute, spoken to as Linux's are:
//! the ALSA control device `/dev/snd/controlC0`, asked what `amixer` asks
//! -- `ELEM_INFO` to find the card's "Master Playback Volume" and "Master
//! Playback Switch" by their names, `ELEM_READ` and `ELEM_WRITE` to read
//! and set them.
//!
//! # A protocol and a device
//!
//! Split as [`crate::pcm`] is: what is said -- the payloads' bytes, what a
//! reply means -- is here, tested on the build machine against a card held
//! in memory ([`Simulated`]); the calls that say it are in `device`,
//! compiled for the target alone, through the C library. The payloads are
//! bytes at their offsets, pinned by a test to the kernel's layout
//! (`kernel/src/audio_alsa_ctl.rs`: `SndCtlElemId`, `SndCtlElemInfo`,
//! `SndCtlElemValue`), so nothing here needs `unsafe`.
//!
//! # A card's master, whatever its range
//!
//! What a card's volume runs over is read from the card, not assumed.
//! SlateOS's card has one value from 0 to 100; a Linux host's may have two
//! channels from 0 to 87. A level here is a share of the card's range, 0 to
//! 100 -- `amixer`'s percentage -- every channel set to it, and read back as
//! the channels' mean. A card with no master switch is muted by turning its
//! volume to the bottom, and unmuted by turning it back to where it was.
//!
//! # Where it works today
//!
//! On a Linux host. On SlateOS, not yet, for the PCM device's reason: a
//! native program's `ioctl` on a sound device answers `ENOTTY`
//! (`requests/e-ad-no-application-can-reach-the-sound-device.md`), so
//! [`Master::open`] fails, and says why; it works unchanged once a native
//! program can reach the card.

use std::fmt;

use crate::pcm::Errno;

/// The control device: card 0's.
pub const CONTROL_PATH: &str = "/dev/snd/controlC0";

/// `sizeof(struct snd_ctl_elem_id)`.
pub const ELEM_ID_SIZE: usize = 64;
/// `sizeof(struct snd_ctl_elem_info)` on x86-64.
pub const ELEM_INFO_SIZE: usize = 272;
/// `sizeof(struct snd_ctl_elem_value)` on x86-64.
pub const ELEM_VALUE_SIZE: usize = 1224;

// The requests, as `_IOC(dir, 'U', nr, size)` encodes them, written out and
// held to the formula and to the kernel's own asserted values by a test
// (`kernel/src/audio_alsa_ctl.rs`, `self_test`).

/// `SNDRV_CTL_IOCTL_ELEM_INFO` (`_IOWR`, 0x11, 272 bytes): what an element
/// is -- found by its name, answered with its number, type and range.
pub const IOCTL_ELEM_INFO: u32 = 0xC110_5511;
/// `SNDRV_CTL_IOCTL_ELEM_READ` (`_IOWR`, 0x12, 1224 bytes): an element's
/// values.
pub const IOCTL_ELEM_READ: u32 = 0xC4C8_5512;
/// `SNDRV_CTL_IOCTL_ELEM_WRITE` (`_IOWR`, 0x13, 1224 bytes): set them.
pub const IOCTL_ELEM_WRITE: u32 = 0xC4C8_5513;

/// The master volume's name, as every card that has one calls it.
pub const VOLUME_NAME: &str = "Master Playback Volume";
/// The master switch's: on is sounding, off is muted.
pub const SWITCH_NAME: &str = "Master Playback Switch";

/// `SNDRV_CTL_ELEM_IFACE_MIXER`: where a card's volumes are.
const IFACE_MIXER: i32 = 2;
/// `SNDRV_CTL_ELEM_TYPE_BOOLEAN`.
const TYPE_BOOLEAN: i32 = 1;
/// `SNDRV_CTL_ELEM_TYPE_INTEGER`.
const TYPE_INTEGER: i32 = 2;
/// `SNDRV_CTL_ELEM_ACCESS_READ`.
const ACCESS_READ: u32 = 1;
/// `SNDRV_CTL_ELEM_ACCESS_WRITE`.
const ACCESS_WRITE: u32 = 1 << 1;
/// The most values one element holds: `value.integer.value[128]`.
pub const MAX_VALUES: usize = 128;
/// An element name's room, its terminating NUL included.
const NAME_BYTES: usize = 44;

/// Where the payloads' fields are.
mod at {
    // struct snd_ctl_elem_id, at the start of every payload here.
    /// `id.numid`.
    pub const ID_NUMID: usize = 0;
    /// `id.iface`.
    pub const ID_IFACE: usize = 4;
    /// `id.device`.
    pub const ID_DEVICE: usize = 8;
    /// `id.subdevice`.
    pub const ID_SUBDEVICE: usize = 12;
    /// `id.name`, 44 bytes.
    pub const ID_NAME: usize = 16;
    /// `id.index`.
    pub const ID_INDEX: usize = 60;
    // struct snd_ctl_elem_info, after its id.
    /// `type`.
    pub const INFO_TYPE: usize = 64;
    /// `access`.
    pub const INFO_ACCESS: usize = 68;
    /// `count`: how many values.
    pub const INFO_COUNT: usize = 72;
    /// `value.integer.min`, 8-aligned after `owner`.
    pub const INFO_MIN: usize = 80;
    /// `value.integer.max`.
    pub const INFO_MAX: usize = 88;
    // struct snd_ctl_elem_value, after its id.
    /// `value.integer.value[0]`, 8-aligned after the `indirect` word.
    pub const VALUE_VALUES: usize = 72;
}

/// The control device's one call: `ioctl(device, request, payload)`, where
/// `payload` is `request`'s size.
pub trait ControlSys {
    /// # Errors
    ///
    /// The kernel's errno.
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno>;
}

impl<S: ControlSys + ?Sized> ControlSys for Box<S> {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        (**self).ioctl(request, payload)
    }
}

impl<S: ControlSys + ?Sized> ControlSys for &mut S {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        (**self).ioctl(request, payload)
    }
}

/// Why the master volume could not be had, or set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerError {
    /// The control device would not open.
    Open(Errno),
    /// The card has no master volume this can set: none of that name, or
    /// one that is not a number, cannot be both read and written, or runs
    /// over no range.
    NoMaster,
    /// The card refused a request.
    Refused(Errno),
    /// The card's answer could not be read.
    Garbled,
}

impl fmt::Display for MixerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(e) => write!(f, "the sound card's controls would not open ({e})"),
            Self::NoMaster => f.write_str("the sound card has no master volume"),
            Self::Refused(e) => write!(f, "the sound card refused the request ({e})"),
            Self::Garbled => f.write_str("the sound card's answer could not be read"),
        }
    }
}

impl std::error::Error for MixerError {}

/// One element: its number, and what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Element {
    numid: u32,
    /// Values: one a channel.
    count: usize,
    min: i64,
    max: i64,
}

/// A sound card's master volume and its mute.
#[derive(Debug)]
pub struct Master<S: ControlSys> {
    sys: S,
    volume: Element,
    /// `None` for a card with no master switch, muted by its volume.
    switch: Option<Element>,
    /// For a card muted by its volume: the level it was at, to go back to.
    muted_at: Option<u8>,
    /// The level last set, and the value it was set as: read back while the
    /// card still holds that value, it is that level -- a card of 87 steps
    /// set to 50 holds 44, which is 51 of 100, and a slider moved to 50
    /// must not then read 51.
    set: Option<(u8, i64)>,
}

impl<S: ControlSys> Master<S> {
    /// The card behind `sys`'s master volume and switch, found by name.
    ///
    /// # Errors
    ///
    /// [`MixerError::NoMaster`] for a card with no master volume this can
    /// set; [`MixerError::Refused`] when the card will not answer -- as
    /// every request on SlateOS does today (see the module's docs).
    pub fn open(mut sys: S) -> Result<Self, MixerError> {
        let volume = match find(&mut sys, VOLUME_NAME, TYPE_INTEGER) {
            Ok(Some(volume)) if volume.max > volume.min => volume,
            Ok(_) | Err(MixerError::Refused(Errno::ENOENT)) => return Err(MixerError::NoMaster),
            Err(e) => return Err(e),
        };
        // A card without a switch is muted by its volume; one that will not
        // say whether it has one is not a card this can trust to answer.
        let switch = match find(&mut sys, SWITCH_NAME, TYPE_BOOLEAN) {
            Ok(switch) => switch,
            Err(MixerError::Refused(Errno::ENOENT)) => None,
            Err(e) => return Err(e),
        };
        Ok(Self {
            sys,
            volume,
            switch,
            muted_at: None,
            set: None,
        })
    }

    /// Whether the card mutes with a switch of its own, rather than by its
    /// volume.
    #[must_use]
    pub const fn has_switch(&self) -> bool {
        self.switch.is_some()
    }

    /// The master volume, 0 to 100: its channels' mean, as a share of the
    /// card's range. While muted by the volume, the level it goes back to.
    ///
    /// # Errors
    ///
    /// The card refused, or answered what could not be read.
    pub fn level(&mut self) -> Result<u8, MixerError> {
        if let Some(level) = self.muted_at {
            return Ok(level);
        }
        let values = read(&mut self.sys, self.volume)?;
        if let Some((level, raw)) = self.set
            && values.iter().all(|&v| v == raw)
        {
            return Ok(level);
        }
        let count = i128::try_from(values.len()).map_err(|_| MixerError::Garbled)?;
        let sum: i128 = values.iter().map(|&v| i128::from(v)).sum();
        Ok(percent_of_sum(sum, count, self.volume.min, self.volume.max))
    }

    /// Whether the card is muted: its switch off on every channel -- or,
    /// for a card with none, muted here by its volume.
    ///
    /// # Errors
    ///
    /// The card refused, or answered what could not be read.
    pub fn muted(&mut self) -> Result<bool, MixerError> {
        match self.switch {
            Some(switch) => Ok(read(&mut self.sys, switch)?.iter().all(|&v| v == 0)),
            None => Ok(self.muted_at.is_some()),
        }
    }

    /// Set the master volume to `level` of 100 (more is 100), every channel
    /// alike. While muted by the volume, only the level to go back to.
    ///
    /// # Errors
    ///
    /// The card refused.
    pub fn set_level(&mut self, level: u8) -> Result<(), MixerError> {
        let level = level.min(100);
        if self.muted_at.is_some() {
            self.muted_at = Some(level);
            return Ok(());
        }
        self.write_level(level)
    }

    /// Write `level` to every channel, and remember it was asked for.
    fn write_level(&mut self, level: u8) -> Result<(), MixerError> {
        let raw = raw_of(level, self.volume.min, self.volume.max);
        write(&mut self.sys, self.volume, raw)?;
        self.set = Some((level, raw));
        Ok(())
    }

    /// Mute the card, or let it sound: its switch, every channel alike --
    /// or, for a card with none, its volume to the bottom and back.
    ///
    /// # Errors
    ///
    /// The card refused, or answered what could not be read.
    pub fn set_muted(&mut self, muted: bool) -> Result<(), MixerError> {
        if let Some(switch) = self.switch {
            return write(&mut self.sys, switch, i64::from(!muted));
        }
        match (muted, self.muted_at) {
            (true, None) => {
                let level = self.level()?;
                write(&mut self.sys, self.volume, self.volume.min)?;
                self.muted_at = Some(level);
                Ok(())
            }
            (false, Some(level)) => {
                self.write_level(level)?;
                self.muted_at = None;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The device, given back.
    pub fn into_inner(self) -> S {
        self.sys
    }
}

/// The element named `name`, if the card has one of type `ty` that can be
/// read and written and holds between one and [`MAX_VALUES`] values;
/// `Ok(None)` for one that is not that.
fn find<S: ControlSys>(sys: &mut S, name: &str, ty: i32) -> Result<Option<Element>, MixerError> {
    let mut info = [0u8; ELEM_INFO_SIZE];
    put_id(&mut info, 0, name);
    sys.ioctl(IOCTL_ELEM_INFO, &mut info)
        .map_err(MixerError::Refused)?;
    let numid = get_u32(&info, at::ID_NUMID).ok_or(MixerError::Garbled)?;
    let found_ty = get_i32(&info, at::INFO_TYPE).ok_or(MixerError::Garbled)?;
    let access = get_u32(&info, at::INFO_ACCESS).ok_or(MixerError::Garbled)?;
    let count = get_u32(&info, at::INFO_COUNT).ok_or(MixerError::Garbled)?;
    let count = usize::try_from(count).map_err(|_| MixerError::Garbled)?;
    let (min, max) = if ty == TYPE_BOOLEAN {
        (0, 1)
    } else {
        (
            get_i64(&info, at::INFO_MIN).ok_or(MixerError::Garbled)?,
            get_i64(&info, at::INFO_MAX).ok_or(MixerError::Garbled)?,
        )
    };
    let usable = numid != 0
        && found_ty == ty
        && access & (ACCESS_READ | ACCESS_WRITE) == ACCESS_READ | ACCESS_WRITE
        && (1..=MAX_VALUES).contains(&count);
    Ok(usable.then_some(Element {
        numid,
        count,
        min,
        max,
    }))
}

/// `element`'s values, one a channel.
fn read<S: ControlSys>(sys: &mut S, element: Element) -> Result<Vec<i64>, MixerError> {
    let mut value = [0u8; ELEM_VALUE_SIZE];
    put_u32(&mut value, at::ID_NUMID, element.numid);
    put_i32(&mut value, at::ID_IFACE, IFACE_MIXER);
    sys.ioctl(IOCTL_ELEM_READ, &mut value)
        .map_err(MixerError::Refused)?;
    (0..element.count)
        .map(|channel| {
            value_at(channel)
                .and_then(|off| get_i64(&value, off))
                .ok_or(MixerError::Garbled)
        })
        .collect()
}

/// Set every one of `element`'s values to `raw`.
fn write<S: ControlSys>(sys: &mut S, element: Element, raw: i64) -> Result<(), MixerError> {
    let mut value = [0u8; ELEM_VALUE_SIZE];
    put_u32(&mut value, at::ID_NUMID, element.numid);
    put_i32(&mut value, at::ID_IFACE, IFACE_MIXER);
    for channel in 0..element.count {
        put_i64(
            &mut value,
            value_at(channel).ok_or(MixerError::Garbled)?,
            raw,
        );
    }
    sys.ioctl(IOCTL_ELEM_WRITE, &mut value)
        .map_err(MixerError::Refused)
}

/// Where channel `channel`'s value is in an element value's payload:
/// `value.integer.value[channel]`, for a channel below [`MAX_VALUES`].
fn value_at(channel: usize) -> Option<usize> {
    if channel >= MAX_VALUES {
        return None;
    }
    channel.checked_mul(8)?.checked_add(at::VALUE_VALUES)
}

/// The mean of `count` values summing to `sum` as a share of `min..=max`,
/// 0 to 100, rounded to the nearest -- from the sum, so two channels at 0
/// and 87 of 87 are 50, not the share of their mean rounded first. A mean
/// outside the range is at its end, and a range of nothing is 0.
fn percent_of_sum(sum: i128, count: i128, min: i64, max: i64) -> u8 {
    if max <= min || count <= 0 {
        return 0;
    }
    let lo = i128::from(min).saturating_mul(count);
    let hi = i128::from(max).saturating_mul(count);
    let into = sum.clamp(lo, hi).saturating_sub(lo);
    let span = hi.saturating_sub(lo);
    let percent = div_round(into.saturating_mul(100), span).unwrap_or(0);
    u8::try_from(percent.clamp(0, 100)).unwrap_or(100)
}

/// `level` of 100 as a value in `min..=max`, rounded to the nearest; for a
/// range of nothing, its bottom.
fn raw_of(level: u8, min: i64, max: i64) -> i64 {
    if max <= min {
        return min;
    }
    let (lo, hi) = (i128::from(min), i128::from(max));
    let span = hi.saturating_sub(lo);
    let into = div_round(span.saturating_mul(i128::from(level.min(100))), 100).unwrap_or(0);
    i64::try_from(lo.saturating_add(into).clamp(lo, hi)).unwrap_or(max)
}

/// `n / d` rounded half away from nought, for `n >= 0` and `d > 0`; `None`
/// for a `d` that is not.
fn div_round(n: i128, d: i128) -> Option<i128> {
    if d <= 0 {
        return None;
    }
    n.checked_add(d / 2)?.checked_div(d)
}

/// Write an element id for `numid` -- or, with 0, for the mixer element
/// named `name` -- at the start of `payload`.
fn put_id(payload: &mut [u8], numid: u32, name: &str) {
    put_u32(payload, at::ID_NUMID, numid);
    put_i32(payload, at::ID_IFACE, IFACE_MIXER);
    put_u32(payload, at::ID_DEVICE, 0);
    put_u32(payload, at::ID_SUBDEVICE, 0);
    put_u32(payload, at::ID_INDEX, 0);
    let name = name.as_bytes();
    let n = name.len().min(NAME_BYTES.saturating_sub(1));
    if let (Some(dst), Some(src)) = (
        payload.get_mut(at::ID_NAME..at::ID_NAME.saturating_add(n)),
        name.get(..n),
    ) {
        dst.copy_from_slice(src);
    }
}

/// The NUL-ended name in an element id at the start of `payload`.
fn id_name(payload: &[u8]) -> &[u8] {
    let field = payload
        .get(at::ID_NAME..at::ID_NAME.saturating_add(NAME_BYTES))
        .unwrap_or(&[]);
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    field.get(..end).unwrap_or(&[])
}

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    if let Some(dst) = buf.get_mut(at..at.saturating_add(4)) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_i32(buf: &mut [u8], at: usize, v: i32) {
    if let Some(dst) = buf.get_mut(at..at.saturating_add(4)) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_i64(buf: &mut [u8], at: usize, v: i64) {
    if let Some(dst) = buf.get_mut(at..at.saturating_add(8)) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

fn get_u32(buf: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = buf.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn get_i32(buf: &[u8], at: usize) -> Option<i32> {
    let bytes: [u8; 4] = buf.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(i32::from_le_bytes(bytes))
}

fn get_i64(buf: &[u8], at: usize) -> Option<i64> {
    let bytes: [u8; 8] = buf.get(at..at.checked_add(8)?)?.try_into().ok()?;
    Some(i64::from_le_bytes(bytes))
}

// ---------------------------------------------------------------------------
// A card in memory
// ---------------------------------------------------------------------------

/// A sound card held in memory, answering as the kernel's does
/// (`kernel/src/syscall/linux.rs`, `alsa_control_ioctl_elem_*`): a master
/// volume and a master switch, found by name or number, each value kept to
/// its range. For the tests of a program that sets the volume, so they set
/// none of the machine's.
///
/// By default it is SlateOS's card: one value from 0 to 100, and a switch.
/// A test makes it another -- two channels over another range, no switch --
/// or one that refuses every request with an errno, as a native program's
/// `ioctl` on SlateOS does today.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Simulated {
    /// The volume's values, one a channel.
    pub volume: Vec<i64>,
    /// The bottom and top of its range.
    pub range: (i64, i64),
    /// The switch's values, one a channel -- `None` for a card with none.
    pub switch: Option<Vec<i64>>,
    /// The errno every request answers with, when it is not `None`.
    pub refuse: Option<Errno>,
    /// Every request made of it, in order.
    pub requests: Vec<u32>,
}

/// The simulated volume's number.
const SIM_VOLUME: u32 = 1;
/// The simulated switch's number.
const SIM_SWITCH: u32 = 2;

impl Simulated {
    /// SlateOS's card, at `level` of 100, muted or not.
    #[must_use]
    pub fn new(level: i64, muted: bool) -> Self {
        Self {
            volume: vec![level],
            range: (0, 100),
            switch: Some(vec![i64::from(!muted)]),
            refuse: None,
            requests: Vec::new(),
        }
    }

    /// The element `payload`'s id names: by number, or by name with none.
    fn named(&self, payload: &[u8]) -> Option<u32> {
        let numid = get_u32(payload, at::ID_NUMID)?;
        let has_switch = self.switch.is_some();
        match numid {
            SIM_VOLUME => Some(SIM_VOLUME),
            SIM_SWITCH if has_switch => Some(SIM_SWITCH),
            0 => {
                if get_i32(payload, at::ID_IFACE)? != IFACE_MIXER {
                    return None;
                }
                let name = id_name(payload);
                if name == VOLUME_NAME.as_bytes() {
                    Some(SIM_VOLUME)
                } else if has_switch && name == SWITCH_NAME.as_bytes() {
                    Some(SIM_SWITCH)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn values(&self, numid: u32) -> &[i64] {
        match numid {
            SIM_VOLUME => &self.volume,
            _ => self.switch.as_deref().unwrap_or(&[]),
        }
    }
}

impl ControlSys for Simulated {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        self.requests.push(request);
        if let Some(e) = self.refuse {
            return Err(e);
        }
        let numid = self.named(payload).ok_or(Errno::ENOENT)?;
        let count = self.values(numid).len();
        match request {
            IOCTL_ELEM_INFO if payload.len() == ELEM_INFO_SIZE => {
                let (ty, (min, max)) = if numid == SIM_VOLUME {
                    (TYPE_INTEGER, self.range)
                } else {
                    (TYPE_BOOLEAN, (0, 1))
                };
                let name = if numid == SIM_VOLUME {
                    VOLUME_NAME
                } else {
                    SWITCH_NAME
                };
                payload.fill(0);
                put_id(payload, numid, name);
                put_i32(payload, at::INFO_TYPE, ty);
                put_u32(payload, at::INFO_ACCESS, ACCESS_READ | ACCESS_WRITE);
                put_u32(
                    payload,
                    at::INFO_COUNT,
                    u32::try_from(count).map_err(|_| Errno::EINVAL)?,
                );
                put_i64(payload, at::INFO_MIN, min);
                put_i64(payload, at::INFO_MAX, max);
                Ok(())
            }
            IOCTL_ELEM_READ if payload.len() == ELEM_VALUE_SIZE => {
                let values = self.values(numid).to_vec();
                for (channel, v) in values.into_iter().enumerate() {
                    put_i64(payload, value_at(channel).ok_or(Errno::EINVAL)?, v);
                }
                Ok(())
            }
            IOCTL_ELEM_WRITE if payload.len() == ELEM_VALUE_SIZE => {
                let (min, max) = if numid == SIM_VOLUME {
                    self.range
                } else {
                    (0, 1)
                };
                // As the kernel keeps a written value to its range.
                let written: Vec<i64> = (0..count)
                    .map(|channel| {
                        value_at(channel)
                            .and_then(|off| get_i64(payload, off))
                            .map_or(min, |v| v.clamp(min, max.max(min)))
                    })
                    .collect();
                if numid == SIM_VOLUME {
                    self.volume = written;
                } else {
                    self.switch = Some(written);
                }
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }
}

// ---------------------------------------------------------------------------
// The device
// ---------------------------------------------------------------------------

/// Whether this build's system is one the control device is opened on: a
/// Linux-like target, where the C library's `ioctl` is what reaches it.
pub const DEVICE_SUPPORTED: bool = crate::pcm::DEVICE_SUPPORTED;

/// The control device, open.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Debug)]
pub struct Device {
    file: std::fs::File,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl Device {
    /// Open card 0's control device.
    ///
    /// # Errors
    ///
    /// The errno the open met.
    pub fn open() -> Result<Self, Errno> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(CONTROL_PATH)
            .map(|file| Self { file })
            .map_err(|e| Errno(e.raw_os_error().unwrap_or(Errno::ENODEV.0)))
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl ControlSys for Device {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        use std::os::fd::AsRawFd;
        crate::sys::ioctl(self.file.as_raw_fd(), request, payload)
    }
}

/// No device: this build's system is not one the control device is opened
/// on.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
#[derive(Debug)]
pub struct Device;

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
impl ControlSys for Device {
    fn ioctl(&mut self, _request: u32, _payload: &mut [u8]) -> Result<(), Errno> {
        Err(Errno::ENOSYS)
    }
}

/// Card 0's master volume and switch, through its control device.
///
/// # Errors
///
/// [`MixerError::Open`] when the device will not open -- with
/// [`Errno::ENOSYS`] on a system this crate does not open it on -- and
/// [`Master::open`]'s.
pub fn open_master() -> Result<Master<Device>, MixerError> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Master::open(Device::open().map_err(MixerError::Open)?)
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        Err(MixerError::Open(Errno::ENOSYS))
    }
}

#[cfg(test)]
#[path = "mixer_tests.rs"]
mod tests;
