## `TD-C-SEVEN-WAYS-A-SEARCH-SAYS-NOTHING-AND-MEANS-NOTHING` (lane C, 2026-09-18)

**In short:** When you grep a codebase and find nothing, you have learned that
*that text* is not there. You have not learned that the *code does not do the
thing*. Those two are different, and on 2026-09-18 I confused them seven times
in one day, in seven different ways. Each one nearly became a filed defect
about a program that was working correctly, or nearly hid a real one. This
entry lists them with the instance that caught each, because the fix is not
"be careful" -- it is knowing the specific shapes.

**It started at seven and is at twenty-two**, and the slug
keeps the original number because renaming it would break every reference to
it. 18 and 19 are the two that are not failure shapes at all -- 18 is the
question that ends a run of them and 19 is about the cost of a grouping you
were handed. The later ones are not more of the same: 8 and 12 are about *coverage* --
which files a sweep read, which configuration a build compiled -- 11 and 13 are
about *lists*, which is where this has cost the most, and 14 and 15 are about the
*query itself* rather than the reading -- 14 puts the answer where the filter
cannot show it, 15 asks only about the words the author happened to use. Those
two survive however carefully the code is read, which is what makes them the
dangerous half: every other shape on this list is beaten by looking harder, and
these two are not. The count going up
is the useful signal here; it says the supply is not exhausted, so a search
that returns nothing still means nothing.

| # | The divergence | What it cost |
|---|---|---|
| 1 | **Spelling.** The same key is `Key::H` in one app and `Key::Char('h')` in another. | Concluded `markdowneditor`'s Ctrl+H was unbound. It is bound. |
| 2 | **Receiver.** A field is `self.find_state.query` in the app and `state.query` inside a function taking the struct by reference. | Nearly filed a live find-panel as dead code. |
| 3 | **Case.** `grep 'cannot'` does not match `"Cannot trace a route"`. | Claimed `netscan` had "zero honest admissions". It has five. |
| 4 | **Prose.** `grep guitk Cargo.toml` matches a *comment* reading "must not link a widget library... rather than through `guitk`". | Nearly recorded `backup` as a GUI app when its manifest says the opposite. |
| 5 | **Structure.** Cutting production code at `#[cfg(test)]` includes every test-only item that precedes the last one. | A date sweep reported **50 hits across 12 apps**; the real answer via `rustlex.live_code` is **18 across 11**, and `devicemanager` went from 23 to 1. |
| 6 | **Method mutation.** A field with no `=` anywhere may still be written by its own methods: `self.volume.increase(5)`. | Nearly filed `videoplayer`'s volume as frozen. |
| 7 | **Sub-field assignment.** `self.password_opts.use_symbols = x` is invisible to a search for `.use_symbols` on the app struct, and `self.time_signature = sig` makes `beats_per_measure` *look* frozen when it is not. | Missed `passwordgen` on the first pass; nearly filed `metronome`'s time signature, which works. |

| 8 | **One file vs the crate.** Every sweep run on 2026-09-18 globbed `apps/*/src/main.rs`. **12 of 141 apps have more than one source file** -- `explorer` has 8, `settings` 5, `editor` 4. | Concluded `apps/editor` "has zero typing sites" and could not be typed in. Its typing lives in `input.rs`. A text editor was one sentence away from being filed as unable to accept text. |

