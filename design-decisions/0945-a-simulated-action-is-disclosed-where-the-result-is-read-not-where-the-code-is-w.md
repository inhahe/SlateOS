## 945. A simulated action is disclosed where the result is read, not where the code is written

**Date:** 2026-09-15 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; follows the operator's own precedent in Q21

**In short:** the kernel's firmware command told the operator *"Firmware
update applied for device 1. Reboot required."* No firmware was written.
There is no firmware writer in this kernel and there never has been. The
function's doc comment said `(simulated)` and had said so since the day it
was written -- so the disclosure existed, in the one place the operator
never looks.

### A fabricated action is not a fabricated fact

Three things found earlier the same day were fabricated **facts**: three
block devices, three installed drivers, a website. This is a fabricated
**action**, and it is a different category in a way that matters:

| | fabricated fact | fabricated action |
|---|---|---|
| what it claims | something *is* | something *happened* |
| how it is corrected | delete the value | cannot be un-said; the reader may already have acted |
| worst case here | `/proc` overstates the hardware | the operator reboots to complete a flash that never began |

`PendingReboot` is not a description, it is an **instruction**. That is the
whole of the difference. A wrong fact misinforms; a wrong instruction
recruits the reader into acting on it.

### Lane B's addition: a fabricated action has a sign, and one sign is worse

Their instance, found the same day: `upowerd` printed `CRITICAL: battery at
3% -- executing PowerOff` and contains no power action of any kind. No
syscall, no libcall, no spawned `powerctl`. "Executing" is present tense, so
the one person who ever reads that line -- somebody watching a laptop at 3%
-- was told the machine was powering itself down.

Put beside the firmware case it exposes a distinction neither of us had
alone. **A fabricated action is most dangerous when the action it fabricates
is one that relieves the reader of a duty.**

| | fabricated action | what the reader is told | correct response to the lie | cost |
|---|---|---|---|---|
| firmware | a flash completed | *you must reboot* | reboot | a wasted reboot -- the lie **created** work |
| battery | a shutdown is running | *nothing is required of you* | do nothing | the unsaved work on the disk -- the lie **removed** work |

Same lie, opposite sign. And the removing kind is the one nobody notices in
review, because a line that tells the reader to do nothing produces no
follow-up action whose absence anyone could observe. A false instruction to
*act* gets caught by the act failing; a false assurance that something is
already handled is only caught by the loss it was supposed to prevent.

So when auditing for this shape, the priority order is not by subsystem
stakes. It is: **first the claims that say a protective action is already
under way**, then the claims that demand one, then the claims that merely
describe. Lane B's wording for the whole family is the one to keep -- both
are "instructions wearing the clothes of descriptions".

### What the attention failure looked like

The call site already carried a careful, correct comment about the danger
of applying an update to the *wrong* device -- `fwupdate apply 1O` once
parsed as device 0 -- ending *"firmware is also the one thing here that a
reboot does not undo."* Somebody thought hard about this code, reached the
right conclusion about the stakes, and fixed the argument parsing. **Nobody
asked whether it wrote firmware at all.** The question that was asked was
"which device does this flash", and the answer to "does this flash" was
never in the frame. A sharp question can occupy the space where a blunter
one belonged.

### The decision, and the precedent it follows

Keep the simulation; disclose it where the result is read. This is not a
fresh call -- the operator decided this exact tradeoff on 2026-07-14 as
**Q21**, for lane B's `nft`/`iptables`, which now print *"parsed, NOT
applied -- use `fw`"*. The same answer applies for the same reason: the
parse (here, the bookkeeping) is real work worth keeping, and the harm was
never the simulation but the claim about it.

Lane C's test from triaging ten apps the same day sharpens where the notice
goes: the question is not *is there a disclaimer* but *does it cover what
the value would need in order to be true*. A doc comment cannot, because
the value's reader is not reading the source.

### Rejected: make `apply_update` return `NotSupported`

Tempting, and it is the most honest single line available. Rejected
because it deletes the module's only behaviour and its entire test surface
in exchange for a truth the notice already tells, and because the
bookkeeping is what a real implementation will hang off. §941 is the
governing precedent: a known limitation is **declared and checked**, not
removed or left to fail silently.

### Rejected: rename `UpdateRecord::success`

It means *the record was written*, not *the flash succeeded*, and the name
is genuinely misleading. Left alone and documented instead, because the
field is the history API's contract and renaming it is a change to a
published shape for a problem the doc comment now states at the point of
confusion. Recorded here so the next reader knows it was considered rather
than missed.
