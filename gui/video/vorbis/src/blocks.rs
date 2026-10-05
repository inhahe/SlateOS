//! A packet's block size without decoding it, for a container that times
//! packets as it reads them.
//!
//! An Ogg page gives one time, its granule position, for the last packet
//! ending on it; a demuxer that times every packet (FFmpeg's, and so this
//! project's) needs each packet's length in samples, which is a quarter of
//! the previous block and a quarter of its own. A packet's first bits say
//! its mode, and the mode its block size; a long block's next two bits say
//! the sizes of the blocks either side, which its window was shaped for.
//! [`Blocks`] reads just that much -- what Tremor's `vorbis_packet_blocksize`
//! reads, and FFmpeg's `vorbis_parser.c` -- from the setup header's modes,
//! with none of the decoder's tables kept.

use crate::Error;
use crate::bitpack::BitReader;
use crate::info::{Info, Setup};
use crate::mapping::ilog_below;

/// A stream's block sizes and modes: what a packet's first bits mean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocks {
    sizes: [usize; 2],
    /// Each mode's block, long or not, by mode number.
    modes: Vec<bool>,
    modebits: u32,
}

/// What an audio packet's first bits say of its block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    /// Its mode's number.
    pub mode: usize,
    /// Whether its mode is a long-block one (the mode's block flag): what
    /// decides whether it has window bits, even in a stream whose two
    /// sizes are the same.
    pub long: bool,
    /// Its size, in samples: the stream's short or long block size.
    pub size: usize,
    /// For a long block, the sizes of the blocks before and after it as its
    /// window bits declare them; `None` for a short block, whose window
    /// declares nothing. Tremor shapes the window by these, and overlaps
    /// by the block actually before -- the two agree in any stream an
    /// encoder wrote.
    pub previous: Option<usize>,
    pub next: Option<usize>,
}

impl Blocks {
    /// The stream whose identification and setup headers these are; a
    /// setup header the decoder would refuse is refused here too.
    ///
    /// # Errors
    ///
    /// As [`crate::Decoder::new`].
    pub fn new(id: &[u8], setup: &[u8]) -> Result<Self, Error> {
        let info = Info::parse(id)?;
        let setup = Setup::parse(setup, info.channels)?;
        let modes: Vec<bool> = setup.modes.iter().map(|m| m.blockflag).collect();
        Ok(Self {
            sizes: info.blocksizes,
            modebits: ilog_below(modes.len() as u32),
            modes,
        })
    }

    /// The short and the long block's sizes.
    pub fn sizes(&self) -> [usize; 2] {
        self.sizes
    }

    /// How many modes the stream has: 1 to 64.
    pub fn mode_count(&self) -> usize {
        self.modes.len()
    }

    /// Mode `mode`'s block size; `None` for a mode the stream has not.
    pub fn mode_size(&self, mode: usize) -> Option<usize> {
        self.modes.get(mode).map(|&long| self.size(long))
    }

    /// The block `packet` holds, by its first bits, as the decoder reads
    /// them: a packet the decoder would refuse before decoding anything is
    /// refused here with the same error.
    ///
    /// # Errors
    ///
    /// [`Error::NotAudio`] for a packet whose first bit is set (a header)
    /// or that is empty; [`Error::BadPacket`] for a mode the stream has not,
    /// or a long block's window bits cut off by the packet's end.
    pub fn block(&self, packet: &[u8]) -> Result<Block, Error> {
        let mut opb = BitReader::new(packet);
        if opb.read(1) != 0 {
            return Err(Error::NotAudio);
        }
        let mode = usize::try_from(opb.read(self.modebits)).map_err(|_| Error::BadPacket)?;
        let &long = self.modes.get(mode).ok_or(Error::BadPacket)?;
        let (previous, next) = if long {
            let previous = opb.read(1);
            let next = opb.read(1);
            // The reader's end is sticky: the second fails if either does.
            if next < 0 {
                return Err(Error::BadPacket);
            }
            (Some(self.size(previous != 0)), Some(self.size(next != 0)))
        } else {
            (None, None)
        };
        Ok(Block {
            mode,
            long,
            size: self.size(long),
            previous,
            next,
        })
    }

    fn size(&self, long: bool) -> usize {
        let [short, long_size] = self.sizes;
        if long { long_size } else { short }
    }
}
