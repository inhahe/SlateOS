## TD-C-A-CORRECT-TOOL-ANSWERING-A-NARROWER-QUESTION -- METHOD 2026-09-15

**In short:** four times in one day, a tool gave a true answer to a question
slightly narrower than the one being asked, and the answer read as complete.
Three were tools used carelessly. The fourth was `git log -- <path>` doing
documented, deliberate, correct work — and that one is the reason this is an
entry rather than a note, because there is nothing to fix in the tool and
nothing in its output to notice.

### The four

| the question asked | what the tool answered | how it showed |
|---|---|---|
| "which apps route a file dialog?" | "which contain the string `fn apply_dialog_action`" | count was 11; it was 13 |
| "is the tree free of collapsed messages?" | "are `gui`, `apps` and `scripts` free of them" | 382 files, none of them lane A's |
| "does this test catch the bug?" | "did the build succeed" | a build error read as "the test did not notice" |
| "what commits changed this file?" | "what is the simplest history explaining its content" | lane B's repair invisible |

In all four the output is **true**. Nothing in any of them indicates the
question was narrowed.

### Why the last one is different, and worse

The first three are mistakes with a fix: spell the grep better, name the
corpus, distinguish a compile failure from a test failure. Someone reading the
tool's configuration could have seen the gap.

`git log -- <path>` has no gap to see. History simplification is a designed
feature with a flag to turn it off, and it is answering a genuinely useful
question — *how did this file come to look like this* — which is simply not the
question "who changed it". When lane B and lane A both made the **byte-identical**
repair to one line, the merge was TREESAME to one parent, and the default
follows only that side. Lane B's commit vanished from the listing while
remaining in the history.

    git log --oneline origin/main -- deflate/src/lib.rs
      5a7cf6796                                 <- one commit

    git log --oneline --full-history origin/main -- deflate/src/lib.rs
      ... 9159b3d30, 5a7cf6796                  <- both

**A correct tool cannot warn you that you asked the wrong question.** That is
the whole of it, and it is why "use better tools" is not the lesson.

### What to do instead

**Check content, not provenance, when the claim is "this is fixed."**
`git show origin/main:<path>` answers what anyone actually depends on — the
bytes in the tree they will build. Which commit produced them is a different
and more fragile question, and in this incident the same person got the content
check right and the provenance check wrong in one message about one file.

`--full-history` when the question really is *who changed this*; plain `log`
when it is *how did it get this way*.

More generally, and this is the transferable part: before believing a tool's
answer, say out loud what question it answers. Not what it is *for* — what it
answers. `git log -- <path>` is *for* finding out about a file's history and it
**answers** a narrower thing. The gap between those two sentences is where all
four of these lived.