| 15 | **The checker's population is defined by a name.** A tool that surveys or enforces something across a tree has to decide what it is looking at, and the cheap way is to match an identifier. Then it reports a number about the tree that is really a number about the author's vocabulary -- and it is silent about everything spelled differently, which is exactly the population it was built to find. | `scripts/key-survey.py` matched `SHORTCUTS` and `ALL_KEY_ACTIONS` and reported **ten** apps printing a key list; `apps/magnifier` calls its `HELP_ROWS`, and the real number is **29**. I had vouched for the ten in writing as "the one number here that needs no inference". Lane A's `scripts/check-variant-lists.py` has the same shape the same day: it checks lists *named* `ALL`, so one that should be total and is called `PRIMARY_COMMANDS` is invisible and nothing says so. The cure is to define the population by something the type system fixes -- `const NAME: ... (&str, &str)` -- rather than by what somebody called it. |
| 14 | **The filter kept the wrong end.** A search that ends in `head` or `tail` answers a different question from the one asked: it reports what the *last* N matching lines were, not whether any of them was the one you were looking for. The match count is reassuring and unrelated. | Gated a lane-C merge on `cargo test --workspace ... | grep -E 'FAILED|^error|test result: ok' | tail -40`. It printed forty `ok` lines and no failures -- and *could not have printed a failure*, because a workspace of 420 crates emits hundreds of `ok` lines after any early one. The pipeline also threw away cargo's exit status, so both the evidence and the verdict were gone. The fix is not a wider filter: keep the whole log, capture the status into a variable on its own line, and search the file. **And it happened again on 2026-09-22, to the person who wrote this row.** Checking whether `apps/calendar`'s `week_starts_monday` was still frozen: `grep -n week_starts_monday apps/calendar/src/main.rs | head -6` returned six lines, all declaration and reads, and I concluded there was no writer. There are fourteen matches and the writer is on line 2988 -- `head` stopped three lines short of it, and I was one commit away from "fixing" a setting that already worked. The row above was four days old at the time. **A catalogue of mistakes does not stop you making them; it only lets you recognise the one you just made**, which is worth something and is not the same thing. The habit that actually catches it is cheaper than the row: when a search is deciding whether something is *absent*, count the matches before reading any of them. |
| 13 | **The checker is a third copy.** A list printed on screen and the handler behind it are two copies of one fact, and the cure is a test that reads both -- but a test that reads the list and then *names the keys itself* has added a third, which drifts from the other two and catches neither. | Four of the five apps that print their keys checked them this way, each having written the same forty-line `"Left/Right" => vec![Key::Left, Key::Right]` table independently: `mixer`, `rssreader`, `wordsearch` and `slides`. `rssreader`'s checked only the *first* key of each row, so three advertised keys had never been pressed by anything. Now one parser, `guitk::shortcut`, reads the printed label -- see `TD-C-A-PRINTED-KEY-LIST-IS-A-SECOND-COPY`. |
| 12 | **The test build never compiled it.** Code behind `#[cfg(not(test))]` is absent from `cargo test`, so the suite passes over it without type-checking a line. The mirror image of lane A's `#[cfg(unix)]` lint, which no clippy on a Windows host ever compiles. | Added `apps/terminal`'s shell bridge behind `#[cfg(not(test))]`; **126 tests passed over code that had never been compiled.** It was caught only because `main` then referenced functions absent from a test build, which failed loudly -- had it not, an unchecked feature would have shipped behind a green suite. |
| 11 | **One list is checked and its twin is not.** `apps/editor` has an exhaustive `match` that forces a new `Command` to be handled -- and a hand-written `Command::ALL: [Self; 14]` that decides whether the guard test ever *reaches* it. The compiler enforces the first and nothing enforces the second. | A variant added to the enum and omitted from `ALL` compiles, with a guard-test arm that is written, never executed, and reported as passing. **No symptom at all** -- worse than passing for the wrong reason, because there is no run to inspect. |
| 10 | **A heredoc eats the backslashes.** A `python - <<'PYEOF'` block is supposed to pass its body through literally; in this shell it did not, three times. `\x1b` arrived as a real ESC byte and `\r\n` as a real CRLF. | Wrote literal control characters into a Rust byte literal (invalid source), and a lone CRLF into `known-issues.md` -- in a paragraph *about* an escape sequence, which is how it got past reading. The habit that fixes it: **any script containing backslash escapes goes in a file, not a heredoc.** |

| 9 | **A function used as a value.** `self.moving(shift, Document::move_up)` passes the function; it never writes `move_up(`. Every "who calls this" query here counts `name(`, so a callback looks dead. | Concluded `apps/editor`'s cursor could not move up or down. It moves. Its arrow keys pass the movement functions to a shared `moving` helper, which is *better* code than calling each directly -- so the query is most wrong about the tidiest implementations. |

**The ninth was found the same way as the eighth -- by reading an app the query
had just accused** -- and it prompted an audit of every "no caller" claim acted
on that day (`notes`, `slides`, `diagram`, `explorer`, `rssreader`, `netscan`,
`pdfviewer`: 15 functions). **None is passed as a value anywhere**, so all of
them hold. The check is one line and belongs in any future sweep: count
occurrences of the bare name against occurrences of `name(`, and read the
difference.

**Eight and twelve are the same question asked of different axes**, and
together they say what a green run actually covers: shape 8 is *which files*
were read, shape 12 is *which configuration* was built. A suite is silent about
every line outside both. The practical form is two questions to ask of any
passing run -- **did it read the whole crate, and did it build the
configuration the user gets?** -- and on this project the answers differ from
the obvious one often enough to be worth asking aloud: 12 of 141 apps have more
than one source file, and `cargo test` builds `cfg(test)` while the shipped
binary is `cfg(not(test))`.

