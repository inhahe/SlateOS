//! The encoders, ported from liblzma 5.2.5: what `xz` writes, byte for byte.
//!
//! | liblzma | here |
//! |---|---|
//! | `lz/lz_encoder.c`, `lz_encoder_mf.c`, `lz_encoder_hash.h` | `mf` |
//! | `rangecoder/range_encoder.h` | `rc` |
//! | `rangecoder/price.h`, `price_table.c`, `lzma/fastpos.h` | `price` |
//! | `lzma/lzma_encoder.c`, `lzma_encoder_private.h` | `lzma` |
//! | `lzma/lzma_encoder_optimum_fast.c`, `lzma_encoder_optimum_normal.c` | `optimum` |
//! | `lzma/lzma2_encoder.c` | `lzma2` |
//! | `lzma/lzma_encoder_presets.c` | [`LzmaOptions::preset`] |
//! | `common/stream_encoder.c`, `block_encoder.c`, `block_header_encoder.c`, `index_encoder.c`, `alone_encoder.c` | `container` |
//!
//! liblzma encodes a buffer at a time; this encodes the whole input in one
//! call. The output is the same: liblzma only runs its match finder on a
//! position when more input lies beyond it than any lookahead takes, so no
//! decision depends on where its buffers ended (`mf` says more).

mod container;
mod lzma;
mod lzma2;
mod mf;
mod optimum;
mod price;
mod rc;

use alloc::vec::Vec;

pub use mf::MatchFinder;

use crate::filters::Bcj;
use crate::{Check, Error, Result};

/// `LZMA_DICT_SIZE_MIN`, and the largest dictionary liblzma's encoder takes
/// (1.5 GiB).
const DICT_SIZE_MIN: u32 = 4096;
const DICT_SIZE_MAX: u32 = (1 << 30) + (1 << 29);

/// How the encoder chooses what to code (`lzma_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A quick greedy choice, one byte ahead (presets 0 to 3).
    Fast,
    /// Pricing every way through the next few thousand bytes (presets 4 to
    /// 9, and the extreme presets).
    Normal,
}

/// A compression level, 0 to 9, maybe "extreme" (`xz -0` ... `xz -9e`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    level: u8,
    extreme: bool,
}

impl Preset {
    /// `xz`'s default, level 6.
    pub const DEFAULT: Self = Self {
        level: 6,
        extreme: false,
    };

    /// Level `level`, or `None` above 9.
    #[must_use]
    pub const fn new(level: u32) -> Option<Self> {
        if level > 9 {
            None
        } else {
            Some(Self {
                level: level as u8,
                extreme: false,
            })
        }
    }

    /// The same level, extreme (`-e`): slower, and sometimes a little
    /// smaller.
    #[must_use]
    pub const fn extreme(self) -> Self {
        Self {
            level: self.level,
            extreme: true,
        }
    }

    /// The level, 0 to 9.
    #[must_use]
    pub const fn level(self) -> u32 {
        self.level as u32
    }

    /// Whether it is an extreme preset.
    #[must_use]
    pub const fn is_extreme(self) -> bool {
        self.extreme
    }
}

/// The LZMA and LZMA2 encoder's settings (`lzma_options_lzma`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LzmaOptions {
    /// The dictionary size: 4 KiB to 1.5 GiB.
    pub dict_size: u32,
    /// Literal context bits, 0 to 4.
    pub lc: u32,
    /// Literal position bits, 0 to 4; `lc + lp` at most 4.
    pub lp: u32,
    /// Position bits, 0 to 4.
    pub pb: u32,
    /// How choices are made.
    pub mode: Mode,
    /// A match this long is taken without looking further: from the match
    /// finder's hash length to 273.
    pub nice_len: u32,
    /// The match finder.
    pub match_finder: MatchFinder,
    /// How many candidates the match finder tries; 0 for its default.
    pub depth: u32,
}

impl LzmaOptions {
    /// `lzma_lzma_preset`: the settings `xz -N` uses.
    #[must_use]
    pub const fn preset(preset: Preset) -> Self {
        let level = preset.level;
        // `dict_pow2[]` and the fast presets' `depths[]`.
        let dict_pow2 = match level {
            0 => 18,
            1 => 20,
            2 => 21,
            3 | 4 => 22,
            5 | 6 => 23,
            7 => 24,
            8 => 25,
            _ => 26,
        };
        let fast_depth = match level {
            0 => 4,
            1 => 8,
            2 => 24,
            _ => 48,
        };
        let (mode, match_finder, mut nice_len, mut depth) = if level <= 3 {
            (
                Mode::Fast,
                if level == 0 {
                    MatchFinder::Hc3
                } else {
                    MatchFinder::Hc4
                },
                if level <= 1 { 128 } else { 273 },
                fast_depth,
            )
        } else {
            (
                Mode::Normal,
                MatchFinder::Bt4,
                match level {
                    4 => 16,
                    5 => 32,
                    _ => 64,
                },
                0,
            )
        };
        let mut mode = mode;
        let mut match_finder = match_finder;
        if preset.extreme {
            mode = Mode::Normal;
            match_finder = MatchFinder::Bt4;
            if level == 3 || level == 5 {
                nice_len = 192;
                depth = 0;
            } else {
                nice_len = 273;
                depth = 512;
            }
        }
        Self {
            dict_size: 1 << dict_pow2,
            lc: 3,
            lp: 0,
            pb: 2,
            mode,
            nice_len,
            match_finder,
            depth,
        }
    }

