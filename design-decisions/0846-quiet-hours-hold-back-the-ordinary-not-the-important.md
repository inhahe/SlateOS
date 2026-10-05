## 846. Quiet hours hold back the ordinary, not the important

**Date:** 2026-09-14
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** You can now tell the desktop "do not interrupt me between
these hours". The question decided here is what that should mean. It
means ordinary pop-ups are held back and important ones still get
through -- an alarm, a low battery, a message from a program you have
marked as important -- rather than nothing at all getting through. If you
want total silence you can still switch it on by hand for as long as you
want it; what the *schedule* does is the gentler of the two.

### The decision

Quiet hours install one automatic rule set to **Priority only**, not to
**Total silence**.

| | Priority only | Total silence |
|---|---|---|
| an alarm at 3 a.m. | rings | does not ring |
| an ordinary chat message | held | held |
| a program the user marked Priority | shown | held |
| the user's recourse when it is wrong | mark the program lower | none until they notice |

The argument for total silence is that it is what the words on the switch
sound like, and that a user who asked not to be disturbed and then was
disturbed has been let down.

The argument against, which won: **the cost of the two mistakes is not
symmetric.** A schedule is set once and then runs unattended for months.
If it is slightly too permissive the user is interrupted by something they
would rather have missed, notices, and turns that program down -- the
system tells them about its own mistake. If it is too restrictive they
miss the notification that mattered, and *nothing tells them*; the only
symptom is a thing that did not happen. A setting that fails silently in
the direction of losing information, months after it was configured, is
the worse failure, and the ladder of importances exists precisely so that
"important" can be honoured here.

Total silence remains available as a manual mode, which is the right home
for it: switched on deliberately, for a bounded stretch, by someone who is
thinking about it now.

Reversible in one line (`FocusMode::PriorityOnly` in
`FocusAssistManager::set_quiet_hours`) and the obvious next step if the
operator disagrees is a third choice on the page -- "hold everything" --
rather than changing what the existing setting means.
