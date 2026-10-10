## 1370. HEIC pictures and HEVC video come as a separate install, never in the base system

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q1: B", `operator-answers/2026-10-09-open-questions-answers.txt`), answering `open-questions/F-Q1.md` — Claude's recommendation.

**In short:** an iPhone saves its photographs as HEIC, and opening one means
decoding HEVC, a video format whose patent owners charge for licences. SlateOS
will not carry that code in a fresh install. The first time a HEIC file is
opened, it offers to install "HEIC support" with one click; whoever installs
it takes on the patent question, as with Windows' paid extension. The base
system stays free of HEVC code.

**Decision.** HEVC decoding -- for HEIC photographs, and for HEVC video, which
F-Q8 tied to this answer ("one HEVC decoder would serve both") -- is built as a
separate, replaceable component that a fresh install does not contain: a helper
program (or shared library) that the user can install, and replace with their
own build, since its likely source, libde265, is LGPL 3. A picture or video
that needs it, while it is absent, says so and offers the install.

**Rationale.** The engineering is the same whether or not a fresh install
carries the decoder: the LGPL's condition (a user may swap in their own build)
is met by keeping the decoder out of every program that shows a picture, which
the helper design does anyway -- and a crafted picture attacking the decoder
then attacks a helper that can do nothing else. What differs is only the
patents, a legal and policy question: an optional install keeps the base
system clear of the one kind of code that carries a fee.

**Alternatives.** A, in the base system: iPhone photos open out of the box, at
the risk of royalty claims if SlateOS is ever distributed widely. C, not yet:
HEIC keeps saying it cannot be displayed.

**What it asks, and where it will be done.** `roadmap.md` (lane F): the HEIC
item under AVIF, and the "H.264 and HEVC video" item's HEVC half, both now
"an optional install". The HEIF container reader built for AVIF (§1333) serves
HEIC as is. Nothing is built yet: a fresh install's behaviour (HEIC refused by
name) is already this decision's for as long as the component does not exist.

**How to reverse.** Ship the component in the base image instead of offering
it; nothing else changes.
