## `A-KSHELL-SED-I-TRUNCATES-A-FILE-IT-CANNOT-DECODE` (lane A, 2026-08-25) — ✅ **FIXED** (`1500fdb62`, 2026-08-25)

**Where.** `kernel/src/kshell.rs` — `cmd_sed` (~124158), `sed_apply` (~124286).

**What.** This is the worst of the silent-guess family found so far, because it
does not merely report a wrong answer — it **destroys the file** and reports
success.

```rust
let text = core::str::from_utf8(&data).unwrap_or("");
```

A file that is not valid UTF-8 becomes the **empty string**. `sed_apply` then
has no lines to work on and returns an empty result, and under `-i`:

```rust
crate::fs::Vfs::write_file(&path, output.as_bytes())
```

writes zero bytes over the original. `sed -i 's/a/b/' photo.jpg` empties
`photo.jpg` and **exits 0**. Nothing in the run says anything happened.

Without `-i` the same input prints nothing and exits 0, which is the ordinary
silent-guess shape — indistinguishable from a file that really was empty.

**The second bug, same cause.** `sed_apply` iterates `text.lines()`. Rust's
`str::lines` splits on `\n` *and strips a trailing `\r`*, so every CRLF line
ending is silently converted to LF. `sed -i 's/a/b/' dos.txt` rewrites the whole
file with Unix endings whether or not any line matched. GNU sed does not do
this: to sed, `\r` is an ordinary character in the pattern space.

**Why it survived.** Every existing sed test uses ASCII text with LF endings —
the one shape in which both bugs are invisible. This is the same reason the
whole `str::contains` regex family survived its own suite.

**What the proper fix looks like.** No new dependency: `ere` is **already a byte
engine**. Its own docs say "POSIX regular expressions over byte strings"; `Ch`
is a decoded scalar *or* one undecodable byte, and `Regex::is_match`/`find`/
`captures` all take `BStr<'_>` = `&[u8]`. kshell's sed converts to `&str` purely
to hand back bytes at the far end, discarding the capability on the way through.

So the change is confined to lane A's own tree — `userspace/ere` needs nothing:

- `sed_apply(&[u8], …) -> Vec<u8>`, `sed_emit`, `sed_substitute` and
  `sed_addr_matches` all on bytes.
- Split lines on `\n` only, keeping `\r` as an ordinary byte of the pattern
  space, so CRLF survives a round trip.
- `cmd_sed` passes `&data` straight in; the `from_utf8` disappears rather than
  gaining a fallback.
- `cmd_sed_input` currently takes `input: &str`, so the same conversion is
  happening one level up at its caller; that goes too.

The `-c` half of the `cut` fix is the precedent for what to do where characters
genuinely are needed: report and skip the line, do not empty the file.

**Severity.** Data destruction, silently, exit 0. This should be fixed before
any remaining cosmetic item in the coreutils queue.

### Fixed

`1500fdb62`, in the shape sketched above and with nothing asked of lane B —
`ere` really was already a byte engine, so the whole change was lane A's.

| | was | is |
|---|---|---|
| `sed -i 's/x/y/' photo.jpg` | file emptied, exit 0, no output | edited or left alone byte-for-byte, exit 0 |
| `sed 's/x/y/' photo.jpg` | prints nothing, exit 0 | prints the file with the edit applied |
| `sed -i 's/x/y/' dos.txt` | every `\r\n` becomes `\n`, matched or not | `\r` survives; it is an ordinary character of the pattern space |
| `sed 's/[[:cntrl:]]$//'` on CRLF | could not see the `\r` to match it | strips it, as GNU does |
| `cat photo.jpg \| sed 's/x/y/'` | refused, exit 1 | runs |

`sed_lines` replaces `str::lines` and splits on `\n` alone; `sed_apply`,
`sed_emit`, `sed_substitute` and `sed_addr_matches` are `&[u8]` throughout.
`sed_substitute` lost its UTF-8 round trip on the way out *and* the branch that
guarded it, which had been commented as unreachable because the subject had
been a `str` — with a byte subject the question does not arise, since every
byte written is copied from the subject or from the replacement template.
`dispatch_with_input` moves `sed` from the "still text-only" group into the
byte-clean one, and `cmd_sed_input` takes `&[u8]`.

**Pinned by** self-test rung 54, which is deliberately built on the two shapes
that hid this for the whole life of the suite: a fixture file of real
undecodable bytes (`\x00\x01\xff\xfe … \x80\xc3 … \xed\xa0\x80`) edited in
place, and CRLF text. The in-place case is run **twice**, first with a script
that matches nothing — the old code did not need a match to truncate, so a
fixture that matched would have tested the smaller half of the bug.
