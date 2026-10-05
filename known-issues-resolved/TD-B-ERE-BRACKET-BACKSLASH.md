## TD-B-ERE-BRACKET-BACKSLASH — a backslash inside `[...]` is unescaped, where POSIX and GNU make it a member (lane B, 2026-08-24) — **FIXED** 2026-10-01

**Resolution (2026-10-01).** Fixed more widely than the plan below, because
measuring it showed the plan was half the problem. The engine read C escapes
*outside* brackets too: `grep 'a\tb'` and `grep -E 'a\tb'` matched a tab where
glibc reads `\t` as a `t`. So the engine now has no C escapes at all, exactly
as glibc's `regcomp` has none, and a backslash in a bracket is a member. The
two languages that do have C escapes resolve them before the pattern reaches
the engine, which is where their GNU originals do it: GNU sed already did
(`sed.rs` `normalize_regex`), and awk now does through `ere::awk`, a
transcription of gawk 5.2.1's `make_regexp` and `parse_escape`, compiled under
the new `Syntax::POSIX_AWK` (glibc's `RE_SYNTAX_POSIX_AWK`: a backslash in a
bracket quotes, the GNU operators are letters, a malformed interval after an
atom is a literal brace). `bre::to_ere` no longer doubles backslashes in
brackets, and `emacs` compiles its rebuilt brackets with the new
`backslash_escape_in_lists` bit. Measured matrix (grep 3.11, sed 4.9, gawk
5.2.1 `--posix`, bash 5.2 `=~`) is in `engine.rs`'s tests; harness cases in
`grep-diff.sh`, `sed-diff.sh` and a new escape section of `awk-diff.sh`. awk's
`/(.)\1/` divergence (design-decisions §333) went with it: POSIX's awk table
makes `\1` the octal escape, as gawk reads it. Behaviour changed for the other
lanes' callers: `kshell`'s sed and awk (lane A) and `logviewer`/`renamer`
(lane E) now read `\t` as `t` -- see the requests filed the same day.

**What it is.** In `ere`, `class_char` (`userspace/ere/src/engine.rs`) reads a
backslash inside a bracket expression as starting an escape. POSIX gives a
backslash no special meaning there at all, and GNU agrees in *both* dialects —
measured against the real `grep`:

