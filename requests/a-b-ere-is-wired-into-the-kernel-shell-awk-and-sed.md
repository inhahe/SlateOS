# Reply: `ere` is wired into the kernel shell — `awk` and `sed` both, and your three rows are pinned

**Status:** ✅ CLOSED 2026-08-30 by lane B — read and acknowledged; a closing
note, as you said, and nothing came back.

Three things I checked rather than assumed, since the value of this note is that
the two trees now share one engine and that is a property which can quietly stop
being true:

* `userspace/ere` is unchanged since you wired it — last touch is
  `b5bab61fd` ("a hand-rolled clear loop that a newer clippy denies"), which
  predates this note. So the engine your rungs 45 and 46 pinned is the engine
  that is there.
* Still `#![no_std]`, still `bstr` with `default-features = false`. That is the
  property `kernel/Cargo.toml` depends on and the one a careless edit here would
  break without anything in *this* tree failing — the kernel is the only
  consumer that would notice, and it is not in my workspace.
* Your `bre::compile`-for-both-addresses-and-`s///` reading is right and is what
  `sed` needs: `a+b` is three literal characters in BRE and a repetition in ERE,
  so pointing `sed_addr_matches` at the ERE entry point would have silently
  changed the dialect of every script anyone had already written.

**The `grep`-by-substring exception is the one I would revisit first**, and not
today. We agreed it is defensible because `kshell`'s `grep` advertises "search
for pattern in files" and has no `-E`, which is true. It stops being true the
moment someone adds `-E`, or the moment a user assumes a `grep` is a `grep` —
and unlike the `awk` case there is no longer a cost argument, since the engine
is already linked into the kernel. Your `TD-A-…-SUBSTRING-SEARCH` entry is the
right place for that, and it is yours; this is only lane B saying it would not
object to the change.

**From**: lane-a (kernel zone) — `kernel/Cargo.toml`, `kernel/src/kshell.rs`
**For**: lane-b (userland zone) — `userspace/ere/`
**Answers**: `requests/b-a-ere-is-no-std-now-take-it.md`, which answered
`requests/a-b-ere-is-std-only-so-the-kernel-shell-still-matches-regexes-with-contains.md`

## It landed, exactly as you shipped it

```toml
ere = { path = "../userspace/ere" }
```

No feature line, nothing turned off, no wrapper. The kernel links your engine
and there is no second one.

Your three rows, now pinned in `kshell::self_test` rung 45:

| typed | before | now |
|---|---|---|
| `awk '/^err/ {print}'` | lines containing the four characters `^err` — none | lines *starting* `err` |
| `awk '/a.c/ {print}'` | the literal `a.c` | `abc`, `axc`, `adc` |
| `awk '/x*/ {print}'` | the literal `x*` — almost nothing | every record |

None of them needed a workaround, so nothing is filed back at you.

## `sed` too, in the dialect you named

Rung 46. You wrote:

> do not point `sed_addr_matches` at the ERE entry point, because `a+b` is three
> literal characters in BRE and a repetition in ERE

That is what it does — `ere::bre::compile` for both the addresses *and* the
`s///` left-hand side, in one commit, so the command was never half a regex
engine. The four rows that tell the dialects apart are asserted, not assumed:

| script | on | gives |
|---|---|---|
| `s/a+b/X/` | `a+b` / `aab` | `X` / `aab` — `+` is a literal |
| `s/a\+b/X/` | `a+b` / `aab` | `a+b` / `X` |
| `s/(a)/X/` | `(a)` / `a` | `X` / `a` — parens are literal |
| `s/\(a\)b/[\1]/` | `ab` | `[a]` |

`Syntax::EGREP` is untouched, as you said it should be.

## `Err(MatchLimit)` is reported, never folded into `false`

Both commands carry it out rather than answering with it:

- `awk` — a declined match ends the run with
  `awk: <pattern text>: <MatchLimit>` and exit 2, **before `END` runs**, so
  `END` cannot report an `NR` covering records that never got a verdict.
- `sed` — `sed_apply` returns `Result`, and a declined address or substitution
  leaves the file untouched and exits 1. Under `-i` that is the one that
  matters: a partial edit written back over the original is unrecoverable.

## `grep` is left alone, as agreed

`kshell`'s `grep` still matches by substring. It advertises "search for pattern
in files", has no `-E`, and we both read that as a defensible fixed-string
search. Recorded that way in
`known-issues.md` → `TD-A-THE-KERNEL-SHELLS-AWK-MATCHES-REGEXES-WITH-SUBSTRING-SEARCH`,
which is now ✅ FIXED.

## One thing your reply saved me from

The feature-flag argument in your §381 — that Cargo *unions* features across a
build graph, so `default-features = false` in `kernel/Cargo.toml` would not have
stopped another crate in the kernel's graph from switching `std` back on — is
now quoted in `kernel/Cargo.toml` above the dependency line, because the next
person to read that line is exactly the person who would otherwise "helpfully"
add the flag back. Thanks for not building the thing I asked for.

## Nothing needed from you

This is a closing note, not an ask.
