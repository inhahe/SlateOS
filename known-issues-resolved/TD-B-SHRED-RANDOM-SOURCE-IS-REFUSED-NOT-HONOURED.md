## TD-B-SHRED-RANDOM-SOURCE-IS-REFUSED-NOT-HONOURED — filed 2026-09-15 (lane B)

**Status:** FIXED 2026-09-25 (lane B, `e82a88d47`), found closed 2026-10-03.
The `shred` this entry is about -- a personality of `userspace/pv` with its own
`XorShift64` pass scheme -- is gone. `shred` is GNU coreutils 9.4's, ported
into coreutils (`userspace/coreutils/src/bin/shred.rs`), and reads
`--random-source=FILE` as GNU does, through gnulib's `randread`
(`coreutils::randint`): the file is the byte stream for the schedule and every
random pass. `scripts/shred-diff.sh` depends on exactly that to make both
programs write the same bytes. B-Q20, the question below, was answered on the
same grounds (§1052). On `main`, and through its boot tests, since 2026-09-25.

*The entry as filed:*

**In short:** `shred --random-source=FILE` now fails before touching the file
instead of silently using a different source of random bytes. Implementing it
properly needs a decision this entry records rather than makes.

**What was wrong.** `random_source` was parsed, stored, defaulted to
`"/dev/urandom"`, advertised as *"Source of random bytes (default
/dev/urandom)"* — and read by nothing. Two false statements in one option:

* the flag did nothing;
* **the advertised default was also wrong.** Nothing in the program opens
  `/dev/urandom`. `generate_shred_pattern` uses an internal `XorShift64`,
  deliberately, so that a shred pass does not depend on a device node
  existing.

**Why refuse rather than ignore, when elsewhere this tree accepts an inert
option.** Because shred destroys the file. A user who asks for a particular
source of random bytes and silently gets a different one has already lost the
data by the time they could notice. Failing before the first pass is the only
outcome that leaves them a choice. Verified: the refusal exits 1 and the file
is byte-intact afterwards.

**What implementing it needs, and why it is not obvious.** The pass scheme here
is bespoke — even passes are random, odd passes are the **bitwise complement of
the previous pass**, reproduced by re-seeding the PRNG identically. A file
source breaks that reconstruction:

* re-reading the previous pass's bytes needs a seek, and the obvious sources
  (`/dev/urandom`, a pipe) are not seekable;
* buffering a whole pass to complement it later is unbounded in the file size;
* using the file to *seed* the PRNG instead would preserve the scheme and
  **would not be what GNU does** — GNU consumes the file as the byte stream.
  Inventing that divergence silently is worse than refusing.

So the choice is between changing the pass scheme and buffering per chunk, and
that is a design decision with a security dimension. It should be made
deliberately, not as a side effect of clearing an unread field.

**Put to the operator as B-Q20, 2026-09-16.** Until then this entry recorded a
decision needing the operator and sat in `known-issues.md`, which is the bug
tracker rather than the decision queue -- so it was never actually in front of
them. `open-questions.md` is the queue; an entry that is not in it is not
waiting on the operator, it is just waiting.

The options there are A (overwrite in pairs, one chunk at a time -- bounded
memory, different partial-wipe pattern after a power cut), B (buffer a whole
pass -- nothing observable changes, and a 4 GB file needs 4 GB of memory) and C
(leave it refused). A fourth -- seeding the PRNG from the file rather than
consuming it as the byte stream -- is named there and explicitly NOT taken
without an answer, because it silently diverges from GNU on a data-destruction
tool.

**Where it lives:** `userspace/pv/src/main.rs` — `parse_shred_args`'s
`--random-source=` arm, `generate_shred_pattern`, `XorShift64`.
