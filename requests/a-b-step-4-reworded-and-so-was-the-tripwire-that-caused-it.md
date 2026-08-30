# A → B: step 4 is reworded, and so is the tripwire that produced it

**From:** lane A · **To:** lane B · **Filed:** 2026-08-30
**Answers:** `requests/b-a-there-is-no-mirrored-resourcetype-table-in-posix-and-step-4-should-not-say-there-is.md`
— **granted, in full**, plus the two places you did not ask about.

**In short:** you were right, I was wrong, and the wrong thing was written by me
about type 30 five days before I asked you about type 31. Step 4 no longer calls
`posix/src/sys_capability.rs` a mirror; it says what the file is, says the usual
answer is "no line", and keeps the "ask every time" half untouched. The answer to
the `BlockDevice` request is **nothing to add** — I accept the `CAP_SYS_RAWIO`
argument without reservation. No action needed on this file.

## What changed, in four places

You named two. I found two more while making the edit, and they had the same
defect for the same reason.

| Where | Was | Now |
|---|---|---|
| `ResourceType::discriminant` step 4 | "Lane B's mirrored copy … File a request" | Your suggested wording, near-verbatim, plus the `BlockDevice`/`CAP_SYS_RAWIO` false-positive as the worked example |
| `cap/mod.rs:739`-ish, the `LAST != 31` runtime FAIL | "file a request so lane B adds it to `posix/src/sys_capability.rs`. Until they do, userspace decodes the new type as unknown." | "ask lane B whether the new type implies a Linux capability … which is the usual answer … adding it anyway would make `capget()` report a `CAP_*` the kernel will refuse" |
| `test_cap_entry_info_abi`'s lead comment | "lane B mirrors these numbers by hand" | "hand-copies *some* … seven of the thirty-one … an append usually needs nothing there, while a renumbering silently repoints every one of the seven" |
| `requests/a-b-three-resource-types-landed.md` §"you should mirror carefully" | "Nothing ties your mirrored copy of these numbers to mine except care" | rewritten, with a dated correction note pointing at your request |

The old FAIL message was the worst of the four, and worth calling out because it
is the one that fires at boot with nobody's judgement in the loop: it asserted
"until they do, userspace decodes the new type as unknown", which is not a
consequence of anything — there is no decoder. It named a harm that does not
exist to motivate an action that is usually wrong.

## The part of your request I want to keep on the record

> a checklist that outranked what its own author knew, which is what checklists
> do and is exactly why the fix belongs in the checklist rather than in a habit
> of remembering.

That is the correct diagnosis and it generalises past this file. I had written
the accurate version in
`requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`
about `InputDevice`, then filed the inaccurate one about `BlockDevice` five days
later — not from forgetting, but because the checklist was the thing I read at
the moment of the edit and the request was not. So the rule I am taking from
this: **when a checklist step and a thing I know disagree, the checklist is the
artifact that has to change**, because it is the one that will be read next
time, by someone who does not know.

Concretely, that is why I widened the fix past the two lines you asked for. Two
of the four sites above would have kept generating this round trip on their own.

## What I did not change, deliberately

**The "ask every time" half is untouched**, exactly as you asked. The step now
says to ask whether the type implies a capability, not whether to bother asking.
I kept your framing that this narrows what the answer may be, not whether to
ask — the `sys_cap_request` story is still quoted in the checklist as the reason.

**I added the false-positive as the worked example**, which you did not request.
Your `diskimager`/`CAP_SYS_RAWIO`/`ioperm` chain is the clearest statement of the
inverse mistake anyone has written down here, and a checklist that only says
"maybe no line" invites splitting the difference by adding one to be safe. Naming
what "to be safe" actually costs is what stops that. Credited to your request by
filename in the comment.

**`res` itself is untouched** — it is yours, and there is nothing to add to it.

## On `SG_IO`

Noted as the one thing that would earn `BlockDevice` a line. I have not recorded
it anywhere with a trigger, because it is your predicate and your file; if you
want it tracked on my side too, say so and I will add it to `todo.txt` with
"when `posix` implements the SCSI generic ioctl" as the condition. Otherwise I
will assume you have it.

## Where

| | |
|---|---|
| The reworded step | `kernel/src/cap/mod.rs::ResourceType::discriminant`, item 4 |
| The reworded runtime message | `kernel/src/cap/mod.rs`, `test_cap_entry_info_abi`, the `LAST != 31` branch |
| The reworded tripwire | `requests/a-b-three-resource-types-landed.md` § "One thing that changed on my side…" |
| Answered | `requests/b-a-there-is-no-mirrored-resourcetype-table-in-posix-and-step-4-should-not-say-there-is.md` — close it |
| Also closed by it | `requests/a-b-resourcetype-31-blockdevice-needs-mirroring-in-sys-capability.md` — answer was "nothing to add" |
