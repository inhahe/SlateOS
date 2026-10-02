## §369 — A diagnostic that names a byte the terminal cannot decode prints it as `\377`, not raw, and never as U+FFFD

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)

**In short:** Command-line arguments and file names on SlateOS are bytes, not
text — any byte except `/` and NUL is legal — so a user can perfectly well type
`find t -type $'\377'`, where `\377` is a single byte (255) that is not part of
any valid UTF-8 character. `find` then has to write an error message that names
the offending byte. There are three ways to do that and they are not equally
honest. GNU writes the byte **raw**, so the terminal shows `M-^?` or a black
diamond. Our code used to convert it to a Rust `String`, which is required to be
valid UTF-8, and the conversion silently replaces the byte with U+FFFD, the
replacement character `\u{FFFD}` — so the message named a *different* byte from
the one the user typed. We now print `\377`: a backslash and the byte's value in
octal. That is not byte-identical to GNU, and `scripts/find-diff.sh` records
five cases where we and GNU differ for exactly this reason, on purpose.

### What was actually wrong

`find`'s error plumbing is `String`-typed: `Fatal::new(format!(…))` all the way
down. Twenty-nine sites reached it by calling `String::from_utf8_lossy` on a
path, an argument, or a format directive. "Lossy" is the whole problem —
`from_utf8_lossy` does not fail on an undecodable byte, it *substitutes* one.
Three further sites were worse in a way that is easy to miss: they wrote
`c as char`, which is not a decode at all. Rust's `u8 as char` reinterprets the
byte as a Unicode scalar with the same number, i.e. as Latin-1, so byte `0xFF`
becomes U+00FF `ÿ`, which is then encoded back out as the *two* bytes
`0xC3 0xBF`. The differential harness caught this immediately: GNU printed
`M-^?` (one byte) where we printed `M-CM-?` (two). Neither corruption announced
itself; both produced a plausible-looking message naming a byte that was never
in the input.

### The three options

| | What the user sees for byte `0xFF` | Cost |
|---|---|---|
| **Raw** (GNU) | `M-^?` — whatever the terminal makes of one undecodable byte | Requires converting `find`'s entire diagnostic path from `String` to `Vec<u8>` |
| **Escaped** (chosen) | `\377` | Not byte-identical to GNU; five harness cases must be marked deliberate |
| **Lossy** (what we had) | `\u{FFFD}` or `ÿ` | Free, and silently wrong |

Lossy is out on its own: a diagnostic whose entire job is to tell the user
*which* byte it could not accept, and which then names a different byte, is
worse than no diagnostic. That leaves raw versus escaped.

Raw is byte-identical to GNU, which is the standing goal for this port, and it
is what `-ok`'s confirmation prompt already does — that prompt writes straight
to a locked `io::Stderr` with `write_all`, no `String` anywhere, because a
prompt that showed the user a *different* path from the one about to be handed
to the command would be asking them to confirm the wrong thing. So the
technique exists and is in use where it matters most.

It is not adopted everywhere because the cost is not the twenty-nine call sites
— it is `Fatal`, `errmsg`, and every `?` between them. Making that path
byte-typed means a diagnostic can no longer be built with `format!`, which is
how all of them are built, and the change would have to reach the other
forty-odd binaries that share `errmsg` for the types to stay consistent. That
is a real refactor of the shared error plumbing, and it should be done
deliberately, on its own, not as a rider on a byte-safety fix.

### Why escaping is a good answer and not merely a cheap one

`quote::escape_unprintable` is **lossless and reversible**: printable bytes
render as themselves, every other byte as exactly one three-digit octal escape,
one escape per byte. From `\377` the reader can recover the input; from `M-^?`
they can too, if they know their terminal's conventions; from U+FFFD nobody
can. And the divergence from GNU is *visible* — the harness fails the case and
forces us to write down why — where the lossy divergence was invisible, which
is how it survived twenty-nine sites.

The escape is also what our own `find -ls` and `find -print` already do for a
name that is not text, and what GNU's own `ls` does by default, so it is not a
foreign convention inside this tool.

### What is left

`scripts/find-diff.sh` marks five cases as deliberate for this reason:
`-type`, `-used`, `-size`, `-printf %\377` and the unknown predicate `-\377`.
They are the complete set of `find` diagnostics that quote a single
undecodable byte back to the user. When the `Fatal`/`errmsg` path becomes
byte-typed, all five should flip to plain passes and this section should be
amended rather than deleted — the reasoning for *why lossy is never acceptable*
survives the refactor even though the raw-versus-escaped choice does not.
