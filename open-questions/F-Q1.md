## F-Q1 — [F] iPhone photos (HEIC) will not open. May SlateOS include a decoder for a patented video format? — Status: OPEN (raised 2026-09-25, narrowed 2026-09-27)

**In short:** an iPhone saves every photograph as HEIC, and SlateOS cannot
open one. Opening one means decoding HEVC, a video format covered by patents
that their owners license for a fee. That fee is why Windows sells HEIC support
separately for $0.99 and why Fedora Linux leaves it out. The question is
whether SlateOS should include such a decoder, offer it as a separate install,
or neither. (The other half of this question, AVIF, you answered "yes":
design-decisions.md §1333.)

**Your question: what is hard about "letting users replace the library"?**
You are right that it is nearly trivial. The usable open decoder, libde265, is
under the LGPL (a licence that lets anyone ship it, on one condition). The
condition is that a user must be able to swap in their own build of that one
library. The two things that could get in the way both have easy answers:

- **Rust bakes libraries into each program.** Linked that way, the decoder
  would sit inside every program that shows a picture, and we would owe users
  a way to rebuild all of them. So it goes in a separate library file or
  helper program instead. That is easy, and the helper program is the safer
  design anyway: a crafted picture that attacks the decoder then attacks a
  helper that can do nothing else.
- **This version of the LGPL (3) also forbids locking the user out.** If
  SlateOS ever insists that only software it signed may run, a user's own
  build of the decoder must still be allowed to run. As long as users can
  always add their own signing key, this costs nothing.

So the licence is not the obstacle. The **patents** are, and they are a legal
and policy question rather than an engineering one.

**The options** (the engineering is the same for all three up to the last
step: the decoder is built as a separate, replaceable helper, and the only
difference is whether a fresh install includes it):

| Option | *What changes:* |
|---|---|
| **A.** Include it | iPhone photos open out of the box. If SlateOS is ever sold or distributed widely, the HEVC patent pools may ask for royalties. |
| **B.** A separate install, one click away | The first time a HEIC file is opened, SlateOS offers to install "HEIC support". The base system contains no HEVC code; whoever installs it takes on the patent question, as with Windows' paid extension. |
| **C.** Not yet | iPhone photos keep saying they cannot be displayed. |

**If never answered:** safe. HEIC files show an error saying they cannot be
displayed, as now; nothing else is affected.

**Claude's recommendation:** **B**. Almost A's convenience, with the base
system free of the one kind of code that carries a fee. If SlateOS will only
ever be used privately, A is just as good, and simpler.

**Where it bites:** `gui/imagecodec` (the format dispatch; the HEIF container
reader §1333 builds for AVIF serves HEIC too), a helper program for the
decoder, and the image viewer's "cannot display" message.
