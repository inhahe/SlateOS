## 937. A check can be blind to a population or blind to a property, and they read the same

**Date:** 2026-09-14 · **Decided by:** Claude (autonomous) · **Lane:** A

Named jointly with lane C, who supplied the first half. Recorded because 932
says a fact that matters needs two witnesses, and this is *how* a witness turns
out not to be one.

**In short:** when a check passes and should not have, it failed in one of two
ways, and the distinction tells you where to look. Either it never examined the
thing (**population**), or it examined it and asked the wrong question about it
(**property**). Both print the same word.

**Population blindness** -- the check's scope excludes the defect.

| instance | what it could not see |
|---|---|
| `check-ran-if` resolving a callee by bare name | 719 `fn self_test`; any of them could satisfy the marker |
| `scripts/check-cfg-unix.py` vs the boot's `check_cfg_unix()` | 60 crates versus 415; the launcher was in neither lane's view |
| `check-text-mode-writes` absent from the pre-push hook | every violation until someone spends a boot |
| lane C: a gate enumerating crates by how they spell a call | crates that spell it otherwise |
| lane C: eight theme guards rendering only an opening frame | every frame after the first |

**A false-positive guard can create a false negative that lasts exactly as long
as the defect.** The nastiest variant found so far, because the mechanism is a
deliberate, well-reasoned piece of conservatism.

`scan-orphan-modules.py` drops from its evidence any name shared with another
module -- sound, because a name appearing twice cannot be attributed to one
definition, and the alternative is a stream of false positives. But
`gui/desktop/src/privacy_settings.rs` -- 2,014 lines, `is_allowed`,
`revoke_all`, no callers -- was invisible to it for as long as `apps/settings`
carried its own `PermissionKind` and `AppPermission`. Deleting that duplicate
**un-poisoned the evidence for the original**, and the module surfaced the same
day.

So the scanner could not see the island for precisely as long as a second copy
of two type names existed anywhere in the lane -- and a second copy of a type is
itself a symptom of the duplication the tool exists to find. The guard was
strongest exactly where the problem was worst.

**Why this resists the usual remedies.** Widening the scope does not help: the
scope was right. Strengthening the assertion does not help: the assertion was
right. The evidence was discarded upstream of both, by a rule whose purpose is
to make the tool trustworthy. And it fails **silently and durably** -- not a
wrong answer once, but a blind spot that persists while the condition holds,
and lifts on its own when someone happens to remove the duplicate for unrelated
reasons.

**What it suggests, without a general fix:** a de-duplication or
ambiguity-suppression rule deserves a count of what it suppressed. "Dropped N
names as ambiguous" beside a clean verdict is the difference between "nothing is
orphaned" and "nothing is orphaned among the things I could still attribute".
`scan-orphan-modules.py` already prints a `shares N name(s)` line for modules it
does report, which is the same information for the cases that survived -- the
gap is that suppression is invisible when it removes a module from the report
entirely.

**The sharpest population failure: searching by a key the defect is defined by
lacking.** Not a wrong scope -- a scope that excludes the defective cases *by
construction*, so the search cannot fail and the answer is always clean.

Lane C, 2026-09-14, twice on one small task. They verified that every lane-C
answer in the operator's file had been recorded in `design-decisions.md`, and
reported all accounted for. The check searched by **`C-Q` number**. The two
entries that had not been recorded were exactly the two that **have no number** --
which was the defect being looked for. A true statement about a population that
excluded the cases in question.

Then verifying the same thing by hand, a grep for the question's own prose in
`design-decisions.md` returned **zero**, because a decision's title is not the
question's sentence. Two false negatives in a row, in opposite directions, on
the same small task.

**Why it is worse than an ordinary scope error.** A narrow scope can be widened
once noticed, and a clean result from it still feels provisional. Here the key
*is* the thing that is broken, so widening is not the remedy -- the search must
be run over a population selected by something the defect cannot be defined by.
"Every numbered question is recorded" and "every question is recorded" are
different claims, and only the second was wanted.

**Recognising it:** ask what a defective case would have to have in order to
appear in the result set. If the answer is "the attribute whose absence makes it
defective", the search is incapable of finding anything, and a clean result from
it carries no information at all.

**Property blindness** -- the scope is right and the assertion is weaker than
the claim. The FAT short-name witness is the clean example. Asserting that two
undecodable names *differ* is true of a rendering that dropped the
distinguishing byte entirely; the assertion has to be that `\351` and `\357`
actually appear. One level down from a population miss, and harder to see,
because the check really is looking at the right code.

