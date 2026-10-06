//! A fake PCM device for the tests: it answers as the kernel's does
//! (`kernel/src/syscall/linux.rs`, `alsa_pcm_ioctl`; `kernel/src/ipc/
//! alsa_pcm.rs`) -- the mixer's configuration whatever is asked, a ring of
//! [`RING_FRAMES`] that takes what fits and answers `EAGAIN` when full, a
//! `STATUS` whose `delay` is what is queued -- and records every call, so a
//! test can say what was asked, in what order, and what was written.
//!
//! Time is the fake's to keep: a [`PcmSys::pause`] of `ms` milliseconds
//! plays `ms` milliseconds of the queue, unless the fake is told the ring
//! never drains, or drains slower than it should.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use crate::pcm::{
    Errno, IOCTL_DRAIN, IOCTL_DROP, IOCTL_HW_PARAMS, IOCTL_PREPARE, IOCTL_STATUS, PcmSys,
    RING_FRAMES, hw_params_request,
};

/// A call the fake was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Ioctl(u32),
    Write(usize),
    Pause(u64),
}

/// The fake device.
#[derive(Debug)]
pub struct Fake {
    /// Every call, in order.
    pub calls: Vec<Call>,
    /// Every byte the ring took, in order.
    pub written: Vec<u8>,
    /// Frames queued and not yet played.
    pub queued: u64,
    /// Whether a pause plays any of the queue.
    pub drains: bool,
    /// How many frames a second of pause plays: the mixer's [`RATE`],
    /// unless a test slows the device down.
    ///
    /// [`RATE`]: crate::pcm::RATE
    pub rate: u64,
    /// How `HW_PARAMS` answers: the payload it writes back, or the errno.
    pub configure: Result<Vec<u8>, Errno>,
    /// The errno the next write answers with, once, instead of taking.
    pub refuse_write: Option<Errno>,
    /// The most bytes one write takes, whatever the room.
    pub largest_write: usize,
}

impl Fake {
    /// A device that answers as the kernel's does.
    pub fn new() -> Self {
        Self {
            calls: Vec::new(),
            written: Vec::new(),
            queued: 0,
            drains: true,
            rate: u64::from(crate::pcm::RATE),
            configure: Ok(hw_params_request().to_vec()),
            refuse_write: None,
            largest_write: usize::MAX,
        }
    }

    /// The ioctl requests, in order.
    pub fn ioctls(&self) -> Vec<u32> {
        self.calls
            .iter()
            .filter_map(|c| match c {
                Call::Ioctl(r) => Some(*r),
                _ => None,
            })
            .collect()
    }
}

impl PcmSys for Fake {
    fn ioctl(&mut self, request: u32, payload: &mut [u8]) -> Result<(), Errno> {
        self.calls.push(Call::Ioctl(request));
        match request {
            IOCTL_HW_PARAMS => {
                let reply = self.configure.clone()?;
                assert_eq!(payload.len(), reply.len(), "HW_PARAMS's payload size");
                payload.copy_from_slice(&reply);
                Ok(())
            }
            IOCTL_STATUS => {
                assert_eq!(
                    payload.len(),
                    crate::pcm::STATUS_SIZE,
                    "STATUS's payload size"
                );
                payload.fill(0);
                let state: u32 = if self.queued > 0 { 3 } else { 2 };
                payload[0..4].copy_from_slice(&state.to_le_bytes());
                let delay = i64::try_from(self.queued).unwrap_or(i64::MAX);
                payload[56..64].copy_from_slice(&delay.to_le_bytes());
                let avail = u64::from(RING_FRAMES).saturating_sub(self.queued);
                payload[64..72].copy_from_slice(&avail.to_le_bytes());
                Ok(())
            }
            IOCTL_PREPARE | IOCTL_DRAIN | IOCTL_DROP => {
                assert!(payload.is_empty(), "no payload for {request:#x}");
                if request == IOCTL_DROP {
                    self.queued = 0;
                }
                Ok(())
            }
            other => panic!("an ioctl the kernel would not expect here: {other:#x}"),
        }
    }

    fn write(&mut self, bytes: &[u8]) -> Result<usize, Errno> {
        if let Some(e) = self.refuse_write.take() {
            self.calls.push(Call::Write(0));
            return Err(e);
        }
        let room = usize::try_from(u64::from(RING_FRAMES).saturating_sub(self.queued)).unwrap_or(0)
            * crate::pcm::FRAME_BYTES;
        let taken = bytes.len().min(room).min(self.largest_write);
        self.calls.push(Call::Write(taken));
        if taken == 0 {
            return Err(Errno::EAGAIN);
        }
        self.written.extend_from_slice(&bytes[..taken]);
        self.queued += u64::try_from(taken / crate::pcm::FRAME_BYTES).unwrap_or(0);
        Ok(taken)
    }

    fn pause(&mut self, ms: u64) {
        self.calls.push(Call::Pause(ms));
        if self.drains {
            let played = self.rate * ms / 1000;
            self.queued = self.queued.saturating_sub(played);
        }
    }
}
