## §459 — The fake compositor ships in the library, unconditionally

**Date:** 2026-08-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** To test a graphical program you have to pretend there is a
screen. The pretend screen — a stand-in that answers "open a window" without
one existing — used to be locked inside `oswindow`'s own test code, where no
other program could reach it. It is now a normal, always-compiled part of the
library (`oswindow::testing`), so the text editor's tests can use the same one.
The cost is that it also ships in the real program, where nothing calls it.

**The problem.** An application cannot get a window identifier by itself. It
asks the compositor (the program that owns the screen) to create a window, and
the compositor answers with the id. That is deliberate — §457's rewrite of
`oswindow` removed the old behaviour where each process invented its own ids,
because two processes would confidently invent the same one. The consequence is
that *any* test of an application's event loop needs something on the other end
of the connection answering `CreateWindow`. `oswindow` had exactly such a thing
and it was `#[cfg(test)]`, which means it exists only while compiling
`oswindow`'s own tests and is invisible to every other crate in the tree.

So when `apps/editor` became the first real client
(`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` step (e)), its event loop was
untestable — and so was every other application's, all 138 of them.

**Options.**

1. **Promote the harness to a public module compiled unconditionally.**
   *What changes:* `oswindow::testing::desktop()` is callable from any crate,
   and the released editor binary carries a few hundred bytes of fake
   compositor it never calls.
2. **Put it behind a `testing` Cargo feature.**
   *What changes:* the same API, absent unless the feature is on — but a
   `dev-dependencies` entry that enables a feature on a crate that is also a
   normal dependency turns the feature on for the *normal* build too, via
   Cargo's feature unification. The saving is therefore usually imaginary,
   and it is bought with a second dependency line per app and a build that
   breaks with a confusing error when someone forgets it.
3. **Let each app write its own fake.**
   *What changes:* nothing ships that is not used — and every app encodes its
   own idea of the wire format. This is the option that actively creates bugs:
   a fake that decodes frames slightly wrong makes the *test* pass while the
   real compositor rejects the same bytes, which is worse than no test.

**Chosen: 1.** The deciding argument is option 3's failure mode. A test double
for a *protocol* is not a convenience — it is a second implementation of that
protocol, and the whole value of having one copy is that when the wire format
changes, the double changes with it and every dependent test fails loudly. Two
hundred bytes in a binary is not a cost worth trading that for. `guiremote`
already made the same call for the same reason with `loopback::Pipe`, so this
also keeps the two crates consistent rather than making a reader wonder why one
hid its harness and the other did not.

**What promoting it exposed.** The harness had a bug that only a non-test-module
user could hit: `serve()` skipped submitted frames while decoding requests, so
`drawn()` reported only frames sent since the last call to `serve`. That is
adequate for a test that never gives the compositor a turn, and useless for any
test of an event loop — taking turns is exactly how an event loop makes
progress. Frames now accumulate in `absorb()`. The bug is worth recording
because it is characteristic: a test double kept private is only ever shaped by
one caller, and grows assumptions that caller happens to satisfy.

**Revisit if** a release build ever measures the dead code as a real cost, or
if the harness grows enough that it wants its own crate. Neither is true at
~150 lines.
