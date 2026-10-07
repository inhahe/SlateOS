## 1228. A file a copy cannot carry out is asked about, unless told once to skip such files

**Date:** 2026-09-28
**Lane:** E
**Decided by:** Claude (autonomous) -- the same shape as §1221's taken name,
and within C-Q26's answer (§1418) for where the choice is kept

**In short:** when a copy, move, link or delete meets a file it cannot do --
open in another program, gone, refused -- the file manager now stops and
asks: *Try again*, *Skip*, *Skip all*, or *Stop*. It used to skip the file and
say so in a dialog at the end, so a copy that met a file in use could not be
told to wait and go again. The folder menu's "When a file cannot be done"
keeps the old way for good, for a copy left running unattended.

| Call | Chosen | The other way, and why not |
|---|---|---|
| The default | ask, and wait | skip and say at the end, as it did: a long copy never stalls on a question -- and a file in use is never tried again, when the usual cause (a program holding it) is gone a moment later. Every mainstream file manager asks |
| The answers | try again, skip, skip all, stop; Enter tries again | retry automatically a few times (`ErrorPolicy::RetryN`, which nothing chose): it retried at once, when nothing had changed, and a retry the user times -- after closing the program -- is the one that works |
| Stop at the first failure without asking (`ErrorPolicy::StopOnFirst`) | removed | kept as a policy: nothing chose it, and stopping is one of the answers, given when the user can see what failed |
| The end, after a failure the user answered | the status line alone | the dialog it raised for every failure: a second thing to dismiss about a file the user has just been asked about. A failure nobody was asked about -- after *Skip all*, or a moved source that could not be removed -- still raises it |
| A file that failed part-way, tried again | from its first byte | from where it stood: the chunk whose write failed had been read already, and carrying on would leave it out -- a whole-looking file with a hole in it |
| A copy nobody is watching | the folder menu's "When a file cannot be done": *Ask each time* or *Skip it and say so at the end*, kept in `explorer.yaml` | a question per copy, as for a taken name, or nothing: the menu is where the taken-name choice already is, and one place for both is easier to find |