| pattern | subject | GNU | ours |
|---|---|---|---|
| `grep -E '^[\.]$'` | `.` | matches | matches |
| `grep -E '^[\.]$'` | `\` | **matches** | does not |
| `grep -E '^[\t]$'` | `t` | matches | does not |
| `grep -E '^[\t]$'` | TAB | does not | **matches** |
| `grep -E '^[\w-]+$'` | `w\-` | matches | does not |

So `[\t]` is our tab and GNU's "backslash or `t`". BRE is unaffected in
practice: `bre::to_ere` doubles a backslash inside a bracket precisely to
cancel this out, which is why `grep '[\]'` is right today and `grep -E '[\]'`
is not.

**Why it is not simply "make `\` a member".** The engine is shared, and the
dialects genuinely disagree: POSIX and GNU `awk` *require* `[\t]` to be a tab,
so the current behaviour is correct for `awk` and wrong for `grep -E` and
`sed -E`. The fix is therefore a dialect flag rather than a one-line change.

**The proper fix.** Add a `brackets_take_escapes` flag alongside the existing
case-fold flag on `Regex::new_flags`, default it to *off* (POSIX/GNU
behaviour), and have `awk` turn it on. Then delete the doubling in
`bre::to_ere` — it exists only to compensate — and its test
`a_backslash_inside_a_bracket_is_a_member`, replacing it with one that checks
the untranslated form. Add harness cases to `grep-diff.sh` for the five rows
above.

**Reproduce.** `printf '\\n' | grep -E '^[\.]$'` — GNU prints the backslash,
ours prints nothing.

---

### 2026-08-24 — `gunzip FILE` compresses a file that isn't gzip, instead of refusing — ✅ FIXED same day (lane A, `4d9990f4c`)

**In short:** typing `gunzip notes.txt` does not report "that isn't a gzip
file". It *compresses* `notes.txt` and writes `notes.txt.gz` — the exact
opposite of what was asked. The command cannot tell which name it was invoked
as, so it guesses from the file's contents, and guesses wrong in the one case
where the user was most explicit.

**Where.** `kernel/src/kshell.rs`, `cmd_gunzip` (the mode test is the
`if compress_mode || (!is_gzip && !test_only && !list_mode)` at the top of the
decompress path), and the dispatch entry `"gunzip" | "gzip" => cmd_gunzip(args)`.

**Root cause.** One function serves both commands, and `dispatch` passes only
the arguments — argv[0] is discarded before the function runs. Lacking the name,
`cmd_gunzip` infers the mode from the file's magic bytes: gzip magic means
decompress, anything else means compress. That inference is right for `gzip`
and backwards for `gunzip`, and no amount of care inside the function can fix
it, because the information it needs was thrown away by the caller.

Two consequences beyond the obvious one:

* `gunzip -d FILE` does not help. `-d` is parsed and then ignored (`"-d" => {}`,
  commented "no-op, default for gunzip"), so the magic-byte guess still runs.
  Only `-t` and `-l` suppress it, because they are checked in the same
  condition — which is why self-test rung 29 has to spell `gunzip -t` to reach
  the `not in gzip format` diagnostic at all.
* The same shape is worth checking in the sibling pairs. `bunzip2`/`bzip2`,
  `unxz`/`xz`, `unzstd`/`zstd` and `unlz4`/`lz4` are separate `cmd_*` functions,
  so they are probably fine, but that has not been verified.

**Proper fix.** Pass the invoked name down: `"gunzip" => cmd_gunzip(args, Mode::Decompress)`,
`"gzip" => cmd_gunzip(args, Mode::Compress)`, with `-d` and `-c` overriding it
and the magic-byte sniff kept only as the tie-break for `gzip` with no flags
(where it is genuinely useful — `gzip file.gz` should not double-compress).
Then `gunzip` on a non-gzip file reports `gunzip: 'FILE': not in gzip format`
and exits 1, which is what real gunzip does and what the site already says when
it is reachable.

**Why it wasn't fixed on the spot.** Found while writing rung 29's assertions
for the 111-site statusless-bail sweep, which is a mechanical change across
dozens of commands; folding a behavioural change to the gzip family into that
commit would make both harder to review and to revert. Queued as the next task.

**Severity.** Data-affecting but not destructive: the original file is not
removed, so the outcome is a spurious `.gz` alongside it and a command that
reported success for doing the reverse of its name.

**Fixed 2026-08-24 in `4d9990f4c`**, along the lines proposed above but with one
deliberate departure. The proposal kept the magic-byte sniff "as the tie-break
for `gzip` with no flags (where it is genuinely useful — `gzip file.gz` should
not double-compress)". That would have left the guess deciding the direction in
exactly one case, and the case it decides is one where the two possible answers
are *compress again* and *decompress* — i.e. it would still silently do the
reverse of what `gzip` means, just from a narrower doorway. The fix instead
makes the name decide unconditionally and answers `gzip file.gz` by **refusing**
(`already gzip-compressed, not compressing again`, exit 1), which is what GNU
gzip does. The magic bytes now decide nothing; they are only consulted where the
direction is already settled, to ask whether the file is consistent with it.

The refusal is keyed on the header rather than on a `.gz` suffix — a suffix is a
claim and the header is the fact — and `-o` bypasses it, since naming the output
explicitly reads as deliberate.

Also fixed the `-d` half: `"-d" => {}` became `"-d" | "--decompress"` setting a
real flag, so the flag that had been documented in the command's own usage text
all along now does what the text says.

The two loose ends the entry raised are both closed. Self-test **rung 30**
covers all five directions, each on a file whose contents point the other way,
so the old guess cannot satisfy any of them; the round-trip is asserted on the
recovered bytes rather than on the success line. Rung 29's `-t` workaround is
kept, but now as a test of the sweep rather than a way around this bug, and its
comment says so. Rung 30 also picks up the `-t`-success assertion rung 29 had to
drop for want of a working compressor.

**The sibling pairs were checked and are fine.** `bzip2`/`bunzip2`, `xz`/`unxz`,
`zstd`/`unzstd` and `lz4`/`unlz4` each have separate `cmd_*` functions and
separate dispatch arms; the `un*`/`*cat` arms are decompress-only aliases. Only
gzip/gunzip ever conflated the two directions, which is why only it could guess.
