### [A] Module docs that link a subsystem the file never calls: 8 of 807 kernel modules, and one real new claim -- 2026-09-17

**Status:** OPEN

Lane C's `TD-C-A-MODULE-DOC-IS-THE-ONE-CLAIM-NOTHING-CHECKS` says a `//!`
feature list is the one claim in the tree with no instrument, because both
existing scanners read strings the program *draws*. True, and I had already
hit it twice from the kernel side: `devpower`'s four absent integrations and
the power family's claims to set the CPU governor and dim the display.

So: is a narrow instrument viable? Measured, over all 807 `kernel/src`
`.rs` files -- module docs that contain a rustdoc link `[`crate::X`]` where
the file never references `X::` at all:

| module | links, never calls |
|---|---|
| `devpower.rs` | `devhotplug`, `power` |
| `syshealth.rs` | `eventlog` |
| `alternatives.rs` | `idt` |
| `hardlockup.rs` | `watchdog` |
| `audio_alsa_ctl.rs` | `audio_alsa` |
| `drm/uapi.rs` | `audio_alsa`, `drm` |
| `drm/ati/backend.rs` | `drm` |
| `net/raw.rs` | `syscall` |

**8 of 807 is tractable**, which is the useful part -- a rule that produced
300 hits would be unusable. And it independently rediscovered `devpower`,
which I had found by hand, so it is not merely plausible.

**The discriminator is the verb, and it still needs a human.** Sampled:

| line | reading |
|---|---|
| `syshealth.rs:18` "**Emits events via** [`crate::eventlog`] when thresholds are crossed" | a claimed act. `eventlog` occurs once in the file: in that sentence. **Real.** |
| `hardlockup.rs:5` "see [`crate::watchdog`]" | a cross-reference. Not a claim. |
| `alternatives.rs:26` "see the ISR stubs in [`crate::idt`]" | a cross-reference. Not a claim. |

That is exactly the distinction lane C's `find-claimed-acts` draws for drawn
text, and it is why this cannot be a gate as it stands: "emits ... via X"
and "see X" are indistinguishable to a link scanner.

**The limitation matters more than the finding.** The probe sees rustdoc
*intra-doc links* only. `devpower`'s worst claim -- "PCI config space Power
Management Capability (PM cap) for hardware control" -- is unlinked prose and
is **invisible** to it, as is `powerprofile`'s "control CPU governor, display
brightness" and `energysaver`'s "app throttling, display dimming". Every one
of those was found by reading, not by scanning. So this narrows lane C's
"no instrument at all" to "no instrument for the unlinked majority, and a
cheap one for the linked minority" -- it does not solve it.

**Fixed:** `syshealth.rs:18` now states the gap instead of the capability.
Kept as a stated gap rather than deleted, per dd-950: the line is the shape
of the missing wiring, and `syshealth` genuinely should emit there.

**All eight triaged, so this list is closed rather than pending** -- dd-951's
warning applies to a list like this too, and an untriaged eight left alone
becomes a description of a tree that moved on.

| module | verdict |
|---|---|
| `devpower.rs` | **real** (2 claims; fixed earlier today) |
| `syshealth.rs` | **real** (1 claim; fixed here) |
| `hardlockup.rs` | reference -- "see [`crate::watchdog`]" |
| `alternatives.rs` | reference -- "see the ISR stubs in [`crate::idt`]" |
| `audio_alsa_ctl.rs` | reference -- "the **pure ABI layer**, mirroring [`crate::audio_alsa`]" |
| `drm/uapi.rs` | reference -- describes what *clients* drive through it |
| `drm/ati/backend.rs` | reference -- "[`crate::drm`]'s backend enum", naming a type's owner |
| `net/raw.rs` | reference -- "syscall handler ([`crate::syscall`]); this module is the mechanism" |

**Precision: 2 of 8 files.** Too low for a gate, fine for a lead generator,
and 8 hits cost about ten minutes to read.

**And the false positives have a shape, which is the refinement worth
keeping.** They describe *inbound* or *parallel* relationships -- `net/raw`
says the syscall layer calls INTO it, `audio_alsa_ctl` says it mirrors a
sibling, `backend.rs` names a type's owner. The true positives describe
*outbound acts*: "emits events via", "coordinates", "notified to save
state". So it is not only the verb -- it is the direction of the call the
sentence implies, and a link scanner cannot see direction at all. Anyone
tempted to gate this should filter on outbound verbs first and expect the
remaining false positives to be sentences about architecture.
