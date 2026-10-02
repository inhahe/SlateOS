## `A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ` (lane A, 2026-08-25) — **open**, carried as counted debt — **74 of 800 remain**

> **Burn-down log.** 2026-10-02: 77 → 74; `cmd_applaunch`, `cmd_appstore`
> and `cmd_findex` left the ledger with their commands: the modules behind
> them were removed with the kernel's program and file-type lists
> (design-decisions §1528). Nothing was fixed; the sites went with the code.
> `check-option-refusal.py` confirms the count.
>
> **Burn-down log.** 2026-10-01: 78 → 77; `cmd_secureboot` left the ledger.
> `secureboot records 1O` printed the ten newest records, a default standing in
> for a count it could not read. It is refused with a usage line now. Fixed in
> passing while the command was being reworked for design-decisions §1501;
> `check-option-refusal.py` confirms the count.
>
> **Burn-down log.** 2026-09-10 (forty-sixth batch): **the guessed value was a
> valid object id, so the command succeeded against something else.** 81 → 78;
> `cmd_fileshare`, `cmd_secpolicy` and `cmd_authbroker` left the ledger. Pinned by
> self-test rung 124.
>
> The theme is the harm rather than the value. `0` in these spaces is not a
> placeholder, it is a live object, so the command neither failed nor did nothing:
>
> | Command | With an unreadable id it | |
> |---|---|---|
> | `share access abc rw` | called `set_share_access(0, ReadWrite)` and printed **`Share #0: Read/Write`** | a write to an object nobody named, reported as success |
> | `secpolicy label abc user admin` | labelled entity 0; with no id at all it *read* entity 0's label back | |
> | `authbroker revoke abc` | refused — grant ids start at 1, so the guess hit an `id == 0` sentinel | but printed a usage line, which names the form of the command and not the fault in theirs |
>
> `authbroker` is the near-miss and the distinction is worth keeping: **a sentinel
> that happens to catch a guess is luck rather than a check.** It was never
> silently wrong, and it could not say what was wrong either.
>
> **Method note, after batch 45 got this wrong.** The sites were chosen by asking
> `check-option-refusal.py` which ledger entries dropped, not by pattern-matching
> the source. My own regex for "a guessed value" disagrees with the checker's —
> it matches 14 sites of the shape `parts.get(1).unwrap_or(&"0").parse()` that the
> ledger does not count, because there the parse *does* refuse and the default
> serves an absent operand. Those are a real defect and a different one; see
> `TD-A-AN-ABSENT-OPERAND-DEFAULTS-TO-A-LIVE-OBJECT-ID`. Using the checker as the
> oracle is what kept this batch's arithmetic right.
>
> **Burn-down log.** 2026-09-10 (forty-fifth batch): **the guess was a correct
> default for an ABSENT operand.** 84 → 81 across 84 → 81 functions;
> `fswatch read`'s count, `assoc add`'s priority and `ionice set`'s level left
> the ledger. Pinned by self-test rung 123.
>
> These three survived forty-four batches because the code reads correctly.
> `[PRIORITY]` and `[level]` are optional and 100 and 4 are their documented
> defaults, so the fallback is *right* — for the operand that is missing. The
> same value then answered "you omitted it" and "you typed something I could not
> parse", and only the second is a mistake.
>
> `fswatch read` is the sharpest, because there the fallback could not serve the
> absent case even in principle: the default is applied above as the string
> `"20"`, so the `parse` is only ever reached with a word the operator typed and
> `unwrap_or(20)`'s only reachable purpose was to swallow a malformed one.
> `fswatch read 3 abc` read twenty events and said nothing.
>
> Two of the three sit directly below an operand that already refuses by name —
> `fswatch`'s watch id, `ionice`'s class. One operand refusing and the next
> guessing inside the same command is the clearest evidence available that this
> was an oversight rather than a policy.
>
> **On the count — and a correction to what this log first said.** It claimed the
> ledger was "a number, not a list" and that 84 → 81 could not be verified. That
> is false. `scripts/option-refusal-ledger.txt` enumerates all of it, one line per
> enclosing function with a count, and `scripts/check-option-refusal.py` enforces
> it in both directions: a new guessed-value site in an unlisted function fails,
> and an entry claiming MORE sites than exist is *also* reported, because that
> means the site was fixed and the count was not lowered.
>
> Which is exactly what happened here. This batch was committed without touching
> the ledger, and the boot test refused in 116 s with
> `cmd_assoc: 1 fewer than expected`, and the same for `cmd_fswatch` and
> `cmd_ionice`. The three entries are now removed and the gate reports
> **81 guessed-value sites across 81 functions**, which is the count in this
> heading rather than an assumption behind it.
>
> The hedge was worse than the arithmetic it was protecting: it asserted that
> something could not be verified without looking for the thing that verifies it,
> and the verifier is named in the error message the gate prints.
>
> **Burn-down log.** 2026-08-30 (forty-fourth batch): **the guesses whose value
> was the widest one its space has.** 91 → 84 across 91 → 84 functions;
> `cmd_fwsettings`, `cmd_namespace`, `cmd_autostart`, `cmd_pidns`,
> `cmd_taskmon`, `cmd_audiomux` and `cmd_sshd` left the ledger. Pinned by
> self-test rung 112.
>
> Every entry left in the ledger is a single site, so from here on a batch is
> chosen by what the guess *means* rather than by how many of them a function
> has. This one is the set where the substituted number is not an arbitrary
> placeholder but the value that means *all of it* or *the top of the tree* — so
> the guess did not narrow the thing the operand named, it **opened** it:
>
> | Command | Guessed | What that value means there |
> |---|---|---|
> | `firewall add … <port> …` | `0` | the `list` arm renders port `0` as `*` — **every port** |
> | `autostart add … [uid]` | `0` | root, and the item runs at boot |
> | `namespace create … [PARENT_ID]` | `ROOT_NAMESPACE` | outside the container it was to nest in |
> | `pidns create [parent]` | `0` | the root PID namespace, then printed back as `(parent: 0)` |
> | `tmon register … [parent_pid]` | `0` | the top of the task tree |
> | `amux stream … [pid]` | `0` | a real and important process (batch 34's harm) |
>
> `firewall add web in tcp 8O allow` is the one to remember: it did not add a
> rule for the wrong port, it added an **allow rule for all 65536**, and printed
> `port 0` in a success line that reads as confirmation.
>
> **Three more §645 word catch-alls went with it, all in that same arm**, and
> all three widening: `_ => Both`, `_ => Any`, `_ => Allow`. Because the arm is
> gated on `parts.len() >= 6`, every operand is required — so none of the three
> was ever standing in for an *absent* word; each existed solely to absorb a
> misspelled one. `blcok` produced an **allow** rule and said "allow". As in
> batch 43a these were invisible to the gate (no `.parse()` in a `match` on a
> `&str`) and were found by reading around a flagged site.
>
> **`cmd_sshd` is the batch's counter-example and was included for it.** Its
> guess was already being caught — `0` is not a listening port and the range
> check below rejected it — so the fix is not a refusal that was missing but a
> *message* that could not say which word was wrong: `Invalid port` answered
> `sshd port 0` and `sshd port 8O22` identically. Worth recording because it is
> the shape a burn-down is most likely to skip as "already fine".
>
> **`cmd_audiomux` also carried the D2 shape next door**: `[output_id]` was
> `and_then(|s| s.parse().ok())` into an `Option`, where an unreadable word is
> indistinguishable from an omitted one, so `amux stream foo 5 1O` routed the
> stream to the default output and reported success.
>
> Rung 112 asserts the converse as well as the refusals — `pidns create 99`
> reaches `pidns::create` and is rejected *there*, by a different message —
> because a rung made only of refusals would pass just as well against a helper
> that refused everything, which is the failure mode a burn-down batch is most
> likely to introduce.
>
> **Burn-down log.** 2026-08-30 (forty-third batch, part b): **the expression
> evaluator**. 95 → 91 across 93 → 91 functions; `eval_test` and
> `tokenize_arith` left the ledger, and **every entry that remains is now a
> single site**. Pinned by self-test rung 111.
>
> This batch is a different shape from the forty-two before it. Those changed
> a call site; this one gave `eval_arithmetic` **an error channel it never
> had**. It returned a bare `i64`, so *every* way of failing — an unreadable
> operand, a literal too big for `i64`, division by zero, an unclosed paren, a
> character outside the language — produced `0` and returned it as an answer,
> with exit status `0` besides. Nothing downstream could tell a computed zero
> from a failed one, so this was §600's prohibited shape sitting under the
> shell's whole expression language rather than under one option.
>
> The whole recursive-descent family (`parse_expr` → `parse_atom`),
> `tokenize_arith` and `eval_test` now return `Result<_, ArithError>`, and the
> eleven call sites each say what a failure means there: `$((…))` substitutes
> nothing, a C-style `for` refuses to start or stops rather than looping to the
> 10,000-iteration cap, `let` leaves the variable at its **previous** value
> instead of overwriting it with the guess, and `test`/`expr` exit **2**, which
> is what separates *malformed* from *false* — without it a typo in `$n` makes
> `[ "$n" -eq 0 ]` true and silently takes the zero branch.
>
> **Nine defects here were outside the ledger's four.** Five had no `.parse()`
> (division by zero in `/` and `%`, `parse_atom`'s `_ => 0`, the swallowed
> unclosed paren, and the tokenizer's silent skip of any unknown character,
> which made `6 & 3` evaluate to `6` where bash says `2`); the remaining four
> were the accepted trailing operands (`1 2` → `1`) and the lone `=`, `&` and
> `|` skips. Same lesson as part (a), from the other direction: the count is a
> floor.
>
> **Where the references disagree, this shell follows dash.** `x=abc;
> $((x))` is `0` in bash and an error in dash; bash's `0` *is* the invented
> value §600 prohibits. On integer overflow the two disagree with each other
> (bash wraps, dash saturates), which is the clearest possible sign that there
> is no right value to substitute — so it is refused. An *unset* variable stays
> `0`, because that is the documented rule everywhere and `$((count + 1))` on a
> fresh variable must be `1`. Recorded as design-decisions.md §646.
>
> One incidental fix: `((x = 5))` used to call `eval_cfor_expr` for the
> assignment and then evaluate the same string a second time for its status,
> which only worked because the tokenizer dropped the `=`. `eval_cfor_expr` is
> now `eval_arith_stmt` and returns its value, so the double evaluation is
> gone — and `$((i = i + 1))`, which previously printed a value without ever
> assigning it, now assigns.
>
> **Burn-down log.** 2026-08-30 (forty-third batch, part a): the rest of the
> **two**-site functions except the expression evaluator's. 131 → 95 across
> 111 → 93 functions: `cmd_netthrottle`, `cmd_pmcstat`, `cmd_policyengine`,
> `cmd_prefetch`, `cmd_printmgr`, `cmd_progmgr`, `cmd_reclaim`, `cmd_seq`,
> `cmd_strings`, `cmd_svcstart`, `cmd_telnetd`, `cmd_tracemon`, `cmd_vmfrag`,
> `cmd_vmguest`, `cmd_volumeosd`, `cmd_wifiscan`, `cmd_windowrules` and
> `cmd_writeback` left the ledger entirely. Pinned by self-test rung 110.
>
> **The tier is now `eval_test 2` + `tokenize_arith 2` and nothing else**, so
> the ledger is 2×2 + 91×1. Those four are batch 43b and are deliberately not
> in this one: they are all inside `eval_arithmetic`, which returns a bare
> `i64` and has eleven call sites, so fixing them is an interface change to
> the shell's expression evaluator rather than a call-site edit like the other
> thirty-six. Splitting the batch keeps a mechanical change and a design
> change out of one commit.
>
> **Seven of the defects fixed here had no `.parse()` in them at all, and so
> were never in the ledger.** They are `match` arms on a *word*, ending
> `_ => SomeDefault`, where the arm for "the operand was absent" and the arm
> for "the operand was misspelled" are the same arm. The gate keys on
> `.parse()` and `from_str_radix`, so it cannot see them; they were found by
> reading the code *around* flagged sites. This is a third, structural reason
> the ledger total is a floor rather than a measurement — the header of
> `scripts/option-refusal-ledger.txt` already records two historical
> under-counts, and this one is different in kind from both: it is not a
> detector bug that can be fixed, it is the detector looking at the wrong
> syntax for this half of the defect family. **The fix pattern** is to spell
> the default out as its own arm and give `_`/`other` a `refuse_operand` that
> lists the valid words, so that absence and unreadability stop sharing a
> branch. Where they were:
>
> | Site | What the `_` arm did |
> |---|---|
> | `wifiscan discover` security | a typo for `wep` filed an unprotected access point as **WPA2-PSK**, and the listing showed it as protected |
> | `wifiscan discover` band | filed the network on **5 GHz**, next to a channel number that band does not have |
> | `progmgr uninstall` option | did a **Full** uninstall — deleting the files the operator had just asked to keep |
> | `progmgr snap` scope | took a **full** snapshot where `settings` or `data` was asked for |
> | `policyengine add` effect | **Allow** — a security policy that fails open and reports success |
> | `policyengine add` category | filed the rule under **System**, where it never matches the requests it was written for and may match ones it was not |
> | `pmcstat sample` event | filed the reading against **Cycles** whatever counter was named |
>
> `policyengine`'s is recorded in the code with its own reasoning: the obvious
> "safer" repair is to flip the fallback to `Deny`, and that is rejected there,
> because it would still act on a word nobody typed, and a policy engine that
> quietly denies is a machine that mysteriously stops working.
>
> **Three older shapes recurred, and two guards-by-luck were retired.**
> `telnetd kick` was batch 42's `usize::MAX` sentinel again, reaching the
> operator as *"Session #18446744073709551615 not found or inactive"*.
> `tracemon read` was a limit that truncates evidence — `read 5O` returned
> twenty events and printed `Trace events (20):`, so the count in the header
> was the guess, not the buffer. `vmfrag update` and `writeback dirty` were
> measurements being filed: both write a number the operator supplied into a
> statistics table, where an invented one is indistinguishable from a reading
> the kernel took itself, and `vmfrag`'s guess of `0` means *no
> fragmentation*. The guards-by-luck were `telnetd port` and `vmguest resize`,
> where the guessed `0` happened to be the one value a following range check
> rejects — a refusal nobody designed, and the identical line in a command
> whose default is a *valid* number would have gone through silently.
>
> **`windowrules action` is the batch's sharpest number**, because its meaning
> is decided by the *previous* word: `p1` is an x-coordinate for `position`
> and a percentage for `opacity`, so guessing `0` snapped a window to the
> screen corner in one case and made it invisible in the other — and the rule
> fires later, when nobody is watching the shell. The same arm also answered a
> mistyped *rule id* by reprinting the syntax, which tells an operator who got
> the syntax right to go and re-read it; absence still gets the usage block,
> and an unreadable word is now named. `cmd_seq` had the same split, twelve
> lines apart: `seq 1O` refused while `seq 1 1O` guessed `1` and printed one
> line, so `for i in $(seq 1 $n)` ran once with nothing on stderr to say why.
>
> **Burn-down log.** 2026-08-30 (forty-second batch): the **two**-site
> functions, alphabetically as far as `cmd_netns` — twenty-nine of the
> forty-nine. 189 → 131 across 140 → 111 functions: `cmd_audio`, `cmd_audit`,
> `cmd_backupsched`, `cmd_cal`, `cmd_cgroup`,
> `cmd_clipsync`, `cmd_cursorsettings`, `cmd_deskicons`, `cmd_dmastat`,
> `cmd_drvmon`, `cmd_entropy`, `cmd_envvars`, `cmd_eyeprotect`,
> `cmd_filetransfer`, `cmd_fwupdate`, `cmd_gamemode`, `cmd_hdrdisplay`,
> `cmd_hexdump`, `cmd_hwrng`, `cmd_iolatency`, `cmd_iotdevice`, `cmd_kbmacro`,
> `cmd_ksmstat`, `cmd_lavg`, `cmd_mdns`, `cmd_mediakeys`, `cmd_netdiag`,
> `cmd_netlat` and `cmd_netns` left the ledger entirely. Pinned by self-test
> rung 109.
>
> **Unlike batches 40 and 41, this one does not exhaust its tier**, and the
> distinction is worth keeping rather than tidying away: twenty two-site
> functions remain (`cmd_netthrottle` through `tokenize_arith`), so the ledger
> is 20×2 + 91×1. Batch 43 is those twenty, after which the tier really is
> empty and every remaining function carries exactly one site. Recording the
> cut-off by *name* rather than as "the two-site batch" is what stops the next
> reader trusting the tier header over the ledger — the ledger is the count,
> and it says forty sites are still there.
>
> **This batch changed the shared printer, which no previous batch had to.**
> `cal` and `hexdump -n` take their operands *directly*: there is no subcommand
> word between the command and the number. Passing `""` as the subcommand
> printed `cal: : \`1O' is not a month`, an empty middle field that reads as a
> defect in the shell rather than as an absence. All ten family printers now go
> through one `refuse_operand(cmd, sub, format_args!(…))` that collapses the
> field when there is no subcommand. Recorded as design-decisions.md §644,
> which also covers the two changes this forced on
> `scripts/check-selftest-wording.py`: `format_args!` had to join its list of
> print macros (a deferred print is still a print, and without it ~40 *correct*
> rungs were reported as failures), and five older needles that spanned both
> halves of the message were shortened to the body-only form the other ~40
> refusal rungs already use — rather than teaching the gate to compose across a
> helper boundary, which its own header forbids.
>
> **Two new shapes, on the "where the guess ends up" axis:**
>
> * **A sentinel chosen because it "cannot exist" — which the operator then
>   reads.** `mdns unregister <idx>` guessed `usize::MAX`, and the not-found
>   path printed it: *"Service #18446744073709551615 not found or inactive"*.
>   `netns ifconfig <ns>` did the same with `u32::MAX`. The sentinel is picked
>   to be unmistakable *to the code*, and it is — but nothing stops it reaching
>   the screen, where it is a number the operator never typed and cannot map
>   back to what they did type.
> * **The different combinator: `filter_map(…ok())`, which drops rather than
>   guesses.** `iotdevice group <name> 1,2,e` built a group of two devices from
>   a list of three and reported success. It is the same defect one step
>   further along — the unreadable word does not become a wrong value, it
>   becomes *no* value, and the resulting object is quietly short. Nothing in
>   `unwrap_or` grep-space would have found it; it was next to a site that was
>   in the ledger. The whole list now fails if any member is unreadable.
>
> **The sharpest single site was `fwupdate apply <id>`**, which guessed device
> `0`, flashed it, and printed "Reboot required". Firmware written to the wrong
> device on a typo is the one consequence in forty-two batches that is worse
> than `directio`'s overwritten file head, because it is not confined to a
> filesystem.
>
> **Also in the batch, on the earlier axes:** *filed as a measurement* —
> `lavg update` (folded into a moving average that outlives the command),
> `netlat rtt`/`proc` (fabricated samples folded into percentiles),
> `entropy add`/`drain` (over-credits, then spends, the entropy pool);
> *binds a resource* — `mdns register <port>` advertised a guessed port to the
> whole link; *acts on the wrong object* — `dmastat register`, `ksmstat
> register` (pid 0 is the kernel's own), `envvars expand`, `eyeprotect
> enable`/`disable`, `iotdevice set`, `kbmacro event key`. `mediakeys register`
> defaulted to **pid 1**, init — the one process on the machine that is never a
> media player. `cgroup io-limit`'s guessed `0` was the *unlimited* sentinel, so
> a typo did not set a wrong cap, it removed the cap.
>
> **Burn-down log.** 2026-08-30 (forty-first batch): every function carrying
> **three** sites, which is now none. 267 → 189 across 166 → 140 functions;
> twenty-six functions left the ledger entirely — `cmd_audioeq`,
> `cmd_buddyinfo`, `cmd_cgroupfs`, `cmd_directio`, `cmd_display`,
> `cmd_displaycal`, `cmd_dpiscaling`, `cmd_fault_inject`, `cmd_fontpreview`,
> `cmd_httpd`, `cmd_ipclog`, `cmd_memcg`, `cmd_msivec`, `cmd_netindicator`,
> `cmd_netmon`, `cmd_netusage`, `cmd_pftrack`, `cmd_power`, `cmd_raidmgr`,
> `cmd_rcustat`, `cmd_surroundsound`, `cmd_taskbar`, `cmd_timezone`,
> `cmd_ttystat`, `cmd_vmballoon` and `parse_datetime_to_ns`. Pinned by
> self-test rung 106.
>
> **The batch-40 defect did not recur, and the reason is worth writing down
> rather than claiming as care.** Batch 40 mislabelled five diagnostics by
> search-and-replacing a statement that was not unique to the function being
> edited. Batch 41 anchored every edit on an adjacent distinguishing line —
> almost always the `match module::fn(…)` immediately below — and the edit tool
> *rejected* three of them outright as ambiguous (`cmd_pftrack`'s `top`,
> `cmd_memcg`'s three byte counts, `cmd_netmon`'s three ids). The mechanism
> that prevented the repeat was the tool refusing a non-unique match, not
> vigilance; a batch done with a script that takes the first match would have
> reproduced batch 40 exactly. Whoever automates this should make ambiguity an
> error, not a choice.
>
> **A new axis, and the one rung 106 is sorted by: where the guess ends up.**
> The three shapes named by batch 40 (guards that worked by luck, strict beside
> guessing, limits that truncate evidence) sort this defect by *how the old code
> survived review*. Batch 41 makes a second sort worth having — by what the
> invented number touches, because that decides how long it outlives the
> command:
>
> * **It reaches a file.** `directio write <path> <data> [offset]` guessed
>   offset `0`, so a mistyped offset did not fail to write — it wrote over the
>   **head** of the file and reported "DIO wrote N bytes". Nothing later
>   distinguishes those bytes from data. This is the only site in forty-one
>   batches where the guess is persisted.
> * **It binds a resource.** `httpd start` / `httpd tls` guessed `8080` / `443`
>   behind an `if port == 0` guard — the batch-40 "guard that worked by luck"
>   shape, but *inverted*: the default here is legal, so the guard was
>   unreachable for exactly the input it looks like it exists for. The guard was
>   kept (an explicit `httpd start 0` still deserves it); what changed is that
>   the unreadable word no longer arrives at it wearing a legal value.
> * **It is filed as a measurement.** `ttystat read`/`write` (cumulative device
>   totals), `buddyinfo update`/`split`, `rcustat end`, `ipclog` — a guessed
>   count is indistinguishable from an observed one the instant it lands, and
>   every later `stats` reading is quietly wrong by that much. Third batch
>   running in which this is the largest sub-group.
> * **It is echoed back as confirmation.** `displaycal red|green|blue` (all
>   three default `220` and print the value back), `netindicator report`,
>   `surroundsound calibrate`. The operand the caller can check against the
>   output is the one that was made up — so the output *confirms the guess*.
>
> **The sharpest single site was not any of those:** `timezone detect <lat>
> <lon>` guessed `0.0` for either coordinate, so `timezone detect 51.5 0.1O`
> answered for the Gulf of Guinea and printed a timezone name with no hint that
> half its input had been discarded. A wrong answer shaped like a right one is
> what the whole campaign is about.
>
> **Two sites had no guard at all behind the guess**, i.e. not even luck:
> `cgroupfs mem <path> <bytes>` silently set the memory cap to **0** for an
> unreadable byte count, and `netmon close <id>` closed connection **0**. Both
> are act-on-the-wrong-object rather than report-the-wrong-number.
>
> **One entry in this batch is not a command arm**, and it is the one that
> generalises: `parse_datetime_to_ns` parsed the *date* fields strictly
> (`.ok()?`, propagating to `touch`'s existing "invalid date" refusal) and the
> *time* fields with `.unwrap_or(0)` — strict and guessing inside a single
> function, six lines apart. `touch -d '2026-08-30 12:3o:00'` meant 12:00:00
> exactly. It also now refuses a fourth colon-separated field instead of
> dropping it. Investigating it surfaced a separate defect in `cmd_touch`,
> logged below as `A-TOUCH--D-CANNOT-ACCEPT-THE-FORMAT-ITS-OWN-USAGE-LINE-PRINTS`.
>
> **Burn-down log.** 2026-08-29 (fortieth batch): every function carrying
> **four** sites, which is now none. 332 → 267 across 185 → 166 functions;
> nineteen functions left the ledger entirely. `cmd_webcam`, `cmd_vmzone`,
> `cmd_bpfstat`, `cmd_defaultapps`, `cmd_diskstat`, `cmd_displayarrange`,
> `cmd_epollstat`, `cmd_eventlog`, `cmd_kbsettings`, `cmd_kthread`,
> `cmd_netqueue`, `cmd_netsyslog`, `cmd_procstat`, `cmd_sysresource`,
> `cmd_upnp`, plus the pid arms of `cmd_filelock`, `cmd_netsock`,
> `cmd_pipestat`, `cmd_schedclass` and `cmd_taskstats`, which share the
> statement text. Pinned by self-test rung 104.
>
> Three shapes recur often enough to be worth naming for whoever takes the
> next batch:
>
> * **Guards that worked by luck.** `netsyslog port`, `procstat get`/`register`
>   and `upnp add` *did* reject a mistyped number — but only because the
>   guessed default was `0` and a separate guard rejects `0`. That collision is
>   accidental. The same code with a legal default (say `514`) would have
>   accepted the typo in silence, and the shared message named neither mistake.
>   A site that looks defended is not necessarily defended *on purpose*, and
>   the gate cannot tell the difference — it counts the `unwrap_or`, not the
>   guard three lines down.
>
> * **Strict and guessing on the same line.** `kthread state <id> <running|…>`
>   refused an unrecognised state word while silently rewriting the id beside
>   it to 0. `netsyslog forward <ip> [port]` parsed and refused a malformed
>   address, then invented the port and printed it back as confirmation — so
>   the one operand the caller could check against the output was the one that
>   had been made up. Where one operand is strict, the reader assumes the arm
>   is strict.
>
> * **Limits that truncate evidence.** `eventlog recent/errors/source/category`,
>   `netsyslog recent`, `procstat topcpu/topmem`, `sysresource avg/history`. A
>   short list reads exactly like a complete one, and `sysresource avg` printed
>   the invented window size into its own caption ("Average CPU (last 10
>   samples)"). These read as harmless — nothing is written — but they are the
>   sites most likely to be believed.
>
> And, as in batch 39, several operands here are not settings but
> **measurements being filed**: `diskstat read/write`, `netqueue rx/tx`,
> `sysresource sample`, `bpfstat run`. Each is added to a cumulative counter
> and read back later as though observed, so a guessed number is
> indistinguishable from a real one the moment it lands. Refusing the typo is
> the last point at which the difference still exists.
>
> **The batch also produced a defect of its own, and a gate for it** — see
> `A-KSHELL-A-DIAGNOSTIC-CAN-NAME-A-COMMAND-IT-DID-NOT-COME-FROM` below. Five
> of the twenty functions were converted by a whole-file search-and-replace of
> a statement that was not unique to the function being edited.
>
> **Burn-down log.** 2026-08-29 (thirty-ninth batch): the batch that was not
> about this defect at all, and moved its count anyway. Batch 39 went after a
> *third* shape of §600 — an operand read as a number and dropped in **silence**
> when the word would not read, with no message, no default and no non-zero
> exit — tracked separately as
> `A-KSHELL-AN-OPERAND-READ-AS-A-NUMBER-AND-DROPPED-IN-SILENCE`. 22 sites
> converted there. Three of them turned out to *also* carry a guessed value in
> the same few lines (`mlink battery`'s percentage, `qs set`'s two operands), so
> this heading fell 335 → 332 across 186 → 185 functions as a side-effect.
>
> The general point is worth more than the three sites: **the shapes co-locate.**
> An arm careless enough to drop one word silently is the same arm that guesses
> at the next one, so reading an arm to fix either shape tends to surface the
> other. That argues for finishing this backlog arm-by-arm rather than
> pattern-by-pattern — the gate can only sort by pattern, but the defects are
> distributed by author attention.
>
> **Burn-down log.** 2026-08-29 (thirty-eighth batch): `cmd_iomem` (5),
> `cmd_ioport` (6), `cmd_vmmap` (6), `cmd_kprobes` (4), `cmd_pciids` (4),
> `cmd_usbpolicy` (3), `cmd_sysrq` (1) and `cmd_gpu` (1) cleared — 30 sites
> across 8 functions. Pinned by self-test rung 102.
>
> **The counts moved differently from every previous batch, and that is the
> finding.** The heading went 346 → 335, a fall of 11, while 30 sites were
> actually fixed. Both numbers are right. The gate had been reporting 346 when
> the true figure was **365**: `scripts/check-option-refusal.py` matched a
> literal `.parse()`, so the base-16 spelling of the identical defect —
> `u16::from_str_radix(v, 16).unwrap_or(0)` — was **invisible to it**. Nineteen
> sites across eight functions had never been seen, let alone exempted. So the
> real movement is 365 → 335 across 194 → 186 functions, and every earlier
> total printed by this gate was an undercount by the same 19.
>
> **In short:** a gate that measures a debt can only be trusted about the part
> of the debt it can see, and nothing in its own output says which part that is.
> This one had been green for months over code it was written to catch. The
> detector was extended in the same change that paid the debt it was hiding, so
> the ledger gained nothing from the discovery.
>
> * **How it was found, which is the part worth reusing.** Not from the total —
>   346 looked exactly as healthy as 365 would have. It was found by picking two
>   functions that *ought* to have been in the ledger, `cmd_pciids` and
>   `cmd_sysrq`, and noticing they were not merely un-exempted but **absent**.
>   An entry claiming too many sites is already reported by the checker (that is
>   what the `stale` arm is for); an entry that was never written cannot be. The
>   only audit that catches this is to go looking at a specific function rather
>   than at the count — the same lesson as
>   `A-KSHELL-THE-OPTION-GATE-COUNTS-ONE-LINE-AND-RUSTFMT-USES-FOUR`, where the
>   gate was matching lines and `cargo fmt` had wrapped the statements it wanted.
>
> * **`cmd_sysrq` continues the thirty-seventh batch's theme exactly, and is the
>   most severe site in this one.** `sysrq mask` narrows what the magic SysRq key
>   is permitted to do; it has no other purpose. The guess was
>   `unwrap_or(0x7F)`, and `kernel/src/fs/sysrq.rs:205` documents `0x7F` as
>   "All categories enabled". So a mistyped mask did not narrow the permission
>   set, it **opened all of it** — reboot and crash included — and printed
>   `SysRq mask set to 0x7f` as though that had been the request.
>
> * **`cmd_sysrq` also carried a different defect the gate cannot see at all,
>   and it is worse.** The five key operands were read with
>   `parts.get(1).and_then(|s| s.chars().next())`, which does not read a
>   character — it reads the first one and discards the rest. The sysrq keys are
>   single letters naming destructive actions, so `sysrq trigger boot` was obeyed
>   as `b` (reboot) and `sysrq trigger info` as `i` (kill every process). This is
>   not a guess but a *truncation*, and it is more convincing than a guess
>   because the letter acted on is one the operator really typed. Fixed with a
>   new `required_key` helper; the same shape had already been fixed once for
>   `find -type`, where the valid set is closed and a whitelist sufficed.
>
> * **`cmd_usbpolicy` is the third instance of "the guess is the value that
>   restricts nothing", reached by a third idiom.** A rule's vendor and product
>   ids are `Option<u16>`, and `kernel/src/fs/usbpolicy.rs:203` matches with
>   `rule.vendor_id.is_none_or(|v| v == vid)` — so `None` does not mean "unset",
>   it means **match every vendor**. The old `…from_str_radix(..).ok()` turned a
>   mistyped `vid=` into exactly that: a rule aimed at one device silently became
>   a rule matching all of them, and with `decision=deny` that is every USB
>   device on the machine. Note this site spells no `unwrap_or` at all, so the
>   extended detector still does not see it; it was found by reading the
>   function while fixing its neighbour.
>
> * **Two catch-alls in the same command, both of which changed the meaning
>   rather than defaulting it.** `parse_usb_decision` ended in
>   `_ => Decision::AskUser`, so `usbpolicy add r denny` installed an *ask* rule
>   where a *deny* was meant and reported it added — and `usbpolicy default`,
>   which sets the decision for every device matching no rule, ran through the
>   same parser. `parse_usb_class` ended in `_ => UsbClass::Other`, which is not
>   a fallback but **a real class that real devices belong to**, so
>   `class=stroage` produced a rule quietly governing the wrong class. Both now
>   return `Option`, and `Other` is still reachable — by spelling it.
>
> * **The address and id functions are the wrong-object mutation, as `cmd_cpuset`
>   was in the previous batch, but with a sharper edge: zero is not a
>   placeholder in any of these spaces.** Port 0 is a real port, base address 0
>   is a real region, pid 0 is a real process, vendor id 0 is a real id. So
>   `iomem unregister 0xFFFG` did not fail — it unregistered whatever was at base
>   `0` and answered `iomem: unregistered 0x0`, echoing back the same
>   hexadecimal that had just been mistyped. `kprobes register` is the same
>   shape at its worst: it installed a probe on the **null address** and printed
>   `Registered probe id=N` with a genuine id, which is precisely what let the
>   mistake survive — the operator has a probe, it is simply not on the function
>   they named, and nothing in the output says so.
>
> * **`cmd_pciids` is the query case, and queries are not the mild case.** A
>   mistyped vendor id became `0` and the command answered `Vendor: Unknown` —
>   a real answer, about a vendor that was never asked about, and
>   **indistinguishable from the correct answer for a vendor genuinely not in
>   the database**. A lookup that cannot read what it was asked to look up has
>   to say so, or neither of its two outcomes can be trusted.
>
> * **`cmd_gpu fill` was fixed on both routes and then reordered.** A bad `0x`
>   literal and an unlisted colour name both ended at the same default blue, and
>   on a command whose entire output *is* the screen a wrong fill looks exactly
>   like a right one. The argument is now validated *before*
>   `virtio::gpu::is_available()` is consulted: a word the shell cannot read is
>   wrong on a machine with no GPU exactly as on one with a GPU, and the old
>   order would have made the diagnostic depend on whether QEMU was started with
>   `-device virtio-gpu-pci` — the §604 trap, in the self-test as much as in use.
>
> * **New helpers.** `required_hex` (the `required_num` of base 16) and
>   `required_key` (an operand that must be exactly one character) join
>   `optional_hex` from the previous batch; `FromHexStr` gained a `u8` impl for
>   `pciids`' class and subclass bytes.
>
> **Not fixed here, and deliberately.** `cmd_iomem`'s `name` and `cmd_ioport`'s
> `name` previously defaulted to the placeholders `"DEV"` and `"PORT"` when
> absent; both are now required, because a region registered under a name nobody
> chose is unfindable in the `list` output that is the only way to see it. The
> remaining `unwrap_or` defaults in these functions — `iomem register`'s size of
> `0x1000` and `ioport`'s access width of `1` — are genuine documented defaults
> for *absent* operands and are kept, now routed through `optional_hex` /
> `optional_num` so a *present but unreadable* operand is refused instead.

> **Burn-down log.** 2026-08-29 (thirty-seventh batch): `cmd_firewall` (4),
> `cmd_parentaltime` (4), `cmd_service_limits` (4) and `cmd_cpuset` (4) cleared —
> 362 → 346 across 194 → 190 functions. Pinned by self-test rung 101.
>
> **In short:** these four were batched together because they share a property
> no previous batch had, and it is the one that makes this class dangerous
> rather than merely untidy. **In all four, the value that was guessed is the
> setting that enforces nothing.** A firewall rule with no port matches every
> port; a screen-time limit of zero minutes is not a strict limit but an absent
> one; a service quota of zero is printed by the command itself as
> `unlimited`. So the substitution did not produce a *wrong* restriction, which
> a user might eventually notice — it produced *no* restriction, and then
> printed the success line that says the restriction was applied.
>
> * **`cmd_firewall` is the most severe instance found in thirty-seven
>   batches**, and the severity was verified in the subsystem rather than
>   assumed. `net/firewall.rs:146` documents `dst_port: 0` as "any port",
>   `:142` documents `src_ip` `0.0.0.0` as "any", and the matcher at `:1467`
>   reads `if rule.dst_port != 0 && rule.dst_port != port` — it honours both.
>   Therefore `fw allow in tcp port 8O8O` (letter O) did not install a rule for
>   a wrong port and did not fail: **it opened every inbound TCP port** and
>   printed `Rule 3 added`. There were four distinct ways to reach that state,
>   not one: the unreadable port; the unreadable address (`ip 10.0.0.256` →
>   accept from anywhere); `prefix_part.parse().unwrap_or(32)`; and — the
>   quietest — `_ => { i += 1; }`, which discarded an unrecognised keyword *and*
>   left its operand to be re-read as a keyword, so `fw allow in tcp prot 80`
>   (for `port`) threw away both words and installed an any-port rule. Every
>   defect existed twice, because the IPv6 arm is a hand-copied twin; the gate
>   is what found the second copy, after the first was fixed and it reported
>   `2 fewer than expected` instead of 4.
> * **A prefix-length ceiling that never existed.** `/33` on an IPv4 rule (or
>   `/129` on IPv6) parsed as a `u8` and was stored. What a matcher does with a
>   prefix wider than its address is undefined by anything written down here, so
>   this was not a guessed value but an unchecked one — found while fixing the
>   arm, and fixed with it. `0` remains legal: it is "any address" said
>   deliberately, which is exactly the distinction the rest of this batch is
>   about.
> * **`cmd_service_limits` is the clearest instance anywhere in kshell**,
>   because the proof is in the same function. The `set` arm read
>   `val.parse().unwrap_or(0)` four times; the `list` arm, forty lines above,
>   renders a limit of `0` as the literal string `"unlimited"`. So
>   `slimit set web rss=1O24` removed the limit and then printed the limits back
>   with `unlimited` in the column the operator had just tried to fill.
> * **`cmd_parentaltime` is the one where nobody is placed to notice.** A
>   parental control restricts one person and is read by another, so a silent
>   failure has no observer. `parentaltime::record_usage` gates enforcement on
>   `daily_limit_minutes > 0`, so `ptime limit 1 6O` printed
>   `Daily limit set to 0 min` — a line that reads like a total lockout and
>   means the opposite. Its `use` operand is the inverse case and is refused for
>   the opposite reason: usage *accumulates*, so a mistyped `ptime use 1 4O`
>   charged 30 minutes that were never spent and reported the invented number as
>   fact.
> * **`cmd_cpuset` is the variant, and is included to mark the boundary.** Its
>   four `id` operands defaulted to `0`, which is not a wider setting but a
>   *different object* — set 0 exists, `cpuset init` creates it, and
>   `cpuset destroy 1O` destroyed it while printing `destroyed id=0`. Its two
>   mask operands are the widening case proper: an unreadable mask became `0xF`,
>   four CPUs nobody asked for, echoed back in the same hex the operator had
>   just typed.
> * **A new shared helper, `optional_hex`.** The mask sites used
>   `…and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0xF)`,
>   an idiom with **8 further occurrences** still in the file. `from_str_radix`
>   is an inherent method rather than a trait one, so there was nothing in
>   `core` to bound a generic on; a three-line `FromHexStr` trait with a macro
>   impl for `u16`/`u32`/`u64` supplies it. Future batches inherit the tool.
> * **An incidental find, fixed in place: fifteen diagnostics named a command
>   that routes elsewhere.** Every usage and error line in `cmd_firewall` said
>   `firewall`, but the dispatcher sends `firewall` to `cmd_fwsettings` — a
>   different subsystem (`fs::fwsettings`) with no `allow`/`deny` at all. The
>   command is `fw`, which is what its own help arm has always printed. So a
>   user who followed the usage text verbatim reached the wrong command. Not a
>   guessed value, but the same failure mode one layer up: output stating
>   something untrue with complete confidence.
>
> **Not fixed here, and deliberately.** The `direction` and `protocol` operands
> of a firewall rule already refuse (`_ =>` prints usage and returns), so they
> were never in this class. And `cmd_fwsettings` — the *other* firewall command
> — was not audited in this batch; it is `fs::fwsettings`, a separate subsystem,
> and pulling it in would have made the batch about two things.

> **Burn-down log.** 2026-08-29 (thirty-sixth batch): `cmd_printf` (5) cleared —
> 367 → 362 across 195 → 194 functions. Pinned by self-test rung 100.
>
> **In short:** every batch before this one was the same shape — a word could
> not be read, a value was invented, and the fix was to refuse. **This one is
> not that shape, in both halves.** The parser was wrong even on words it read
> *successfully*, so a diagnostic bolted onto it would never have found the
> worst case; and the fix is not a refusal, because POSIX requires `printf` to
> print the substituted value anyway. What was missing was the sentence and the
> exit status, not the substitution.
>
> * **The case no error path could ever have caught.** `printf '%d' 010`
>   printed **10**. C prints **8** — `printf`'s integer operands are
>   `strtoimax(s, &end, 0)`, base 0, where a leading zero is octal. The site
>   read `.parse::<i64>().unwrap_or(0)`, and `"010".parse::<i64>()` *succeeds*.
>   Verified on the host, not assumed. **A wrong value that parses cleanly
>   cannot be diagnosed**, so the whole burn-down technique — find the
>   `unwrap_or`, add a refusal — was blind to it. It required replacing the
>   parser. The other two divergences are merely loud, because `parse` errors
>   and so the guessed 0 was at least reachable by a diagnostic: `0x10` is 16 in
>   C and was rejected by `parse`, and `%u` of `-1` is
>   `18446744073709551615` in C (the negation modulo `UINTMAX_MAX + 1`) and was
>   likewise rejected. `printf_scan_integer` now implements base 0 outright —
>   whitespace, sign, `0x`/`0X`, `0b`/`0B`, leading-`0` octal, and `endptr`
>   semantics — mirroring lane B's `userspace/coreutils/src/bin/printf.rs`
>   `scan_integer`, which was written against the same C definition. The two
>   `printf`s must not disagree about what a numeral means.
> * **The fix is diagnose-and-still-print, not refuse.** POSIX: the conversion
>   happens with the value read so far, and only the exit status records the
>   fault. `printf '%d %d\n' abc 5` must print `0 5`. So the zero was never the
>   defect — the defect was that it was *only* the zero. `cmd_printf` never
>   called `set_exit` on any path, so `printf '%d' abc` printed `0` and reported
>   **success**. Three sentences now distinguish the three C conditions:
>   `expected a numeric value` (`endptr == nptr`), `value not completely
>   converted` (digits then junk), `numerical result out of range`. Wording is
>   lane B's, byte for byte, so a script grepping one shell's diagnostic finds
>   the other's.
> * **A *missing* operand stays silent, and that is load-bearing.** POSIX treats
>   it as zero; `""` scans with `consumed == 0 == s.len()`, so
>   `printf_note_fault` returns early. Without that carve-out the diagnostic
>   would fire on `printf '%d\n'` and stop meaning "you typed something I could
>   not read".
> * **The diagnostics are printed after the whole format, not where the fault
>   is found** — forced by kshell having exactly one output stream. See
>   `design-decisions.md` §632; the short version is that
>   `printf '%d-%d' 1 abc` would otherwise emit `1-`, then the complaint, then
>   `0`, splicing a sentence into the middle of the data. GNU interleaves
>   harmlessly only because its complaint is on stderr.
> * **Three layout defects surfaced once the specifier parser was rewritten**,
>   none of them guessed values, all of them silent wrong output:
>   `%05d` of −42 printed `00-42` (Rust's `{:0>5}` is fill-and-align over the
>   whole *rendered* value, sign included; C puts the zeros after the sign and
>   prints `-0042`); `%5x`, `%5X` and `%5o` **dropped the field width entirely**
>   unless zero-padding was also asked for, so `%5x` of 255 printed `ff`; and
>   `-` (left-justify) and `.N` (precision) **were not parsed at all** — the
>   specifier was echoed literally *and* its argument left unconsumed, which
>   desynchronised every conversion after it on the same line. The layout now
>   goes through one `printf_field`, which is what makes the sign/padding order
>   and the two opposite meanings of `.N` (maximum length for `%s`, minimum
>   digit count for `%d`) statable in one place.
> * **Silent data loss, unrelated to any guess.** POSIX reuses the format until
>   the operands are used up: `printf '%s\n' a b c` prints three lines. This
>   printed one and discarded `b` and `c` without a word, exit 0. The reuse loop
>   stops when a pass consumes no argument, or `printf hi a` would print `hi`
>   for ever.
> * **C's space flag is deliberately not accepted.** `% d` is a conversion in C,
>   so `printf '100% done\n'` prints `100 0one` there. Accepting it would have
>   been C-correct and a surprising regression in a shell where that string is
>   overwhelmingly a literal percent sign; `%`-then-space falls through to the
>   literal echo instead, and the doc comment says so rather than leaving the
>   omission to look like an oversight.
> * **The ledger line vanished rather than shrinking.** The new code contains no
>   `.parse()` at all, so `check-option-refusal.py`'s D1 regex has nothing to
>   match and `cmd_printf 5` was deleted from
>   `scripts/option-refusal-ledger.txt` outright.

> **Burn-down log.** 2026-08-29 (thirty-fifth batch): `cmd_widgets` (6),
> `cmd_iperf` (6), `cmd_swapact` (6), `cmd_fsbench` (5), `cmd_datausage` (5) and
> `cmd_elog` (5) cleared — 400 → 367 across 201 → 195 functions. Pinned by
> self-test rung 99.
>
> **In short:** rung 98's batch guessed **0 for a pid**, and the harm there was
> that 0 names a real and important process. This batch is the variant where
> the guessed number is not a value at all but a **sentinel the callee reads as
> "use the default"** — and that is worse, because the substituted run's output
> is *byte-identical* to the run the user would have got by omitting the
> argument entirely.
>
> * **The sentinel cases.** `iperf client <host> <port> 1O` (letter O) became
>   duration 0, which `tcp_client_test` reads as `DEFAULT_DURATION_POLLS`; the
>   test ran to completion and printed a full throughput report. `iperf server
>   <port> 3OO` did the same through `tcp_server_test`'s 3000-poll default.
>   `elog tail 5O` printed twenty events, which is exactly what a bare `elog
>   tail` prints. Every `fsbench` iteration count is the same shape. **There is
>   no observable difference between "you mistyped and I ignored you" and "you
>   said nothing and I used the default"** — so unlike batch 34, where a wrong
>   pid at least appeared in the output, here the transcript cannot be audited
>   after the fact either. Nothing anywhere records that a word was discarded.
> * **`swapact register` is this batch's forge-then-check pair** (the shape
>   first named in batch 29, and the reason "there is a guard downstream" is not
>   a defence). `swapact::register` in `kernel/src/fs/swapact.rs:138` rejects
>   only a full table and a duplicate name — it validates *nothing* about what
>   it is told. So `unwrap_or("/dev/sdb1")` in the shell arm did not merely
>   mislabel an action: a bare `swapact register` **created** a swap area of
>   1,000,000 pages named after a device that does not exist and reported
>   success. `swapact list` then displayed it, and `swapact in /dev/sdb1 100`
>   recorded traffic against it. A guess on a path that cannot fail manufactures
>   the record that makes the next guess look legitimate.
> * **The `_ => SwapType::Partition` catch-all went with it**, though the gate
>   does not count those: a misspelt `zrma` registered a *partition*, and the
>   success line printed `[partition]` — the wrong answer stated in the
>   vocabulary of a right one.
> * **`required_num` vs `optional_num` was chosen by meaning, not mechanically.**
>   Where the operand *names the thing being acted on* it is required —
>   `swapact`'s `<pages>`, `datausage record`'s `<rx>`/`<tx>`, `limit add`'s
>   `<bytes>`, `limit alert`'s `<pct>`, all written unbracketed in their own
>   help. Where the help brackets it and documents a resting value it is
>   optional — every `fsbench` count, every `elog` `[count]`, `iperf`'s
>   `[duration]`. Getting this backwards would turn a required operand into an
>   optional one, which is a behaviour change wearing a bug fix.
>   `datausage limit alert` is the instructive one: 80 *looked* like a
>   documented default and was not, so `alert home 9O` printed "Alert threshold
>   for 'home': 80%" and the operator had no way to see that 90 was not what was
>   set.
> * **Four `parts[N]` indexing sites disappeared as a side effect** in
>   `cmd_datausage` (they were guarded by a `parts.len() >= 4` check, so no
>   panic was reachable today — but the guard and the index were three lines
>   apart and only the helper makes that safety local).
>
> **A wording defect found and fixed alongside, because this batch was about to
> make it worse.** `article_for` picks "a"/"an" from the first *letter*; English
> picks it from the first *sound*; a one-letter noun is precisely where those
> diverge. `cmd_cpick` shipped **"is not a x coordinate"**, and `cmd_wsnap`
> carried a comment saying it had renamed its own nouns to
> "horizontal"/"vertical" *only* to dodge this — so the file held two
> workarounds and one live defect for a single cause, and `cmd_widgets` was
> about to add two more sites of it. `article_for`'s doc comment offers an
> escape hatch (write the article into the noun, `"an x coordinate"`), but that
> hatch does not serve `required_num`, whose *other* message is
> `missing {noun}` — which the hatch turns into "missing an x coordinate". A
> noun that reads correctly in **both** sentences needs no hatch, so
> "horizontal"/"vertical" is now the file-wide convention and `cmd_wsnap`'s
> local dodge is recorded as its origin. Rung 99 pins it; the rung-98-era
> assertion `b"is not a x coordinate"` was updated in the same change.

> **Burn-down log.** 2026-08-29 (thirty-fourth batch): `cmd_pidfd` (5),
> `cmd_userfault` (5), `cmd_oomkiller` (5), `cmd_prociso` (6), `cmd_coredump`
> (5) and `cmd_devfreq` (5) cleared — 431 → 400 across 207 → 201 functions.
> Pinned by self-test rung 98. (The 432 → 431 step is `cmd_energysaver`, cleared
> in passing by the toggle sweep of 2026-08-29 and logged under
> `A-KSHELL-A-TOGGLE-WORD-IT-DOES-NOT-RECOGNISE-MEANS-OFF` rather than here;
> the heading above was left reading 432 at the time, and is corrected now.)
>
> **In short:** all thirty-one sites were the same statement —
> `parts.get(N).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0)` — and in
> twenty of them the operand was a **pid**. `unwrap_or(0)` there is not a
> default. It is a placeholder, chosen because it looked inert, and it is not:
> pid 0 is the idle task.
>
> * **The success line launders the substitution.** Every one of these arms
>   prints the number it used. `pidfd signal l23` printed `pidfd: signal to pid
>   0` — a sentence that is true about what happened and reads as confirmation
>   of what was asked. There is no wording in which a substituted value and a
>   supplied one look different, which is why the fix has to be a refusal and
>   cannot be a better message.
> * **`oomkiller kill` is the sharp end.** A pid the shell could not read
>   became a request to the out-of-memory killer to kill pid 0. Nothing in the
>   transcript distinguishes that from having typed `0`.
> * **`prociso` is the instructive one, and it is why "there is already a guard"
>   is not a defence.** Its six sites *did* refuse an unreadable word. But only
>   as a coincidence: the word became 0, and 0 happened to be an invalid id, so
>   a downstream `if id == 0` caught it — and then printed `Usage: prociso
>   attach <pid> <ns_id>`, blaming the synopsis rather than naming which of the
>   two operands was unreadable. A refusal that rests on a fact about the value
>   space stops refusing the day that fact changes, silently and with no test
>   failing. This is the second shape (after batch 29's forge-then-check pairs)
>   where the *absence* of a visible bug today is load-bearing on an accident.
> * **Three catch-all `match` arms went with them, though the gate does not
>   count those.** `userfault fault <pid> mnor` recorded a *missing*-page fault;
>   `userfault resolve <pid> <ns> cpy` recorded a *zero-page* resolution when
>   `copy` was meant; `devfreq governor <id> powersace` set **ondemand**. Each
>   then named the substitute in its success line. These are §600's second
>   prohibited shape in `match` clothing: the `_ =>` arm is an answer, and a
>   catch-all that returns a value cannot say "I did not understand you".
> * **`oomkiller exempt` was the toggle defect hiding in this batch.**
>   `parts.get(2).copied().unwrap_or("true") != "false"` meant every word that
>   was not exactly `false` **shielded the process from the killer**. That is
>   the failure direction that wedges a machine rather than the one that loses a
>   process, and it is the same asymmetry rung 97 documented for `vpn
>   killswitch`.
> * **One destructive arm.** `coredump cleanup 5O` deleted every dump but the
>   newest **ten** and reported `keeping 10`, which reads as a statement of what
>   was done rather than as a correction of what was asked.
>
> Helper choice was made per site by meaning, not mechanically: `required_num`
> where the operand names the thing being acted on (a pid, an id), and
> `optional_num` only where the help documents a resting value — `coredump
> cleanup`'s keep count, `devfreq register`'s frequency bounds, `coredump
> record`'s synthetic-dump fields. Getting that split wrong would turn a
> required operand into an optional one, which is a behaviour change wearing a
> bug fix.
>
> **Burn-down log.** 2026-08-26 (thirty-third batch): `cmd_prochistory` (6)
> cleared — 438 → 432 across 209 → 208 functions. Not pinned by a rung.
>
> **In short:** `prochistory` is the shell's record of which processes ran and
> how they ended. Six of its operands were guessed when they could not be read,
> and the guesses were **0, 0, 0, 0, 10, 10**.
>
> * **The exit code is the site that matters, and it is a new shape.** Every
>   earlier row in this log is about a guess that names the *wrong thing* — the
>   wrong cpu, the wrong pid, the wrong limit. `prochistory exit 412 l` (a
>   lowercase L for a 1) recorded exit code **0**, and 0 is not merely a wrong
>   code: it is the code that means *the process succeeded*. The row then prints
>   next to whatever `reason` was given, so `Crashed` and `exit=0` can appear on
>   the same line of the crash listing — a record that contradicts itself, in a
>   table whose entire purpose is reading after the fact.
> * **Batch 29's rule again, by a different mechanism.** `record_exit` normally
>   answers `NotFound` for a pid it cannot find, so a guessed pid 0 on the
>   `exit` arm was at least visible. But `record_start` **cannot fail** — it
>   pushes unconditionally — so a guessed pid 0 there *manufactures the running
>   entry* that makes the next guess succeed. Again the forging path and the
>   checking path are one function apart in the same file.
> * **A guessed pid strands a row rather than mislabelling it.** `record_exit`
>   closes the newest entry matching `pid == p && end_ns.is_none()`. An entry
>   filed under a pid nobody will type again can therefore *never* be closed: it
>   shows as RUNNING in `running` and `recent` for the rest of the boot, and
>   only `MAX_HISTORY` eviction removes it. The guess is not correctable by
>   repeating the command correctly.
> * **A word-vocabulary guess that moved a counter.** Not a `.parse()` site, so
>   the gate never counted it, but it was the same defect: `parse_exit_reason`
>   fell back to `ExitReason::Unknown` for any word outside its vocabulary.
>   `Unknown` is a reason a caller can legitimately *mean*, so "did not know"
>   and "typed something unreadable" became the same record. Worse,
>   `record_exit` increments `total_crashed` only for `Crashed` and
>   `OutOfMemory` — so a mistyped `crahsed` filed the entry as Unknown *and*
>   left the crash count one short, after which `stats` and the `crashed`
>   listing disagree with nothing to say which is right. It now returns
>   `Option`, `unknown` is in the vocabulary explicitly, and the refusal names
>   the whole vocabulary.
> * **The two `[n]` limits are the quiet ones.** `recent 5O` printed ten rows,
>   indistinguishable from a successful `recent 10`: nothing in a truncated
>   listing tells you whether the number you typed is the number that truncated
>   it. `crashed [n]` is the same and worse to get wrong, being the listing an
>   operator reads to decide whether a crash happened at all.
>
> The help arm moves to `end_help_arm` (so `prochistory help` exits 0 while an
> unknown subcommand exits 1) and gained a line naming the `reason` vocabulary.
> The `cmd_prochistory` ledger line is deleted rather than zeroed, since a count
> larger than the number of real sites is itself reported by the gate.
>
> **Burn-down log.** 2026-08-26: `cmd_irqstat` (1) cleared — 439 → 438 across
> 210 → 209 functions. **Not a burn-down; a side effect, and worth recording as
> one.** The `irqstat register <num> <name>` subcommand guessed `0` for an
> unparseable IRQ number and `"irqN"` for a missing name. It was not fixed — it
> was *deleted*, along with the whole mutable table it wrote into, when
> `fs::irqstat` became a projection of `idt::vector_counts()` (see
> `A-IDT-COUNTS-ONLY-EXCEPTIONS-AND-CALLS-THE-TOTAL-INTERRUPTS`). There is no
> longer a table to register a line in, so the arm has no meaning to restore.
> The general lesson: a guessed operand is sometimes a symptom of a command that
> should not exist, rather than of a missing parse. `check-option-refusal.py`
> found this on its own — the ledger entry claiming a site that no longer existed
> failed the gate, which is the behaviour its header advertises and the reason
> the counts are kept per-function rather than as a global total.
>
> **Burn-down log.** 2026-08-26 (thirty-second batch): `cmd_signalq` (6)
> cleared — 445 → 439 across 211 → 210 functions. Not pinned by a rung.
>
> **In short:** `signalq` tracks signals queued to processes. Where it could not
> read a number it used **pid 0** and **signal 0**. Two of its six guesses were
> unlike anything earlier in this burn-down, and fixing the arms surfaced two
> defects that are not guesses at all (filed separately, above).
>
> * **The purest instance of batch 29's rule yet.** `send` is the only write in
>   this module that *creates* the record it writes to; `deliver`, `block` and
>   `unblock` all return `NotFound` for a pid they cannot find. The forging and
>   the checking are one function apart in the same file. `signalq send` with no
>   arguments manufactured **pid 0** and queued it a `DivideError` — after which
>   `deliver 0`, which had nothing to find a moment earlier, succeeded.
>
>   **The auto-create is not the bug here, and was deliberately left.** Once the
>   fabricated seed goes (see the entry above) there is no `register` arm and no
>   other way for a process to enter this accounting, so creating on first
>   signal is the intended entry point. The guessed key was doing all the damage
>   by itself — which is a useful sharpening of the rule: *what makes a
>   create-on-write path dangerous is not that it creates, but that it can be
>   reached with a key nobody supplied.*
> * **A guessed default that is not "unspecified" but a specific alarming
>   fault.** `send`'s `sig_num` fell back to `0`, and `0` maps to `DivideError`.
>   A mistyped signal number therefore filed a divide-by-zero against a process
>   rather than recording something obviously blank.
> * **`pending`'s guess produced a reassuring answer to a question nobody
>   asked.** `pending` on a nonexistent process returns an empty list, so the
>   arm printed `No pending signals for pid 0.` — well-formed, calm, and about
>   the wrong process. `deliver`'s is the opposite and the worst of the six: it
>   is destructive and unrepeatable, clearing the entire pending queue and
>   folding the count into `total_delivered`, so a guessed pid did not misreport
>   a backlog, it *discarded* one.
>
> **The missing subcommand was the undo, for the fourth batch running.**
> `signalq::unblock` was reachable only from the module's own self-test, so
> `blocked_mask` was monotonic from the shell — a signal could be blocked and
> never unblocked. With rqstat's creation (batch 30) and zramstat's discard
> (batch 31), that is three consecutive batches plus this one, which is enough
> to stop calling it a coincidence:
>
> > **The subcommand that is missing is the one that undoes.** These arms were
> > written to demonstrate a feature, and demonstrating means showing a counter
> > go up. The operation that brings it back down has no demo value, so it was
> > never wired — and the column it governs is monotonic by construction, which
> > looks like data rather than like a gap.
>
> This is now a *screen* in its own right, and a cheaper one than batch 29's:
> for each remaining module, list its `pub fn`s and check every mutator has a
> shell arm. An unreachable `pub fn` in one of these accounting modules is the
> signature.
>
> **Burn-down log.** 2026-08-26 (thirty-first batch): `cmd_zramstat` (6, plus
> one uncounted) cleared — 451 → 445 across 212 → 211 functions. Not pinned by
> a rung; nothing asserts on `zramstat` shell output. Predicted by the batch-29
> screen, like batch 30.
>
> **In short:** `zramstat` tracks compressed swap — RAM used as a swap device,
> with the pages squeezed on the way in. Where it could not read a number it
> used **device 0**, a **2 GB** disk size, and for a write, **4096 bytes in,
> 2048 out**. Three things here were not in earlier batches.
>
> * **The guessed id was a hit, not a miss.** Every previous batch's guessed id
>   was eventually caught by `NotFound` — the wrong key usually names nothing.
>   Not here: `create_device` hands out ids from `next_id`, counting up from 0,
>   so **device 0 is whichever device was created first**, and it exists
>   whenever anything exists. This is the complement of batch 29's rule rather
>   than an instance of it:
>
>   > **Where ids are assigned sequentially from zero, a guessed id needs no
>   > forged receipt — it lands on the oldest real record.** The safety net is
>   > not weakened, as in batch 29; it is simply never reached.
>
>   That matters most for `remove`, which is destructive: a mistyped id deleted
>   the first device and every counter it had accumulated, and printed
>   `removed 0` as success.
> * **The guessed pair was the reassuring answer.** 4096/2048 is exactly
>   **2.00x**, which is what a healthy zram is supposed to show. A mistyped
>   write therefore did not read as anomalous — it read as evidence the
>   compressor was working. That is the `cputhr` row (*the guessed default is
>   always the reassuring value*) landing on a ratio rather than a rate. And
>   4096 is the wrong page size for this OS, which uses 16 KiB pages, so it was
>   also batch 29's borrowed-from-Linux mistake a second time. `compr` is the
>   denominator of that ratio in both `devices` and `compression_ratio_x100`,
>   so `[compr]` became `<compr>` under batch 28's §607 override. `orig`
>   additionally selects a *classification* — `record_write` files the write as
>   a zero page when `orig_bytes == 0` — so an unreadable size could put a
>   write in the wrong category as well as at the wrong magnitude.
> * **The uncounted name guess contradicted the record it created.** `create`
>   fell back to the name `"zram1"` while the id counter starts at 0, so the
>   first `zramstat create` produced **device 0 named "zram1"** — and the
>   module's own self-test names its fixture `zram0`, so the shell and the
>   module disagreed about what to call the same device. A wrong name is not a
>   wrong cell in a device table; the name is the thing that distinguishes one
>   device from another.
>
> **The §607 override now has a rule instead of two precedents.** Batch 28
> promoted `[size]` to `<size>` because it was a denominator; this batch also
> promoted `create`'s `[size]`, which is *not* one. What both actually share:
>
> > **An operand that says what a created object IS has no defensible default**,
> > because the default asserts a fact about a device nobody described. Ratios
> > and capacities are read as configuration, not as the shell's opinion.
>
> So `create`/`register` subcommands take required operands regardless of
> brackets, and the mechanical read-the-brackets rule continues to govern
> everything else.
>
> **A missing subcommand again, and again a structural one.** `zramstat` had no
> `discard` arm, so `record_discard` — the only operation that returns memory
> to the pool — was reachable only from the module's own self-test, and the
> shell's `mem=` column was **monotonic by construction**. Batch 26/27's
> generalisation for the third batch running; as in batch 30 the missing
> operation was not cosmetic. Added, with its `bytes` operand required, since
> `mem_used.saturating_sub(bytes)` corrupts in the destructive direction and
> the saturation can clamp to zero — mmapstat's `unmap` row, reached here
> before a default was ever written rather than after.
>
> **Burn-down log.** 2026-08-26 (thirtieth batch): `cmd_rqstat` (6) cleared —
> 457 → 451 across 213 → 212 functions. Not pinned by a rung; nothing asserts
> on `rqstat` shell output. Batch 29's rule was used as a *screen* rather than
> a description: grepping the remaining modules for a register-like function
> beside an `init_defaults` that seeds an empty `Vec` predicted `rqstat` before
> any of its shell code was read, and the prediction held.
>
> **In short:** `rqstat` tracks each CPU's run-queue — how many tasks are
> queued, how long they wait, how often the scheduler moves one CPU's work to
> another. Where it could not read a number it used **cpu 0**, a **1000 ns**
> wait, and for a migration, the pair **cpu 0 → cpu 1**. Three separate things
> turned out to be wrong here, and only the first is the ledger's.
>
> * **The command had no `register` subcommand at all.** `rqstat init` seeds an
>   empty table and `rqstat::register_cpu` existed in the module with nothing
>   calling it, so `enqueue`, `dequeue`, `balance` and `wait` returned
>   `NotFound` for the entire life of the shell. That is batch 26/27's
>   generalisation again — *a module whose only caller is an interactive
>   command is missing exactly the operation no test needed* — except that this
>   time the missing operation was **creation**. It is also why the guessed
>   `unwrap_or(0)` had never yet done damage: there was no cpu 0 for it to hit.
>   The defect was latent, not absent, and adding the `register` arm is what
>   would have armed it. Fixed together, in that order, deliberately.
> * **`balance` guessed a *relationship*, not a value.** Its two operands had
>   two *different* defaults — `from` fell back to 0 and `to` to 1 — so a
>   mistyped pair did not record a vague or zeroed event, it fabricated a
>   migration between two specific CPUs that never exchanged a task. And it
>   fabricated the **same edge every time**, which is the part that matters:
>   repeated identical errors read as a pattern, and a load-balancer report
>   whose whole purpose is to show which CPUs feed which would show a hot 0→1
>   edge that is an artifact of typing.
> * **`rqstat::record_balance` was a write that could not fail** — a module
>   bug, outside the ledger's scope, found while fixing the shell arm above it.
>   It looked up each CPU with an independent `if let Some(…)` and then returned
>   `Ok(())` unconditionally, so a balance naming a CPU that does not exist was
>   *reported as success* and still advanced `total_balances`. The module's own
>   self-test asserts `enqueue(0).is_err()` before registration, on the stated
>   grounds that "no phantom CPU is created" — `record_balance` was the one
>   operation that broke its own module's contract. It also made the `pushes`
>   and `pulls` columns disagree by construction: a half-applied balance
>   increments one side and not the other, and nothing in the output says so.
>   Now both ids are resolved before either is written, and the self-test
>   checks refusal from **both** directions with no partial effect — one-sided,
>   because an implementation that resolved only the first id would still pass
>   a one-directional test.
>
> **Burn-down log.** 2026-08-26 (twenty-ninth batch): `cmd_mmapstat` (6, plus
> two uncounted) cleared — 463 → 457 across 214 → 213 functions. Not pinned by
> a rung; nothing asserts on `mmapstat` output.
>
> **In short:** `mmapstat` records how much memory each process has mapped.
> Where it could not read a number it used **pid 0** and a **4096-byte** size.
> 4096 is the page size of Linux on x86 — it is *not* this OS's, which uses
> 16 KiB pages. So the default was the right answer to the same question asked
> about a different computer, which is exactly why nobody would look twice at
> it.
>
> **This confirms batch 28's row rather than adding one.** `mmapstat`'s
> `init_defaults` also seeds an **empty** table, so `register`'s guessed pid 0
> manufactures the process that `map`, `unmap` and `protect`'s guessed pid 0s
> then find. Two independent modules, same structure — mutually-corroborating
> guesses are a property of the *shape* (an empty registry plus a
> register-then-record command set), not a coincidence in `kstack`. That makes
> it worth stating as a rule:
>
> > **Where a command can both create a record and write to one, a guessed
> > key is worse than a guessed value, because the create path forges the
> > receipt the write path checks.** The usual safety net — "a bad id gets
> > `NotFound`" — is exactly what stops working.
>
> Two things `kstack` did not have:
>
> * **`record_unmap` subtracts.** It does
>   `total_bytes = total_bytes.saturating_sub(size)`, so the guess corrupts an
>   accumulator in the *destructive* direction, and the saturation can clamp it
>   to zero — destroying the evidence that anything was mis-subtracted. On a
>   16 KiB-page system, unmapping "one page" guessed as 4096 stranded 12288
>   bytes against the process for the rest of the boot.
> * **Two uncounted non-numeric guesses**, the same "plus one uncounted"
>   pattern batch 27 set precedent for. `register`'s name fell back to
>   `"unnamed"` despite the usage line already calling it required — so the
>   `procs` table could hold a process whose name is the one field that would
>   have identified it. And `map`'s type had a `_ =>` arm collapsing onto
>   `Anonymous`, so `mmapstat map 7 16384 fille` recorded an anonymous mapping
>   and printed `type=anon`: a report that is wrong about the single field the
>   operand exists to set. The type operand keeps its documented default when
>   *absent* and is refused when present-and-unreadable — `optional_num`'s
>   distinction, in enum clothing.
>
> **Burn-down log.** 2026-08-26 (twenty-eighth batch): `cmd_kstack` (6) cleared
> — 469 → 463 across 215 → 214 functions. Not pinned by a rung: no self-test
> asserts on `kstack` output, which is itself the reason the guesses survived
> this long.
>
> **In short:** `kstack` records how much of each CPU's kernel stack is in use.
> Four of its subcommands took a CPU number, and when they could not read the
> word you typed they used **cpu 0**. `register` did the same and also invented
> a **16384-byte** stack. So `kstack register l 8192` — a lowercase L for a 1 —
> registered cpu 0 with a 16 KiB stack and said so; the later `kstack usage l
> 900` then found that cpu and recorded against it.
>
> **The new row is about guesses that corroborate each other.** Every previous
> row assumed the guessed value lands in a table that already exists, so the
> wrong row is at least a row someone put there. Here `init_defaults` seeds an
> **empty** CPU list, so `register`'s guessed cpu 0 *manufactures the very CPU*
> that the next three guesses then resolve against successfully. Nothing ever
> returns `NotFound`; there is no dangling reference to trip over. The table is
> not inconsistent — it is **self-consistently wrong**, which removes the last
> witness a reader had.
>
> Two aggravations worth recording, both of which recur elsewhere:
>
> * **16384 is `kconsole`'s `80x25` again** (batch 27's provenance row): it is
>   simultaneously this OS's page size and the textbook kernel stack size, so it
>   reads as documentation rather than as a guess. Worse than `80x25`, though,
>   because `list` *divides* by it — `pct = high_water * 100 / stack_size` — so
>   the guess silently rescales a **derived** column the operator reads as a
>   measurement.
> * **`record_usage(cpu, 0)` is an uncorrectable accumulator** (the `ipcns`
>   thesis applied to a mean): it increments `samples` and adds 0 to
>   `total_used_samples`, so a phantom sample drags the `avg` column down
>   permanently. There is no operation that retracts a sample; the only repair
>   is to discard the whole table.
>
> `overflow` and `guard` had **no usage line at all**, so their operand was
> undocumented as well as guessed — the batch-26/27 generalisation holding
> again, that a module whose only caller is an interactive shell command is
> missing precisely what no test ever needed. Both now have one, and the arm
> uses `end_help_arm` so an explicit `kstack help` exits 0 while an unknown
> subcommand still exits 1.
>
> **Burn-down log.** 2026-08-26 (twenty-seventh batch): `cmd_kconsole` (6, plus
> one uncounted) cleared — 475 → 469 across 216 → 215 functions. Pinned by
> `kshell::self_test` rung 95.
>
> **In short:** `kconsole` manages virtual consoles — the text screens you
> switch between. When it could not read a number you typed it made one up, and
> the number it made up was **80 columns by 25 rows**. That is not an arbitrary
> default; it is what a console *is* to almost everyone who has seen one. So
> `kconsole resize 2 l32 43` — a lowercase L where a 1 belonged — quietly shrank
> a 120-column console to 80 and printed `Resized console 2 to 80x43.`, a
> sentence that no amount of knowing the subsystem lets you recognise as wrong.
>
> **The new row is about where the guessed value comes from, not what it does.**
> Every previous row in this table asked what the guess *is* — a length, a
> ceiling, a selector, an accumulator — and derived the severity from that. This
> one is about the guess's *provenance*. `0` is recognisable as a placeholder
> because nothing means zero on purpose; `80x25` is recognisable as nothing at
> all, because it is the answer the reader would have given.
>
> | Guessed number is a… | What happens | How you find out |
> |---|---|---|
> | **canonical value of its own domain** (`cmd_kconsole resize`) | the wrong size is applied and reported in a sentence that is internally consistent and idiomatic | you don't — the only witness is `kconsole list`, and only if you already knew the old size |
>
> This sharpens the `cputhr` thesis rather than adding to it. That thesis was
> that the guessed default is always the *reassuring* value — the one that says
> nothing is wrong. Here the reassuring value and the *textbook* value coincide,
> which removes the last defence a reader has. Against a guessed `0` you can at
> least ask "why would anything be zero?"; against a guessed `80x25` there is no
> question to ask. A corollary worth carrying forward: **the more plausible a
> default is as documentation, the more dangerous it is as a guess.**
>
> **It is also the cleanest illustration of §607 found so far, and it needs only
> one command to be it.** `create <name> [cols] [rows]` and `resize <id> <cols>
> <rows>` take the *same two operands*, bracketed in the subcommand that merely
> needs somewhere to start and angled in the one that exists for no other purpose
> than to set them. Previous batches had to put two commands side by side to show
> that the usage line, not the operand's type, decides required-versus-optional.
> Here the contrast is four lines apart in the same `match`: `create rung95` with
> no size still means 80x25, `resize 2 132` with no row count is refused, and
> `create rung95 l20` — present and unreadable — is refused just like `resize`.
>
> **The id guess is the misdiagnosis row again** (`cmd_cgmem`, batch 25):
> `init_defaults` seeds ids 1/2/3 with `next_id: 4`, so id `0` is unreachable and
> `kconsole switch l` answered `Error: NotFound`. Safe, but pointed at the wrong
> remedy — "that console does not exist" says *create one*, when the truth says
> *fix the typo*.
>
> **The uncounted one** is `create`'s name, `unwrap_or("ttyN")` — not a
> placeholder the module expands, a console literally named with the letter N.
> It is self-limiting in the way that hides it: `create` rejects duplicate names,
> so the *first* omission succeeds and every one after it fails with
> `AlreadyExists`, which reads as a collision with something the user never
> created. It now says `kconsole: create: missing console name`.
>
> **Rung 95 owns its fixture, and doing so required the module to gain a destroy
> path** — the same discovery as batch 26, in a module that had the same gap for
> the same reason. `kconsole::create` only ever pushed, `MAX_CONSOLES` caps the
> vector at 16, and nothing removed. See
> `A-KCONSOLE-CAN-CREATE-A-CONSOLE-AND-HAS-NO-WAY-TO-DESTROY-ONE`. That this is
> now the second consecutive batch to uncover a missing teardown is itself a
> finding: *a module whose shell command has no destructive subcommand is worth
> checking for a missing destructor*, because nothing else in the system was
> ever the caller that would have needed one.
>
> The rung establishes its premise before relying on it (`2: tty1 type=fb
> 120x40`), pins severity with a before/after equality on both `kconsole list`
> and `kconsole stats` across nine refusals, then proves §607's positive half by
> creating `rung95` with no size operands and asserting the list shows
> `80x25` — and destroys it again, so `/proc/kconsole` does not carry a console
> the boot battery invented.

> **Burn-down log.** 2026-08-26 (twenty-sixth batch): `cmd_fdtable` (6, plus
> one uncounted) cleared — 481 → 475 across 217 → 216 functions. Pinned by
> `kshell::self_test` rung 94.
>
> **In short:** `fdtable` shows and edits the table of open files a process
> holds. Every one of its operands was guessed as `0` when it could not be read.
> The process-id guesses were then caught by a guard and turned into a
> complaint about the command's *shape*; the file-descriptor guesses were not
> caught at all, and `0` is the first descriptor a process ever opens — so
> `fdtable close 42 1O` closed it and said "Closed fd=0 for PID 42".
>
> **This is the first command in the burn-down that had already noticed the
> problem and protected the wrong operand.** Both pid sites read
>
> ```rust
> let pid = pid_str.parse::<u32>().unwrap_or(0);
> if pid == 0 { shell_println!("Usage: fdtable close <pid> <fd>"); set_exit(1); return; }
> ```
>
> which is the **conflation** row (`cmd_shmem`, `cmd_blkread`): one sentinel
> serves as both the guess and the invalidity marker, so the shell reports that
> the *form* of the command was wrong when the form was right and only the word
> was unreadable. The reader's obvious response is to re-type the same shape and
> get the same complaint. Note that the guard was not useless — `open`
> auto-creates a table for whatever pid it is handed, so without it a mistyped
> pid would have built a real fd table for process 0 — it was simply spent on
> the operand that was not doing the damage.
>
> **The new row is the descriptor, and it turns on how the id space is
> allocated.** Batch 25 established that a guessed `0` in `cgmem` was *safe but
> dishonest*, because `next_id` starts at 1 and so cgroup 0 is unreachable. The
> fd table inverts exactly that property: `next_fd` starts at **0**, so the
> guess names the first descriptor the process ever opened — the entry most
> likely to exist and the oldest one in the table. Reachability is not a
> property of the number `0`; it is a property of where the allocator starts.
>
> | Guessed number is a… | What happens | How you find out |
> |---|---|---|
> | **selector in an allocation-ordered id space that starts at 0** (`cmd_fdtable close`) | destroys the process's first and oldest descriptor, reports success | you don't — and no later call can re-issue the number |
>
> **And it is not undoable at the number.** `open` and `dup` both allocate from
> the monotonic `next_fd`, which never goes backwards and never reuses a freed
> value. Re-opening the same path after a mistaken close returns *some later
> fd*. In a table where the number is the interface — 0 being stdin by universal
> convention — that is a permanent renumbering, not a recoverable deletion. This
> is a stronger form of irreversibility than the `ipcns` accumulator row: there
> the *count* could not be repaired, here the *identity* cannot.
>
> **`dup` is the additive twin.** The same guessed `0` there does not remove an
> entry, it manufactures a second alias for the wrong descriptor, consumes a
> number from `next_fd`, and bumps `total_dups` — which has no decrement. So
> closing the surplus alias afterwards still leaves `dups=1` where the truth was
> `dups=0`, the same laundered-audit-trail problem batch 25 found in `cgmem
> charge`. One mistyped word thus reaches both the destructive and the
> accumulating failure modes depending only on which subcommand it lands in.
>
> **The uncounted one** is `open`'s path, `parts.get(2).copied().unwrap_or("")`,
> whose emptiness was folded into the *same* usage line as the pid — so an
> omitted path and an unreadable pid produced byte-identical output. It now says
> `fdtable: open: missing path`.
>
> **Rung 94 owns its fixture, and the first draft did not.** The rung needs a
> descriptor to exist so it can prove the refused `close` did not destroy it.
> The first attempt borrowed the tables `fdtable::self_test` builds earlier in
> the boot battery — and panicked on `FDs for PID 100 (0):`, because that
> self-test *ends* with `*STATE.lock() = None`, precisely so its fixtures cannot
> masquerade as live processes in `/proc/fdtable`. The assertion that caught it
> was the one written to make exactly that assumption fail loudly rather than
> silently test an empty table, so the gate worked as designed; the lesson is
> that a rung must build what it acts on.
>
> Doing that properly required the module to gain a process-exit path, which it
> did not have — see
> `A-FDTABLE-HAS-NO-PROCESS-EXIT-PATH-AND-THE-TABLE-VECTOR-ONLY-GROWS`, a
> genuine leak (256-slot cap, nothing ever freed a slot) that stayed invisible
> until a caller finally needed the operation. The rung now opens one descriptor
> for pid 424242, pins severity with a before/after equality on both `fdtable
> show 424242` and `fdtable stats` — the first proves the descriptor is still
> listed, the second that no dup was tallied — and releases the fixture with
> `fdtable exit 424242`, asserting the released count is exactly 1, which would
> read 0 or 2 if any of the seven refusals had actually run. Neither reader
> takes the `with_state` path, so neither perturbs `ops` and the captures are
> comparable.

> **Burn-down log.** 2026-08-26 (twenty-fifth batch): `cmd_cgmem` (6, plus
> three uncounted) cleared — 487 → 481 across 218 → 217 functions. Pinned by
> `kshell::self_test` rung 93.
>
> **In short:** `cgmem` tracks how much memory each cgroup (a named group of
> processes with a memory ceiling) is using. Bare `cgmem create` invented a
> cgroup called `cg0` with a ceiling nobody chose; a mistyped page count was
> read as 1; a mistyped `rss`/`cache` was read as `rss`; and a mistyped cgroup
> id was answered "no such cgroup".
>
> **Three shapes in one command, each a refinement of an earlier row rather
> than a repeat of it.**
>
> **1. The ids are the familiar misdiagnosis.** `init_defaults` sets `next_id:
> 1` and seeds no cgroups, and `create` only ever hands out `next_id`, so
> cgroup 0 is unreachable. The guessed `0` was therefore *safe* — it removed
> nothing, charged nothing — and dishonest in the same breath: `cgmem remove
> 1O` answered `remove error: NotFound`, which says the cgroup does not exist.
> It does. The word naming it could not be read, and the reader is sent to
> check `list` instead of at what they typed.
>
> **2. The `create` limit is a *policy ceiling*, and this is the new row.**
> Every earlier guessed number was a selector, a measurement, an address, or a
> quantity. This one is the *threshold everything else is compared against*:
> `record_charge` tests `usage_pages > limit_pages` on every call. `cgmem
> create web 5OOOO` built a cgroup limited to 100000 pages — double the
> intended 50000 — and echoed it back as though chosen.
>
> What makes it worth its own row is *why it does not currently hurt*. The
> comparison's only output is `high_events`, which no command prints and no
> accessor exposes (`A-CGMEM-THE-LIMIT-IS-COMPARED-AND-THE-RESULT-IS-NEVER-SHOWN`).
> So today a guessed ceiling has no observable consequence at all — it is not
> merely undetected but **unfalsifiable**, masked by a second defect. And
> fixing that second defect, which is the right thing to do, is precisely what
> would make this guess bite. That is a trap worth naming: *a burn-down that
> looks harmless because something else is broken is not harmless, it is
> queued.*
>
> | Guessed number is a… | What happens | How you find out |
> |---|---|---|
> | **policy ceiling masked by a second defect** (`cmd_cgmem create`) | nothing, until the thing that reads the ceiling is fixed | you don't — and fixing an unrelated bug is what makes it start |
>
> **3. The page counts refine the `ipcns` accumulator row to *partially*
> reversible.** The previous batch established that a guess `+=`'d into a total
> with no inverse is uncorrectable in place. `cgmem` has an inverse —
> `record_uncharge` puts `usage_pages` back — so at first glance it is the mild
> version. It is not. `c.charges` and `state.total_charges` are `+= 1` with
> nothing that ever decrements them, so **the correction is itself recorded**.
> Un-charging a guessed 1 leaves `charges=1 uncharges=1` where the truth was
> `charges=1 uncharges=0`. The quantity can be repaired; the record of what
> happened to it cannot. That is arguably worse than `ipcns`, where at least
> the wrongness stays in one number instead of being laundered into a
> plausible-looking history.
>
> **The uncounted third operand is the one that hides best.** `[rss|cache]` was
> read as `parts.get(3).copied().unwrap_or("rss") == "cache"` — `toggle_word`'s
> `matches!` defect in a two-word alphabet. The expression has no way to say "I
> did not understand you", so every spelling that was not exactly `cache`
> evaluated to `rss`, and `cgmem charge 1 500 cach` charged 500 pages to the
> wrong bucket and reported success. Two things make that worse than a mis-set
> flag:
>
> * **It is invisible in the aggregate.** `record_charge` adds the pages to
>   `usage_pages` either way, so `cgmem list`'s headline number is correct and
>   only the breakdown beside it is wrong — and the breakdown is exactly what
>   distinguishes memory that can be reclaimed under pressure (`cache`) from
>   memory that cannot (`rss`). Nothing in the output looks off.
> * **On the un-charge side it breaks an invariant.** `record_uncharge` floors
>   the aggregate and the bucket with two independent `saturating_sub` calls,
>   so un-charging `cache` from a cgroup holding none deflates `usage_pages`
>   and nothing else. See
>   `A-CGMEM-UNCHARGE-SATURATES-PER-BUCKET-AND-IN-AGGREGATE-INDEPENDENTLY` —
>   an independent defect, reachable by typing `cache` correctly, and not
>   cleared by fixing the parse.
>
> The refusal lives in a new `memory_kind_arg` helper next to `cmd_cgmem`
> rather than in the generic operand family, because it is specific to this
> command's alphabet and grouping it with `toggle_arg`/`required_num` would
> imply otherwise.
>
> **Rung 93 also pins the other half of §607.** The kind is bracketed
> `[rss|cache]` in the usage line, so an *absent* word keeps its documented
> default: `cgmem charge 1 7` still charges rss, and the rung asserts it lands
> in `rss=7 cache=0`. Only an unreadable word is refused. Two uncounted guesses
> in one command that differ in exactly this way — `create`'s `<name>` is
> refused when absent, `charge`'s `[rss|cache]` is not — is the clearest
> illustration so far of why §607 keys on the brackets rather than on whether a
> default exists in the code.
>
> **Found while reading for evidence:** two independent `cgmem` defects, both
> logged separately and both **since fixed** —
> `A-CGMEM-UNCHARGE-SATURATES-PER-BUCKET-AND-IN-AGGREGATE-INDEPENDENTLY` and
> `A-CGMEM-THE-LIMIT-IS-COMPARED-AND-THE-RESULT-IS-NEVER-SHOWN`. Fixing the
> second is what armed this batch's ceiling guess: `high_events` is now printed,
> so a wrong ceiling is finally falsifiable. The trap named above was not
> hypothetical — it was resolved within the hour, and the refusal had to be in
> place first.

> **Burn-down log.** 2026-08-26 (twenty-fourth batch): `cmd_colortemp` (7,
> plus two uncounted) cleared — 494 → 487 across 219 → 218 functions. Pinned
> by `kshell::self_test` rung 92.
>
> **In short:** every operand of `colortemp` is written `<required>` in the
> command's own synopsis, and every one of them was implemented optional. The
> shell did not merely guess at words it could not read — it also guessed at
> words that were never typed, so bare `colortemp set` meant "profile 1, 4000
> Kelvin" and did it.
>
> **The half that is new: the guess for an *absent* word, not an unreadable
> one.** Twenty-three batches of this burn-down have been about the second
> prohibited shape in §600 — a word read, not parsed, replaced. `cmd_colortemp`
> has those too, but its `set` arm had already been refusing unreadable ids
> before this batch. What it had not been refusing was an id that was not
> there: `parts.get(1).unwrap_or(&"1")`. §607 says a bracketed `[operand]`
> keeps its documented default and only an *unreadable* one is refused. The
> converse is what applies here — an operand written `<id>` has no documented
> default, so inventing one is the same defect wearing the other shape. Every
> operand in this function is angle-bracketed.
>
> **The refusal it did have was anonymous.** `set 1 4O00` printed `Invalid
> profile ID`, which never says which word could not be read, and in a
> two-operand command names the wrong one. That is the §604 wording rule: a
> refusal must name the word it could not read.
>
> **The mode guess points the wrong way.** `mode` matched four names and had
> `_ => TempMode::Off` — an uncounted guess, because the checker matches numeric
> parses and this one is an enum. `off` is not a neutral reading of a typo. It
> is the one value of the four that stops the profile doing anything: three of
> the four modes are ways of being *on*, and every misspelling collapsed onto
> the fourth. `colortemp mode 1 sunsynk` turned the night-light off and printed
> `Mode: Off` as though that had been the request. The `cputhr` thesis — the
> guessed default is always the *reassuring* value — inverts here: the guess is
> the value that quietly disables the feature, and it still reads as a success.
>
> **The severity row this batch adds: *delayed manifestation*.** Every earlier
> row is about what the guess did at the moment it was typed. Here most of the
> values are *stored, not applied*. `set_day_night` writes a pair and returns.
> `set_schedule_times` writes three numbers and returns. Nothing recomputes
> until the next `update_for_time`, which in real use is a clock tick hours
> away. So the guess produces no wrong output at all when the command is typed
> — it produces a wrong *colour on the screen* at 20:00, with nothing on screen
> connecting it to a line typed at lunchtime.
>
> | Guessed number is a… | What happens | How you find out |
> |---|---|---|
> | **stored, not applied** (`cmd_colortemp` `daynight`, `schedule`) | nothing, until the next transition | the screen is the wrong colour hours later, with no line to blame |
>
> **`schedule` is the sharpest case, because its operands are *times*.** The
> guessed sunset was `1200`, and the success line does not echo the digits
> typed — it formats them back as `20:00`. So `colortemp schedule 1 12OO 420
> 30` answered a typo with a plausible-looking schedule, in a shape that made
> it look deliberate, to take effect at an hour when whoever typed it would be
> long past connecting the two.
>
> **`update` reads like a query and is not one.** `update_for_time` *writes*
> `p.current_kelvin` and bumps `total_adjustments`. A guessed `720` did not
> merely answer a question about noon that nobody asked; it set the profile's
> current temperature to whatever noon implies. This is the one honest
> mitigation in the function: the success line is `Temperature at {:02}:{:02}`,
> so it does echo the time it used, and a reader who looked would see `12:00`
> where they meant something else. That makes it the most discoverable guess
> here, which is not the same as discovered.
>
> **`create` was the second uncounted one**, and matters because the table is
> small: `MAX_PROFILES` is 8, and bare `colortemp create` built a real profile
> named `Profile`. A handful of them exhausts the table with entries nothing
> can tell apart — the same shape as `ipcns create`'s `unnamed` in the previous
> batch, in a namespace an eighth the size.
>
> **A note on the pin.** Rung 92's severity assertion is an *equality* between
> two `colortemp list` captures, not a needle, and that is forced rather than
> chosen. `list` prints the current temperature as `{} {}K [{}]`, whose longest
> run of fixed text is the three bytes `K [` — below `check-selftest-wording.py`'s
> four-byte `MIN_FIXED_RUN`, and rightly so, since three bytes cannot anchor an
> alignment. `current_kelvin` is exactly the value `set` and `update` write, so
> the only way to assert it went unchanged is to compare the whole capture. The
> needle that remains is the schedule line, which `list` prints *only* when the
> mode is `Scheduled` or `SunSync` — so its presence is itself the proof that
> the mistyped `sunsynk` did not fall through to `Off`. (Neither `list_profiles`
> nor `stats` takes the `with_state` path, so neither perturbs the `ops`
> counter and the before/after captures are comparable.)
>
> **Found while reading for evidence:** an inverted day/night pair panicked the
> kernel from the shell. Fixed separately —
> `A-COLORTEMP-AN-INVERTED-DAY-NIGHT-PAIR-PANICS-THE-KERNEL`.

> **Burn-down log.** 2026-08-26 (twenty-third batch): `cmd_ipcns` (7) cleared —
> 501 → 494 across 220 → 219 functions. Pinned by `kshell::self_test` rung 91.
>
> **A new row for the taxonomy, and the first one about *recovery* rather than
> about what the guess did.** In every earlier batch the question was what the
> guessed value meant. Here the question is what happens *after* you notice.
>
> `ipcns shm` takes two guessed operands, and before this batch they answered
> the same class of typo in opposite ways — which one you got depended only on
> which word you fumbled:
>
> | Typed | What the guess did | What you saw |
> |---|---|---|
> | `ipcns shm 1O 1024` | id → `0` | `error: NotFound` — an error, but the *wrong* error |
> | `ipcns shm 10 1O24` | size → `4096` | `ipcns: shm ns=10 4096B` — a success line |
>
> One character apart. The first was **safe only by accident**: `init_defaults`
> sets `next_id: 1` and seeds no namespaces, and `create_ns` only ever hands out
> `next_id`, so namespace 0 is unreachable and `destroy_ns(0)` / `record_*(0, …)`
> could only return `NotFound`. Nothing was destroyed — but the message says the
> namespace is missing when in fact the *word naming it* was unreadable, so the
> reader goes to `ipcns list` looking for a namespace that was never in doubt.
> That is the **misdiagnosis** row, already known from `cmd_aiostat`.
>
> **The second is the new one.** `record_shm` is `shm_segments += 1; shm_bytes
> += bytes`, and the subsystem publishes no inverse — the whole public surface
> is `init_defaults`, `create_ns`, `destroy_ns`, `record_shm`, `record_sem`,
> `record_msg`, `ns_list`, `ns_info`, `stats`. There is no `unrecord`, no
> setter, nothing between "add to the total" and "destroy the namespace."
>
> So the guess is not merely wrong; **it is wrong in a way that the obvious
> correction compounds.** Notice the typo, re-run the line with `1024`, and you
> do not replace the guessed 4096 — you add the right number underneath it, and
> the namespace now reports two segments totalling 5120 where one of 1024 was
> meant. Getting back to the truth means destroying the namespace and rebuilding
> every other record in it.
>
> **`sem`'s guess of `1` is worse than `shm`'s 4096 in one specific way**: 4096
> is at least a conspicuous round number, whereas `1` is also the *likeliest
> true value*. A `sem_total` inflated by a guessed 1 is indistinguishable from a
> `sem_total` that was correctly told 1, so nothing in the output ever invites
> the second look that would catch it. This is the `cputhr` thesis again — the
> guess is the reassuring value — sharpened: here the reassuring value is
> reassuring precisely because it is usually *right*.
>
> **Scope, stated honestly:** these are bookkeeping counters. Nothing in the
> kernel allocates against `shm_bytes` or refuses on `sem_total`; the damage is
> to a report, not to an allocation. It belongs with `cmd_taskio` and
> `cmd_aiostat` in the **measurement** row — a number nobody measured, filed as
> though somebody had — and what this batch adds to that row is that when the
> measurement is an accumulator, the error is not just undetected but
> *uncorrectable in place*.
>
> **One non-numeric guess in the same function, which the ledger does not
> count**, was fixed alongside: `create`'s name defaulted to `"unnamed"`, so a
> bare `ipcns create` built a real namespace called `unnamed` and consumed an
> id. Names are not unique and ids auto-increment, so a second bare `create`
> gave a second `unnamed`, distinguishable only by id.
>
> The `[bytes]` and `[count]` operands stay **optional** per §607 — they are
> bracketed in the synopsis, so an omitted word still means 4096 / 1 / 256. Only
> an unreadable one is refused. Rung 91 asserts both halves, and pins the
> severity with a single `ipcns list` line reading `shm=1(7777 B) sem=0(0)
> msg=1(256 B)` after a refused size and a corrected one — which before the fix
> would have read `shm=2(11873 B)`.

> **Burn-down log.** 2026-08-26 (twenty-second batch): `cmd_groupmgr` (7)
> cleared — 508 → 501 across 221 → 220 functions. Pinned by
> `kshell::self_test` rung 90.
>
> **The severity that every earlier batch was one step away from: in an id space
> of *security principals*, `0` is not an arbitrary member — it is the
> superuser.** The selector batches (1–13) could fairly say "acts on the wrong
> object, and you notice, because the object you meant is still there." That
> reading depended on the id space being CPUs, shared-memory regions, quota
> names — spaces where zero is just the first element. `groupmgr`'s ids are
> users and groups, `init_defaults` seeds GID 0 as `root`, and **every one of
> this function's guesses named root on whichever axis it sat.**
>
> | Typed | Asked for | Actually asked for |
> |---|---|---|
> | `groupmgr delete 1O` | delete group 10 | **delete the `root` group** |
> | `groupmgr adduser 1O 500` | add UID 500 to group 10 | add UID 500 to the **root group** |
> | `groupmgr adduser 100 5O0` | add UID 500 to group 100 | add **root** to group 100 |
> | `groupmgr rmuser 1O 500` | remove UID 500 from group 10 | remove UID 500 from the **root group** |
> | `groupmgr user 5O0` | which groups is 500 in? | which groups is **root** in? |
>
> **`delete` is the sharpest site in the entire burn-down**, because it is
> destructive and unguarded: `delete_group` is a bare
> `state.groups.retain(|g| g.gid != gid)` that consults neither `group_type` nor
> membership. So `groupmgr delete 1O` — one mistyped character — destroyed the
> root group and printed `Deleted group 0.` as a success.
>
> **Refusing the word does not make the operation safe, and this entry should
> not be read as claiming it does.** A correctly typed `groupmgr delete 0` still
> destroys `root`. That is a separate defect with a separate fix, filed as
> `A-GROUPMGR-DELETE-HAS-NO-GUARD-AND-GROUPTYPE-SYSTEM-PROTECTS-NOTHING`. The
> two compound; clearing the guess only removes the path that reached the
> unguarded operation *by accident*. (That second defect was fixed on
> 2026-08-30: `delete_group` now refuses any `GroupType::System` group, and
> `create_group` refuses to mint one.)
>
> **Two non-numeric guesses in the same arm were fixed too, though the ledger
> counts neither.** Leaving them would have been patching around the same defect
> in the same twelve lines:
>
> - `create`'s **name** defaulted to `"newgroup"`, so `groupmgr create 1000`
>   produced a real group indistinguishable from one somebody meant to call
>   that. A guessed name is not a milder guessed number.
> - `create`'s **type** ran through `parse_groupmgr_type`, whose `_ =>
>   GroupType::User` fallback meant `groupmgr create 1000 devs sistem` created a
>   plain user group while the operator believed they had asked for a system
>   one. **This guess points the wrong way in the opposite sense from `delete`:**
>   a request for *more* privilege quietly answered with *less*. Both directions
>   from one function is the same lesson `diskquota` taught one batch earlier —
>   the guess is not biased toward safety, because it was never reasoning about
>   safety.
>
> `parse_groupmgr_type` now returns `Option`, and the caller decides separately
> what an *absent* word means (`User`, the documented default, §607) from what an
> unreadable one means (a refusal). Those are different questions and the
> fallback answered both with one value.
>
> Rung 90 pins all seven refusals, both `create` controls, and — the assertion
> that pins the *severity* rather than the wording — that `groupmgr get 0` still
> finds `root` after a refused `delete`.

> **Burn-down log.** 2026-08-26 (twenty-first batch): `cmd_diskquota` (7)
> cleared — 515 → 508 across 222 → 221 functions. Pinned by
> `kshell::self_test` rung 89.
>
> **The first batch where the same guessed constant is wrong in two opposite
> directions inside one function.** Every previous entry could state a single
> bias: `cputhr` always guessed the *reassuring* value, `cgiostat` always
> guessed the *inverting* one. `diskquota` substituted `0` for any unreadable
> number, and `0` is an extreme of this operand's range at **both** ends — as a
> *limit* it is the strictest value there is, as a *request* it is the emptiest.
>
> | Typed | `0` stood in for | What it means there | Result |
> |---|---|---|---|
> | `diskquota set alice user 100 2O0` | the **hard limit** | the strictest possible | alice cannot write one byte |
> | `diskquota check alice user 5OO` | the **size asked about** | the emptiest possible | `ALLOWED` — to a question nobody asked |
> | `diskquota update alice user 1O` | the **byte delta** | the identity | usage silently stops tracking reality |
>
> So the guess is not biased toward safety, and not biased toward permissiveness
> either. It is biased toward **whatever the surrounding code happens to make
> `0` mean** — which the shell never considered, because it was not choosing a
> value at all, only filling a hole.
>
> **Why the `set` case is a lockout and not merely a wrong number.**
> `QuotaEntry::status` asks `bytes_used >= hard_limit_bytes`, so a hard limit of
> zero reports `HardExceeded` on an entry storing *nothing* — zero is not less
> than zero. `check_quota` asks `new_usage > hard_limit_bytes`, so every write
> of a single byte is denied. What completes it is the state being replaced:
> `check_quota` returns `Ok(true)` for a name with **no entry at all**. One
> mistyped character therefore moved a user from *unrestricted* to *cannot write
> one byte*, and reported it as a quota successfully set. Rung 89 pins this
> directly — after a refused `set`, `diskquota check` must still answer for an
> unquotaed name, proving the refusal left nothing half-configured.
>
> **The honest mitigation, recorded because the last three entries earned the
> habit.** Unlike `cputhr temp` — where the guess wrote back the value already
> in the field, leaving the state byte-for-byte unchanged — `diskquota set`
> echoes the guess in its success line: `soft=100 hard=0`. A reader who checks
> the numbers can see it. The defect is that nothing *makes* them look, and the
> line's grammar asserts success.
>
> **`update`'s deltas are `i64`, deliberately, not by default.** Freeing space
> is a negative delta, so reaching for `u64` here would refuse exactly the
> arguments that are correct — the point made in `required_num`'s own doc
> comment. Rung 89 asserts `diskquota update … -50` still succeeds, so a later
> "tightening" that swallows the minus sign fails the build.
>
> **§607 holds:** `update`'s synopsis brackets `[file_delta]`, so an omitted
> file delta still means zero; only a word that is present and unreadable is
> refused.
>
> **Scope note.** The `files` arm's two sites are fixed on the same terms, but
> their damage is *latent*: nothing in the tree reads `soft_limit_files` or
> `hard_limit_files`, so a guessed file limit of zero locks nobody out today. It
> becomes the same lockout as `set` the moment that enforcement is written,
> which is the argument for refusing the word now rather than when it bites. See
> `A-DISKQUOTA-FILE-COUNT-LIMITS-ARE-STORED-AND-NEVER-COMPARED`.
>
> **That moment arrived 2026-08-27**, when the enforcement landed. The `files`
> arm's guessed `0` is an active lockout now, and the refusals recorded here are
> what stop it — a case of the burn-down paying off before the fault it
> anticipated could bite anyone.

> **Burn-down log.** 2026-08-26 (twentieth batch): `cmd_shmem` (7) cleared —
> 522 → 515 across 223 → 222 functions. Pinned by `kshell::self_test` rung 88.
>
> **The mildest severity in the burn-down, and the one that shows what the
> counted ledger is actually counting.** Every other batch has been able to name
> damage: a wrong object acted on, a number nobody measured filed as fact, a
> guess that meant the negation of what was typed. `shmem` has none of that.
> All seven sites guessed `0` and then, on the very next line, tested for `0`
> and bailed — so **the guessed value never reached the subsystem**. No state
> was written, nothing was corrupted, nothing was invented.
>
> What was destroyed is only the diagnosis, and all of it. `0` was doing two
> jobs: it was the guess *and* it was the marker for "invalid". That collapses
> three different mistakes onto one answer:
>
> | Typed | The mistake | What the shell said |
> |---|---|---|
> | `shmem create foo` | the size is **missing** | `Usage: shmem create <name> <size>` |
> | `shmem create foo 1O24` | the size is **mistyped** (letter O) | `Usage: shmem create <name> <size>` |
> | `shmem create foo 0` | the size is **readable and invalid** | `Usage: shmem create <name> <size>` |
>
> A synopsis is a specific claim: *you got the form wrong*. In rows two and
> three the form was right. The operator is told to re-read a syntax line that
> already matches what they typed, while the actual fault — one character, or a
> value the subsystem will not accept — goes unmentioned. That is the sixth
> severity row, **conflation**: not a wrong action, but one answer standing in
> for three questions, and it is the answer to none of them.
>
> `shmem attach` is the same defect with a second edge. It takes two numbers,
> and `shmem attach 1O 3` and `shmem attach 3 1O` printed the *identical* line —
> so the message did not even narrow which of the two words was the problem.
> They are now `` `1O' is not a region id `` and `` `1O' is not a process id ``.
>
> **The `delete 0` sentinel was dropped, not reworded.** Region ids are
> allocated from 1 (`init_defaults` seeds `next_id: 1` in
> `kernel/src/fs/shmem.rs`), so `delete 0` finds nothing
> and `shmem::delete` answers `NotFound` — which is true, and is the same answer
> `delete 999` gets. A shell-side rule singling out zero would invent a
> distinction the id space does not have. Zero was only ever special *because it
> was the guess*; once the guess is gone the special case has no reason to
> exist. This is the general shape to look for in the remaining batches: a
> sentinel test that looks like validation is often just the guess's shadow.
>
> **Why the checker could not have found this by shape.** `cmd_blkread`
> (rung 86, eighteenth batch) had a structurally identical guess-then-test-the-
> guess pair, and D1 did not flag it, because there the parse sat behind a
> function boundary the statement-level regex cannot cross. The two batches
> together are the argument for the ledger being keyed by *function* rather than
> by pattern: the checker finds the sites it can see, and the count is what
> keeps the ones it cannot from being forgotten.
>
> Rung 88 pins all three answers as distinct, both `attach` orders as distinct,
> and carries a control — `shmem create zzrung88 1024` still succeeds — so the
> three refusals cannot be passing by having simply broken `create`.
>
> **§604 note.** Two assertions in the first draft of rung 88 were
> `assert_output_lacks(.., b"Usage: shmem create")` and the same for `delete`.
> The wording gate rejected both as unfireable, and was right: after the fix
> that text exists nowhere in `cmd_shmem`, so the needle matched no format
> string and a misspelling of it would have passed forever. Both were replaced
> rather than weakened — one by a comment (the guarantee is structural: the arm
> cannot print a synopsis it no longer contains, which is stronger than a
> runtime check), one by a positive `contains b"Error:"`, which is fixed text in
> the arm's own `shell_println!` and asserts something the absence could not —
> that the word reached `shmem::delete` and the *subsystem* answered.

> **Burn-down log.** 2026-08-26 (nineteenth batch): `cmd_cputhr` (7) cleared —
> 529 → 522 across 224 → 223 functions. Pinned by `kshell::self_test` rung 87.
>
> **The guessed number is never a random wrong number. It is the reassuring
> one.** Every batch so far has asked *what the guess broke*; this one is the
> first where the answer is that the guess is indistinguishable from good news.
> All three of `cputhr`'s numeric operands defaulted to a value that means
> "nothing is wrong":
>
> | Typed | Meant | Recorded | What the recorded value says |
> |---|---|---|---|
> | `cputhr temp 0 9O000` | 90.0 °C | **65.0 °C** | a normal load temperature |
> | `cputhr cap 0 8OO` | 800 MHz | **2000 MHz** | a cap so loose it is barely one |
> | `cputhr throttle 0 1O00` | 1000 ms | **100 ms** | the shortest stall worth recording |
>
> A selector guess (batches 1–13) acts on the wrong object and you notice,
> because the object you meant is still sitting there untouched. A *measurement*
> guess files a number nobody measured. This is that, plus a bias: the invented
> number is always the one that makes the machine look healthier than the
> operator was trying to say it was. `temp` is the case that matters — someone
> types a temperature only when they are recording something abnormal, and the
> guess replaces the abnormal reading with a normal one, then prints a success
> line and repeats the invented figure to everyone who later runs `cputhr cpus`.
>
> **And for `temp` the guess was literally a no-op, which took reading the
> subsystem to see.** `cputhr::init_defaults` starts CPU 0 at `temp_mc:
> 65_000` — the same 65 000 the shell guessed. So on a freshly-initialised
> machine `cputhr temp 0 9O000` wrote back the value already in the state: the
> command changed nothing at all, and said it had set a temperature. A command
> that reports success, makes no change, and cannot be told apart from one that
> worked is the end state this whole entry exists to remove. It is also a
> reminder that the severity of a guessed default cannot be judged from the
> shell alone — 65 000 looks like an arbitrary plausible number until you open
> `kernel/src/fs/cputhr.rs`.
>
> `clear` is the fourth arm and a different shape again: it takes a CPU number
> and erases that CPU's throttle state. Guessing `0` there never failed into a
> diagnostic the way `aiostat`'s id guesses did, because **CPU 0 exists on every
> machine** — so `cputhr clear` with the operand omitted or mistyped silently
> cleared core 0 and reported it. `clear` was also the one arm the usage block
> never documented, which was survivable while omission meant "CPU 0" and is not
> survivable now that omission is refused; a `clear <cpu>` line was added in the
> same change, per §299 — a gate whose rule is undocumented is just a wall.
>
> §607 keeps the bracketed operands: `throttle <cpu> [ms]`, `cap <cpu> [mhz]`
> and `temp <cpu> [millicelsius]` all still work with the operand omitted, and
> rung 87 asserts `cputhr cap 0` still applies the documented 2000 MHz. Only the
> unreadable word is refused. Unlike batch 18 there is no compounding here —
> every bracketed operand is *last*, so no guess can eat the word another
> operand needed.
>
> **A note on the wording gate, in the direction of the corrections above rather
> than against them.** Rung 87 asserts ` cap=800MHz` against the `cpus` arm,
> whose cap string is built at run time (`alloc::format!("{}MHz", …)` /
> `String::from("none")`) — the same construction that was restructured in
> `cmd_cgiostat`. The gate **accepted** it, because the leading space makes
> ` cap=` a four-byte fixed run and `MIN_FIXED_RUN` is 4. That is the behaviour
> predicted when the sixteenth entry was corrected, now observed rather than
> reasoned about. The `cpus` arm was therefore left alone in this commit: it has
> the same discoverability weakness `cgiostat` had, but saying so is an argument
> to be made on its own, not a claim that a gate demanded it.
>
> **Burn-down log.** 2026-08-26 (eighteenth batch): `cmd_brightness` (7) cleared
> — 536 → 529 across 225 → 224 functions. Pinned by `kshell::self_test` rung 85.
>
> **The guess and the arity guard compound, and the result is a message that is
> false in both halves.** `cmd_brightness`'s `set` arm was fixed for this in an
> earlier batch and is quoted at the top of this entry; what was missed then is
> that `mode [id] <type>` has the identical shape — the *optional* operand
> first, so only the operand count says which word is which.
>
> `bright mode auto` is the form the command's own help text documents. It did
> this:
>
> 1. `parts[1]` is read as the display id unconditionally, so `auto` is parsed
>    as a number. It fails. `unwrap_or(1)` makes it display 1 — **and the word
>    is now consumed.**
> 2. The mode operand is looked for at `parts[2]`, which is absent, so it
>    becomes `""`.
> 3. The command answers ``brightness: mode: `' is not a mode``.
>
> Neither half of that sentence is true. The mode *was* supplied. The thing that
> could not be read was a word the command had already decided was a display id.
> And it quotes the empty string back at someone who typed exactly what the help
> text told them to. This is the first site in the entry where the guess does not
> merely cause a wrong action or a wrong explanation but **manufactures the
> evidence for its own wrong explanation** — the emptiness the message complains
> about was created two lines earlier by the guess.
>
> The lesson generalises to the rest of the ledger and is worth carrying into
> triage: **wherever a synopsis brackets its *first* operand, the guess is
> load-bearing for the arity check below it, and fixing the guess alone is not
> enough** — the index has to be chosen by operand count first. Grep the
> remaining functions for `[` appearing before `<` in their usage lines.
>
> The same batch removes three `.ok()`-then-report-success sites under
> `mode`/`dim`/`undim`. `bright mode 9 auto` discarded the `NotFound` that said
> nothing had been set and printed `Mode → Automatic` anyway. That is the
> discarded-`Result` defect rather than the guessed-operand one, but it is the
> same failure viewed from the other end of the statement — the guess invents an
> input nobody supplied, and `.ok()` invents an outcome nobody got — so it is
> fixed here rather than filed for later.
>
> `up`/`down` keep their documented defaults per §607 and the rung asserts they
> still work. Before this, `bright up 1O` raised display **1** instead of display
> 10 and printed a percentage that was perfectly true of a screen the user was
> not looking at.
>
> **Burn-down log.** 2026-08-26 (seventeenth batch): `cmd_aiostat` (8) cleared —
> 544 → 536 across 226 → 225 functions. Pinned by `kshell::self_test` rung 84.
>
> **A fifth row, and it is about what the command *says* rather than what it
> does.** Every batch so far has been about a command acting wrongly and
> reporting success. `cmd_aiostat` allocates its ring ids from 1, so the guessed
> `0` never collided with a live ring — `aiostat destroy 1O` genuinely failed.
> It failed like this:
>
> ```
> aiostat: error: NotFound
> ```
>
> which says that the ring the user named does not exist. The ring exists. The
> word `1O` is what could not be read. **A confident wrong diagnosis is not a
> milder version of no diagnosis** — a bare failure sends the reader back to
> what they typed; this sends them into the subsystem to hunt for a ring that
> was sitting there the whole time. The cost of a defect is measured in where it
> makes someone look next, and this is the worst answer available.
>
> That generalises, and it is the reason this batch gets a write-up rather than
> a line. A large share of the remaining ledger sits behind a `NotFound`-style
> lookup, and wherever the guessed sentinel happens *not* to name a live object,
> the D1 defect stops producing a wrong **action** and starts producing a wrong
> **explanation**. Those sites look harmless when skimmed — the command errors,
> the status is 1, the shape looks like a refusal — and they are not. When
> triaging the rest of the ledger, do not treat "it already fails" as evidence
> that a site is low priority.
>
> The rest of the command is shapes already catalogued. `create`'s pid is the
> fact the ring is keyed on, so `unwrap_or(0)` produced a ring owned by a
> process that does not exist and reported it created, after which every
> submission against it looked legitimate. The `submit`/`complete` counts are
> the rung-82 measurement shape: `aiostat submit 3 1O24` recorded **one**
> submission where 1024 were meant, and left a counter with nothing anywhere to
> contradict it.
>
> Per §607 the bracketed operands keep their defaults — `create <pid> [sq_size]
> [cq_size]`, `submit <ring_id> [count]` — and rung 84 asserts they still work.
>
> **The wording gate fired again**, in the opposite direction from the last
> batch. Rung 84's closing assertion was written
> `assert_output_lacks(.., b"NotFound")`; the gate reported it as *an assertion
> that can never fire*, because `NotFound` reaches the screen only through a
> `{:?}` and appears in no format string in the shell. Rewritten against the
> `aiostat: error:` prefix, which is fixed text, it says the same thing in a
> form the gate can check.
>
> **Be precise about what the gate did and did not establish** — the first
> version of this paragraph said the old assertion "would have passed against an
> unfixed kernel", and that is simply false. Checked: an unfixed `aiostat
> destroy 1O` prints `aiostat: error: NotFound`, so `lacks b"NotFound"` would
> have **failed**. The assertion discriminated correctly at run time. What the
> gate objects to is narrower and still worth objecting to: it cannot verify
> that `NotFound` is text this command can produce, so it cannot tell a working
> `lacks` from one whose needle is misspelled — and a misspelled `lacks` passes
> forever, silently, against every kernel. The assertion was right by luck of
> spelling, and depending on that luck is the thing the gate exists to stop.
>
> Two corrections in two consecutive entries, both in the same direction —
> crediting the gate with more than it found — is itself the finding. The gate
> is a spelling check on assertions with a four-byte fixed-run rule. It is not
> an oracle, it does not know what a command *should* print, and every claim
> here that it "caught a bug" should be read back against what it actually
> reported before being believed.
>
> **Burn-down log.** 2026-08-26 (sixteenth batch): `cmd_cgiostat` (8) cleared —
> 552 → 544 across 227 → 226 functions. Pinned by `kshell::self_test` rung 83.
>
> **A fourth row for the table below, and it is the worst one.** In the first
> thirteen batches the guessed number selected the wrong object; in `cmd_splice`
> it wrote to the wrong address; in `cmd_taskio` it invented a measurement. In
> all three the command did *less* than, or *other* than, what was asked. Here
> it does the **opposite** of what was asked, and the guessed value is a
> perfectly legal one:
>
> | The guessed number is a… | What the command does | How you find out |
> |---|---|---|
> | **inversion** — `cmd_cgiostat` | the guessed `0` is the subsystem's word for *unlimited*, so the request is negated | you don't — the cgroup exists, `create` reported success, and the cap it was created to enforce simply isn't one |
>
> `cgiostat create web 100O000` — a capital O where a zero belongs — asked for a
> 100 MB/s bandwidth cap and created a cgroup with **no cap at all**, printing
> `cgiostat: created 'web' → id 3 (bw=0 iops=0)`. Every other shape in this entry
> fails toward doing less than was asked. This one fails toward doing the exact
> reverse, in a subsystem whose entire purpose is to say no. A throttle that
> silently becomes no throttle is the one failure a throttle must not have.
>
> Note what makes it invisible in a way the other three are not. A guessed
> selector leaves the object you meant untouched, so you notice. A guessed write
> address destroys something, so eventually you notice. A guessed measurement is
> at least *anomalous* if you look hard. But an uncapped cgroup looks exactly
> like a cgroup that is not being asked for much — it is only ever discovered by
> the load that the cap was supposed to prevent.
>
> Per §607 the limits stay **optional**: `create <name> [bw_limit] [iops_limit]`
> brackets them, so omitting them to mean unlimited is a documented request that
> must keep working, and rung 83 asserts it still does. The fix removes the
> *guess*, not the *default*. Same for the bracketed byte counts in `read`/
> `write`; the cgroup id, written `<cg_id>`, becomes `required_num`.
>
> **The `list` arm changed too, and that part is not incidental.** It built the
> limit string separately — `alloc::format!("{}B/s", …)` or
> `String::from("unlimited")` — and interpolated the result through a trailing
> `bw={}`. So the unit and the word `unlimited` existed nowhere a reader of
> `cmd_cgiostat` could see them, and nothing short of booting a kernel could
> establish that a zero limit prints as `unlimited` rather than as `0`. That is
> the same inversion the `create` arm was being fixed for, hiding one layer
> down, in the one line whose job is to *reveal* it. Both spellings are now
> branches of the format string.
>
> `check-selftest-wording.py` is what surfaced it: it rejected the rung's
> `bw=100000000B/s` and `bw=unlimited` assertions as text the command cannot
> print, and pointed at the interpolated `bw={}` that made them underivable.
>
> **Correction, made the same day, because the first version of this paragraph
> overstated the gate's role and this file is only useful if it is trustworthy.**
> The gate did *not* force the code change. Its rule is a run of four consecutive
> bytes of fixed text, and the rejected needles fell one byte short — `bw=` is
> three. Measured directly against the checker's own `producible()`, ` bw=unlimited`
> with a single leading space is accepted against the **old** format string:
>
> | needle | against old `… throttle={} bw={}` |
> |---|---|
> | `bw=unlimited` | rejected (fixed run `bw=`, 3 bytes) |
> | ` bw=unlimited` | **accepted** (fixed run ` bw=`, 4 bytes) |
>
> So widening the needle by one character would have satisfied the gate and left
> the `alloc::format!` in place. The code change stands on the argument above it
> — that a zero limit rendering as `unlimited` should be visible in the function
> that renders it — and not on any demand from the checker. What the gate
> actually did was smaller and still worth having: it drew attention to a line
> whose two spellings were invented outside its own format string. Claiming it
> had "found a defect the fix was forced to address" would make the gate sound
> stronger than it is, and would teach the next reader to stop investigating at
> the first plausible story.
>
> **Burn-down log.** 2026-08-26 (fifteenth batch): `cmd_taskio` (8) cleared —
> 560 → 552 across 228 → 227 functions. Pinned by `kshell::self_test` rung 82.
>
> Taken with the batch before it, this one completes a three-way split that is
> worth stating explicitly, because it changes how severe a given site is:
>
> | The guessed number is a… | What the command does | How you find out |
> |---|---|---|
> | **selector** (id, mode, switch) — batches 1–13 | acts on the wrong object, or on none, and reports success | the thing you meant is still there, unchanged; you notice when you look at it |
> | **address to write to** — `cmd_splice`, §607 | overwrites bytes that existed before the command ran | you don't; the old bytes are gone and the only record is a success line |
> | **measurement** — `cmd_taskio` | files a number nobody measured into a counter | you don't, and neither does anyone reading the counter afterwards |
>
> `taskio read 5 8l92` (letter L) recorded **4096 bytes** of reads against pid 5
> and printed `taskio: read 4096 bytes for pid 5`. Nothing was damaged and
> nothing acted on the wrong object. A counter simply acquired a value that no
> measurement produced — and from that moment it is indistinguishable from one
> that did. This is the failure mode with the longest half-life in the entry:
> a missing statistic announces itself, a fabricated one is read later, by
> someone who was not there when the command was typed and has nothing but the
> number to go on.
>
> `taskio register 1O` was sharper still, because unlike almost every other site
> in this entry it had **no sentinel guard at all** — not even the useless
> `if id == 0` — so it registered **pid 0**, printed `registered pid 0`, and
> created a task record for a process that does not exist. That record then
> became the destination for every subsequent mistyped measurement, which is how
> two independent instances of this bug compound into a plausible-looking table
> of numbers about a process that was never running.
>
> Note the helper differs from the batch before it and the difference is
> principled, not stylistic: none of `taskio`'s defaults were ever documented —
> the synopsis has always read `read <pid> <bytes>`, both required — so
> `required_num` removes nothing a user could have been relying on. `splice`'s
> were documented, so it took `optional_num`. §607 is the rule that decides
> which.
>
> **Burn-down log.** 2026-08-26 (fourteenth batch): `cmd_splice` (9) cleared —
> 569 → 560 across 229 → 228 functions. Pinned by `kshell::self_test` rung 81.
>
> **This is the batch that changes what the entry is about.** Every one of the
> thirteen before it guessed a *selector* — an id, a mode, a switch. The command
> then acted on the wrong object, or on no object, and reported success. That is
> bad, and it is recoverable: the thing the user meant is still sitting there
> untouched, and the wrong thing can usually be put back.
>
> `cmd_splice` is the first place in this family where the guessed number is an
> **address in a file the command is about to write to**.
> `splice copy src dst 0 1O24 4096` — one mistyped character in the destination
> offset, a capital O for a zero — fell to `unwrap_or(0)` and wrote four
> kilobytes over the *beginning* of `dst`, then printed
> `Copied 4096 bytes: src -> dst`. There is no id to re-look-up and no setting
> to restore. The bytes that were at offset 0 are gone, and the only record of
> the mistake is a success line.
>
> The four length operands are the same defect one step quieter.
> `unwrap_or(1024 * 1024)` means an unreadable length asks for a megabyte, so
> `splice copy a b 0 0 4O96` moved 256× what was asked for and reported the
> megabyte back as the byte count — a number that is *accurate*, describing an
> operation nobody requested.
>
> All nine are genuine optional operands with genuine documented defaults
> (`[src_offset]`, `[len]`), so the fix is `optional_num` and not
> `required_num`; making them required would fix the guess by breaking the
> documented short form. See §607 for why that tradeoff was resolved the way it
> was even here, where the guess is the destructive one.
>
> One uncounted defect went with them: all four transfers shared a single
> `if parts.len() < 3 { "Usage: …" }` covering both paths, so the refusal knew
> something was absent and had already discarded which. `splice_paths` now names
> whichever of source and destination is the missing one.
>
> **Burn-down log.** 2026-08-26 (thirteenth batch): `cmd_focusassist` (9)
> cleared — 578 → 569 across 230 → 229 functions. Pinned by
> `kshell::self_test` rung 80.
>
> Eight of the nine were the familiar `unwrap_or(0)` sentinel, but `mode`,
> `addapp`, `rmapp` and `addsched` each ran *one* `if a == 0 || b.is_empty()`
> guard over two-to-four operands and answered it with one synopsis. That is a
> distinct defect from the sentinel and worth naming separately: even when the
> guard correctly decides that something is wrong, it has thrown away which
> operand it was, so the user is handed the syntax of a line they already typed
> correctly. Split into per-operand `missing …` lines.
>
> The ninth is the reason the batch got a rung. `focusassist on [id]` is
> *documented* to default to profile 1, so `…parse().ok().unwrap_or(1)` read
> like the documentation rather than like a guess — this is the shape most
> likely to survive review. It does not survive use: `focusassist on 2o`
> silenced every notification under profile 1 and then printed profile 1's
> name back, which a reader takes as confirmation of what they asked for. Every
> earlier batch tested `optional_num` against a default that was a *sentinel*
> (`0` for unlimited, absence for query), where the wrongness is visible in the
> output. This is the first assertion that the distinction also holds when the
> default is a value the user could plausibly have meant.
>
> Three uncounted defects rode along, all of them §600's *other* prohibited
> shape — a word read and silently dropped, which the ledger does not count:
>
> * **`focusassist addsched … 1,Tue,9`** built a Monday-only schedule. The day
>   list was `if let Ok(n) = part.parse() { if n < 7 { … } }` — two conditions,
>   no `else` on either — so an unreadable day and an out-of-range one were both
>   discarded in silence and the command reported success. Two thirds of the
>   request vanished without a word.
> * **`focusassist addsched zznap 25:70 26:00 1`** was accepted and stored. The
>   time parser checked that both halves were integers and not that either was a
>   time, so it produced a schedule that could never fire — a silent no-op with a
>   success line, discoverable only by waiting for it not to happen.
> * **`autofs` / `autogame` / `autopres`** did three wrong things in eight lines
>   each: they accepted a narrower vocabulary than the rest of the shell (`true`,
>   `enable`, `1` all refused here and accepted elsewhere), answered both an
>   omitted operand and a misspelt one with the same synopsis, and discarded
>   `set_auto_*`'s `Result` under an unconditional success line — so a failed
>   write still printed `Auto fullscreen: ON`.
>
> **Burn-down log.** 2026-08-26 (twelfth batch): the boolean sweep — 21 sites
> across 16 commands, of which 3 were counted (`cmd_battery`, `cmd_fileshare`,
> `cmd_swapcfg`) — 581 → 578 across 231 → 230 functions. Pinned by
> `kshell::self_test` rung 79.
>
> This batch was organised by *shape* rather than by command, because the shape
> had been copied faster than any per-command sweep could catch it:
> `matches!(word, "on" | "true" | …)`. `matches!` has exactly two outputs and
> both of them are answers, so it cannot express "I did not understand you" —
> every unreadable word became `false`.
>
> That is not a neutral failure. `false` is the permissive side of most of these
> settings, so the shell failed *consistently toward less protection*:
> `reslimit enforce` stopped enforcing, `datausage limit block` stopped
> blocking, `kernelbuild auto` stopped rebuilding — each reporting the setting
> as applied and exiting 0.
>
> Drift made it reachable. Five different vocabularies had grown across the 21
> sites, the sharpest split being `1`: seven sites accepted it and six others,
> printing the *identical* `Usage: … <on|off>` line, did not. So a user who
> learned from `bootcfg activity 1` that `1` means on got, from
> `datausage limit block home 1`, a data cap silently switched off — taught the
> wrong lesson by documentation that was word-for-word the same in both places.
>
> The fix is `toggle_word`, the single place that decides which words mean what,
> reached through `required_toggle` (no query form; absence is an error) or
> `toggle_arg` (§605, has one). `batt ac` keeps its own diagnostic because it
> accepts two words the shared vocabulary does not, and a refusal naming a
> narrower vocabulary than the command accepts would teach a reader to stop
> using a word that works.
>
> **Burn-down log.** 2026-08-26 (eleventh batch): `cmd_a11y` (10) cleared —
> 591 → 581 across 232 → 231 functions. Pinned by `kshell::self_test` rung 78.
>
> Six of the ten were the set/query conflation, which makes this the third
> consecutive settings command dominated by it — it is now clearly the default
> failure of the shape rather than a quirk of any one command. So this batch
> stopped fixing instances and built the helper: `toggle_arg` returns
> `Option<Toggle>` where `Toggle` is `Query` or `Set(bool)`, so absence — and
> only absence — selects the query. See §605; the reason it needs three
> outcomes rather than two is that "no word" and "a word I could not read" are
> the two situations the bug consists of confusing, and an `Option<bool>`
> return has nowhere to put the difference.
>
> Four sites the ledger cannot count, and they are a good sample of why the
> counted number is a floor rather than a total:
>
> * **`a11y regelem 1 buton Save`** registered a *generic* element and said so.
>   An unknown role fell through a `_ =>` to a default role, so the element
>   existed, was announced, and had the wrong semantics — the screen reader
>   would describe a button as a plain element for the life of the process.
> * **`a11y announce alrt`** did two wrong things at once: it downgraded the
>   priority to `normal` and it ate the word, so the *message* announced was
>   whatever followed. Now refused, and the refusal enumerates
>   `Priorities: low, normal, high, alert`.
> * **`a11y inject key F1`** injected scancode 0. There is no sentinel to key
>   on here, which is the general lesson: `required_num` is needed even where a
>   zero-check would appear to work, because 0 is a legitimate scancode.
> * **Seven `.ok()` / `let _ =` discards** across `inject`, `set_font_scale` and
>   the toggles, each reporting success for an operation that had failed.
>
> **Burn-down log.** 2026-08-26 (tenth batch): `cmd_filepicker` (10) cleared —
> 601 → 591 across 233 → 232 functions. Pinned by `kshell::self_test` rung 77.
>
> The least interesting batch so far, and worth logging for exactly that
> reason: all ten sites were the *same* line — a dialog id read with
> `unwrap_or(0)` — repeated across ten arms, and dialog 0 is never a live
> dialog, so every one of them acted on nothing and reported success. Ten
> hand-edits of an identical line is how a transcription error gets in, so the
> conversion was done by a one-shot script that asserted its own arity (exactly
> ten matches, or abort) and was then deleted. Keeping such a script would
> imply it is a tool; it is a proof that a mechanical edit was mechanical.
>
> The one judgement call: `nav`, `select` and `filename` take a path after the
> id, and the path guard was previously fused with the id guard, so a missing
> path was reported as a missing id. They are now split, because the two
> mistakes need different messages to be actionable.
>
> **Burn-down log.** 2026-08-26 (ninth batch): `cmd_peninput` (10) cleared —
> 611 → 601 across 234 → 233 functions. Pinned by `kshell::self_test` rung 76.
>
> Contains one instance of each of the four failure modes the family has
> turned up, which makes it the best single worked example in this entry:
>
> * **Silence.** `pen rm abc` — inner `if let Ok(id)` with no `else` — printed
>   nothing and exited 0.
> * **A sentinel that cannot do the job.** `sim` used `unwrap_or(0)` guarded by
>   `if pen_id == 0 { synopsis }`. That looks like validation and is not: it
>   cannot tell an *omitted* operand from a *mistyped* one, and the synopsis it
>   prints never names the offending word, so the user is told the syntax of a
>   command they typed correctly.
> * **No sentinel available.** `pen map 3 x click` bound button **0**, and 0 is
>   a real button, so no guard of that shape could have caught it.
> * **A guessed enum.** `register` accepted any word as a pen type.
>
> This batch also produced the article bug behind §606. The rung asserted
> `` `4o96' is not an x coordinate `` and the wording gate refused it, because
> `optional_num` hard-coded `a` and the kernel could only say `a x coordinate`.
> Fixed here by renaming the noun to "horizontal coordinate" — a fix that is
> better English but does not generalise, which is what motivated `article_for`
> two batches later.
>
> **Burn-down log.** 2026-08-26 (eighth batch): `cmd_screenshot` (10) cleared —
> 621 → 611 across 235 → 234 functions. Pinned by `kshell::self_test` rung 75.
>
> Region and window captures read their geometry with `unwrap_or`, so
> `scap region 0 0 8oo 600` captured an 0×600 region and reported a successful
> capture. Same shape as `wsnap addzone` in the sixth batch: a geometry operand
> is a field in a set, and one bad field yields a degenerate result that the
> command has no reason to think is wrong.
>
> Two `ALLOWED` entries were added to the wording gate in this batch
> (`("scap", b"(1920x1080)")` and `("scap", b"(800x600)")`): the resolution is
> assembled with `{}x{}` from values the gate cannot constant-fold, so it
> cannot see that the literal is producible. Recorded because an allow-list
> entry is a small hole in a gate and should never be added silently.
>
> **Burn-down log.** 2026-08-26 (seventh batch): `cmd_wintiling` (11) cleared —
> 632 → 621 across 236 → 235 functions. Pinned by `kshell::self_test` rung 74.
>
> This batch names the fourth and last failure mode in the family, and it is
> the worst of them: **saying nothing at all.** Three arms — `rmws`, `rm` and
> `float` — were spelled
>
> ```rust
> if let Some(w) = parts.get(1) {
>     if let Ok(id) = w.parse::<u32>() { … }   // no `else`
> } else { <synopsis>; set_exit(1); }
> ```
>
> The *outer* `if let` had an `else`, so an omitted operand was reported. The
> *inner* one did not, so `tile rmws abc` produced no output whatsoever and
> exited 0. Set against the other three modes the ledger has turned up —
> guess a value, answer a query, report a clamp — silence is the hardest to
> notice, because there is no sentence to disbelieve. A user who typed
> `tile rm 1O` (letter O) and saw nothing would reasonably conclude the window
> was gone.
>
> Three more sites the ledger cannot count, all of a different shape again:
>
> * **A guessed enum, from two copies of one parser that had drifted.** The
>   layout `match` existed twice — in `create` and in `layout`. `layout`
>   refused an unknown word; `create` fell through `_ => MasterStack` and
>   reported the workspace as created, so `tile create work grd` silently
>   built the wrong layout. Fixed by moving the parser onto the enum as
>   `TilingLayout::from_str`, which makes the divergence impossible to
>   reintroduce rather than merely fixing this instance of it.
> * **Two errors discarded with `.ok()`** (`gap`, `ratio`), against the
>   explicit rule in `CLAUDE.md` §9. `tile gap 999 5` for a workspace that
>   does not exist printed `Gap → 5px` and exited 0 — a setting reported as
>   applied to nothing.
> * **A clamp the caller could not see.** `set_master_ratio` stores
>   `ratio.clamp(10, 90)`, but the shell echoed the number it was *given*, so
>   `tile ratio 1 200` printed `Master ratio → 200%` while the workspace held
>   90. This is worth separating from the guessed-value family because the
>   value is not guessed — it is *known* and then misreported. The fix refuses
>   out-of-range rather than echoing the clamp: a clamp the caller cannot see
>   is a guess by another name, and one the ledger's regex will never find.
>
> Of the eleven counted sites, the interesting ones are `add`'s destination
> workspace (no guard at all, so a typo put the window on workspace 1 and said
> so) and `retile`/`windows`, where the default — workspace 1, and the "all
> workspaces" sentinel 0 — is documented and correct for an *absent* operand
> and had simply been extended to an unreadable one. Those two took
> `optional_num`; the rest took `required_num`.
>
> **Burn-down log.** 2026-08-26 (sixth batch): `cmd_winsnap` (12) cleared —
> 644 → 632 across 237 → 236 functions. Pinned by `kshell::self_test` rung 73.
>
> Half of this function was the set/query conflation the fifth batch named, now
> confirmed as the dominant shape in the settings commands rather than a
> `cmd_vdesktop` quirk: **six** of the twelve sites — `enabled`, `preview`,
> `corner`, `thirds`, `edge`, `animation` — reported the current value in
> response to a word the user meant as a set. `wsnap enabled of` (one `f`)
> printed `Snapping: true` and exited 0. Note that it printed the value the
> user was trying to *change away from*, so the output reads as confirmation
> of the opposite of the request. Two spellings produced it: a `_ =>` catch-all
> after the `on`/`off` arms, and `if let Some(x) = …parse() … else { query }`.
> Both are now `match parts.get(1) { None => query, Some(word) => … }`, so
> absence still queries and a present word is either understood or refused.
>
> The shape unique to this command is **geometry**. `addzone` takes four
> percentages and each defaulted to 0, so one mistyped field
> (`addzone lay z 0 0 5oo 500`) built a zone of zero area and reported
> `Zone 'z' added to 'lay'` — indistinguishable from a correct run, and
> invisible until the layout is applied and a window disappears into a 0×0
> rectangle. A guessed *dimension* is worse than a guessed *id* in one
> specific way: an id usually names something that does not exist, and the
> command then says so; a zero dimension is always valid input to the layer
> below, so nothing downstream can object.
>
> Two sites worth separating from the other ten:
>
> * `wsnap remove abc` — **no guard at all**, the same shape as `vd unpin` in
>   the fifth batch. It dropped window 0's tracking state and announced
>   `Removed tracking for window 0`, exit 0. This is the third batch in a row
>   to turn one up, so the "ledger is a lower bound" note below is not a
>   one-off caveat: the missing-guard shape has no `unwrap_or` for the gate's
>   regex to match, and only reading finds it.
> * `wsnap screen` already had a `w > 0 && h > 0` guard and already exited 1 —
>   the *status* was right. What it could not do is name the word: a mistyped
>   width, an omitted one, and an honest `screen 0 0` all printed the same
>   synopsis. It now names the unreadable word, and answers `screen 0 0`
>   separately with `a screen must be at least 1x1`. A correct exit status is
>   not the same as a usable diagnostic, and the gate cannot tell them apart.
>
> `snap`'s position operand was likewise already refused, but anonymously
> (`Usage: wsnap snap <wid> <left|right|…>`); it now says which word it could
> not read, with the alternatives on a second line.
>
> **Burn-down log.** 2026-08-26 (fifth batch): `cmd_vdesktop` (18) cleared —
> 662 → 644 across 238 → 237 functions. Pinned by `kshell::self_test` rung 72.
>
> The guessed *id* here is the shape the earlier batches already covered. What
> is new is three arms where the guess turned a **set into a query**, so a
> mistyped operand produced a reply that was true, useful, and about something
> else:
>
> * `vd visible abc` — `unwrap_or(vdesktop::current())` listed the *current*
>   desktop's windows, under its own heading. Nothing in the reply suggested
>   the operand had been discarded.
> * `vd anim slid` — set nothing and printed `Animation: slide
>   (none/slide/fade/overview)`, which is verbatim the answer to `vd anim`. The
>   typo and the query were the same output.
> * `vd wrap zzz` — the catch-all `_ =>` arm answered `Wrap: false`.
>
> All three keep their absent-operand behaviour, which is documented and
> intended; only the present-but-unreadable case is now refused. That is the
> distinction `optional_num` exists for, and `visible` is its first use outside
> the numeric cases.
>
> Two further sites the *ledger cannot count*, found by reading rather than by
> the gate — the second such pair in two batches, which is now a pattern worth
> stating: **the ledger's figure is a lower bound.** `vd unpin abc` had no
> guard at all, so it unpinned window 0 and announced `Unpinned window 0` with
> status 0 while the window the user meant stayed pinned. And `vd wp <id>` for
> a desktop that does not exist fell out of an `if let` chain with no `else`,
> printing *nothing* and exiting 0 — which reads as "this desktop has no
> wallpaper".
>
> **Burn-down log.** 2026-08-25 (fourth batch): `cmd_partmgr` (21) cleared —
> the largest remaining entry — 683 → 662 across 239 → 238 functions. Pinned
> by `kshell::self_test` rung 71.
>
> This one matters for *what it names*: disks and partitions, the one place in
> the shell where acting on the wrong object is not undone by retyping the
> command. Three shapes turned up that the earlier batches had not:
>
> * **A guess that was reported as a completed operation.** `pmgr create 1 abc
>   100` read the start offset as 0 and put the partition on top of the
>   partition table, then printed `Partition #3 created (100MB unformatted)` —
>   which is exactly what a correct run prints.
> * **A guess that answered a query successfully and wrongly.** `pmgr parts
>   abc` guessed disk 0 and printed `No partitions on disk #0` with status 0.
>   A missing disk and a mistyped disk gave the same answer, and both looked
>   like an empty one.
> * **A boolean read as "anything that is not `off`".** `let v =
>   parts.get(4).copied() != Some("off");` made `OFF`, `0`, `false` and every
>   typo of `off` mean **on** — so a user clearing a boot flag set it, and the
>   shell reported `Flag boot = true`. This is the 22nd site in the function
>   and the one the ledger could not count, because it is not a `parse()` call
>   at all. It is the same shape as the 21 `matches!(…, "on" | "true" | "yes" |
>   "1")` sites still outstanding (see the boolean sweep below), and the first
>   evidence that the ledger's count is a *lower* bound on this class.
>
> Two operands were deliberately **not** made required. `pmgr label <d> <p>`
> and `pmgr mount <d> <p>` with the trailing word absent clear the label and
> the mount point — a real operation, and the only way to express it, because
> `split_whitespace` cannot yield an empty word. §600 is about words that could
> not be *read*, not words that were not typed. Both now report `Label cleared`
> / `Mount point cleared` rather than `Label: ` with nothing after it, so the
> two outcomes cannot be confused in a transcript.
>
> **Burn-down log.** 2026-08-25 (third batch): `cmd_colorpicker` (23) cleared —
> the single largest entry in the ledger — 706 → 683 across 240 → 239
> functions. Pinned by `kshell::self_test` rung 69.
>
> This one is worth recording for *what* was being guessed. `cpick open
> #gggggg` did not refuse the colour: `Color::from_hex` returned `None`, the
> `unwrap_or` substituted black, and the shell answered `Picker #3 opened
> (#000000)`. That reply is indistinguishable from the reply to `cpick open`
> with no colour at all, which documents black as its default — so the one
> case the user could not tell apart from success was the failure. The fix
> keeps the *absent* default (it is documented and intended) and refuses only
> the *unreadable* word, which is the distinction the whole §600 rule turns on.
>
> Two further shapes went with it. `cpick palrm <pal> 1o` deleted swatch **0**
> — a real swatch, the first one — and said `Removed color`; a mistyped index
> destroyed a colour the user could see, and told them it had done what they
> asked. And `cpick model <id>` was conflated with `cpick model <id> rbg`: one
> `else` served both the question *what are the models?* and the answer
> *`rbg` is not one of them*, printing the list and exiting **0** either way,
> so a transposition was reported as a successful answer to a question that
> had not been asked. Split, as the 33 other conflated arms were.
>
> The six per-width helpers this backlog had accumulated (`required_u32`,
> `required_u64`, `required_i32`, `optional_u32`, `optional_u64`) were
> collapsed into generic `required_num<T>` / `optional_num<T>` immediately
> before this batch; see `design-decisions.md` §603 for why, and for the
> survey of the remaining backlog that settled it.

> **The denominator moved, and not because anything regressed.** 2026-08-25:
> the gate was matching its regex against a *line*, and `cargo fmt` — not the
> author — decides whether one of these chains fits on one. Matching against a
> statement instead found **466 more sites of the identical shape**, all of
> which were in the shell the whole time. Every figure in the log below was
> measured under the old, line-granular gate and is therefore an undercount of
> what those batches actually left behind; they are kept verbatim because the
> *work* they describe was real and is still done. The running total is
> restated above against the true denominator: **706 remained across 240
> functions** at the moment the gate was corrected (the header carries the
> live figure), and the 94 sites the log below records as fixed bring the known
> total to 800 — not 334. (Whether those 94 batches also swept up wrapped-form
> siblings that the old gate never listed is unknowable, so 800 is stated as
> the floor it is.) No function's count went down when the gate was corrected,
> which is the check that this is the gate seeing more rather than a
> re-attribution of what it already saw. See
> `A-KSHELL-THE-OPTION-GATE-COUNTS-ONE-LINE-AND-RUSTFMT-USES-FOUR`.
>
> **Burn-down log.** 2026-08-25 (second batch): `cmd_monitors` (13),
> `cmd_userns` (10), `cmd_reslimit` (10) and `cmd_splitview` (9) cleared —
> 42 sites, four more ledger lines deleted, 282 → 240 across 114 → 110
> functions. Pinned by `kshell::self_test` rung 66, each refusal paired with a
> control running the same command with a readable operand.
>
> These four were picked for one property the first batch did not have: their
> defaults are not merely wrong, they are **inverted**.
>
> * `reslimit` documents `0` as *unlimited*, so `.unwrap_or(0)` answered a
>   mistyped memory/CPU/IO/process limit by removing the limit entirely.
>   `reslimit setmem 5 abc` asked for a cap and got none, and said `Memory
>   limits set for group 5`.
> * `userns uidmap <ns> <inner> <outer> <count>` mapped a mistyped `outer` to
>   `0` — which is **root's UID in the parent namespace**. The one mapping that
>   must never be produced by accident was the one an unreadable word produced,
>   and it printed `Added UID mapping`.
> * `monitors add` defaulted to `1920x1080@60`: a guess plausible enough that
>   the wrong monitor looked like the right one.
> * `splitview orient <id> <h|v>` was `if parts[2] == "v" { V } else { H }`, so
>   the spelled-out `vertical` — the form a reader would expect to work —
>   silently meant *horizontal*.
>
> Three defects outside the counted shape were found in the same read and fixed
> with it, because each is the same rule in a different disguise:
> `reslimit setio … lwo` tested `== "low"` and so made every other spelling
> mean "not low priority"; `splitview add <id> <win>` used
> `and_then(parse().ok())`, collapsing *no window named* and *unreadable
> window* into one `None`, so a typo added an empty pane; and `monitors add`
> with too few words printed its usage and exited **0**.
>
> One helper was added: `required_i32`, for `monitors pos`. A monitor position
> is signed — the desktop origin is the primary display, so a screen to its
> left has negative `x` — and reusing `required_u32` would have refused exactly
> the coordinates that are correct. Rung 66 asserts the negative case.

> **Burn-down log.** 2026-08-25: `cmd_installer` (18), `cmd_fstune` (17),
> `cmd_certmgr` (10), `cmd_fontmgr` (6) and one stray `<top>` site cleared
> together — 52 sites, five ledger lines deleted, 334 → 282 across 119 → 114
> functions. They went as one change because they were one *idiom*: 42 of the
> 52 were the character-for-character identical line
> `let id: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);`,
> now a shared `required_id` helper. The rest split into numeric operands
> (`required_u32` / `optional_u32` — which is where the omitted-vs-mistyped
> distinction described below finally lives in code rather than in this
> paragraph) and fourteen `match … { _ => Default }` arms over user-typed
> *names*, where the default variant is the guess.
>
> Worst instance found: `installer remove abc` did not refuse. The id became 0,
> so it removed whatever session 0 was, printed `Removed`, and exited 0.
> `remove` is the subcommand where "guess which object they meant" is least
> defensible, and it had the same one-line idiom as `info`.
>
> See the sibling entry `A-KSHELL-TWO-CHECKERS-READ-COMMENTS-AS-CODE` for what
> this sweep turned up inside the gates themselves.

**In short:** 334 places in the shell read a word the user typed, fail to make
sense of it, and quietly substitute a default. `theme set 300 0 0` sets the red
channel to 0 rather than complaining that 300 is not a byte. This is the same
defect as the two entries above — a wrong answer reported as a success — but it
predates the rule by most of the shell's history, so it is being *pinned* and
burned down rather than fixed in one commit. (`colorpicker rgb 300 0 0` is the
concrete one: the red channel becomes 0, so asking for an out-of-range red
gives you black.)

**Where.** `kernel/src/kshell.rs`, in 119 functions. The exact per-function
counts are in `scripts/option-refusal-ledger.txt`; the shape is
`s.parse::<T>().unwrap_or(D)` or `parts.get(n).and_then(|s| s.parse().ok())
.unwrap_or(D)`.

**Why it is a defect and not a style.** `None` from `parts.get(n)` means the
argument was *omitted*, and a default is then exactly right. `Err` from
`parse` means it was *supplied and unreadable*, and a default is then a guess.
Collapsing the two — which is what `and_then(…).unwrap_or(…)` does in one
expression — makes the wrong case indistinguishable from the right one at
every call site at once.

Sampled instances, to show the range:

| Site | The guess | Consequence |
|---|---|---|
| `cmd_colorpicker` RGB components (7 sites) | `.and_then(parse).unwrap_or(0)` | `colorpicker rgb 300 0 0` yields black, silently |
| `cmd_namespace`, a mount-namespace id | `.unwrap_or(mount_ns::ROOT_NAMESPACE)` | an unreadable id operates on the *root* namespace |
| `cmd_pidns`, a pid argument | `.unwrap_or(0)` | pid 0 is the idle task |
| ~~`cmd_installer` (18 sites)~~ | ~~assorted~~ | fixed 2026-08-25; `installer remove abc` removed session 0 |
| ~~`cmd_fstune` (17 sites)~~ | ~~assorted~~ | fixed 2026-08-25; tuning knobs that silently took 0 |

**Why it is pinned rather than fixed now.** 334 edits across 119 functions is a
sweep in its own right, and leaving the gate unwired while it is done would let
*new* instances land ungated in the meantime. The ledger inverts that: the
number is frozen today, a 335th site fails the build immediately, and the
backlog can only shrink. §296 established this pattern; this is its second use.

**How to burn it down.** Pick a function, split the two cases (`match
parts.get(i) { None => default, Some(v) => v.parse().map_err(refuse)? }`),
lower its count in `scripts/option-refusal-ledger.txt` — or delete the line
when it reaches zero. The checker reports an entry that claims *more* sites
than exist, so a fix that forgets to lower the count fails the build too; the
ledger cannot silently rot into a rubber stamp.

**Not a regression.** True since each site was written.
