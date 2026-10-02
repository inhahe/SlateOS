## TD-B-NINE-CRATES-ALLOW-TWO-CORRECTNESS-LINTS-CRATE-WIDE — 2026-09-15 — OPEN

**In short:** nine `userspace/` programs switch off two warnings that exist to
catch crashes -- "this array access might be out of range" and "this sum might
overflow" -- for the whole program at once. In `gdb` that hid two real crashes:
typing `gdb ""` (an empty filename) made the debugger panic instead of printing
an error. Fixed in `gdb`; the other eight are unaudited.

**The crates:** `ar`, `ftp`, `gdb`, `ldd`, `objdump`, `readelf`, `stty`,
`telnet`, `zip` -- each with `#![allow(clippy::arithmetic_side_effects,
clippy::indexing_slicing)]` at crate level. `indexing_slicing` is `warn` at the
workspace level (root `Cargo.toml`), so these allows are a deliberate opt-out.

**Why they exist, and it is a fair reason.** These are binary-format parsers.
Measured on `gdb`: removing the allow reports **542 sites** -- 268 arithmetic,
176 indexing, 98 slicing -- and nearly all are offsets into a DWARF section or
a remote-serial packet whose length was checked a few lines earlier. Rewriting
those as `.get()` would be churn with no correctness gain.

**Why it is still debt.** The justification written in `gdb` was *"indexing/
slicing is gated by length checks at the call site"*. That was checked rather
than trusted and it was **false**: both argv loops did `arg[0]`, and an argv
entry may be empty. A crate-wide allow whose justification is "every site is
gated" cannot be verified by reading it, and it covers precisely the sites that
are not -- which are the ones handling input whose length the program does not
control.

**The fix, and it is cheap.** Not removing the allow: re-arming the lint where
lengths are unknown. `#[deny(clippy::indexing_slicing)]` on the two argv
parsers in `gdb` turned up **six further sites** beyond the two panics, all
safe-but-incidentally so (an `is_empty()` check two lines above an `[0]`, a
`while i < len` above an `[i]`). They are now `first()`, `get(i)` and
`get(colon + 1..)`, where the bound is structural and a later edit cannot
separate the guard from the access. Clippy refuses the build if `arg[0]`
returns -- verified by putting it back.

~~**What is left:** the same treatment for the other eight. None has the
`arg[0]` argv pattern (checked), so there is no known panic in them today; what
is unknown is whether any of their 500-odd allowed sites touches a length the
program does not control.~~

### That unknown is now measured, 2026-09-16: no defect in any of the eight

Every parser in the eight crates that reads a length the program does not
control was read. **All eight check before they index.** Stated per crate,
because "I looked and it was fine" is not a measurement anyone can re-check:

| crate | the uncontrolled-length path | the guard that makes it safe |
|---|---|---|
| `zip` | central directory, `data[pos + N]` for N up to 45, where `pos` comes from a field *in the file* | `if pos + 46 > data.len()` immediately above the loop body; `cd_offset + cd_size > data.len()` before that. Extraction adds an output cap (`inflate_limited`), a declared-size comparison and a CRC. |
| `readelf` | ELF header/section reads at a file-supplied offset | `if end > data.len()` returning a structured `TruncatedData { what, offset, needed, available }` |
| `objdump` | same shape, same code | same guard, character for character |
| `ar` | archive member headers | `data.len() < AR_MAGIC.len()`, then `offset + AR_HDR_SIZE > data.len()` each iteration |
| `stty` | the `-g` restore string, which a user pastes in | `parts.len() < 39` before `parts[0..38]` |
| `telnet` | bytes off the wire | **`process_incoming` does not index at all** -- it is a `for &byte in input` state machine. `handle_subnegotiation` guards with `is_empty()` and reads its second byte with `.get(1)`. |
| `ftp` | PASV reply, and the command line | `parts.len() != 6` before `parts[0]`; the other site is `splitn(3, ..)`, which yields at least one element by construction |
| `ldd` | ELF reads | bounded, and its allow carries a written justification; uses `saturating_sub` for the reported remainder |

**The arithmetic half is safe for a reason worth writing down rather than
rechecking each time.** The offsets come from `u32` fields widened with
`as usize`. On the 64-bit target `cd_offset + cd_size` cannot overflow a
`usize`, so the additions in the guards are sound *because of the widening*,
not because anyone bounded them. On a 32-bit target they would not be, and the
guard would be the thing that overflows.

**What this does and does not retire.** It retires the open question -- there is
no reachable panic behind these allows. It does not retire the debt: the
guarantee is still by inspection, and `gdb` is the proof that inspection fails,
since its allow was justified as "gated by length checks at the call site" and
two argv sites were not. The remedy the entry proposes -- re-arm the lint on
the input parsers specifically -- is still worth doing and is now cheap, because
the sites that would need it have been identified.

**Method, since the count in this entry came from a grep.** `cargo clippy
-- --force-warn clippy::indexing_slicing --force-warn
clippy::arithmetic_side_effects` reports through a crate-level `allow`, so the
real population is measurable without editing anything: 73 findings in `ftp`
and `telnet` alone, against the 96 bare `[0]`s a grep suggested across all
eight. Neither number is the finding; the finding is which of them sit behind
an unchecked length, and that is 0.

**Where it lives:** the nine `userspace/<crate>/src/main.rs` crate attributes;
`gdb`'s is the worked example.
