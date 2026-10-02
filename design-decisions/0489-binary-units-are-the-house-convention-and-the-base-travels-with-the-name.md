## §489 — Binary units are the house convention, and the base travels with the name

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** SlateOS shows file and disk sizes to people, and until now every
program worked out for itself how to turn a byte count into text like "1.5 MB".
Forty-seven of them did, and twenty-nine got it wrong in the same way: they
divided by 1024 — which gives you *mebibytes* — and then printed "MB", which
means something 5% smaller. So the file manager told you a file was 1.00 GB
when it was really 1.07 GB. The decision here is (a) that Slate says "MiB" for
the 1024-based numbers it actually computes, and (b) that no program works this
out for itself any more: there is now one function, and it is impossible to ask
it for 1024-based arithmetic with 1000-based labels.

**Glossary, since the whole decision turns on two near-identical words:**
*KB/MB/GB* are the SI (metric) units — a kilobyte is exactly 1000 bytes, the
same "kilo" as in kilometre. *KiB/MiB/GiB* ("kibibyte", "mebibyte") are the
binary units — 1024 bytes — which exist because computers allocate memory in
powers of two. They differ by 2.4% at KB and by 10% by TB, so at disk sizes the
choice is visible to a user reading a number.

### What was decided

1. **Binary (IEC) units are the default** for file sizes, memory, and anything
   the kernel counts. `design.txt` already writes `KiB`/`MiB` throughout, and 14
   of the 18 self-consistent implementations already did. Critically, all 29
   broken ones were *already computing* base-1024 values — so correcting the
   label changes no arithmetic anywhere, only the text.
2. **Decimal (SI) units where the domain is genuinely decimal:** network
   transfer and throughput, and disk capacity as a vendor sells it (a "4 TB"
   drive is 4×10¹² bytes).

   The dividing line, stated so it can be applied without re-deriving it:
   **bytes *moved over a link* are SI; bytes *occupying storage* are IEC.** A
   file is IEC wherever it appears — in the explorer, in an archive listing, in
   a remote-desktop transfer panel — because it is the same file in all of
   them. An interface's `rx_bytes`/`tx_bytes` counter is SI wherever it
   appears, for the same reason: the tray indicator, the network settings page,
   `netmanager` and `vpnmanager` all report that one counter, and two windows
   quoting one number must not disagree about what it says. Nine call sites are
   SI under this rule — six in `gui/desktop` (the tray indicator's size and rate
   halves, the network settings page, the storage and update settings pages, and
   the resource monitor's network graph), plus `netmanager`, `vpnmanager`, and
   the link half of `remotedesktop` (whose file-transfer half stays IEC, which
   is why that file now has two named formatters instead of one).
3. **The base and the unit names are one table**, selected together by a
   `Unit` enum in `textfmt::bytes`. A caller picks a *family*, never a divisor,
   so the defect that motivated this cannot be written again.

### Alternatives considered

**Decimal everywhere.** Argument for: it is what disk vendors, network gear and
macOS all use, so users' numbers would match the label on the box. Argument
against: it would have changed every displayed number in the tree rather than
every displayed *label*, contradicted `design.txt`'s own prose, and made memory
and page-cache figures — which really are powers of two — read as untidy
decimals. Rejected because it is the larger and less honest change: the numbers
the code computes are binary.

**Let each call site keep its own convention, just fix the labels.** This is the
minimal change and was tempting: 29 label corrections and nothing else. Rejected
because it leaves the *cause* in place. The tree's own evidence is that the
convention drifts when it is restated 47 times — `gui/desktop` was displaying
one set of network counters in two units on one screen — and the next
`format_size` someone writes would have had to guess again.

**A single function with a `decimals` parameter.** Rejected: the existing copies
used 0, 1 and 2 decimal places with no pattern, and a parameter would have
preserved that arbitrariness under a shared name. One decimal place, everywhere.

### Why this is a Claude decision and not an operator question

The part with a real tradeoff — binary vs decimal — is settled by the project's
own spec, which writes `KiB` throughout, and by the fact that one option is a
no-op on the arithmetic. The part that is not a tradeoff at all is that a
program must not divide by one number and label with another. If the operator
prefers decimal as the user-facing default, it is now a one-line change per call
site rather than a rewrite of 47 functions — which is itself part of the reason
to centralise first and ask second.

**Consequence to expect:** file sizes across the desktop now read `MiB`/`GiB`
where they read `MB`/`GB` before, and the numbers are unchanged. Sizes near the
top of a unit no longer read `1024.0 KiB` (see `textfmt::bytes`, "the rounding
lie"), and nothing saturates at `GB` any more — a 4 TiB disk used to be
described as `4096.00 GB`.
