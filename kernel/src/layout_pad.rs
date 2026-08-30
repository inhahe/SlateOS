//! Deliberate, build-time-selectable padding at the front of `.text`.
//!
//! # Why a knob whose only effect is to move code around
//!
//! Every benchmark number this project records under QEMU's TCG carries a
//! confound that no amount of repetition can remove. TCG bounds a translation
//! block at the guest 4 KiB page, so a hot loop whose backward branch crosses a
//! page boundary is retranslated far more often and costs roughly 1.7x per
//! iteration. Whether a given loop straddles a boundary is a property of its
//! *address*, not of its code — and relinking the kernel changes the address of
//! every function after the file that was edited. So an unrelated one-line
//! change in an unrelated module can make `aes_encrypt_block` 1.7x slower, and
//! that slowdown:
//!
//!   * reproduces perfectly on every re-run, because the layout is fixed;
//!   * survives every variance guard the harness has, for the same reason;
//!   * and therefore arrives wearing the strongest label the harness can
//!     issue — "replicated", which the reader is invited to read as "caused by
//!     the diff".
//!
//! `scripts/bench-history.py` now warns about this in prose, and
//! `scripts/straddle-check.py` can confirm it after the fact for a specific
//! function. Neither *measures* it. This module is what makes it measurable:
//! it lets us build the same source at several different code placements and
//! observe how much each benchmark moves when nothing but the addresses
//! changed. That spread is the benchmark's layout sensitivity, and a movement
//! inside it is not evidence about the diff no matter how well it replicates.
//!
//! # Using it
//!
//! Set `SLATEOS_TEXT_PAD` to a decimal byte count at build time:
//!
//! ```text
//! SLATEOS_TEXT_PAD=1024 cargo build -p kernel --release
//! ```
//!
//! Unset (the default) means zero padding, which is byte-for-byte the layout we
//! have always built. The kernel reports the value it was built with in its
//! boot banner, so a bench record can say which placement produced it rather
//! than the harness having to remember.
//!
//! **Choose values that are not multiples of 4096.** A pad of exactly one page
//! shifts every function by exactly one page, which preserves every
//! page-straddle relationship in the kernel — it would look like a layout
//! perturbation and perturb nothing. 64, 1024, 2048 and 3072 are useful; 4096
//! and 8192 are not.
//!
//! Note also that function alignment coarsens the shift: the pad is byte-sized,
//! but the code after it is realigned, so a pad of N moves the following
//! functions by N rounded up to their alignment. That is fine for calibration —
//! we need *different* layouts, not a specific offset — but it means a pad of 1
//! and a pad of 15 may well produce the identical image. The build records the
//! kernel's SHA-256 anyway, so an accidental duplicate is visible rather than
//! silently double-counted.

/// Parse the build-time pad size.
///
/// Deliberately strict: an unparseable value fails the build rather than
/// falling back to zero. A typo'd `SLATEOS_TEXT_PAD=1O24` that silently built
/// an unpadded kernel would be recorded as a distinct layout sample that is in
/// fact a duplicate of the baseline, which is precisely the kind of
/// "measurement that cannot fail, presenting as a measurement that found
/// nothing" this whole exercise exists to eliminate.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "const fn: every panic and every overflow here is a compile-time \
              error, not a runtime one, and `<[u8]>::get`/`checked_mul` are \
              usable in const context only as of a later toolchain"
)]
const fn parse_pad(value: Option<&str>) -> usize {
    let bytes = match value {
        Some(text) => text.as_bytes(),
        None => return 0,
    };
    if bytes.is_empty() {
        return 0;
    }
    let mut acc: usize = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let digit = bytes[i];
        if digit < b'0' || digit > b'9' {
            panic!("SLATEOS_TEXT_PAD must be a decimal byte count");
        }
        acc = acc * 10 + (digit - b'0') as usize;
        i += 1;
    }
    // A cap, because the pad lands in the kernel image and the bootloader has
    // to load it. 1 MiB is far more than any layout sweep needs (the whole
    // point is to re-roll 4 KiB page alignment) and a fat-fingered extra zero
    // is much more likely than a legitimate huge value.
    if acc > 1024 * 1024 {
        panic!("SLATEOS_TEXT_PAD is capped at 1048576 bytes");
    }
    acc
}

/// Number of padding bytes prepended to `.text`, as chosen at build time.
///
/// Reported in the boot banner; see `main.rs`.
pub const PAD_BYTES: usize = parse_pad(option_env!("SLATEOS_TEXT_PAD"));

