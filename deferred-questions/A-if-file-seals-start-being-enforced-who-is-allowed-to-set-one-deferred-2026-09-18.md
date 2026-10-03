## [A] If file seals start being enforced, who is allowed to set one? — deferred 2026-09-18

**Trigger:** the first caller of `sealing::add_seals` outside `kshell`, or any
change that makes `fs/vfs.rs` / `fs/handle.rs` consult a seal. Either event
turns this from hypothetical into a policy that ships.

**In short:** the kernel can record that a file is permanently unchangeable,
and nothing checks that record, so the mark does nothing today. If someone
makes it real, an unanswered question becomes load-bearing: *who may apply an
irreversible restriction to a file?* Right now the answer would be "anyone
who can call the function", because there is no permission check at all.

**Why it is deferred rather than queued.** Nothing uses sealing: every
`sealing::` reference outside `kernel/src/fs/sealing.rs` is `procfs.rs`
(`stats`, `list_sealed`) or `kshell.rs` (`add_seals`, `get_seals`), and the
write and truncate paths contain no reference to seals. So the policy has no
consequences until enforcement lands, and `open-questions.md` is 32 entries
deep with none of lane A's seven answered. Asking now would pad the queue
this file exists to protect.

**What was already done, so this is not a loose end.** `sealing.rs`'s module
doc now leads with **NOT ENFORCED YET** and says a sealed file is writable;
`known-issues.md` carries the enumeration; and `stats()`'s `denied` counter is
documented as only ever able to read 0, so `/proc` cannot be misread as
"nobody tried" when the truth is "nothing is checked".

**The shape of the decision, recorded now while the evidence is fresh.**
`grep SEAL kernel/src/cap/rights.rs` returns nothing, and `add_seals`
contains no capability check, so all three options below are open:

| option | consequence |
|---|---|
| a new right, e.g. `SET_SEAL` | costs a bit in `Rights` (`DISTINCT` is currently 16 and pinned, so the pin moves), and every legitimate sealer needs granting it |
| no right — anyone may seal | simplest, and a denial-of-service: any process that can open a file for write could make it permanently unwritable |
| seal only what you own | no new right, but "own" needs defining against the existing file-tag model, and it does not cover a shared file, which is the case the feature's own doc names |

The middle option is the one to be careful about: irreversibility plus no
authority check is a combination that cannot be walked back per-file, only by
removing the feature.