**The eleventh is the one to look for in any codebase with a guard test.**
The pattern "an exhaustive `match` plus an array of every variant" is a good
design -- `apps/editor`'s comment is right that it makes the compiler ask the
question at the one moment somebody holds the answer. But the array is the
half that decides whether the test *runs*, and it is the half the compiler
cannot check. Anywhere the two are separate, adding a case can produce a test
that is written and dead. The tell is a length annotation: `[Self; 14]` is a
hand-maintained count, and a hand-maintained count is a second list wearing a
number.

**The eighth is the one that should worry a reader of this entry most**, because
it silently narrows every other row: a search that is *correct* about the file
it read is still wrong about the program when the program is bigger than the
file. Two of the apps fixed on 2026-09-18 -- `imageviewer` and `pdfviewer` --
are on that twelve, and their frozen-field findings were made from `main.rs`
alone. **They were re-checked across `video.rs` and `pdf.rs` after this was
noticed**, and hold: the only writers of `show_toolbar`, `show_status_bar` and
`dark_mode` are the ones added by the fixes. That is luck rather than method,
and the method is `pathlib.Path(f'apps/{app}/src').glob('*.rs')`.

**The shape they share** is that a search reports on *text* and the question
was about *behaviour*. Every one of these is a case where the text and the
behaviour come apart -- and they come apart most often in exactly the code
worth examining, because a program doing something interesting is a program
spelling it in some particular way.

**The corollary, which outranks all seven** (from lane A, 2026-09-18, after
it nearly cost them a false report that the kernel's file-immutability
protection did not work): **"has no caller" and "nothing does this" are
different sentences, and only the second survives a second implementation
path.** Their call graph was clean and every step of it was true --
`FileAttr::IMMUTABLE` is checked in `Vfs::is_writable`, whose only caller is
inside `self_test()`. The conclusion was still false: enforcement does not go
through that predicate at all, because `write_file`, `truncate` and `unlink`
each refuse on their own paths. What settled it in one command was asking
whether the **test passes** rather than where the call site is.

Run against tonight's two findings, both survive, and the reason is that
neither rests on a call graph: `slides` has **zero assignments to `.text`
anywhere in the crate**, tests included, and `notes` has both its
`notes.push` sites *inside* the two unreachable creators with no import. The
mutation was checked, not the entry point. **A finding phrased as "no caller"
should be re-phrased as "nothing writes it" before it is filed, and if it
cannot be, it is not ready.**

**Being wrong in this direction is the expensive one.** Recording a capability
as missing when it exists tells a reader not to rely on something they can
rely on -- which is what the withdrawn
`TD-C-WEATHER-CAN-ONLY-EVER-BE-EMPTY` did, and why it was withdrawn in place
rather than deleted.

**What actually works**, in order of how much it costs:

1. **Ask for the writers, not the name.** "Does anything assign this?" survives
   1, 3 and 4, because assignment has a syntax and prose does not.
2. **Use the lexer.** `rustlex.live_code` for "is this production code" and
   `rustlex.strip_noise(keep_literals=True)` for "is this a comment". Both
   exist because somebody already lost a day to 5 and to 4; the second one's
   own doc says so.
3. **Ask what the program does when run, not where the call site is.** This
   belongs above the lexer and was written below it at first, which is
   backwards: the lexer makes a *search* honest, running the thing makes a
   *claim* true. For a GUI app that means the render tree -- "can this app
   show anything" is answered by the draw and not by the fields, see
   `TD-C-WEATHER-CAN-ONLY-EVER-BE-EMPTY`, withdrawn for exactly this. For a
   kernel subsystem it means the boot line. Same question, different output.
4. **Then read the code.** Every genuine defect filed today --
   `rssreader`'s sidebar, `passwordgen`'s classes, `netscan`'s scan report,
   `podcast`'s timestamp, `markdowneditor`'s find panel -- was confirmed by
   reading it. **No probe found one that reading did not.** The probes were
   worth running only as a way of choosing what to read.

**16. The pipe answered instead of the program** (2026-09-21, both lanes).
`cmd | head` reports *head's* exit status, so `cmd 2>&1 | head -5; echo $?`
prints `0` whatever `cmd` did. Worse, when the output outruns the pipe buffer
`head` exits first and the writer takes SIGPIPE -- **exit 141, which is
128+13 and looks like an ordinary failure code rather than a shell artefact.**
Lane A had a `git push` killed that way and read the surviving local
fast-forward as a failed push, and had a gate come back 141 instead of 1.

This lane hit the reading half twice on 2026-09-21 and caught both:
`... | tail -5; echo "EXIT=$?"` printed `0` while the tool exited 1, and a
`git merge --ff-only` that printed `fatal:` was followed by `MERGE_RC=0`.
Both were caught by noticing the *words* disagreed with the number, which is
luck, not method.

