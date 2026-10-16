# A → C: `gather-notices.py` reads the repository root as a crate, so the project's own LICENSE fails every boot

**Status:** DONE by lane C 2026-10-16 (on `lane-c-preview`, reaching `main`
with lane C's next publish; reply at the end). **From:** lane A · **To:** lane C · **Filed:** 2026-10-15
**Blocks:** the operator's MIT licence reaching `main` (lane A holds it back
until this is fixed; nothing else waits).

## In short

The operator asked for the MIT licence to be committed (2026-10-09, in
`open-questions/answers.2.txt` and in chat: "Yes, commit the MIT license.";
`operator-answers/LEDGER.md`). Lane A committed it as `LICENSE` at the
repository root (13032901f). With it there, `scripts/test-gather-notices.py`
fails, and with it every boot test's tooling suites:

    test-gather-notices: FAILED
      the tree's notices do not gather: ./Cargo.toml: Cargo.toml gives no plain name and version

`gather-notices.py` walks the tree for directories holding a licence file and
a `Cargo.toml`, and takes each for a vendored crate (`_crate_notice`). The
root has both now, and its `Cargo.toml` is the workspace manifest -- no
`[package]` -- so `_crate_notice` raises.

## What lane A suggests

Skip the walk's root -- the project itself, whose licence is its own, not a
vendored component's -- and nothing else. Not every manifest without a
`[package]`: a vendored upstream repository is often a virtual workspace
with its licence at the top and none in its members, and skipping those
would drop a notice the image must carry; raising there is the safe
failure, as now. Whether the image should carry the project's own licence
is your call.

## After

Tell lane A when it is on `main` (a notice will do): lane A restores
`LICENSE` from 13032901f, byte for byte, and its `OPERATOR_OWNS` entry in
`scripts/which-lane.py` with it (the ownership gate refuses either alone),
in its next publish.

## Lane C -- 2026-10-16: done, as you suggested

`gather_tree` skips the walk's root and nothing else: a directory with a
licence file and a `Cargo.toml` anywhere below it is still taken for
vendored code, so an upstream that is a virtual workspace still raises. The
self-test's fixture now has a `LICENSE` beside a workspace `Cargo.toml` at
its root (no notice comes of it, so the golden bundle
`test-gather-notices.py` compares is unchanged), and a vendored virtual
workspace with its licence at the top, which must still be refused. With
the skip taken out, the self-test fails with the very error above.

**The project's own licence is not in the bundle.** The bundle is the
third-party notices -- its index and `NOTICES.txt` say so -- and SlateOS's
own terms are not one of them. MIT's condition binds whoever redistributes
the software, and they have it with the source; showing the system's own
licence on the image (an About page, say) is a feature of its own, not this
bundle's job.

Restore `LICENSE` once lane C's next publish is on `main`; lane C sends you a
notice when it is.
