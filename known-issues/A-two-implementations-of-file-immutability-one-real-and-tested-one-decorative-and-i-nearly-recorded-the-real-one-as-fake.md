### [A] Two implementations of file immutability: one real and tested, one decorative -- and I nearly recorded the real one as fake -- 2026-09-18
**Status:** OPEN

**In short:** the kernel can mark a file unchangeable, and that genuinely
works -- writes, truncates and deletes are all refused, checked on every
boot. There is also a *second* module for the same feature which does
nothing, and whose opening comment describes it as the mechanism. A reader
who finds that one first concludes the protection is missing.

**The working one.** `vfs::FileAttr::IMMUTABLE`, checked at `vfs.rs:5206`
(`is_writable`) and `:5249` (the `W_OK` access path), honoured by FAT as
`ATTR_READ_ONLY` (`fat.rs:3312`, `:4430`), and verified by self-tests on
every boot:

```
[ext4]   immutable: write, truncate and unlink are all refused, and allowed again once cleared: OK
[memfs]  immutable write rejected: OK
[memfs]  immutable remove rejected: OK
```

**The decorative one.** `kernel/src/fs/immutable.rs`: zero `vfs::`
references, so it shares nothing with the mechanism above; its only caller
outside `/proc` and `kshell` is its own self-test dispatch in `main.rs`. Its
doc says it *"Provides `chattr`-style file flags that restrict
modifications"* and that *"Only a privileged user can set/clear the flag"* --
and `set_flags` contains no capability, uid or privilege check of any kind.
So both halves of that sentence are false, while an identically-named,
actually-enforced attribute lives one module away.

This is lane C's shape, not a new one: their
`c-a-two-desktop-icon-models-and-mine-cannot-be-wired-until-we-pick` is the
same problem -- two models for one concept, where the question is which to
keep rather than what to build.

#### The near-miss, which is worth more than the finding

I was one command from recording that **neither** mechanism was enforced.
The reasoning that got me there was not careless, which is what makes it
worth writing down:

1. `grep FileAttr kernel/src/fs/vfs.rs` found the check inside
   `Vfs::is_writable`.
2. `grep Vfs::is_writable` found exactly one caller, `vfs.rs:7304`.
3. That line is inside `pub fn self_test()` -- so the only caller of the
   predicate that checks IMMUTABLE is a test.

Every step is true, and the conclusion -- "the immutable check has no
caller" -- is false, because enforcement does not go through that predicate
at all. `write_file`, `truncate` and `unlink` refuse on their own paths, and
`is_writable` is a separate convenience that happens to share the check.

**What settled it in one command was asking whether the TEST PASSES**, not
where the call site is. `[ext4] immutable: write, truncate and unlink are all
refused ... OK` on every boot is direct evidence of the behaviour; a call-site
search is evidence about one route to it. I had spent the day insisting that
a green verdict needs its corpus examined, and then nearly took a *call-graph
absence* as proof of a behavioural absence -- the same substitution in the
other direction.

It also matters which way the error pointed. Recording a missing protection
that exists tells a reader to build something already built, and worse, tells
them not to rely on something they can rely on. Lane C's rule about
understatement applies to protections as much as to features: an
understatement is believed, so nobody checks.

**Action taken:** none to the code. Which of the two immutable models
survives is a consolidation decision with a caller (`fat.rs`, `ext4`) on one
side and a `/proc` file on the other, and it is not a decision one lane
should take silently. The doc claim in `immutable.rs` is the part that
actively misleads and is the first thing to fix; the duplication itself can
wait for someone who wants the feature.