    /// The checks of `is_options_valid` and `lz_encoder_prepare`:
    /// [`Error::Unsupported`] (liblzma's `LZMA_OPTIONS_ERROR`) for settings
    /// liblzma refuses.
    pub(crate) fn validate(&self) -> Result<()> {
        let lclp_ok =
            self.lc <= 4 && self.lp <= 4 && self.lc.saturating_add(self.lp) <= 4 && self.pb <= 4;
        let nice_ok =
            self.nice_len >= self.match_finder.hash_bytes().max(2) && self.nice_len <= 273;
        let dict_ok = (DICT_SIZE_MIN..=DICT_SIZE_MAX).contains(&self.dict_size);
        if lclp_ok && nice_ok && dict_ok {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }

    /// The LZMA2 property byte for these settings: the dictionary size, as
    /// a 7z LZMA2 coder or an `.xz` filter records it.
    #[must_use]
    pub const fn lzma2_prop(&self) -> u8 {
        lzma2::dict_size_prop(self.dict_size)
    }
}

/// A filter before LZMA2 in an `.xz` block (`xz --x86`, `--delta`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreFilter {
    /// The delta filter (`--delta=dist=N`), `distance` 1 to 256.
    Delta {
        /// The byte distance.
        distance: u32,
    },
    /// A branch converter, positions numbered from `start` (`--x86=start=N`),
    /// which must keep the architecture's alignment.
    Bcj {
        /// The architecture.
        arch: Bcj,
        /// The position of the first byte.
        start: u32,
    },
}

/// What an `.xz` file is written with: `xz`'s options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XzOptions {
    /// The integrity check (`--check`).
    pub check: Check,
    /// The filters before LZMA2, the first applied first -- the order `xz`'s
    /// command line names them in. At most three.
    pub filters: Vec<PreFilter>,
    /// LZMA2's settings.
    pub lzma2: LzmaOptions,
    /// `--block-size`: blocks of this many bytes, each compressed alone;
    /// `None` for one block.
    pub block_size: Option<u64>,
}

impl XzOptions {
    /// What `xz -N` writes: CRC-64, LZMA2 at the preset, one block.
    #[must_use]
    pub const fn preset(preset: Preset) -> Self {
        Self {
            check: Check::CRC64,
            filters: Vec::new(),
            lzma2: LzmaOptions::preset(preset),
            block_size: None,
        }
    }

    fn validate(&self) -> Result<()> {
        self.lzma2.validate()?;
        let filters_ok = self.filters.len() <= 3
            && self.filters.iter().all(|f| match *f {
                PreFilter::Delta { distance } => (1..=256).contains(&distance),
                PreFilter::Bcj { arch, start } => start & arch.alignment().wrapping_sub(1) == 0,
            });
        let check_ok = self.check.is_supported();
        if filters_ok && check_ok && self.block_size != Some(0) {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }
}

/// `.xz` as `xz -N` writes it.
pub(crate) fn xz_preset(data: &[u8], preset: Preset) -> Vec<u8> {
    container::xz(data, &XzOptions::preset(preset))
}

/// `.xz` with chosen options.
pub(crate) fn xz(data: &[u8], options: &XzOptions) -> Result<Vec<u8>> {
    options.validate()?;
    Ok(container::xz(data, options))
}

/// `.lzma` with chosen options.
pub(crate) fn lzma_alone(data: &[u8], options: &LzmaOptions) -> Result<Vec<u8>> {
    options.validate()?;
    Ok(container::lzma(data, options))
}

/// Raw LZMA2.
pub(crate) fn lzma2_raw(data: &[u8], options: &LzmaOptions) -> Result<Vec<u8>> {
    options.validate()?;
    let mut out = Vec::with_capacity((data.len() / 2).saturating_add(16));
    lzma2::encode(data, options, &mut out);
    Ok(out)
}

/// Raw LZMA with the end-of-payload marker.
pub(crate) fn lzma1_raw(data: &[u8], options: &LzmaOptions) -> Result<Vec<u8>> {
    options.validate()?;
    let mut out = Vec::with_capacity((data.len() / 2).saturating_add(16));
    container::raw_lzma1(data, options, &mut out);
    Ok(out)
}
