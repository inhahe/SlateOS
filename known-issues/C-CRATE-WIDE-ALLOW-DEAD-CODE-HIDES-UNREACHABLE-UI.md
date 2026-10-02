## `C-CRATE-WIDE-ALLOW-DEAD-CODE-HIDES-UNREACHABLE-UI`

**In short:** many app crates start with a line that switches off the compiler's
"you wrote this and never used it" warning for the whole file. That warning is
the single most useful signal for finding a feature that was built and never
connected — it is exactly what would have flagged the dead scroll offsets — and
these crates have it turned off everywhere rather than at the few places that
need it.

**Where:** `#![allow(dead_code)]` at the top of, among others,
`apps/radio/src/main.rs:1` and `apps/netscan/src/main.rs:22`. netscan's is why
`sidebar_scroll` and `topology_zoom` sat dead without a warning; radio's is why
`genre_scroll` did.

**Why it cannot simply be deleted, which is the interesting part.** Removing
radio's produces **31 warnings**, and every one of them is downstream of a
single fact: `fn main()` is `let _app = RadioApp::new();`, so `render()` is
never called outside tests, so everything `render()` uses is dead too. The
allow is not hiding sloppiness; it is hiding
`C-NO-APP-IS-WIRED-TO-AN-EVENT-LOOP`. Deleting the allow today would bury the
one or two real findings under 30 false ones.

**The proper fix** is therefore ordered: wire the apps to an event loop first;
then the allows can come off, and what remains warned about is genuinely
unreachable code worth deleting. Doing it in the other order produces noise
that gets suppressed again.

**In the meantime:** do not add `#![allow(dead_code)]` to a *new* crate, and
when a specific item is legitimately unused, allow it on that item rather than
on the crate.
