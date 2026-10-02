## 946. A publisher with no subscriber is indistinguishable from a working feature

**Date:** 2026-09-16 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; **Found by lane B, in my code**, an hour after I wrote it
and a day into both of us auditing everyone else's. Numbered here at their
request, because a series split across bands for attribution is worse than a
series.

**In short:** I added a syscall that sets the console keyboard layout. It is
capability-gated behind its own right, validates its argument, refuses an
unknown layout, has exactly one publisher, is published read-only through
`/proc/keylayout`, and has a ring-3 fixture confirming the round trip. **No
key types differently.** Nothing in the keystroke path reads the value. Every
check passes and the feature does nothing.

### The mirror of 942

942 is a **verdict with no corpus** -- a check that ran over nothing and
reported green. This is a **value with no reader** -- a write path that
completed correctly and changed nothing. They are the same defect seen from
opposite ends, and the reason this needs its own number is that every
instrument built for 942 looks at the *writer*:

| what we built | what it proves | passes on a value nothing reads |
|---|---|---|
| capability bit + gate ordering | an unprivileged caller is refused first | yes |
| dispatch probe | the syscall number is registered | yes |
| `/proc` round trip | the kernel stored what was sent | yes |
| link of the libc stub | the symbol exists and the prototype matches | yes |
| ring-3 fixture | userspace can set it and read it back | yes |

Five checks, five green, nothing typed differently. **From the publisher's
side a missing subscriber is invisible by construction**, because every
question the publisher can ask is about publishing.

### The tell existed. It could not have discriminated.

Lane B's observation, and it is a good one: `keylayout.rs:28` carries
`#![allow(dead_code)]` -- a module saying *nothing calls this* in the one
place the compiler would otherwise say it out loud. They had spent the
previous hour building a gate that reports exactly that shape and did not
point it at the thing they were wiring.

**But it would not have helped, and the reason matters more than the tell.**
**514 of this kernel's modules carry the same attribute.** A signal present
in 514 places discriminates in none of them. That is 942's noise lesson at
scale -- lane B's first switched-off-test pass printed 41 unchanging lines and
buried five real findings underneath; this is the same defect with an order of
magnitude more hay. So `#![allow(dead_code)]` is not a detector for this. It
is a detector that has been saturated, and a saturated detector reads exactly
like a quiet one.

### What would have caught it

One question, asked of the value rather than of the code that writes it:
**who reads this, and can I name the function?** Not "is it published" --
publishing was never in doubt. The answer for the layout was `kbl translate`,
a diagnostic that asks what a key *would* map to, and nothing else.
`keyboard.rs` mentions `keylayout` zero times; `scancode_to_ascii` is a
hardcoded scan-code-set-1 table; the compositor takes its layout from a third
mechanism entirely.

The rule generalises past syscalls: for any state the kernel accepts from
outside, the acceptance path and the *consumption* path are two separate
things to verify, and only one of them is fun to build.

### Why the fixture still passes, and why that is correct

`ctest-keylayout` confirms through `/proc/keylayout`, which is real kernel
state, so it goes green on a layout nothing translates through. That is not a
defect in the fixture -- it is honest about testing the publisher, and it
confirms through the publisher rather than the setter precisely because 1074
has no getter. **A green result there must not be read as "the keyboard
works."** Lane B put that sentence in `localectl`'s output rather than leaving
it implied, which is the right place for it: in front of the person who would
otherwise conclude it.

### The consumer is not a one-line fix, and that is why this entry exists

The obvious wiring -- one `keylayout::translate` lookup in front of the
existing table -- **deadlocks the keyboard**. `ioapic.rs:739` calls
`keyboard::handle_scancode()`; that is the IRQ 1 handler, and it reaches
`scancode_to_ascii` synchronously in interrupt context. `translate` takes
`STATE.lock()`. A task inside `set_active` holding that lock plus a keyboard
interrupt on the same CPU is an unrecoverable lockup, with the keyboard as the
thing that stops responding.