/// The padding itself.
///
/// Filled with `0xCC` (`int3`) rather than zeroes: this lives in an executable
/// segment, and if control ever reaches it we want an immediate, unmistakable
/// breakpoint exception rather than a slide through `add [rax], al` into
/// whatever follows.
///
/// `#[used]` keeps it from being dropped as an unreferenced static, and
/// `linker.ld` names the section explicitly with `KEEP()` so that it is placed
/// first in `.text` — ahead of `*(.text .text.*)` — which is what makes it
/// shift everything else.
#[used]
#[unsafe(link_section = ".text.slateos_layout_pad")]
static LAYOUT_PAD: [u8; PAD_BYTES] = [0xCCu8; PAD_BYTES];

unsafe extern "C" {
    static __text_start: u8;
    static __layout_pad_start: u8;
    static __layout_pad_end: u8;
}

/// Read a linker-defined symbol's address as an integer the optimiser must
/// treat as unknown.
///
/// # Why this exists — the bug it fixes
///
/// LLVM's object model guarantees that two *distinct* global objects live at
/// *distinct* addresses. That is a licence to constant-fold: given `&A` and
/// `&B` for two different globals, `&A == &B` is `false` and the equal-branch
/// is dead code, deletable without ever consulting the linker.
///
/// `__text_start` and `__layout_pad_start` are two distinct declarations. They
/// are also, by construction, the *same address* — `linker.ld` emits them back
/// to back with nothing between:
///
/// ```text
/// .text : ALIGN(4K) {
///     __text_start = .;
///     __layout_pad_start = .;
///     KEEP(*(.text.slateos_layout_pad))
/// ```
///
/// So the source says "compare two addresses" and the optimiser reads
/// "compare two distinct globals", concludes they differ, and compiles
/// `self_test_pad_is_first_in_text` down to its failure path with the success
/// path removed entirely. Measured on the 2026-08-19 release sweep: the string
/// `"pad byte(s) at the head of .text"` did not occur *anywhere* in the padded
/// release binary, while the FAIL string did. The self-test had become a check
/// that always fires — the exact inverse of this project's signature defect
/// (a check that cannot fire), and worse, because `main.rs` halts on it, so
/// every optimised nonzero-pad kernel wedged at boot.
///
/// Passing the address through an empty `asm!` block with the value as an
/// `inout` operand fixes it at the root. The compiler must assume the block
/// wrote an arbitrary integer into that register, so nothing it knew about the
/// symbol survives — including that it was a symbol at all. The comparison
/// then has to happen at run time, against what the linker actually emitted,
/// which is the only place the question has an answer.
///
/// `core::hint::black_box` would very likely work today, but its documentation
/// is explicit that it guarantees nothing and may become a no-op. A check that
/// silently reverts to always-failing on a compiler upgrade is not a check.
#[must_use]
fn opaque_addr(symbol: *const u8) -> usize {
    let mut addr = symbol as usize;
    // SAFETY: the template assembles to nothing — `/* {0} */` is an assembler
    // comment, and exists only because rustc rejects an operand the template
    // never names. So no instruction executes and there are no effects to
    // describe. `inout(reg)` merely forces the value through a register the
    // compiler must then treat as opaque. `nomem` is accurate (no memory is
    // touched — note we take the *address*, never a load through it),
    // `nostack` is accurate (nothing is pushed), and `preserves_flags` is
    // accurate (no instruction runs, so no flag can change). Deliberately
    // *not* `pure`: purity would let the optimiser reason about the result
    // again, which is the whole thing being prevented.
    unsafe {
        core::arch::asm!(
            "/* {0} */",
            inout(reg) addr,
            options(nomem, nostack, preserves_flags)
        );
    }
    addr
}

