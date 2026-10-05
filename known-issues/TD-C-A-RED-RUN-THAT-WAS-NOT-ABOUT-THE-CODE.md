## `TD-C-A-RED-RUN-THAT-WAS-NOT-ABOUT-THE-CODE` (lane C, 2026-09-18)

**In short:** A full `cargo test --workspace` came back red with what looked
like three missing dependencies in `gui/window`. Nothing was wrong with
`gui/window`. I had run `cargo clippy` against the same `target/` directory
while the test run was going, and clippy leaves behind artifacts that the
documentation tests cannot use. The same run passes alone.

**What it looked like:**

```
   Doc-tests oswindow
error[E0463]: can't find crate for `appearance`
  --> gui\window\src\app.rs:95:5
error[E0463]: can't find crate for `guiremote`
error[E0432]: unresolved imports `crate::DISPLAY_VAR`, `crate::PixelFormat`
error: doctest failed, to rerun pass `-p oswindow --doc`
```

**Why it is worth a file of its own: the symptom names the wrong fix.**
"Can't find crate for `appearance`" reads exactly like a missing entry in
`Cargo.toml`, and the obvious next move is to go and add one -- to a manifest
that is correct, in a crate that is fine, which would then have to be undone.
The tell is in the `rustdoc` command line the error prints underneath it:

```
--extern 'appearance=...	arget\...\libappearance-c585cc90e74addac.rlib'
```

**The crate is named right there.** rustdoc was told exactly where it was and
still could not use it, which is not what a missing dependency looks like --
a missing dependency has no `--extern` at all. That one line separates "the
manifest is wrong" from "the file on disk is not what it should be", and it is
the only thing in the output that does.

**The mechanism.** `cargo clippy` and `cargo test` share `target/`, and clippy
compiles metadata-only: it type-checks and emits an `.rlib` with no generated
code in it, because it never needed any. A later `rustdoc` asked to *link*
against that file finds nothing to link. So the two commands are not
independent, and running them at the same time against one directory makes the
second one's result a fact about the first.

**What to do instead.** Do not interleave them. Run the lint pass and the test
pass one after another, or give the lint pass its own `--target-dir` and
**delete it when the run finishes** -- a scratch target dir is 10-40 GB and
`CLAUDE.md` is explicit that leaving one behind is a leak.

**A postscript, because this entry shipped with the defect it describes.** The
Windows path quoted above went into the file as `src` + a raw **BEL** byte +
`pp.rs`. The heredoc carrying the Python that wrote it collapsed one level of
backslash, so the doubled escape I had written arrived as a single one, and
Python turned the surviving `\a` into the byte it names. The pre-push gate
caught it -- the same gate that caught
lane A's raw NUL the same afternoon, in a sentence describing code that strips
NULs.

**Python warned me and named the wrong escape.** The same mangling also produced
`\w`, which is *invalid*, so Python printed `SyntaxWarning: "\w" is an invalid
escape sequence`. I read that warning, reasoned about `\w` -- correctly, it
stays literal and the output was fine -- and moved on. `\a` is a *valid* escape,
so it produced no warning at all and silently became a control byte. **The
diagnostic names the harmless one precisely because it is the one Python cannot
handle**; the dangerous one is by definition the one it handles quietly. A
warning about escapes in a string is a warning about *every* escape in that
string, not only the one it prints.

The rule I already had -- write files from a script on disk, never a heredoc --
is the one that prevents it, and the one I keep not following for "just this
short edit".

**A second instance the same day -- and my account of it was itself wrong, in
the way this entry is about.** A report reached me claiming
`audit-cli-fabrication --check` was red **on main**, blocking every lane's
pre-boot, naming `passwd` and `unshare`, with a careful and correct account of
why a naive version of that audit would flag both.

**What I verified:** it is green. In a worktree whose HEAD was exactly
`origin/main` -- 213 crates, zero hits, `unshare` classified as "inert but
refusing honestly (not deletable)", `passwd` not mentioned. Lane A
independently reports green at a different commit. The two exonerations the
report proposed already existed, added 2026-09-15: `delegates_io` for a command
whose I/O is done by a first-party helper crate, and `refuses_honestly` for one
that declines rather than pretends.

**What I got wrong, twice over.** I inferred a stale checkout, and I attributed
the report to lane A because they were the peer I had been corresponding with.
Both were inferences presented as findings. Lane A did not send it; their copy
is not stale -- both exonerating commits are ancestors of their HEAD, and their
run is green. Had that stood, this file would have credited a lane-A diagnosis
for prompting two fixes that were already written weeks earlier, which is a
false attribution of exactly the kind we had both spent the day chasing in
code: an artefact that reads as true and points at the wrong source.

**Where the report actually came from is unknown, and the evidence says it is
a mechanism rather than a correspondent.** It arrived inside the captured
output of one of my own background `git push` tasks, between the status line
and the push's ref updates -- and then arrived **again, byte-identical, in the
same position, in the next push task**. A correspondent does not resend the
same paragraph to the same byte; a channel does. Searched for and not found in
the tracked tree (`git grep`), in `scripts/`, or in the pre-push hook chain, so
it is not something the push itself prints.

**Lane A looked and did not find it**, which is a real negative rather than a
silence: 549 captured background-task outputs in their session searched for the
paragraph's distinctive phrases, including their own `git push` tasks -- the
same kind of task, and the same position in the output, where mine appeared
twice. Zero hits. So a harness-wide mechanism should have produced it there too
and did not, which points at something local to this session rather than to the
tool we both run. One observer failing to reproduce is not proof of absence,
and the asymmetry is the whole content: two identical sightings here, none
there.

That is as far as the evidence goes, and the entry stops there. The honest
statement is that an unattributed report of a red gate was wrong about the
gate, and that I compounded it by supplying a source and a cause from context
rather than from evidence. Naming the one remaining peer would be the identical
move with a different name in the slot.

**The fix is provenance, and it is one line.** `audit-cli-fabrication` now ends
its verdict with the commit it measured, with `+dirty` when the checkout is not
the commit it names, and its failure path says to compare trees before code.
**A gate that fails without saying which tree it read is indistinguishable from
a gate that is wrong** -- "green at a2841e076" and "green at e944af0e0" are two
facts, where "it is green" against "it is red" is an argument. That holds
regardless of who sent what, which is why the change stays.

**And the shape generalises past tools to messages.** I applied "name the
commit you measured at" to my script within minutes of the incident, and did
not apply "name the source you are quoting" to the sentence I wrote about it.
The second is the cheaper of the two and I had the harder one in hand.

**The general form, which is the reason this is filed rather than muttered:**
a build system with shared state means **a red run is not automatically about
the code**, exactly as a green one is not automatically about the tests. Both
directions need the same question asked -- *what else was touching this
tree?* -- and the answer here cost one re-run rather than an afternoon only
because the `--extern` line was read before the manifest was opened.
