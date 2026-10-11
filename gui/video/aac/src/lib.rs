//! AAC sound, decoded as FFmpeg's fixed-point decoder decodes it: a
//! translation of FFmpeg 8.1.3's `aac_fixed` (`libavcodec/aac/`), module by
//! module, held bit for bit to that decoder's output (design-decisions
//! §1377).
//!
//! Fixed point, so every sample has exactly one right value, the one
//! FFmpeg's C computes; its handling of damaged streams comes with it. The
//! object types it decodes are FFmpeg's fixed-point decoder's: AAC Main,
//! LC and LTP, their error-resilient forms with LD and ELD, and HE-AAC's
//! SBR and PS over them. USAC (xHE-AAC), which that decoder refuses, is
//! refused here too.
//!
//! What is translated so far, and from where:
//!
//! - [`mpeg4audio`]: the AudioSpecificConfig's common part
//!   (`mpeg4audio.c`).
//! - [`layout`]: which channel each element is, and the order FFmpeg's
//!   output gives them in (`aacdec.c`, `aacdec_tab.c`).
//!
//! The oracle the crate is held to is `tools/ffaac/build.sh`'s FFmpeg and
//! `tools/pcm_expect.py`'s record of what it decodes.

#![forbid(unsafe_code)]
#![allow(
    dead_code,
    reason = "translated a module at a time: what nothing calls yet is called once the \
              decoder above it is in, and this allow goes then"
)]

pub(crate) mod layout;
pub(crate) mod mpeg4audio;