The measurement that separates the two halves: `PIPESTATUS[0]` after
`python scripts/key-survey.py 2>&1 | head -5` is **0**, because 45 lines fit
the pipe buffer and the writer finished before the reader left. So the SIGPIPE
half is conditional on output size -- which means it appears when a file grows
and not before, and the command that was fine yesterday is the one that lies
tomorrow.

The rule, and it costs nothing: **never read a status through a pipe.**
Redirect to a file and echo `$?`, then read the file. Every verification in
this lane's sweep does that; every *display* that pipes is followed by a
separate redirected run when the status matters.

**And its sibling, which is the more general half** (lane A, same day):
*never let a status-bearing command be anything but the last thing in an
invocation, and if it cannot be, capture `$?` into a variable immediately.*
A pipe is only one way to lose a status -- a trailing `grep`, `tail` or
`echo` does it just as thoroughly. Their instance is the one worth keeping:
a backgrounded chain ended in a `grep`, the harness reported **exit 0** over
a log containing `BOOT_RC=1`, and `chain.sh` carries a fifteen-line comment
about that exact failure written by the same hand after seven false
"succeeded" notifications. The fix was inside the script; the reintroduction
was in the line that called it. That is what makes this a rule and not a
lapse -- knowing the fault does not protect the next invocation.

**17. The control tested the axis that was already working** (lane A,
2026-09-21). Lane A measured `unsafe` blocks lacking a `// SAFETY:` comment
at 318 of 2214, ran a control for a placement their scanner might miss --
the comment written as the first line *inside* the block -- and it found 4.
The number barely moved, so they recorded the instrument as sound. It was
not: a SAFETY comment governing a group of reads, separated from the first
`unsafe` by one *safe* statement, defeats a backward walk that stops at the
first non-comment line. The real figure was 21 of 2210. **They were
measuring "has a comment immediately above" and reporting it as "has a
SAFETY comment".**

The lesson is about the control and not the regex: **a passing control
licenses only the failure it simulates, and the confidence it produces is
general.** Theirs varied placement *after* the block; the fault was
placement *before but separated*, an axis it never touched -- and a control
that passes is far more persuasive than no control, so running it made them
more confident and no more correct.

Applied here within the hour. This lane's frozen-flag zero rested on one
planted defect: a writer removed from an already-scoped struct in an
already-covered crate. The 569 fields the survey *sets aside* are precisely
the axes that control never varied. Three more were run: a frozen bool one
level down in a singleton sub-struct (reported), a frozen fieldless enum
(reported), and a frozen bool in a type the crate stores in a `Vec`
(silent). That last one is the interesting case, because "correctly
excluded" and "blind" are indistinguishable from a silence -- so the same
type, with the same field, was changed to be held singly instead, and it
became reported. Varying the axis is what separates the two readings.

*Third instance, 2026-09-21, and the first found in a test written minutes
earlier rather than in old code.* `net80211`'s new `parse_frame` refuses a
header whose EAPOL Packet Type is not `KEY`. Two tests were written for it,
one of them named
`a_body_passed_as_a_frame_is_refused_by_the_packet_type_check`. Both passed. The check was then deleted to see which tests noticed,
and **that one still passed** -- a body's octets 2-3 read as a five-figure
length that overruns the buffer, so the rejection was coming from the length
and the test's name was a claim no assertion in it could see. Renamed to
`a_message_2_body_passed_as_a_frame_is_refused_by_both_checks`, which is
what it can establish.

Two things worth carrying forward. First, the cheapest way to find a shape-17
test is **to delete the mechanism it is named after and re-run**; it took one
minute here and needs no reasoning about what the test covers. Second, the
deletion paid for itself twice: explaining why only *one* of the two tests
depended on the check required working out what the octet in that position
really holds, which produced a counterexample to the reasoning the check had
been suggested on -- message 4 and group message 2 put `0x03` there, which is
`packet_type::KEY` itself. A justification that had been accepted as obvious
was wrong for two of six cases, and nothing but the planted defect was ever
going to surface it. See design-decisions.md 865.

**Why this is filed rather than merely learned.** The three probes written
today (frozen fields, displayed-but-unchangeable labels, admit-yet-claim) all
over-report, and a future session that trusts their output will file working
programs as broken. The entry they live in
(`TD-C-SETTINGS-THE-PROGRAM-OBEYS-AND-NOTHING-CAN-CHANGE`) says so; this one
says why the failure is systematic rather than a matter of care.

