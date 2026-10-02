## TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE (lane B, 2026-08-16) — **fixed in `sort` 2026-08-16** (the other utilities are tracked in the follow-up entry below)

**In short:** When you mistype an option, `sort` tells you so in the wrong
words. glibc's `getopt_long` — the thing every GNU utility's option errors come
from — uses one set of sentences for short options (`-x`) and a *different* set
for long ones (`--foo`). Ours uses the short-option sentences for both, so
every long-option mistake is reported in a form GNU never prints. Separately,
and worse, the mistyped option is printed raw, so an option name containing a
newline forges an extra line of output.

Measured against GNU coreutils 9.4 under `LC_ALL=C`, all eight differ:

| Command | GNU | ours |
|---|---|---|
| `sort -x` | `invalid option -- 'x'` | `unknown option -- x` |
| `sort -k` | `option requires an argument -- 'k'` | `option requires an argument -- k` |
| `sort --fo` | `unrecognized option '--fo'` | `unknown option -- fo` |
| `sort --k` | `option '--key' requires an argument` | `option requires an argument -- k` |
| `sort --s` | `option '--s' is ambiguous; possibilities: '--sort' '--stable'` | `ambiguous option -- s` |
| `sort --sort` | `option '--sort' requires an argument` | `option requires an argument -- sort` |
| `sort --stable=x` | `option '--stable' doesn't allow an argument` | `option doesn't take an argument -- stable` |
| `sort --output` | `option '--output' requires an argument` | `option requires an argument -- output` |

Two details are easy to miss. GNU quotes the offending option even in the short
forms (`-- 'x'`, not `-- x`). And the long missing-argument message names the
option **as resolved**, not as typed: `--k` is reported as `'--key'`.

**Where it lives.** `userspace/coreutils/src/bin/sort/main.rs` —
`unknown_option()`, `missing_argument()`, the ambiguous and
doesn't-take-an-argument branches, and `show()`. `sort` is where it was found
because `sort` is the only utility with a real `getopt_long`; the other 84 parse
argv by hand and will inherit the same shapes as they gain long options, so the
right fix is a shared option-error helper rather than eight strings in `sort`.

**Why the harness missed it.** `scripts/sort-diff.sh` grew its option section
in the same session as the option parser, and the cases it compares are the
ones the parser was written to handle. None of the eight above is among them —
a differential harness only tests the differences someone thought to try.
Whatever fixes the wording should add all eight, and the fix is not done until
the harness fails without it.

**The security half.** The option name is interpolated raw:

```text
$ sort $'--fo\nsort: /etc/shadow: Permission denied'
sort: unrecognized option '--fo
sort: /etc/shadow: Permission denied'
```

That is GNU's output, verbatim — glibc wraps the option in literal quotes and
escapes nothing inside them. It is reachable without an attacker controlling
argv: a file named `--fo⏎…` in a directory, and any script running `sort *`,
puts it there. The fix is to render it with `coreutils::quote::quote`, which
produces `'--fo'` for every ordinary option name — byte-identical to GNU — and
diverges only where GNU would emit a raw control byte. So this costs no
fidelity on any benign input, which is why it should not need a
differs-on-purpose entry in the harness.

### Resolution (2026-08-16) — and a correction to the paragraph above

All eight shapes now match glibc byte for byte, along with seven more found
while fixing them. `sort`'s option diagnostics are `getopt_error`,
`short_flag_error`, `invalid_option`, `short_missing_argument`,
`unrecognized_option`, `long_missing_argument` and `long_unwanted_argument`;
`show()` is gone.

**"Why the harness missed it" was diagnosed wrong, and the real reason is worth
more than the bug.** The entry above says the option section simply lacked the
eight cases. It did not: `--bogus`, `-x`, `--rev=x`, `--key`, `-k`, `-o`,
`--output`, `--sort`, `--r`, `--d` and `--s` were *all* already there, compared
character by character against `$GNU`, and all passing. They passed because
**`$GNU` was the wrong program.** The harness defaults to the `sort` on `PATH`,
which on this host is MSYS2's — and MSYS2 is a Cygwin derivative that links
`msys-2.0.dll` instead of glibc. **Its getopt is not glibc's**, and the two
disagree on every message in this family:

| command | msys-2.0 (coreutils 8.32) | glibc (coreutils 9.4) |
|---|---|---|
| `sort -x` | `unknown option -- x` | `invalid option -- 'x'` |
| `sort --bogus` | `unknown option -- bogus` | `unrecognized option '--bogus'` |
| `sort --s` | `ambiguous option -- s` | `option '--s' is ambiguous; possibilities: …` |
| `sort --key` | `option requires an argument -- key` | `option '--key' requires an argument` |
| `sort --rev=x` | `option doesn't take an argument -- rev` | `option '--rev' doesn't allow an argument` |

So the implementation was a faithful copy of a Windows porting artifact, and the
harness certified it. The lesson is not "add more cases": a differential harness
is only as good as the thing it differs against, and "GNU sort" on this host is
two different programs. `scripts/sort-diff.sh` now runs the option section
against `wsl -e env LC_ALL=C sort` (`$GLIBC`, skipped with a message where WSL
is absent) and keeps `$GNU` for the `--files0-from` cases, whose text comes from
`sort` itself and `strerror` rather than from getopt.

The same wrong reference is why the fixture in `tests/quotearg-gnu.txt` names
`sort (GNU coreutils) 9.4` while this harness was reading 8.32 — two references
in one tree, neither aware of the other.

**Seven more differences found while fixing the eight.** Each was measured, not
predicted:

