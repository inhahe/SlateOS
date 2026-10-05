## §93 — osh byte strings: `Vec<u8>` + the `bstr` crate, not a hand-rolled `ShellStr`

**Date:** 2026-07-30
**Decided by:** Claude (autonomous)

**The problem.** osh models every shell string as a Rust `String`, so it cannot
carry a byte a POSIX filesystem permits but UTF-8 forbids. Our own filesystem
allows every byte except `/` and NUL, so `aÿb` is a legal filename that the
shell cannot name — and the failure mode is not a diagnostic, it is a *wrong
file*: `from_utf8_lossy` turns the byte into U+FFFD and the shell then opens,
`stat`s or deletes something else. This is CLAUDE.md rule 7 violated at the
root of the userspace. Fixing it means changing osh's string type
(TD-OILS-BYTE-STRINGS).

**The decision.** The shell string type becomes plain `Vec<u8>` / `&[u8]`, with
the `bstr` crate (`default-features = false`, `features = ["std"]`) supplying
the str-like methods over it, plus a small in-tree `bytes.rs` for the handful
of operations `bstr` deliberately does not have.

**Alternative considered: a hand-rolled `ShellStr(Vec<u8>)` newtype.** *Pro:* no
external dependency in a core OS binary; we control the whole API surface and
can name methods after shell semantics rather than after `str`. *Con:* it is
several hundred lines of substring search, splitting, trimming, case folding
and UTF-8 chunk iteration that must be written, tested and maintained — and
`memchr`-quality substring search is genuinely hard to get both correct and
fast. Reimplementing it would be the "correct-but-naive code" the design spec
warns about, in a hot path (every `${v#pat}`, every field split).

**Alternative considered: keep `String` and smuggle bytes through a private
encoding** (surrogate escapes as WTF-8, or a private-use-area mapping, as
Python's `surrogateescape` does). *Pro:* a very small diff — the type never
changes. *Con:* it is not total (a value can already legitimately contain the
escape characters, so encode/decode is ambiguous), every syscall boundary needs
an encode/decode pass that is easy to forget, and `${#v}` / `${v:o:l}` would
silently count the escapes. This is precisely the kind of quick fix CLAUDE.md
forbids: it makes the bug rarer instead of absent.

**Why `bstr` wins.** Its real dependency tree is one crate deep — `oils -> bstr
-> memchr` — with no proc macros and no `syn`; it builds clean for
`x86_64-slateos` (verified); it is the de-facto standard for this exact problem
in Rust (ripgrep, gitoxide); and its API is deliberately `str`-shaped, so the
conversion of 60k lines of osh is close to mechanical rather than a redesign.

**What stays a `String`.** Variable names, function names, option names,
`set -o` flag names, format specifiers, and anything else the shell grammar
already restricts to a portable-character-set identifier. Those cannot be
non-UTF-8 without the *parser* having accepted a non-UTF-8 name, and keeping
them as `String` preserves `HashMap<String, _>` lookups with `&str` keys. What
becomes bytes is **values, literal word text, captured command output,
positional parameters, environment strings, paths, and every diagnostic that
interpolates one of those**.

**What `bytes.rs` has to add.** Three things `bstr` cannot give us without its
heavyweight `unicode` feature (which pulls `regex-automata`):

* Unicode case mapping for `${v^^}` / `${v,,}` / `${v@U}` — `bstr`'s
  `to_lowercase`/`to_uppercase` are ASCII-only without that feature, so we walk
  `utf8_chunks()` and case-map the valid runs while passing invalid bytes
  through unchanged.
* bash's character *counting* rule for `${#v}` and `${v:off:len}`: an invalid
  byte counts as exactly one character. `bstr`'s `char_indices()` already
  yields `(i, i+1, U+FFFD)` for one, which is the rule, so this is a thin
  wrapper rather than a reimplementation.
* `bfmt!`, a concatenating byte-string builder, because `format!` has no
  byte-string counterpart and `Display for BStr` is *lossy* — using it to build
  a shell value would reintroduce the very corruption being fixed. A macro that
  concatenates rather than parses a format string keeps that door shut by
  construction: there is no `{}` that can silently accept a `String`-formatted
  byte string. (The external `format-bytes` crate was rejected: it is a proc
  macro built on the deprecated `proc-macro-hack`, and buys only syntax.)

**How to reverse.** The change is a type change, not an architectural one: if
`bstr` ever became a problem, `bytes.rs` is the seam — vendor the dozen methods
osh actually uses into it and drop the dependency, with no change at the call
sites.
