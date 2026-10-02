## 818. A passwordless account is never locked, rather than being lockable and then let through

**Date:** 2026-09-07
**Lane:** C
**Decided by:** Operator (Claude recommended C)

**In short:** if an account has no password, the lock screen had nothing to
check, so locking the screen produced a screen that anybody could dismiss --
security theatre, and worse than none, because it looks locked. From now on an
account with no password is simply never locked in the first place. The screen
does not appear, so nothing pretends to be protecting anything.

**The question.** `open-questions.md` -> the lock-screen entry of 2026-08-24.
Four options: accept the empty password (today's behaviour), refuse it (which
locks the user out of their own machine forever), never lock such an account
at all, or accept it only if the account was passwordless before the lock.

**The answer: C**, which was also the recommendation. It is the only one of
the four that is neither a hole nor a trap. Refusing strands the user;
accepting is a lock that does not lock; the fourth option is the third with a
race condition attached.

**What a user sees.** Setting a password on the account is what turns locking
on. That is a discoverable relationship and a true one, which the previous
behaviour was not.