**Why the pairing is worth a section.** The remedies differ. A population miss
is fixed by widening scope, and the fix is mechanical once named --
`--all-targets`, resolve through the module path, scan every `.rs`. A property
miss is not: widening scope does nothing, and the only route is to ask what a
*wrong* implementation would still satisfy. Reaching for the first remedy
against the second is how a check gets rewritten, re-run, and still believed.

**A third mode, found by lane C on 2026-09-14: substitution.** The check examined
a real thing, asked a fair question, and answered it correctly -- about something
else.

    cargo test -p sysinfo     ->  26 passed

The package in `apps/sysinfo` is named `sysinfo-app`. `sysinfo` is a *different*
real package, `userspace/sysinfo`. So the command did not fail; it tested another
crate and printed a true success. The new test in `apps/sysinfo` had never been
compiled. Verified afterwards with `cargo pkgid`: **three** of the nine
directories whose package name differs from the directory resolve to a different
crate this way -- `backup`, `indexer`, `sysinfo` -- while the other six error,
which is the honest failure.

**Why this is worse than the first two and worth its own name.** Population and
property blindness both produce an *absence*: a crate not compiled, a frame not
rendered, an assertion too weak. An absence is at least the kind of thing a
careful reader can go looking for. Substitution produces a *presence* -- a green
line about real tests that really ran. There is nothing missing to notice.

The tell existed and was read past: 26 tests in a crate the author had been
reading all afternoon, which has 63. The number was visible; the word "ok" was
what got read.

**What it adds to the diagnostic.** "Would this go red" is not sufficient here,
because the command *would* go red -- for the wrong crate. The question that
catches substitution is narrower: **did the check examine the thing I named?**
For anything `-p`-scoped, `cargo pkgid -p <name>` answers it in one command, and
a gate assembled from `-p` flags carries this failure permanently in a way a
`--workspace` gate cannot.

**The diagnostic that separates them:** describe an implementation that is
clearly broken and check whether the test still passes. If yes and the code is
in scope, it is a property miss. If the broken code is never compiled, read, or
enumerated, it is a population miss. Asking *would this go red* rather than
*did this go green* is the whole of it.


**The remedy for all three, and it is cheaper than any of them: a positive
control.** Before a clean report means anything, show the check can produce a
dirty one. Feed it something you know is broken and watch it fire.

This was earned the hard way on 2026-09-14, twice in five minutes. Verifying
whether a rule applied to an entry, the extraction regex silently failed to
match a multi-line `re.compile(`, leaving the pattern `None`; the comprehension
that used it short-circuited to empty, and the script printed its conclusion --
the *favourable* one -- unconditionally. A check that could not fail. On the
retry, the probe set was wrong in the opposite direction: every probe was
legitimately negative, because the real pattern demands an uppercase status
word. Only an assertion that the instrument must fire on a known positive
caught it. It read:

    assert any(pat.match(p) for p in probes), "instrument never matches anything"

The point is not the assert. It is that **a negative result from an unexercised
instrument carries no information at all**, and is indistinguishable from a
negative result from a working one. Population blindness needs the scope
widened and property blindness needs the assertion strengthened, but both of
those presuppose the check runs. A dead instrument fails before either question
is meaningful, and it is the only one of the four that costs nothing to rule
out.

**A fifth, found by lane C on 2026-09-14: the test performs the missing step
itself.** Distinct from the four above, and the hardest to see, because scope,
subject and assertion are all correct.

`DesktopShell` wrote `shortcuts.yaml` on every rebind and never read it back, so
a rebound key was lost at the next start. There was a test asserting exactly the
property that was broken -- *the rebind must survive a restart* -- and it passed
throughout. It built a fresh shell and called `fresh.load_shortcuts()` **by
hand**. It simulated the restart by opening the door itself, then asserted the
room was lit.

Nothing about it is dishonest. It examines the right object, asks the right
question, and its assertion is strong. What it does wrong is in its *setup*: it
supplies the step production omits, so it measures the code's behaviour in a
world the code never runs in.

**Why "would this go red" does not catch it.** Break `load_shortcuts` and the
test does go red -- it is a real test of a real function. The defect is not in
the function under test but in the **caller that does not exist**, and no
assertion about `load_shortcuts` can see an absent call in `ShellSession::start`.

**The diagnostic:** ask what the test does *before* the assertion, and whether
production does those things too. Every line of setup that production does not
perform is a premise the test has quietly granted itself. For a persistence
pair, the test must reach the state the way a user does -- restart the session --
rather than by invoking the load directly.