- The ambiguous list is printed in the order the options are **declared** in
  GNU's `struct option[]`, not alphabetically. `LONG_OPTIONS` was alphabetical
  and is now GNU's order, which makes the array order observable output. It was
  measured rather than recalled because recall got it wrong: `--random-sort`
  precedes `--random-source`. The instrument is one command — an empty prefix
  matches every option, so `sort --=x` prints the whole table.
- `unrecognized option` echoes the **whole** argument including any `=VALUE`:
  `sort --fo=bar` says `'--fo=bar'`, not `'--fo'`.
- `doesn't allow an argument` names the **resolved** option, like the long
  missing-argument message: `--stab=x` reports `'--stable'`.
- A non-ASCII short flag was rendered `other as char`, which maps `0xC3` to `Ã`
  and re-encodes it as two bytes — an option nobody typed. It is a byte now.
- gnulib's `argmatch` resolves an option's *argument* by prefix exactly as
  getopt resolves the option's name. We did not do this at all, so `sort
  --sort=hum` and `sort --check=q` — both valid — were refused.
- `argmatch` has a second sentence, `ambiguous argument '' for '--check'`, for a
  prefix matching several words. Which sentence you get turns on whether the
  candidates *mean* different things, not how many there are: `quiet` and
  `silent` share a value, so a prefix matching only those two resolves.
- The "Valid arguments are" list groups words that share a value onto one line
  (`- 'quiet', 'silent'`). It is now generated from the same table the match
  uses, so the list cannot drift from the matcher.

**The differs-on-purpose prediction was also wrong, in a small way.** Quoting
the option name is free for every name a person would type, as predicted — but
not for a name containing a byte that is not printable, where glibc emits the
raw byte and we emit `\303` or `\001`. That is the divergence working as
intended, so the harness carries exactly two `xfail_getopt` cases for it, and
they XPASS loudly if we ever stop escaping.

**Verified**: 38 `sort` unit tests, including `every_getopt_sentence_matches_glibc`
and `an_option_name_cannot_forge_a_second_diagnostic_line`, which hold the glibc
literals so they are checked on a host with no reference `sort` at all. The
harness fails without the fix — checked by building the pre-fix binary and
running it, not by assuming. On the rebuilt `scripts/sort-diff.sh`, with the
glibc reference in place:

| tree | result |
|---|---|
| pre-fix `sort` (built from `HEAD` before this change) | **247 passed, 33 differed**, 2 differ on purpose |
| post-fix | **280 passed, 0 differed**, 2 differ on purpose |

The 33 are the eight documented shapes plus the seven further differences found
while fixing them, times the arguments that exercise each. Note also that the
*old* harness scored these same 33 cases as passing, because it was comparing
against MSYS2's `sort` — which is the whole point of the correction above.

### Follow-up (2026-08-25) — the whole suite reaches glibc now, not just this one section

The correction above repaired *one* harness, and repaired it by adding a second
reference to it: `sort-diff.sh` grew a `$GLIBC` that shelled out to `wsl -e env
LC_ALL=C sort` for the option section, while `$GNU` stayed pointed at the host's
MSYS2 `sort` for everything else. That was the narrowest fix that could work, and
it left the diagnosis — "a differential harness is only as good as the thing it
differs against" — applying word for word to the other forty-four harnesses,
every one of which was still reading MSYS2.

All 45 now source `scripts/diff-wsl.sh`, which re-execs the whole harness inside
WSL and resolves the reference there:

```
$ for f in scripts/*-diff.sh; do
    [ "$f" = scripts/all-diff.sh ] && continue
    grep -q '^\. "\$(dirname "\$0")/diff-wsl.sh"' "$f" || echo "NOT MIGRATED: $f"
  done
$                      # no output
```

There is no longer a `$GNU` anywhere in `scripts/` that can be MSYS2's. The
two-references-in-one-tree hazard named at the end of the correction above is
gone structurally, not harness by harness — which is the difference between a
fixed bug and a bug that cannot recur. A new harness written tomorrow inherits
the glibc reference by sourcing the preamble; there is no step it can forget.

Three things that came with it, all relevant to this entry:

- **The subject moved as well as the reference.** `diff-wsl.sh` builds ours for
  `x86_64-unknown-linux-gnu` rather than `x86_64-pc-windows-gnu`. The old Windows
  build was not a different compilation of the same program — `coreutils::stdfd`
  is `#[cfg(target_os = "linux")]`, so the harnesses were measuring a binary with
  no write-error exit path in it.
- **The reference is now version-pinned by being a real one.** This entry closes
  by noting that `tests/quotearg-gnu.txt` names `sort (GNU coreutils) 9.4` while
  the harness was reading 8.32. Both sides of that mismatch were host artifacts;
  the WSL reference reports its own version and `DIFF_NEED` skips loudly rather
  than silently agreeing when it is absent.
- **A stale subject can no longer pass.** `diff_assert_fresh` compares the built
  artifacts against every source file that feeds them and refuses to run if a
  source is newer. The old path had no such check — see
  `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES` and §382 in
  `design-decisions.md`.

**Still outstanding, and it is the same bug one subsystem over.**
`scripts/osh-bash-diff.py` compares a Windows `osh.exe` against
Git-for-Windows' `bash.exe`, which is the identical mistake this entry is about:
a Cygwin-derived reference certifying a Windows-porting artifact. Four entries in
this file exist only because of it. Tracked as
`TD-B-THE-SHELL-HARNESS-STILL-MEASURES-AGAINST-MSYS-BASH`.
