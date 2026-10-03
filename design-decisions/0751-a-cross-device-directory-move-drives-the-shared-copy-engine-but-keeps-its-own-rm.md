## 751. A cross-device directory move drives the shared copy engine but keeps its own `rm -r`, and its failure carries no sentence

**Date:** 2026-09-03
**Decided by:** Claude (autonomous)
**Lane:** B
**Superseded in part, the same day, by §757:** the "keeps its own `rm -r`" half
lasted one day. `mv` now calls `coreutils::remove`, which `rm` calls too. The
part of this entry that still stands is section 2 (the message-less error case)
and the *reasoning* in section 1, which correctly named splitting the walk off
`Rm` as the real fix — §757 is that fix, not a reversal of this.

**In short:** `mv dir /other/filesystem/` has to copy the tree and then delete
the original. The *copy* half is now one call into the engine that `cp -r`
already uses — that was the point of the four-stage extraction. The *delete*
half is thirty lines newly written in `mv.rs`, even though `rm.rs` in the same
crate is a recursive deleter. And the error type had to grow a case that
carries no message at all. Both of those look like the wrong call at a glance,
which is why they are written down.

### 1. The copy is shared; the removal is not

`rm.rs` deletes trees. Writing a second walk that deletes trees is exactly the
"two places for the same bug" this whole extraction exists to avoid, and the
argument for sharing is the same one that moved 1,300 lines of `cp`'s walk into
`copy.rs` in stage 4 (§750).

It is nonetheless the wrong shape here, and the reason is that **GNU does not
share it either — what it shares is a different thing.** Upstream's `mv` calls
`rm()` (`mv.c:238`), but `rm()` is `remove.c`, a library module, and what `mv`
hands it is a `struct rm_options` with nine fields fixed at `rm_option_init`
(`mv.c:87`): never interactive, not `-f`, recursive, `preserve_root` on,
`one_file_system` off. Our `rm.rs` is not that module. Its `Rm` struct is a
350-line machine wired to `rm`'s own command line — the `-i`/`-I` prompts, the
write-protected-file question, `--preserve-root=all`, `--one-file-system`, its
own `Verdict` and `Question` enums, and byte-slice paths rather than `Path`.
`mv` needs none of it and can supply none of it.

| | share `rm.rs`'s walk | `mv.rs` writes its own |
|---|---|---|
| what has to move first | `Rm`'s prompts, preserve-root, one-file-system and byte-path plumbing, split into a policy `mv` can fill in | nothing |
| size of that | a second extraction on the scale of stages 1–4, in a *different* binary | — |
| what `mv` ends up calling | a walk with eight knobs, seven of them pinned | a walk with one (`-v`) |
| duplicated logic | none | post-order descent plus `unlink`/`rmdir`, ≈30 lines |
| where a divergence would show | one place | two |

The deciding fact is the last row's cost weighed against the second row's. What
is duplicated is *the shape of a post-order walk*, not a policy: `rm`'s
interesting behaviour is all in the questions it asks before deleting, and `mv`
asks none of them. So the two walks cannot come to disagree about anything a
user can observe except the wording of two sentences, and those are pinned by
`scripts/mv-diff.sh` §22 against GNU directly.

**This is a deferral, not a refusal.** The right end state is a shared `remove`
module — `remove.c`'s position in the tree — with `rm.rs` and `mv.rs` both
driving it, and it should be done when `rm.rs` is next opened for its own
reasons. It is logged as tech debt — `known-issues.md` →
`TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS`, which also records the
one place the two walks already behave differently and why the extraction has
to wait on `TD-B-RM-WALKS-BY-PATH-…` — so the second walk cannot quietly become
permanent. What it must *not* be is a rider on the stage that made directories
movable at all: that stage's certification is "the harness numbers do not
move", and refactoring a third binary inside it would destroy the meaning of
that check.

### 2. `Failed` gains a variant that carries no message

`mv`'s cross-device fallback reports failures as `Failed { what, err }` — the
sentence GNU prints for that step, plus the errno. The type exists precisely
because the sentence *cannot* be derived from the errno: an unreadable source
and an unwritable destination directory both give `EACCES`, and GNU answers
`cannot open 'f' for reading` for one and `cannot create regular file 'd/g'`
for the other.

A tree copy breaks that premise. It fails at as many entries as went wrong, and
the engine has already reported each one, naming that entry, where it happened.
There is no single sentence left to carry.

Three ways to express it:

| | *What changes* |
|---|---|
| invent a sentence (`cannot move X to Y`) | the user gets a summary line GNU never prints, under the ten real ones |
| carry the first failure's sentence | nine diagnostics are silently dropped and the one kept is arbitrary |
| **a variant with no message** | the caller stops and restores the `-b` backup; nothing extra is printed |

The third is chosen, and it is GNU's own behaviour rather than a compromise:
`copy_internal` reports and returns false, and `do_move` prints nothing on top
of it (`mv.c:186`). The cost is that `Failed` is no longer a struct whose
fields can be read — every printer goes through `Failed::report`, and there are
three of them. That is enforced by construction rather than by memory: a
printer added later cannot destructure past the enum, which is the failure mode
a `bool` flag beside the struct would have had.

### Reversing either

Decision 1 reverses by extracting `rm.rs`'s walk into a shared module and
deleting `mv.rs`'s `remove_tree`; the three `removing_the_source_*` tests are
the specification the shared one would have to meet, and they are written
against `remove_source` rather than against the walk so that they survive the
swap. Decision 2 reverses by giving `Failed::Reported` a sentence — the arm in
`copy_across_devices` is the only place that constructs it — but doing so means
printing something upstream does not, and `scripts/mv-diff.sh` compares stderr
byte for byte.
