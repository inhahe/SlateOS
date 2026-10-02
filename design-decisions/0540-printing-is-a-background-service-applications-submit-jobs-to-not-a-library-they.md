## 540. Printing is a background service applications submit jobs to, not a library they link

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Operator (Claude recommended B, the shared library; operator chose C) — `open-questions.md` → C-Q4, answered `c`: *"let's do c since we should do it eventually anyway, no point putting it off with a stop-gap solution in its place"*

**In short:** nothing in this OS can print. Two halves of a printing system exist and have never been introduced to each other — the PDF viewer knows how to work out *which pages* to print, and the desktop knows about *printers*, paper sizes, copies and a job queue. The connection between them will be a **background service**: an application hands a print job to a running system service and is then done with it. The job survives the application closing, and it can be cancelled from anywhere. This is more work than either shorter path, and it was chosen on the grounds that it is where printing has to end up anyway, so a stop-gap would be built only to be thrown away.

### What already exists

Both halves are real code with tests, not placeholders. Neither is reachable by any application.

| | In the PDF viewer | In the desktop |
|---|---|---|
| Works out which pages to print | yes — including `1-3, 5, 7-9` | only a single "from page X to page Y" |
| Knows what printers exist | no | yes |
| Copies, paper size, double-sided, quality | no | yes |
| Queue of pending jobs, cancel, pause | no | yes |
| Reachable by any application | **no** | **no** |

The split itself is not the mistake. A page range is something only the document can know, because it is the only thing that knows how long it is; the printer list is something only the system can know. What is missing is the join.

### Why a service rather than a library

The three live options were really one question asked three ways: *what does an application depend on in order to print?*

- Depending on **the desktop** (option A) means every application that prints is built together with the program that draws the screen — a taskbar change could stop the PDF viewer compiling — and printing only works while the desktop is running.
- Depending on **a library** (option B) removes the build tangle but keeps the job inside the application's own process. Close the application and the job goes with it.
- Depending on **a service** (option C) means the application depends on a message format, not on code. The job outlives the sender by construction, because it was never the sender's to begin with.

The third is what every other operating system converged on, and the reason is the property the first two cannot have at any price: **a print job is not part of the application's lifetime.** A user who prints a forty-page document and closes the viewer expects the pages to keep coming. B can be extended to A's capability, but it cannot be extended to that without becoming C.

### The operator's reasoning, and why it overrode the recommendation

The recommendation was B — cheaper, and the last moment at which moving the code was cheap. The operator's answer rejected the framing rather than the estimate: *"we should do it eventually anyway, no point putting it off with a stop-gap solution in its place."*

That is consistent with this project's standing rule that effort is not a cost to be minimised and that a convenient intermediate is never temporary. B's real defect is not that it is worse than C — it is that it is a *way station* on the road to C, and way stations accumulate callers. Four applications written against a printing library are four applications to rewrite when the service arrives. The recommendation had priced the work and not the rewrite.

Recorded because the estimate was not wrong and the conclusion still was: **"cheap now and compatible with the right answer later" is not the same as "on the way to the right answer later."** A library and a service differ in who owns the job, which is exactly the thing callers build assumptions around.

### Rejected

- **A — applications call the desktop's printing code directly.** Printing would work in the PDF viewer within the hour, at the price of coupling every printing application to the whole desktop. Cheap to create, expensive to undo, and the tangle grows with each new caller.
- **B — move printer handling into a shared library.** The recommended option. No build tangle, nothing visible different from A for a user, and roughly half a day's work. Rejected as a way station: it settles the dependency question the wrong way (the job lives in the application's process) and every caller written against it is a caller to migrate.
- **D — leave both halves unwired.** Printing stays impossible and the two models drift further apart, since each is edited for its own reasons. The two already disagree about what a page range is.

### What this obliges

- Printing is not unblocked by a quick change; it is a real piece of work with a message format, a service, and a queue that outlives its clients. Nothing else is currently blocked on it.
- The PDF viewer's page-range parser is the good half and should be lifted into the job format, not reimplemented — it already handles `1-3, 5, 7-9`, which the desktop's from/to pair cannot express.
- Until the service exists, neither half gains new callers. Wiring one application directly is exactly the stop-gap this decision refused.