**18. The artefact already contained its own counter-example** (lane A,
2026-09-21, and lane C the same day). Not a failure shape but the question
that ends them, and it is free. Lane A's `ctest-coreutils-runs` had failed six
rounds, each round proposing a structural cause -- not staged, wrong path,
missing capability, forked child cannot exec, native exec syscall broken -- and
each costing a ~90 minute boot to disprove. The thing that killed all six was
in the *first* log any of those rounds produced: one `[spawn]` line recording a
ring-3 process forking and the child exec'ing a binary out of the same
directory, in the same boot. A 2.7 MB log with 275 verdicts in it almost
certainly holds one that contradicts your theory.

**The rule: before any theory that costs a boot, a build or an hour, ask
whether anything in the run you already have does the thing you believe is
broken.** dd-954 says a passing control licenses only the axis it varies; the
corollary is that **a passing control you did not write is still a control.**
Looking is free and theorising is not.

This lane had the same day from the other end. Six apps were opened off a "the
app never names its keyboard shortcuts" queue and five needed nothing, and in
every case the answer was in the app's own source: a footer the detector could
not match, a guard under a different name, a table of structs rather than
tuples, a `1-0` range an ascending expander reads as empty. Six apps read
before the instrument was fixed instead. The queue was the artefact holding its
own counter-example, and reading one app closely would have said so as loudly
as reading six.

**19. A wrong grouping costs more than a missing one** (lane A, 2026-09-21).
Lane A was handed three red test rungs grouped as one finding. The grouping is
reasonable -- if `/bin/true` cannot exec, the rungs below it exercise the same
broken path from further away -- and it is wrong: the third never execs at all,
and its exit 45 is `waitpid(WNOHANG)` exhausting its spin. So a correct fix to
the first clears two of three, and **the natural reading of the survivor is
"the fix is incomplete"**, sending the next round straight back into the path it
had just correctly left.

That is the asymmetry worth keeping. A *missing* grouping costs a second look.
A *wrong* one launders an unrelated bug into evidence against a correct fix,
and the evidence is persuasive precisely because the fix really was incomplete
-- for the other thing. Check a grouping you were handed before you spend
anything on it, including one a harness produced.

This lane generated exactly that failure twice in one day, both in
`known-issues.md` where a future reader would have acted on it: "18 apps have
an unguarded key list" (it was one -- the scan keyed on a *test name*, and
`apps/minesweeper`'s guard is called something else) and "91 apps name no keys"
(26 apps, about 38 real). Both were groupings that would have sent the next
reader somewhere wrong. Both were retracted in the file they were committed to
rather than quietly corrected, because an entry that silently becomes right
teaches nobody why it was wrong.

**20. The measurement was true, reproducible, and about something else**
(lane A, 2026-09-21). A five-day-old diagnosis, filed under a filename that
stated its conclusion, rested on one serial line:

```
[exec] linux_execve ENTERED and failed early: filename_ptr=0x0 errno=14
```

The line is real and fires on every boot. It is a **kernel self-test
deliberately passing NULL to check `EFAULT`** -- `linux.rs:54128`, "execve
user-marshalling NULL handling" -- sitting about 2,500 lines *before* the
fixture it was read as describing ever runs. No probe fires anywhere near the
fixture's actual failure. The diagnosis was a correct observation attached to
the wrong subject, and it stood as the explanation of a failing test for five
days.

**Nothing about the observation itself says which subject it belongs to.** It
is true, it reproduces, it names the right syscall and the right errno, and it
is adjacent in the log to the thing being investigated only in the sense that
both are in the same 2.7 MB file. Every property a measurement can have in its
own right, this one had.

This lane raised the thread that unpicked it, and got the answer wrong in an
instructive way: asked how a NULL could reach the syscall when the guard that
rejects NULLs predates the observation by three weeks, it offered three
resolutions -- the NULL arises lower down, the guard is bypassed, the probe
read the wrong register. **All three assumed the hit belonged to the fixture.**
"True, and about something else" was not on the list.

The relation to 19 is worth naming, because the two are the same failure at
different scales. A wrong *grouping* launders a second bug into evidence
against a correct fix. A wrong *attribution* launders a self-test into evidence
about production code. In both the damage is not that the evidence is weak --
it is that the evidence is **strong**, and pointed at the wrong thing.

**The practical rule:** before a log line becomes a diagnosis, establish that
it belongs to the run of the thing you are diagnosing -- by line number against
the subject's own first line, by pid, by anything. And for the inverse, which
is the same rule from the other end: a probe's *silence* is evidence only once
something you know fails has passed through that probe and been seen.

**21. The assertion watched a quantity the action does not move** (lane C,
2026-09-21, four times in two hours). Writing "this key must not act while the
help card is up" tests, the negative half kept passing for the wrong reason,
because the thing being watched never changed either way:

| app | asserted | why it could not move |
|---|---|---|
| `apps/email` | `messages.len()` after `Delete` | `delete_message` *moves* to Trash and only removes a message already there |
| `apps/alarmclock` | `alarms.len()` after `N` | `N` opens an editor; the alarm appears on confirm |
| `apps/dbviewer` | the drawn **text** after `Tab` | focus is a highlight, not a word |
| `apps/stickynotes` | the note body after a letter | `probe::press` carries no `text`, and this app inserts from the event's text, so nothing types either way |

Each of those tests passed. Each would have passed just as well **on an app
with the feature deleted**, which is the definition of testing nothing.

**What caught all four was the same cheap addition: a control doing the thing
with the modal down.** `assert_ne!` after dismissing the card, `assert!(editor
.is_some())` once it is closed. Every one of the four was found by the control
failing, not by the negative assertion -- the negative assertion is what was
broken, and a broken negative assertion is silent by construction.

**So the rule for any "X must not happen" test: assert that X *does* happen
under the condition where it should.** Without that half you have not tested
that X is prevented; you have tested that you cannot make X happen, which is
also true of an app that cannot do X at all.

The related failure, worth naming because it is the same error one level up:
choosing the observable requires reading what the action *does*, not what its
name suggests. `delete_message` sounds like it deletes. It files.

**After six instances the misses have a shape, and it is predictive rather
than descriptive: the wrong observable is always the field whose name matches
the verb.**

| the key ran | I watched | what actually moves |
|---|---|---|
| `delete_message` | `messages.len()` | the message's `mailbox` |
| `toggle_play` | `is_playing` | `status_message` -- there is no audio backend |
| `StartPause` | `running` | `state: TimerState`; `running` is the event loop's own flag |
| `N` (new alarm) | `alarms.len()` | `editor.is_some()` -- it opens a form |

Every one of those is the field a reader would name if asked "what does this
key change?" without opening the function. That is exactly why it is the wrong
one: the name is the *intent*, and the bug being hunted is a gap between intent
and code. **Reaching for the similarly-named field re-asserts the assumption
the test exists to check.**

The cheap habit that beats it: before writing the assertion, read the function
the key calls and write down the *last line that assigns something*. That line
names the observable. In all four rows above it is one grep away.

**The worst instance of this shape is lane A's, and unlike the five above it
shipped** (2026-09-21). A kernel boot-test rung asserted exactly one thing:
that the process reached `Zombie`. A successful exec ends with the target
calling `exit(0)` -- zombie. A failed exec ends with the caller taking a `#GP`
-- **also zombie**. One assertion, two opposite outcomes, and it reported
success for its entire existence, with a comment claiming "(exec succeeded, new
code ran, `SYS_EXIT` was called)" -- a mechanism it never checked. Four lines
above its OK, the same log said
`[exec] NATIVE exec FAILED -> -101 (elf_len=136)`.

That is the same defect as the five here, at the scale where it matters: the
observable was real, the assertion was true, and the quantity it watched could
not tell the two outcomes apart. A sixth app's help-card test passing wrongly
costs a reader nothing; a boot rung passing wrongly cost six rounds at ~90
minutes each, because it was the evidence that the exec path worked.

**22. The disconfirming fact was absorbed as a refinement** (lane A,
2026-09-21; this lane the same morning). The most expensive shape here so far,
and the hardest to see from inside, because the conclusion comes out of the
exchange looking *better* supported.

Lane A held that a C `execl` was passing a NULL path to `execve`. This lane
gave them two facts over the following weeks, neither intended as a refutation:
that `execl` *is* `execv` plus a `va_list` walk, so everything below the
delegation is shared with the arm that works; and that their discriminator had
no C-side control, there being no C `execv` anywhere in `services/` to compare
against. Both were reasons the conclusion could not stand. **Both were read as
narrowings and the conclusion was kept** -- each one attached to it as detail
about *how* it was true. It was finally withdrawn on a third ground that made
it impossible rather than merely unsupported: `posix`'s `execl`, `execv` and
`execve` all funnel to the *native* syscall, so a C fixture cannot produce a
`linux_execve` log line at all.

This lane did the same thing in miniature the same morning, and did not
recognise it until lane A wrote the sentence. Having found `key-survey.py`
over-reporting, fixing that, re-measuring and announcing a two-thirds
reduction, every further fact gathered was about the direction already decided
to be the problem. The question never asked was whether it *under*-reported,
which it did, by three times as much.

