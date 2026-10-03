## 1155. A cancelled asynchronous lookup says so, and counts as answered for its batch

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `getaddrinfo_a` looks names up in the background, and
`gai_cancel` takes a request out of the queue before it runs. On glibc the
cancelled request is then left saying "still being looked up" for ever: its
`gai_error` answers `EAI_INPROGRESS` -- while a second `gai_cancel` calls it
finished -- and the batch it came in never counts it, so a caller waiting
for the batch waits for ever and a batch to be announced is never announced.
Here a cancelled request's `gai_error` is `EAI_CANCELED`, as the
getaddrinfo_a(3) manual page says it is, and cancelling it counts as its
answer: the waiting caller returns, the announcement comes.

| | glibc 2.39 | here |
|---|---|---|
| `gai_error` of a cancelled request | `EAI_INPROGRESS`, for ever | `EAI_CANCELED` |
| `gai_suspend` on it | not woken by the cancellation | woken; then `EAI_ALLDONE`, nothing listed being looked up |
| `getaddrinfo_a(GAI_WAIT, ...)` whose request another thread cancels | waits for ever | returns 0 |
| a `GAI_NOWAIT` batch with a cancelled request | never announced | announced once the rest have answers |

**The alternatives:** reproduce it, so that a program which cancels a request
and polls `gai_error` until it stops saying `EAI_INPROGRESS` -- the loop the
interface invites -- spins for ever, and a waiting batch hangs; or refuse to
cancel (`EAI_NOTCANCELED` always), which glibc does not do either. glibc's
own answers disagree with each other here (in progress to `gai_error`,
finished to `gai_cancel`); the manual page gives the answer taken.

**Not a difference:** `gai_suspend` answers `EAI_ALLDONE`, not 0, when every
request it is given already has its answer. glibc's code meant 0 there -- the
test it has for it can never succeed -- but "all done" is the error's own
name for that case, and programs written against glibc have met only this.

**Where:** `posix/src/gai_a.rs`.