**Cheap mechanical proxy, since the above needs judgement:** an asymmetric pair,
where `save_x` has production callers and `load_x` has callers only in tests.
Lane C's `scripts/check-tested-but-uncalled.py` reports exactly that shape. Its
first version reported **915** hits by asking the broader question "written and
never read back", almost all of them library APIs with no consumer yet; narrowed
to the asymmetric-pair shape it reports two, one of them a live data-loss bug.
A gate answering 915 times is one nobody reads, which is its own way of being a
check that cannot fire.

**A sixth failure, and the only one where the check was right: the alarm was
read through the assumption that caused the damage.** Distinct from the five
above, all of which are ways a check fails to see. Here it saw perfectly.

On 2026-09-14 lane C created `scripts/rustscan.py` with `cat >`, destroying an
existing 418-line module imported by seven scripts. `check-gate-call-sites.py`
refused the push within minutes, saying `rustscan.py` is invoked with
`--self-test`, "which appears nowhere in the script". That is an exact
description of a file somebody has replaced. It was read as "my new library
needs a self-test", because the reader assumed the file it named was theirs --
which is precisely the belief that caused the overwrite.

Three tells were present and all were missed: `git status` said `M`, not `??`;
the gate named the file; and the commit's own diffstat read
`582 ++++++++-----------------`. **A file you just created cannot have
deletions in it.** Any `-` in that bar proves the name was taken.

**The remedy is not a better check.** It is a message that challenges the
reading rather than describing the state. "`rustscan.py` is invoked with
`--self-test`, which appears nowhere in it" is precise and passive; the same
message ending "...and this file changed in your working tree 4 minutes ago" is
unmissable, because the added clause contradicts the assumption instead of
sitting quietly beside it. Where a gate can cheaply name *why now*, it should.

**A verification rule falls out of the repair**, and it generalises past this
incident: when checking that a thing is the original rather than a substitute,
**at least one check must be a property a substitute would not bother to
forge.** Confirming the restored file, a passing `--self-test`, seven present
importers and a green call-site gate would all have held for a plausible
replacement that happened to carry a self-test. Line count and git date would
not. Three checks of the first kind are worth less than one of the second.

**A seventh: the test's fixture is the defect under test.** Worse than an
incomplete scope, because repairing the defect keeps the test green.

Lane C, 2026-09-14. `apps/settings` invented its data -- five pages of plausible
records built in `SettingsState::new`: three accounts with `example.com`
addresses and login counts, an adapter connected at `192.168.1.100` with a green
link dot on a system that cannot enumerate interfaces, an update history with
Windows-style KB numbers including a *failed* GPU driver. Deleting the invented
accounts turned **six tests red at once** -- every one had been using those three
production records as its fixture without saying so. They read as
self-contained and were not.

**The part that would not have been predicted:** it surfaced only because the
list became **empty**. Had the three invented accounts been replaced with three
real ones, all six tests would have gone on passing against whatever the machine
happened to have, and the migration would have been called clean. So the defect
is invisible to the test file, invisible to a passing run, and invisible to a
*correct fix* -- it shows up only when the production data goes away entirely.

Lane A's smaller version the same day: the immutability test asserted that a
write was refused, and `truncate` and `remove` were unenforced. One of three
enforcement points, and it read exactly like coverage. The shared tell is that
**neither is visible in the test file** -- in both cases you have to change the
production side to find out what the test was really standing on.

**A test suite can be the strongest argument for a decision that has been
reversed.** Lane C's formulation, 2026-09-14, and it sharpens the mode above:
*tests pin the format the consumer currently parses, which is only the
specification if the format is not the thing under decision.*

They filed a request asking lane A to build `/sys/hardware/*`, whose central
recommendation was **"take the tests as the specification, not my field list"** --
normally excellent advice. Those 33 tests pin `parse_kv_file`, and `key=value`
is exactly the layout `§850` had decided against the previous day, in a decision
lane C themselves made. A producer written against them would have satisfied its
consumer perfectly, contradicted a decision already on `main`, and passed every
test.

**Why it is the hardest version of a stale premise to resist.** A stale comment
is only prose. A green test suite is *evidence of intent*, it is executable, and
pointing at it feels like rigour rather than inertia. Handing it to whoever
implements the other side is handing them a reason not to read the decision --
and the more thorough the suite, the more persuasive the wrong format becomes.
Thirty-three tests are harder to argue with than one sentence in
`design-decisions.md`, and they are wrong.