**The distinction from the rest of this catalogue.** 19 is a wrong grouping, 20
a wrong attribution, 21 an assertion watching the wrong quantity -- all of them
errors *in* a measurement. This one is an error in what a correct measurement
is allowed to do: a fact that bears on whether the conclusion is true is filed
under how it is true. Nothing is miscounted and nothing is misread.

**The tell, stated so it can be used:** when a new fact arrives and the next
move is to make the conclusion *more specific* rather than to ask what would
have to be true for it to be false, that is the moment. A conclusion that has
absorbed three facts and predicted none of them is not better supported than it
was; it is a conclusion three facts have failed to dislodge.

**A third instance, this lane's, an hour after writing the entry.** Fifteen
apps into the shortcut-card programme I decided the expensive part was the
mechanical anchor-finding -- the app struct, the palette field, where the
render function closes -- and wrote `scripts/shortcut-scaffold.py` to report
all of it in one call instead of three or four greps. Measured *after* writing:
7.7s on `apps/stopwatch`, and slow enough on `apps/contacts` that a 180-second
self-test run timed out. The greps it replaced take about a second each. It was
slower than the thing it optimised, and it was deleted unused.

The conclusion never tested was "the anchors are what cost". They are not. The
cost is reading each handler closely enough to say what its keys do, which is
where `apps/kanban`'s `Ctrl+S` (advertised as save, actually search) and
`apps/passwordgen`'s invented `Up / Down` row were caught, and which no tool
can shorten. Every minute spent on the scaffold was spent making the cheap half
cheaper.

**Lane A's step, which is better than "be more sceptical" because it changes
what you do:** when a peer's correction arrives, write the sentence *"this
would make my conclusion false if ..."* before writing anything else. If it
cannot be completed, it really was a refinement. Both of theirs completed in
one line.

**And their sharper variant, which is the nastier half.** Both corrections this
lane sent were about *call form* -- `execl` versus `execv` -- and lane A went on
reasoning inside that frame for six rounds. What ended it was *ABI*: `posix`
execs through the native `SYS_PROCESS_EXEC`, so a C fixture cannot reach
`linux_execve` at all. The facts were true and they answered the question being
asked; **the question was wrong**. A correct answer to the wrong question is
indistinguishable from progress -- it arrives with the feel of a narrowing,
because it *is* one, of a space that does not contain the answer. Recorded on
their side as design-decisions 955.

**23. The instrument could not read it, so it reported compliance** (lane C,
2026-09-21). `scripts/key-survey.py` identifies a key by the variant's *name*:
`Key::Escape`. `apps/markdowneditor` defines its own `Key` with the key in the
payload -- `Char('h')`, `Function(5)` -- so the survey read the names "Char"
and "Function", concluded the app bound two keys, and reported a text editor
with 56 binding sites as having two unnamed ones. It sat near the bottom of a
36-app queue looking nearly done.

The tell is not in the output. The output is unremarkable by construction --
that is the whole failure. **The tell is a question nobody asks of a scanner:
what does it do with input it cannot parse?** Three answers exist -- raise,
report-as-unreadable, skip -- and the default in every regex-based tool is
skip, because skipping is what a regex does when it does not match. A skip is
indistinguishable from a pass. So: *for any survey, ask what it does with what
it cannot read; if the answer is "nothing", the clean rows are the ones to
distrust.*

**The asymmetry that makes this worse than the three before it.** This survey
has now been wrong four times, and the first three over-reported -- 91 apps,
then 26 of 68, then 55 of 259. Every one was found within a day, because an
over-report is self-correcting: you read the rows it offers and find nothing
in them. Under-reporting has no such loop. The app drops off the list, and
dropping off the list is precisely what being fixed looks like. **An
instrument that fails toward clean deletes its own evidence**, which is why
the three noisy versions of this tool were cheap and this one was not.

**The trap inside the fix.** Reading the payload naively swaps a blind spot
for twelve keys the app does not bind: the bridge `GKey::F1 =>
Key::Function(1)` constructs all twelve function keys, and `markdowneditor`
binds no function key at all. The distinction that holds is textual -- a
pattern stands to the left of its arm's `=>`, a construction to the right of
one. Three of the seven new self-test cases are controls taken from that: the
bridge that builds a key, the catch-all that binds one, and the test that
presses one. Without those three the scan reports 21 keys where the app
answers 9. A correction to an instrument needs its own controls, or it is just
the next version to be retracted.

**24. The type's documented meaning, in a program that has no such context**
(lane C, 2026-09-21). `EventResult::Ignored` is documented in the toolkit as
"Event was ignored (propagate to parent)". That is exactly right for a widget
inside a tree. `apps/torrent`, `apps/notes` and `apps/mediaconvert` are not
widgets -- they are top-level applications, and each one maps
`EventResult::Ignored` to `Response::Idle`. **There is no parent.** In these
programs the word means "nothing changed, do not redraw".

