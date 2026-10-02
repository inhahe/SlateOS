## 703. `tar` carries every one of GNU's long-option names but performs twelve — a recognised name it cannot honour is refused, not ignored

> The counts written into the body below — 171 names, nine performed — were both
> corrected within the day; see the two amendments at the end. The real figures
> are **172 names, twelve performed**. The body is left as written because the
> second amendment is about *how* the wrong number was arrived at, which is the
> part worth not losing.

**Date:** 2026-08-29
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** GNU's `tar` accepts about 171 spelled-out options (`--verbose`,
`--exclude`, …); ours does the work of nine of them. The question is what to do
with the other 162 when someone types one. Ignoring them is dangerous — a
backup that was told to leave files out would quietly include them and report
success. So this `tar` now knows all 171 names, and answers the 162 it cannot
perform with "recognised but not implemented" and a failure status, which stops
a script instead of misleading it. Knowing the names it does not implement also
turns out to be *required* for the nine it does, for the reason in the second
half below.

**The decision, in two parts.**

1. **The name table lists every option GNU has, not every option we perform.**
2. **A name in the table that we do not perform is a usage error** —
   `tar: option '--exclude' is recognised but not implemented by this tar`,
   status 64 — rather than being silently dropped or reported as unknown.

**Why part 1 is not optional.** An abbreviation is legal if it is a prefix of
exactly one option, so the *set of names* decides what a prefix means. With a
nine-name table, `--ex` is a unique prefix of `--extract` and would extract;
GNU calls it ambiguous and lists fourteen candidates. The sharpest case is
`--verb`, which is ambiguous only because of `--verbatim-files-from` — an option
this `tar` will likely never implement, and whose sole function here is to make
`--verb` an error, exactly as GNU does. A table trimmed to what we implement
does not merely produce a worse message; it silently runs a *different command*
than the same argv would run under GNU. The shared `coreutils::getopt` module
already says this in its own words, so part 1 is house doctrine rather than a
judgement call — it is recorded here only because the 162 dead entries look like
dead weight to a future reader who might tidy them away.

**Why part 2, and what was rejected.**

| Answer | Cost |
|---|---|
| **Refuse (chosen)** | Diverges from GNU visibly: a command GNU runs, we decline. A script using an unimplemented option stops with a clear reason. |
| Ignore it | *What changes:* `tar -cf a.tar --exclude '*.o' src` silently archives the `.o` files and exits 0. The failure is silent, in the direction of doing more than asked, and is discovered when the archive is restored. |
| Report `unrecognized option` | *What changes:* the message says `--exclude` is not a `tar` option. It is; we just don't do it. That sends the reader to check a spelling that was already right. |

Refusing is the only one of the three whose failure mode is a stopped script
rather than a wrong archive, and it is the only one that tells the truth about
why. The divergence from GNU is real and is the price.

**A note on what this replaced, because it makes the stakes concrete.** Before
this change `tar` had *no* long options at all: `--anything` fell through to the
operand branch and became a file name. So `tar -cf a.tar --exclude '*.o' src`
did not ignore the option — it tried to archive two files called `--exclude` and
`*.o`, complained they did not exist, and produced an archive that both lacked
the exclusion and contained a spurious member. The same fall-through also meant
`--` was not an end-of-options marker but a member name to look for, so
`tar -xf t.tar -- a` failed with `tar: --: Not found in archive` where GNU
exits 0.

**How the table was established, and why that matters for maintenance.** The
names, their argument classes, and their *order* were all measured from GNU
1.35 rather than transcribed, then checked exhaustively: every one of the 1381
distinct prefixes of the 171 names was put to the real binary and its verdict
compared against this table's — resolved, unrecognised, or ambiguous, with the
candidate list in order — for **zero** mismatches. That sweep is also what
established that `tar` needs plain name-based resolution rather than the
alias-aware variant: `tar` does have alias pairs (`--extract`/`--get`,
`--preserve-permissions`/`--same-permissions`), but no two aliases share a
prefix, so nothing is made unabbreviatable by keying on names. If a future
change adds a name, that assumption is the one to re-test.

**The order is observable output and must not be alphabetised.** `getopt_long`
lists an ambiguous prefix's candidates in table order, so `--extract` must
precede `--exclude`. It was obtained by asking the binary one question per
letter — every candidate for an ambiguous prefix necessarily shares its first
letter, so 36 queries fix the whole array and cross-letter order is
unobservable. That sweep is also what turned up `--program-name`, which appears
in no `--help` output because argp hides it, and which is therefore in the table
on the authority of the ambiguity lists alone.

**Two amendments, both the same day (2026-08-29).**

1. `--help`, `--usage` and `--version` moved from the refused group into the
   performed one — see 704 — so the counts read *twelve performed*.
2. **The table is 172 names, not 171, and the paragraph above is wrong about
   the order.** The push-time gate `scripts/getopt-ambiguity-check.py` — which
   this entry did not know existed — reads GNU's table with one measurement the
   per-letter sweep cannot match: `tar --=x`. The empty name is a prefix of
   *every* entry, so the ambiguity list is the whole table, in declaration
   order, in one line. Two things fell out. First, it found `--HANG`, a hidden
   debug option that sleeps forever and the only name beginning with a capital
   — invisible to a sweep over `--a … --z`, and therefore missed above. Second,
   the claim that "cross-letter order is unobservable" is false: this one case
   observes all of it, and the real order is argp's grouping by function, not
   the letter-grouped reconstruction that was committed. Both are fixed, the
   exemption that had kept `tar` out of the gate is deleted, and `tar --=x` is
   now byte-identical to GNU's (2806 bytes, exit 64). Nothing else in this entry
   changes — the per-letter order happened to be right *within* each letter, so
   every diagnostic it was reasoned about stayed the same. The lesson worth
   carrying: **enumerate from the utility's own output, never from an alphabet
   you chose**, and check whether the repository already has a gate for what you
   are about to measure by hand.
