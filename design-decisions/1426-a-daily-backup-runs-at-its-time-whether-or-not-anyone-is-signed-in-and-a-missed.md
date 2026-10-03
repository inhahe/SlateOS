## 1426. A daily backup runs at its time whether or not anyone is signed in, and a missed one runs as soon as the machine is on again

**Date:** 2026-09-27 &middot; **Decided by:** Operator (Claude recommended A, with a prompt for missed backups; the operator chose A without the prompt) &middot; **Lane:** C (the question), with D (the service), B (starting it) and E (the backup program)

**In short:** Setting a backup to run every day did nothing. It will be run by
a background service that the system starts at boot, so a daily backup happens
on time even when nobody is signed in. If one was missed because the machine
was off, it runs as soon as the machine is on again -- before anyone signs in,
and without asking.

**The question:** `open-questions.md` C-Q21 (now resolved). **Verbatim:** "It
doesn't matter how long it stays broken as long as it's fixed by the time the OS
is finished ... option A. Though I think that if a backup is missed because the
machine is off, it shouldn't just ask when the user signs on, it should backup if
the machine comes back on after it was missed even if the user hasn't signed on
yet. And in that case, I don't think it should ask the user whether to backup or
not regardless. By the way, I guess backup should be a service handled by our
startup manager ... I don't know if that handles services run all the time too,
or only things loaded when the user logs in. I think the former?"

**On the operator's question:** yes -- the init system starts services at boot,
independent of sign-in; the backup service is one of those. The work is lane D's
(`services/`), with lane B's init starting it and lane E's backup program writing
the schedule the service reads (`BUG-C-BACKUP-SCHEDULE-WRITES-A-FILE-NOTHING-EVER-READS`).
