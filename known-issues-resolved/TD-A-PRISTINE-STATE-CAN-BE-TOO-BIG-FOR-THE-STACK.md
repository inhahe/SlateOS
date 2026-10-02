## TD-A-PRISTINE-STATE-CAN-BE-TOO-BIG-FOR-THE-STACK — `with_pristine` puts two whole tables in the caller's frame

**Status: RESOLVED 2026-08-23** in `71d7148ad`. Re-verified 2026-09-11:
`fs::selftest::with_pristine_swapped` exists at `selftest.rs:199`, `net::bridge`
uses it, and `pristine_bridges()` builds the substitute on the heap one `Bridge`
at a time. This line exists because the entry already said **Fixed in
`71d7148ad`** and that spelling is invisible to a search for a status -- see the
note at the end.

**In short:** the same fix moves a fresh copy of a module's state *in* and
the saved copy *out*, both by value, so two of them sit in the calling
function's stack frame at once. For the `Vec`-backed tables this is nothing.
For one module it was 105 216 bytes each against a 64 KiB kernel stack —
the swap would have overrun the stack three times over before the suite
reached its first assertion, i.e. an instant triple fault at boot.

**Lane A. Found 2026-08-23, by clippy, before the suite was wired.**

`net::bridge`'s `[Bridge; MAX_BRIDGES]` is 105 216 bytes — 16 bridges each
carrying a 256-entry forwarding database — against
`sched::task::TASK_STACK_SIZE` of 64 KiB.

**Two things about this generalise, and are why it has an entry:**

- **Clippy under-reports it.** `large_stack_arrays` sees array
  *expressions*. A `Mutex<State>` whose `State` merely *contains* a big
  fixed-size table is invisible to it, and that is the more common shape.
- **`Box::new([const { X }; N])` does not avoid it.** The array is built in
  the caller's frame and only then copied into the box. Filling the
  allocation slot by slot does avoid it, because a large return value is
  written straight through the destination pointer.

**Fixed in `71d7148ad`:** `fs::selftest::with_pristine_swapped` takes the
pristine value by `&mut` and `core::mem::swap`s it into place, so no whole
`T` is ever materialised in a frame; `net::bridge::pristine_bridges` builds
the substitute on the heap one `Bridge` at a time.

**Measure rather than guess: `build/size_probe.py`.** There is no `size_of`
at the shell and no way to run code in a `no_std` kernel, so it appends
`const _P: [u8; 0] = [0u8; core::mem::size_of::<T>()];` to each file and
reads the size back out of rustc's E0308 ("expected an array with a size of
0, found one with a size of N"). One `cargo check` yields every table's size
at once. Across the 25 converted at the time, `net::bridge` was the only one
over 16 KiB and the next largest was 1280 bytes.

### Why this entry needed a second status line, 2026-09-11

It already had one: **Fixed in `71d7148ad`**. That is a *fifth* spelling of
"resolved" in these documents, alongside `**Status:** RESOLVED`, `— FIXED
<date>` in the heading, `### Resolution`, and `DONE`. Sweeping lane-A entries for
what is still open therefore gave a different answer each time the regex improved:

| rule | open | what it really measured |
|---|---|---|
| no resolved marker in the **heading** | 23 | headings, not entries |
| …or in the first 12 lines of the body | 15 | a window, not the body |
| …or anywhere in the body | 9 | four spellings, not five |
| after reading the nine | **6** | the entries |

Three of the nine were already done: this one, `TD-A-FS-SELFTESTS-NEVER-RUN`, and
`TD-A-AN-ABSENT-OPERAND-DEFAULTS-TO-A-LIVE-OBJECT-ID`. Each cost a fresh
investigation to establish that, and the 273-line first one nearly cost a
re-opening.

This is the same finding as `TD-A-REQUEST-STATUS-HAS-NO-CHECKED-SHAPE-SO-EVERY-READER-COUNTS-DIFFERENTLY`,
which was filed this morning about `requests/` after four measurements of *that*
population gave four numbers. It is not a `requests/` problem. It is what happens
to any status field that nothing checks: every reader writes their own search, and
every search is a different question wearing the same name.

**A one-off measurement cannot fix this and neither can a better regex.** The fix
is a checked shape — a `**Status:**` line in a known position whose first word
comes from a closed vocabulary — ratcheted at the current population so nothing
has to be back-filled at once. `scripts/check-design-decisions-bands.py` already
does exactly this for `**Lane:**` fields one document over, and for the same stated
reason: a field nothing checks is a field that drifts.
