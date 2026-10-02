## TD-B-SED-MISSING-COMMANDS — `l`, `W` and `R` are unimplemented, so `-l N` and `--sandbox` have nothing to act on (lane B, 2026-08-24) — RESOLVED 2026-08-24

**What it is.** Our `sed` implements most of the GNU command set, but three
commands are missing outright, and two options exist only to be accepted:

| Missing | What GNU does | What ours does |
|---|---|---|
| `l` | prints the pattern space unambiguously — non-printing bytes as octal escapes, a trailing `$`, wrapped at the `-l` width with a `\` at each break | ``unknown command: `l'`` (status 1) |
| `W FILE` | writes the *first line* of the pattern space to FILE | ``unknown command: `W'`` |
| `R FILE` | reads *one line* from FILE per cycle and queues it for output | ``unknown command: `R'`` |
| `-l N` | sets the wrap width `l` uses (default 70; `0` means never wrap) | parsed and discarded |
| `--sandbox` | makes `e`, `r`, `w`, `R`, `W` a *parse-time* error: `e/r/w commands disabled in sandbox mode` | accepted and ignored |

**Why `-l` is discarded rather than stored.** A field holding a width that
nothing reads is a claim the program does not honour; the next reader would
have to prove the absence rather than see it. The option is consumed with a
comment pointing here instead. Restore the field in the same change that adds
`l`, not before.

**Reproduce.** `bash scripts/sed-diff.sh` — the cases
`sed -n l`, `sed -n l 0`, `sed -l 3 's/.*/aaaaaaaa/;l'`, `sed -n '$!N;W …'`,
`sed 'R def.txt'`, `sed '1R def.txt'` and `sed --sandbox 'w /tmp/x'` all report
DIFF, each showing our parse error against GNU's output.

**The proper fix.** Implement all three commands and give the two options
something to act on:

1. `l` — escape with GNU's table: a backslash followed by one of `abfnrtv`, or
   a doubled backslash for a backslash itself, and a three-digit octal
   `\ooo` for every other byte outside `[[:print:]]`. Then append `$`,
   and wrap so that each output line is at most `N` columns *including* the
   continuation `\`. `l 0` and `-l 0` disable wrapping; an explicit operand on
   the command (`l 5`) overrides `-l` for that command only. Note that the
   width counts *escaped* columns, not input bytes.
2. `W` — like `w`, but stops at the first newline in the pattern space; shares
   `w`'s open-file table so two `W`s to one name append to one handle.
3. `R` — one line per cycle from a lazily-opened file, appended to the same
   append-queue `r` uses; end of file makes it a silent no-op, and an
   unopenable file is *not* an error (GNU ignores it, unlike `r`'s sibling
   `w`).
4. `--sandbox` — a flag consulted by the *parser*, rejecting `e/r/w/R/W` with
   `e/r/w commands disabled in sandbox mode` at the character where the
   command starts.

Scoped as sed tranche 2b.

**Resolution (2026-08-24).** All five done as described, each measured against
GNU sed rather than written from the description above — which was wrong in one
place worth recording. The wrap width is applied *per escape*, not per byte:
GNU tests `output_width + escape_len + 1 > line_len` before emitting a whole
escape, so `\303` is never torn across a break, and the `+ 1` reserves the
column the continuation `\` will sit in. That is why `-l 1` opens with a bare
`\` and a break — no escape can fit in `1 - 1` columns. Point 1 above says
"at most `N` columns *including* the continuation `\`", which is the same rule
stated in a way that does not tell you what to do with an escape that straddles
the boundary.

Two further behaviours were measured and are now implemented, neither of them
guessable from the manual:

* Two `R`s naming one file **share its read position**, so one cycle takes two
  different lines rather than the same line twice. The handles are interned by
  name at parse time for exactly this reason.
* `-s` (and `-i`, which implies it) **rewinds every `R` source** at each new
  input file, so `sed -s 'R inc' a b` pairs `inc`'s *first* lines with both
  files.

`-l` is read with `atoi`, which cannot fail: `-l 3x` means 3 and `-l x`, `-l ''`
and `-l -1` all mean 0 — never wrap. None is an error, on either side.

`sed-diff.sh` went from 166 passed / 21 differed to 175 passed / 12 differed;
all seven cases named under **Reproduce** above are green, as are `sed w` and
`sed r`, whose message was aligned to GNU's `missing filename in r/R/w/W
commands` while in the area. The remaining 12 are tranches 2c and 2d, and the
`e` gap below.
