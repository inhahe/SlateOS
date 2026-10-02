## 542. The installer refuses an ambiguous size suffix rather than guessing which one the author meant

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Operator (Claude recommended B weakly, and named C the honest option; operator chose C) — `open-questions.md` → Q55, answered `c`

**In short:** an unattended-install config file describes each disk partition with a size like `"100 GB"` or `"32 GiB"`. The installer treated both spellings as the same number — the binary one — so a config asking for `500 GB` on a 500 GB drive actually asked for 537 GB and the install failed to fit. It now **refuses `GB` outright**, with an error naming both alternatives, and accepts only the unambiguous `GiB` and bare `G`. A config using `GB` stops installing until someone edits it. Nothing is ever silently resized.

**Glossary:** `GB` (gigabyte) is decimal — exactly 1 000 000 000 bytes, and what a disk's box says. `GiB` (gibibyte) is binary — 1 073 741 824 bytes, about 7 % more. They diverge further at `TB`/`TiB` (10 %).

### Why this one was asked when its mirror image was not

§489 fixed the display side of the same confusion: code that divided by 1024 and *printed* `GB`. That was fixed without asking anyone, because printing a number under a label that means something else is simply false — there is no convention under which it is right.

The input side is not like that. A suffix in a config file is an **input convention**, and the surrounding ecosystem is genuinely split: `fdisk` and `parted` treat everything as binary, and anyone who has typed `+512M` at a disk prompt expects exactly that. So the tree held two defensible conventions and had silently picked one, which is the shape of question that goes to the operator rather than getting resolved in-lane.

### Why refusing beats picking

The two "pick one" answers share a defect that only became visible once they were written down next to each other: **whichever is chosen, some existing config file means something different than its author intended, and nothing announces it.** Under A the author who wrote `100 GB` from the drive's label quietly gets 7 % more than they asked for. Under B that same author quietly gets 7 % less than they got yesterday. Both failures are silent, and both produce a disk layout nobody chose.

C is the only option where the ambiguity is surfaced to the one party who can actually resolve it — the person who wrote the file and knows which number they meant. The cost is real and was accepted knowingly: **existing config files break loudly instead of being left wrong quietly.** For a tool that partitions disks, that trade is not close. A partition table is not a place to be helpful about a guess.

This generalises past the installer: when an input has two established meanings and the tree cannot tell which was intended, refusing is a better default than defaulting. A rejection costs an edit; a wrong guess costs a disk layout, and gives no sign it happened.

### Rejected

- **A — leave it, every suffix is binary.** The status quo, self-consistent, and what most partitioning tools do. Rejected because self-consistency is not the property at issue: the author who copied "500 GB" off the drive's label still gets a partition that does not fit, and the error points at the partition table rather than at the units.
- **B — honour the spelling: `GB` = 10⁹, `GiB` = 2³⁰, bare `G` = 2³⁰.** The (weak) recommendation, on the sole ground that it would agree with what §489 made the display side do. Rejected because it silently changes what every existing config file does — the same silent-wrong-layout failure as A, merely in the other direction, and newly introduced rather than long-standing.

### What this obliges

- `apps/installer/src/lib.rs` — the `multiplier` match in the partition-size parser, where `"K"|"KB"|"KIB"` all currently map to 1024, up through `TB`.
- The error message must name **both** replacements (`GiB` for what you have now, `GB` was never honoured as decimal), because an author who is refused needs to know which one preserves their existing layout. An error that only says "ambiguous" makes the reader guess again, one level up.
- Documentation and any sample config in the tree must stop using `GB`, or the installer will reject its own examples.
