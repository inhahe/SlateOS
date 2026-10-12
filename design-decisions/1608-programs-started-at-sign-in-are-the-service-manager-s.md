## 1608. The programs started at sign-in are the service manager's, per user, ordered as services are

**Date:** 2026-10-12 &middot; **Decided by:** Operator (by the operator's own conditional rule: C if startup programs can depend on other startup programs or on services, otherwise A; they can, so C. Claude had recommended A.) &middot; **Lane:** C, with A and B

**In short:** nothing starts a program when someone signs in, and three
half-built "startup apps" screens record lists nothing obeys
(`open-questions.md` C-Q33). The operator asked whether startup programs, like
services, can need other programs or services started first -- and if so, to
let them share the service manager's ordering (option C); otherwise a list of
the desktop's own (A). They can: a tray applet needs the tray, many programs
need the input-method daemon, a sync client needs the network and the secret
store. Linux moved autostart into systemd's per-user manager for exactly
this, and macOS's launchd runs a user's agents with the same machinery as the
system's daemons. So the answer is C.

**The answer, verbatim** (`operator-answers/2026-10-09-open-questions-answers.txt`):

> C-Q33: I think Claude told me that the reason to have a service manager is
> that some processes depend on other processes and they must all be loaded in
> the right order? Could that be the case with some startup programs too, that
> they need other startup programs and/or some services to be loaded? If so,
> then maybe option C so that it shares the same logic with the service
> management? Atherwise option A.

Lane A answered the question in it the same day (the dependencies above) and
said lane C would record C unless the operator objected; no objection came.

**What it obliges:**

| Part | What | Whose |
|---|---|---|
| A service manager per signed-in user | started when someone signs in, ending with the session, starting the user's programs in the order their dependencies give -- the system manager's logic, not a copy of it | lanes A and B (the service manager is lane B's) |
| The desktop | says "signed in" and "signed out" to it, and is itself one of the session's units | lane C |
| The list | the user's startup programs as units of that manager; an autostart file a ported program installs becomes one, off until the user turns it on (C-Q33's capability argument still holds) | lane B's format; Settings' editor is lane E's |
| The three old screens | the desktop's own was deleted 2026-10-06; lane E's `apps/startupmanager` and the Settings page become the list's editor | lane E |
