## `A-KSHELL-BYTE-AS-CHAR-MOJIBAKE` — every non-ASCII command line was corrupted before it ran — ✅ FIXED 2026-08-24 (lane A, `22a93f577`)

**In short:** typing `echo café` into the kernel shell printed `cafÃ©`, and
`mkdir Ünicode` created a directory named `Ãnicode`. Not in one command — in
*all* of them, because the corruption happened in the variable expander that
every command line passes through before it is dispatched. Four more scanners
had the same defect. All five are fixed and covered by self-tests.

### The bug is one line, repeated

```rust
result.push(b as char)   // b: u8, result: String
```

`b as char` is **not** a byte copy. It is the Latin-1 map: it produces the
Unicode code point whose *number* equals the byte. For a byte below 0x80 that
is exact — ASCII and Unicode agree there, which is precisely why this survived
so long. For a byte at or above 0x80 it is a re-encoding: `push` then writes
that code point back out as UTF-8, and every code point in `U+0080..=U+00FF`
needs **two** bytes.

So each byte of a multi-byte character becomes its own two-byte character:

| input | bytes in | what `b as char` produced | bytes out |
|---|---|---|---|
| `é` | `C3 A9` | `Ã©` | `C3 83 C2 A9` |
| `→` | `E2 86 92` | `â\u{86}\u{92}` | 6 bytes |
| `🦀` | `F0 9F A6 80` | 4 chars | 8 bytes |

The output is still *valid* UTF-8 — which is why nothing downstream rejected
it, and why a debugger showing the string looks merely wrong rather than
broken. The only reliable detector is the byte **length**, which is why the
self-tests assert on `.len()` and not just on equality.

### The five sites, in descending order of how much they mattered

**1. `expand_vars` — the whole shell.** `execute()` calls it on the command
line before dispatch (`kshell.rs`, the `expand_vars(line)` at the top of
`execute`). Every command, every argument, every path. Three separate copies of
the bridge lived in it: the ordinary byte loop, the single-quoted branch, and
the `$`-followed-by-a-non-name branch. A user could not type a non-ASCII
filename into the kernel shell at all.

**2. `cmd_date`** — literal text in a `+FORMAT` string. `date +'%F café'`
printed the date correctly and the word wrong.

**3. `expand_tr_set` — worse than corruption, this one was silent.** `tr`
matches its input *a character at a time*, so a set built out of Latin-1 chars
contained entries that no input character could ever equal. `tr é e` did not
mangle anything; it did **nothing**, and exited 0. Separately, a byte-indexed
range like `à-â` was computed over bytes, so it could run from the second byte
of one character into the first byte of the next — a range with no meaning.

**4. `awk_eval_expr`'s string literal** — `awk '{print "café"}'`.

**5. `awk_split_print_args`** — the comma splitter for `print` arguments.

### The fix, and the invariant that makes it safe

`expand_vars` became `expand_vars_bytes(&[u8]) -> Vec<u8>`, byte in and byte
out, with a narrowing `expand_vars(&str) -> String` wrapper for the nine
`&str` callers that have not been converted yet (see
`TD-KSHELL-LINE-EDITOR-IS-UTF8` — that is the remaining half of the work). The
wrapper's error arm **reports** rather than substituting: reaching it means one
of those callers stopped being UTF-8, which is a bug in this file and not bad
input, and a `from_utf8_lossy` would hide exactly the thing worth knowing.

The other three sites keep a `Vec<u8>` accumulator and finish through one
shared helper, `finish_ascii_scan`, which exists to hold the argument for why
any of this is safe:

> A UTF-8 continuation byte is always ≥ 0x80, so an **ASCII delimiter can never
> occur inside a multi-byte sequence.** A scanner that only ever branches on
> ASCII therefore only ever splits on character boundaries, and everything
> between its splits is copied through verbatim.

That is what all of these scanners do — they branch on `%`, `,`, `"`, `\`, `$`,
`'`, `` ` ``, `~`. Being byte-indexed was never the bug. Feeding a `String` was.

`expand_tr_set` is the exception and got the opposite treatment: it now returns
`Vec<char>` and scans characters, because `tr` is *genuinely* character-oriented
and a byte set is the wrong data structure, not merely a mis-filled one.

### Two sites deliberately left alone

`cmd_strings` and `cmd_hexdump` both use `b as char` behind a `0x20..=0x7E`
guard. Below 0x80 the Latin-1 map and the identity map agree, so these are
exact. Changing them would have been churn, and this entry exists partly so the
next sweep does not "fix" them.

### Why nothing caught it

`kshell::self_test` section 3 had already found and fixed this **exact bug** in
`interpret_echo_escapes`, months earlier, and documents the mechanism in a
comment. What did not happen at the time was the obvious follow-up: grep the
file for the rest of the class. That sweep — `grep -n 'as char' kernel/src/kshell.rs`,
then *reading* each hit to classify it — took minutes and found seven more
instances of a bug that made the shell unusable in any language but English.

The lesson is not "grep for `as char`". It is that **a bug found in one place
is a report about a class, not about a place.** When a fix lands, the next step
is to enumerate the class, and the enumeration is nearly always cheap compared
to the fix.

### Coverage

`kshell::self_test` sections 9 and 10, both asserting byte lengths explicitly.
Section 10 drives `date` through the real `dispatch_with_input` rather than
calling `cmd_date`, so the `+FORMAT` parsing is covered on the same path a user
takes.