**The question that separates the cases:** is the format itself under decision?
If not, the tests are the specification and re-deriving it from prose is waste.
If it is, the tests describe the losing side, and their greenness is not
evidence about anything except that the old shape was implemented carefully.

**And the mirror image, which lane C caught twenty minutes later:** removing the
network adapters killed three tests genuinely about adapters and **six more**
that clicked an adapter row only as a *vehicle* for testing the shipped `run`
loop -- when a frame is drawn, which events are ours, when the loop stops.
Deleting those along with the feature would have silently dropped event-loop
coverage unrelated to networking. They were re-pointed at another row. So a test
that breaks when you delete a feature is not necessarily a test *of* that
feature, and the question to ask before deleting it is what it would still be
asserting if the vehicle changed.

**Corollary on verification commands, from the same day.** `cargo build` passed
clean on a settings page lane C had gutted, because the dead fields were `pub` on
a `pub` struct and some were read by tests; `cargo test` caught the leftover.
Lane A shipped a deny-level clippy lint after `cargo check` and `cargo fmt` both
came back clean, and paid a 37-minute boot to find out. Three commands answered
three different questions and both lanes needed all three. A clean result is
evidence only about the question the command asks.

**Where the four sit relative to each other.** A test that performs the missing step itself grants itself a premise;
a dead instrument cannot fire at
all; substitution fires correctly about the wrong subject; population blindness
fires about the right subject with the wrong scope; property blindness has the
scope right and asks too little. Test them in that order -- cheapest and most
total first.

**An eighth, and lane C named it on 2026-09-15: the defect is correct on every
instance that exists.** Not a check that cannot see the defect -- a check that
*cannot be written yet*, because the population which would fail it has not
been built or bought. Lane C's wording, on why this kernel does not adopt
Linux's `size` for a disk's capacity: *a wrong unit that agrees with the truth
on all available hardware is untestable by construction -- the test that would
catch it cannot be written until the hardware exists, and by then something
depends on the wrong answer.*

Linux's `size` is in 512-byte units whatever the device's real sector size is,
so `size * sector_size` overstates capacity eightfold on a 4096-byte device --
and every disk either lane can currently test on reports 512, which makes that
product *right on the entire observable population*. A test asserting the
product equals capacity would pass, honestly, for as long as the hardware is
uniform.

**How it differs from the first mode.** Population blindness is a check looking
at the wrong subset of a population that exists. This is the population itself
being unrepresentative *in time*: no subset of today's hardware exhibits the
failure, so no sampling strategy helps and no amount of coverage closes it.
That rules out the usual remedy. What is left is to refuse the ambiguity at the
point of naming -- publish `sector_count` and `sector_size` and let the product
be unconditionally true -- and then assert the *absence* of the tempting name,
which is a thing a test CAN do today. 939 does both.

**The tell, since there is no failing test to alert anyone.** Someone argues
for a convention on the grounds that it works on everything we have. That is
the same sentence as "we cannot currently distinguish this from correct", said
approvingly. Neither lane found this by testing; both found it by reading a
unit definition and noticing it was historical rather than described.

**Lane B generalised this into §1022, *the two faces of a gate that cannot go
red*, and the pair should be read together.** Face one is a report that
overclaims what it computed (`check-help-vs-parser` printed "advertised but
never *read*" over a count of options never *parsed*, so it could not fail in
the way its label promised). Face two is a fixture that cannot reach the code
(`boot-test.sh` attaches no FAT disk, so an `openat2` arm sat unreachable for
six weeks while its test passed honestly on `memfs`). The mode above is what
face two degenerates to when the unreachable population is *hardware* instead
of a fixture. Face one has a false sentence in it and one careful reader fixes
it forever; face two has none, which is why the remedy is an inventory rather
than a correction.

**Said carefully, because the first draft of this got it wrong and lane B
published the error before I caught it.** "No false sentence" does not mean
nobody noticed. `check-gated-selftests.py` had recorded the FAT fixture gap,
correctly, with a termination condition -- *this entry ends the day the harness
attaches a FAT-formatted vda*. The gap was known at the level of "a FAT suite
is skipped"; what nobody drew from it was "therefore every FAT-only arm in the
VFS is unexercised", which is a different and much larger statement about code
that has no banner of its own and so appears in no never-ran report. Face two
is not an absence of vigilance. It is a true, recorded fact whose consequences
reach further than the question it was recorded under.
