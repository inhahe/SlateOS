## 1052. shred --random-source reads its source as GNU's does; the question's premise had gone

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q20), with Claude keeping GNU's
behaviour for a source that is a regular file -- a divergence from the
operator's proposal, made for the reason below and put back to the operator.
Relayed verbatim through lane F's session.

**In short:** `shred` overwrites a file several times to destroy it, and
`--random-source` names where its random bytes come from. When this was
asked, our `shred` refused the option, and supporting it seemed to mean
changing the order of the overwrite. Since then `shred` was replaced by a
port of GNU coreutils 9.4's, in which `--random-source` works exactly as
GNU's does, byte for byte, checked by `scripts/shred-diff.sh`. That port
already does what the operator asked for, in all but one detail.

**The operator's answer, verbatim:**

> I don't understand why the behavior of the random-and-complement passes
> has to change just to support reading from a file or /dev/urandom, which
> should be a completely separate path. And I think the failure mode in
> which one full pass was done but not later passes is many times better
> than the failure mode in which half a file was overwritten and the other
> half not. But I'm having a lot of trouble understanding this particular
> open question in general. But I'd say if the user specifies /dev/urandom,
> just read from that as you pass over the file, as many times as they want.
> If the user specifies a file to read from for the data to overwrite with,
> read that file in chunks as it overwrites the deleted file, then on the
> next pass, start reading the random-data file from the beginning again.
> Fair?

**Against the port:**

| The operator asked for | The port |
|---|---|
| The random source as a separate path from the generator's passes | yes: with `--random-source`, every random byte comes from the source |
| Whole passes, so an interruption leaves full passes done | yes: each pass sweeps the whole file, as GNU's does |
| `/dev/urandom` read continuously, as many passes as asked | yes |
| A file source re-read from its beginning at each pass | **no -- kept as GNU's**: the file is read on from where the last pass stopped, and a file that runs out ends the run with `shred: FILE: end of file` before the next write |

**Why that one detail stays GNU's.** Restarting the file at every pass makes
every random pass write the same bytes; the passes after the first then add
no randomness, which is what multiple passes exist to add. It would also
make `shred --random-source=F` write different bytes from GNU's on the same
input, which is what the port's harness checks. If the operator wants the
restart anyway -- to make a short source last -- it can be an explicit
option rather than a change to GNU's; that is put back to them.
