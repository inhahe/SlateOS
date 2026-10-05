### B-KSHELL-ECHO-E-MANGLES-NON-ASCII. `echo -e` doubled every byte of every multi-byte character — 2026-08-24 — FIXED 2026-08-24 (lane A, `abeaca2aa`)

**Where:** `interpret_echo_escapes`, `kernel/src/kshell.rs` (~line 8691); one
caller, `cmd_echo`, on the `-e` path only.

**Symptom.** `echo -e` corrupted any non-ASCII text, while plain `echo` —
which does not enter this function — printed it correctly.

| input | old output | bytes |
|---|---|---|
| `café` | `cafÃ©` | 5 → 7 |
| `é` | `Ã©` | 2 → 4 |
| `→` | `â\x86\x92` | 3 → 6 |
| `🦀` | `ð\x9f¦\x80` | 4 → 8 |

**Cause.** The function walked `s.as_bytes()` and copied each non-escape byte
through as `result.push(bytes[i] as char)`. For an ASCII byte that is a no-op.
For a byte ≥ 0x80 it is not: `as char` maps `0x80..=0xFF` to
`U+0080..=U+00FF`, which `String::push` re-encodes as **two** UTF-8 bytes. So
each byte of a multi-byte character was expanded into its own two-byte
sequence — the output is not merely wrong, it grows with the input.

**Why it lasted.** Every test of this function was ASCII, and on ASCII the
buggy and correct forms agree exactly. This is the same `as char` mistake
already documented as a *landmine* in `TD-KSHELL-LINE-EDITOR-IS-UTF8` at
`kshell.rs:3583` — that entry flagged the instance on the **input** path as a
hazard for whoever widens the editor's `0x20..0x7F` guard, and did not notice
the identical one on the **output** path, which was not a hazard but a live
bug.

**Reachability** (it is not blocked by the editor's guard): the line editor
refuses bytes ≥ 0x80 from the keyboard, but `source` accepts any *valid-UTF-8*
script — and non-ASCII text is valid UTF-8; `cmd_source` rejects only invalid
sequences — and command substitution feeds arbitrary command output back into
the line. So a sourced or substituted `echo -e` on ordinary accented text
corrupts it.

**Fix.** Iterate `chars()` rather than bytes. `s` is a `&str`, so every
character is whole and valid by construction and pushing it cannot re-encode;
all escape *triggers* are ASCII, so a one-character peek is equivalent to the
old one-byte lookahead.

**Not fixed here, deliberately:** `\xNN` for arbitrary bytes. That needs an
output path that can carry non-UTF-8 (`shell_write` takes `&str`;
`SHELL_OUTPUT` is a `String`) and belongs to the byte-clean expanded-word work
settled as option B in `design-decisions.md` §261.

**Verification.** Changing a parser's iteration model risks silently altering
the inputs that were already correct — and those are the ASCII ones, where
every existing test lived, so a regression would have hidden exactly as the
original bug did. Checked both directions:

- `scripts/echo-escapes-oracle.rs` reproduces the old implementation verbatim
  and diffs it against the new one over **1204 ASCII inputs** — every string
  of length 0..=3 over an alphabet covering each character class, plus every
  ASCII byte alone, backslash-prefixed and embedded. **0 mismatches.** Same
  host-oracle pattern, and for the same reason, as `scripts/bytestr-oracle.rs`.
- `kshell::self_test` (new) is wired into the boot battery, since the kernel
  binary sets `test = false` and `#[cfg(test)]` never runs. It asserts the
  escape table, unknown/trailing backslashes, and 2-, 3- and 4-byte characters
  **by byte length** — a `String` that renders correctly can still carry the
  doubled encoding, so comparing rendered text would not have caught this.