`keyboard.rs`'s own docs already say this about its echo path -- *"a softirq
... cannot block on a lock that task might already hold"* -- and §940 is this
lane's own entry on the identical shape, from the writeback softirq re-entering
the block registry. Both were written before today and neither stopped me
reaching for the obvious fix; what stopped me was tracing the call chain
*upward*, which is where I had stopped looking three times in the same session.

So the consumer follows §940: **the IRQ backs off rather than the registry
becoming IRQ-safe.** `translate_try` with `try_lock`, and on contention the
scancode passes through untranslated -- a keystroke racing a layout change uses
the old mapping, which is a sentence a user can understand.

**The caveat gets stated where the feature is defined, not only here:**
`disable_key` becomes best-effort under race, so a disabled key can get
through. That is fine for someone disabling CapsLock and is exactly the kind of
small caveat that becomes a hole the day somebody reads "disable" as a security
primitive. Its doc comment says, in the negative, that it is ergonomics and not
a control.

### Closed 2026-09-17: the subscriber exists

`scancode_to_ascii` now consults the active layout, so a key types
differently and the feature this entry was written about is real.

Built as the entry prescribed rather than as the obvious lookup:
`translate_try` uses `try_lock` and returns three outcomes, because the
caller is IRQ 1's handler and each case needs a different action there --
`Contended` passes the raw scancode through (a keystroke racing a layout
edit gets the old mapping, which is the documented degradation and not an
error), `Disabled` produces no character, `Mapped` is used, falling back to
the physical key if a layout maps to an extended keycode with no
scan-code-set-1 byte rather than dropping the keystroke silently.

Two details worth keeping. **No translation layer was needed**, because
`keylayout`'s `KeyCode` space *is* scan-code set 1 -- `keys::ESCAPE` is
`0x01`, `keys::KEY_1` is `0x02` -- which the entry had noted and which made
the consumer a lookup in front of the existing table rather than a mapping
exercise. And **the default path is unchanged**: with no active layout,
`translate_try` returns the key it was given, so a tree that never calls
1074 behaves exactly as before.

The `disable_key` caveat is now stated at that function's own signature, not
only here. The reader who would misread it as a security control is reading
the signature.

### The closure was not finished: the subscriber had no guard

Recorded the same day, from lane C's work rather than my own. When I wrote
the closure above I had built the consumer and *not* built anything that
would notice if it went away. `translate_try` could be reverted to return
`Mapped(key)` unconditionally and every signal would stay green:
`keylayout::self_test`'s seven cases exercise `translate`, which is the pure
function and was always correct, and `ctest-keylayout` confirms the setting
and its publication through `/proc`. Neither touches
`keyboard::scancode_to_ascii`, which is private and, until now, called only
from the two real input paths and from no test at all.

So this entry was one step short of its own point. It says a publisher with
no subscriber is indistinguishable from a working feature; a subscriber with
no guard is indistinguishable from a working feature *the day after someone
simplifies it*.

Lane C hit the identical shape the same day and stated the rule better than
I had
(`known-issues.md` -> `TD-C-A-PURE-FUNCTIONS-TESTS-SAY-NOTHING-ABOUT-ITS-CALLER`):
`compute_image_rect` was correct and had six passing unit tests, its one
production caller passed the display size where the image size belonged, and
under that single wrong argument all six wallpaper fit modes collapse to the
same rectangle. Six ways to fit a picture, all drawing the same thing,
feature marked done.

> A unit test of a pure function proves the function. It says nothing
> whatever about whether anybody calls it correctly.

And their prescription, which is what a closure of this entry actually
requires: drive the setting **two different ways through the real entry
point** and assert the results differ.

`keyboard::layout_consumer_self_test` now does that -- baseline types `a`,
an A->B remap types `b`, a disabled key types nothing. Two details are
deliberate. It asserts the *difference* rather than only the expected value,
because an implementation that always returned `b` would satisfy
`== Some(b'b')` while being exactly as broken as one that always returned
`a`. And it checks `u16::from(0x1E) == keys::KEY_A` rather than trusting the
paragraph above where this entry asserts the two spaces coincide: if they
ever diverge, the guard would silently test a different key than the one it
remaps, and would keep passing.