/// Padding bytes actually emitted, as measured by the **linker**.
///
/// # Why this is not just `PAD_BYTES`
///
/// Two reasons, and the second is the one that forced it.
///
/// It is a stronger statement. `PAD_BYTES` is what the compiler was told;
/// `__layout_pad_end - __layout_pad_start` is what ended up in the image. Those
/// can differ — a `linker.ld` that dropped the `KEEP()`, a `--gc-sections`
/// appearing in a future `.cargo/config.toml` — and the version that can be
/// wrong is the one worth reporting.
///
/// And it keeps the *code* identical across pad values, which the whole
/// calibration depends on. Using the constant meant `if PAD_BYTES == 0` folded
/// away in the unpadded build, so `self_test_pad_is_first_in_text` was ~256
/// bytes shorter there and every function linked after it shifted by
/// `pad + 256` rather than `pad`. The 2026-08-19 `--self-test` caught exactly
/// that: a 4096-byte pad produced shifts of 4096, 4336 *and* 4352 instead of a
/// uniform 4096. That is a build differing from the baseline in code as well as
/// in placement — i.e. precisely the confound the sweep exists to isolate,
/// reintroduced by the tool meant to measure it. Reading the length through
/// linker symbols the compiler cannot see makes every arm byte-identical apart
/// from the padding itself.
///
/// Both addresses are read through [`opaque_addr`]. Nothing has been observed
/// folding here — the `textpad=0` message survived into the binary that lost
/// its sibling branch, so the `pad == 0` test was still being evaluated — but
/// this rests on the same "two distinct globals cannot share an address"
/// assumption that deleted the other branch, and the failure mode is a
/// *silent* wrong length rather than a loud one. Read them the same way.
#[must_use]
pub fn pad_bytes() -> usize {
    let start = opaque_addr(core::ptr::addr_of!(__layout_pad_start));
    let end = opaque_addr(core::ptr::addr_of!(__layout_pad_end));
    end.saturating_sub(start)
}

// The parser's rejection cases are checked here rather than in a `#[cfg(test)]`
// module, because a kernel `#[cfg(test)]` module never compiles and therefore
// never runs (see the note in `blkdev.rs`) — a test that cannot fail is exactly
// the defect this module exists to eliminate, and writing one here would be
// self-parody. These are `const` assertions on the real `const fn`, so a
// regression in it is a build error on every build, on every host, with or
// without a test runner.
const _: () = assert!(parse_pad(None) == 0);
const _: () = assert!(parse_pad(Some("")) == 0);
const _: () = assert!(parse_pad(Some("0")) == 0);
const _: () = assert!(parse_pad(Some("64")) == 64);
const _: () = assert!(parse_pad(Some("3072")) == 3072);
const _: () = assert!(parse_pad(Some("1048576")) == 1024 * 1024);
// The rejection cases (`"1O24"`, `"4k"`, `"1048577"`) cannot be asserted the
// same way — a `const fn` that panics is a compile *error*, not a value — so
// they are covered by `scripts/layout-sweep.py --self-test`, which builds with
// each and requires the build to fail.

/// Confirm the linker actually put the pad at the very front of `.text`.
///
/// The claim this module rests on is not "there are N spare bytes somewhere" —
/// it is "everything in `.text` moved by N". That is true only if the pad is
/// placed *first*, which is a property of `linker.ld`, not of this file. A
/// mis-edited linker script (or a section-ordering change from a future
/// toolchain) would drop the pad in the middle of `.text` instead: the kernel
/// would build, boot, and produce a layout sweep whose samples were all subsets
/// of one another, silently under-reporting layout sensitivity.
///
/// So check it on the target, where it is true or not. Runs on every boot.
/// Every value it reads comes from a linker symbol, so this function compiles
/// to the same bytes in every build — see [`pad_bytes`].
///
/// Both addresses go through [`opaque_addr`], without which the optimiser
/// answers the comparison itself, always with "different", and deletes the
/// success path. See that function for the measurement.
pub fn self_test_pad_is_first_in_text() -> crate::error::KernelResult<()> {
    let text_start = opaque_addr(core::ptr::addr_of!(__text_start));
    let pad_start = opaque_addr(core::ptr::addr_of!(__layout_pad_start));
    let pad = pad_bytes();

    if pad == 0 {
        // Nothing was emitted, so there is no placement to verify. Say so
        // explicitly rather than reporting a vacuous pass.
        crate::serial_println!("[layout_pad] no padding in this build (textpad=0)");
        return Ok(());
    }

    if pad_start != text_start {
        crate::serial_println!(
            "[layout_pad] FAIL: pad at {pad_start:#x} but .text starts at \
             {text_start:#x} — linker.ld is not placing \
             .text.slateos_layout_pad first, so a layout sweep would \
             under-report"
        );
        return Err(crate::error::KernelError::InternalError);
    }

    crate::serial_println!(
        "[layout_pad] {pad} pad byte(s) at the head of .text ({text_start:#x}) — OK"
    );
    Ok(())
}
