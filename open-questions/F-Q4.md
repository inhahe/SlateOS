## F-Q4 — [F] AVIF pictures open about twice as slowly as in a browser. Use the browsers' hand-written assembly, or write the fast parts in Rust? — Status: OPEN (raised 2026-09-27)

**In short:** AVIF pictures decode correctly but take roughly twice as long
as in Chrome or Firefox: a full-HD photograph takes about 0.2 seconds here
against 0.09 there. The difference is that the browsers' AV1 decoder (the
part that unpacks the picture) runs about 160,000 lines of hand-written
x86 assembly (instructions for the processor, written directly rather than
compiled), and ours was brought in without it. We can bring that assembly
in -- the browsers' speed at once, but code that Rust's safety checks cannot
look at -- or rewrite the fastest parts ourselves in Rust.

**Measured** (`imagecodec`'s `bench_avif_decode`, one thread, best of five,
on a machine busy with other builds, so treat the figures as rough):

| Picture | Here (Rust only) | Pillow (the browsers' decoder, with assembly) |
|---|---|---|
| 640x480 | 30 ms | 22 ms |
| 1920x1080 | 199 ms | 88 ms |
| 2560x1440 | 354 ms | 161 ms |
| 1920x1080, 10-bit, full colour | 353 ms | 202 ms |

**The options.**

| Option | *What changes:* |
|---|---|
| **A.** Bring in the assembly | Pictures open as fast as in a browser, straight away. SlateOS gains ~160k lines of x86 assembly (rav1d 1.1.0's own count) that the Rust compiler cannot check -- the same code Chrome, Firefox and Android run on every AV1 image and video, and among the most tested there is. Needs the NASM assembler on the build machine (a small install). |
| **B.** Write the fast parts in Rust | Each of the handful of hottest routines is rewritten with the processor's vector instructions from Rust, checked sample-for-sample against the plain version. Stays in checkable Rust, but it is a long piece of work, and the speed arrives routine by routine. |
| **C.** Leave it | Pictures stay about twice as slow as in a browser: fine for a still picture, noticeable for large ones and for AVIF animations. |

**If never answered:** safe, and nothing is blocked: pictures open, just
more slowly (option C). It does not get worse over time.

**Claude's recommendation:** **A.** The assembly is the most exercised code
in its field, it produces exactly the same pixels as the Rust (dav1d's own
tests hold the two to each other), and B would spend a long time reaching
what A gives at once. B stays possible later for any routine that proves to
matter, one at a time. The one real cost is the unchecked code; if keeping
every decoder in checked Rust matters more to you than speed, B.

**Where it bites:** `gui/video/rav1d` (its `asm` feature and the build step
that assembles it, left out when it was vendored -- `VENDORED.md`),
`known-issues.md` "[F] AVIF decoding has no committed benchmark", and every
AVIF picture or animation on the system.
