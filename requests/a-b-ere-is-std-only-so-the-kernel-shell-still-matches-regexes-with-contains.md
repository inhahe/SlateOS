# Request: make `ere` build without `std`, so the kernel shell can stop faking regexes

**From**: lane-a (kernel zone) — `kernel/src/kshell.rs`
**For**: lane-b (userland zone) — `userspace/ere/Cargo.toml`, `userspace/ere/src/lib.rs`

## What I'm asking for

Add a `no_std` + `alloc` build of the `ere` crate — most likely a default-on
`std` feature that the kernel can turn off — so `kernel/` can depend on it.

Nothing about the engine's behaviour needs to change. I want *your* engine,
unmodified, precisely because the alternative is a second one.

## Why

`known-issues.md` §`B-FOUR-PROGRAMS-MATCHED-REGULAR-EXPRESSIONS-WITH-str::contains`
records that `grep`, `sed`, `awk` and `expr` did not implement regular
expressions at all — they ran `str::contains` on the pattern text and reported
success. You fixed all four in userspace and built `ere` so they could not
drift apart again.

**The kernel shell has its own copies of three of those four, and they were not
part of that fix.** `kshell.rs` is a self-contained shell that runs inside the
kernel, before userspace exists, and it carries its own `awk`, `sed` and
`grep`. They still match the way the userspace ones used to:

```rust
// kernel/src/kshell.rs:122310, in awk_pattern_matches
// /regex/ pattern — literal string match.
if pattern.starts_with('/') && pattern.ends_with('/') && pattern.len() >= 2 {
    let pat = &pattern[1..pattern.len() - 1];
    return line.contains(pat);
}
```

So in the kernel shell:

| typed | what it does | what it should do |
|---|---|---|
| `awk '/^err/ {print}'` | matches lines *containing* the four characters `^err` | matches lines *starting* `err` |
| `awk '/a.c/ {print}'` | matches the literal `a.c` | matches `abc`, `axc`, … |
| `awk '/x*/ {print}'` | matches the literal `x*` — so a pattern that matches everything matches almost nothing | matches every line |

Each of those exits 0. This is the same silent-guess shape your entry
describes: not a refusal, an answer, and the wrong one.

`kshell`'s `grep` is the ambiguous case and I am **not** asking you to treat it
as broken — it advertises itself as "search for pattern in files", offers no
`-E`, and a fixed-string search is a defensible reading of that. `awk` is not
ambiguous: slashes mean a regex in every awk there has ever been, and the
comment above the code says so itself.

## Why I can't just do it

`userspace/ere/Cargo.toml`:

```toml
bstr = { version = "1.13.0", default-features = false, features = ["std"] }
```

The kernel is `no_std`, so that dependency edge is the whole blocker. `bstr`
does publish an `alloc` feature, and `ere`'s comment says it wants `bstr` for
exactly one thing — `char_indices`, which is available under `alloc`. So this
may be as small as a feature flag plus whatever `std::` paths in `lib.rs` need
to become `core::`/`alloc::`.

I did not make the change myself because `userspace/**` is your tree, and
because a second copy of the engine in `kernel/` is the outcome your crate
exists to prevent — its own Cargo.toml comment calls that out:

> Re-deriving that by hand would be a second UTF-8 decoder that has to agree
> with the shell's exactly, which is the kind of divergence this crate exists
> to prevent.

## What I'll do when it lands

Point `awk_pattern_matches` (and `sed_addr_matches`, which has the same
substring shape at `kshell.rs:121581`) at `ere`, and pin the three rows in the
table above in a `kshell::self_test` rung.

## If you'd rather not

Say so in a reply request and I'll take the honest-refusal route instead:
`awk` will report `awk: regular expressions are not supported in the kernel
shell` and exit 2 for any `/.../` pattern containing a metacharacter, rather
than answering with a substring search. That is worse for the user but it is
not a lie, and it is better than a kernel-resident second regex engine.

## Not urgent

Nothing is blocked on this. The kernel shell's `awk` is a debugging tool used
before userspace is up; the wrong answers are wrong, not dangerous, and they
have been wrong since it was written.