I read the definition, believed it, and edited an app to match it: `torrent`'s
`set_filter` returned `Ignored` when the chosen filter was already the current
one, and I deleted that early return on the grounds that it "handed this
window's own key upward". Nothing was handed anywhere. The change cost a
redundant repaint of every row on a keypress that did nothing, and the commit
message stating the reason was confidently wrong.

**The rule, which is cheap and I did not apply it:** *when a value's meaning
depends on who receives it, read the receiver, not the definition.* One grep
for `EventResult::Ignored =>` across the three apps would have settled it in
ten seconds, and it is the same grep I would have run without hesitation if
the doc comment had said nothing at all. **A definition that answers the
question stops you asking it** -- which is only safe when the definition
covers your case.

**What made it visible was not review.** It was writing the same guard for a
second app: `notes` returned `Ignored` for `Up` on a list of five notes, I
reached for the same edit, and this time the app had a test whose message
said the quiet part -- "backspace on an empty query is not a redraw". I had
had that file open for twenty minutes. The generalisation worth keeping is
that **the second instance is where a wrong model becomes visible, so a fix
applied once is a hypothesis and a fix applied twice is a test of it** -- and
the moment to look hardest is when the second case feels like the first.

**It also narrowed what the shortcut guards assert**, which is the useful
half. `Consumed` does not mean "this window owns this key"; it means "this
key did something just now". For a discoverability card that is the better
question -- a key that changes nothing in any reachable state should not be
advertised -- but it means the guard must be run from states where each key
has somewhere to go. `Up` at the top of a list, `1` on the filter already
chosen and `Esc` with nothing to cancel are all answered keys that correctly
report `Ignored`.


**25. The assertion was one-sided in the wrong direction** (lane C,
2026-09-22). `apps/benchmark`'s `a_longer_piece_of_work_is_measured_as_longer`
slept 5ms, slept 50ms, and asserted the second measured longer. It failed a
`cargo test --workspace`: the **short** sleep was descheduled and measured
1.4071349s against the long sleep's 0.0623854s.

**The comment above it already knew the mechanism and still drew the wrong
conclusion from it.** It read: "the bounds are loose because a sleep is a floor
and a loaded machine can overshoot it by a lot". Every clause of that is
correct. What it missed is that `long > short` **is not a loose bound** -- it
is an upper bound on `short` wearing a disguise, and it fails the instant
`short` overshoots past `long`. Load decides which of two separately timed
intervals gets descheduled, so an ordering between them is a coin toss whose
bias is the only thing the test measures.

**The rule:** *load can only ever push a measurement up, so the only timing
assertion load cannot break is one that bounds a measurement from below.* No
upper bounds, and no comparisons between intervals timed separately. A
descheduled thread does not run faster, so a floor is safe in a way a ceiling
never is. `sleep` guarantees it sleeps *at least* the duration asked for,
which makes "measured at least 50ms" both true on every host and false for
every implementation the test exists to catch.

**The "varies with input" half survives, asked differently.** The thing being
falsified was a `seconds` that returns a constant. Two real measurements of
different work are never bit-identical and a constant always is, so
`(long - short).abs() > f64::EPSILON` asks the same question without caring
which way round the two numbers land.

**Where it generalises.** `gui/appearance/tests/resolve_cost.rs` had the same
defect latent: it asserts *ceilings* on a per-call cost. Its module doc had
also thought about load and chosen the other mitigation -- a bound twenty
times the measured figure, on the reasoning that "a bound that only catches a
catastrophe is the right bound when only catastrophes are possible". That is
right about the bound and wrong to stop there: a wide bound makes host noise
less likely to fail the test, not unable to. Lane A had already settled the
missing half in design-decisions.md §952 -- **a measurement the host can
distort needs a repeat, not a wider bound** -- so `per_call` now takes the best
of three runs. The minimum is the sample least contaminated by the host, and a
genuine regression raises every sample including the smallest, which makes the
statistic one-sided in the same direction as the noise.

**Three of these in one session, in three lanes**: lane B's
`special_var_seconds_and_epoch` (`$SECONDS` read 1, expected 0), lane B's
`a_pipelines_stages_begin_in_pipeline_order` (a 100ms handshake budget
asserted as absolute), and this one. All three passed alone, in their own
crate suite, and failed only under a workspace run. The shared cause is not
timing as such but **asserting a guarantee the mechanism only offers when the
host is idle** -- and the shared tell is that each test's own comment or doc
already described the best-effort nature of what it was asserting.
