### [C] TD-C-A-PROGRAM-ASKING-FOR-A-CAPABILITY-REACHES-NO-ONE -- 2026-09-30

**Status:** OPEN -- waiting on lanes A, B and F
(`requests/c-abf-a-program-asking-for-a-capability-reaches-no-one.md`).

**In short:** a program that needs more access than it was started with is
meant to ask the user -- saying why -- and the user to answer Allow or Deny in
a security dialog. The desktop's dialog is written and tested
(`gui/desktop/src/security_dialog.rs`) and nothing shows it, because nothing
outside the kernel can answer such a question: every one is refused the
moment it is asked. The user is never asked, and a program that asks is
simply told no.

**Why lane C cannot connect it alone:** the kernel's broker
(`kernel/src/cap/request.rs`) has no syscall for the answering side --
registering as the handler, listing, deciding -- and approving a request
grants nothing (it marks the request approved and inserts no capability).
A request also names a type of resource and no object, which the grant would
need. The desktop's event loop, besides, cannot wait on anything but its
compositor connection and its own deadlines, so it could not hear a question
arrive. All of it is asked in
`requests/c-abf-a-program-asking-for-a-capability-reaches-no-one.md` (lanes
A, B and F); `roadmap.md` §1.5 had the dialog's integration marked done and
says `[-]` now.

**Lane C's half, when those land:** register at session start, show each
request on a surface of its own above every window, turn Allow and Deny into
the decide call, take down the prompt for a request that ended unanswered --
tested against a fake broker and through `TestDesktop`.
