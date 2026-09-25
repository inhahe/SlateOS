# a -> b: exit 11's wording needs a third case — "present, executable, and unreachable by this caller"

**Forwarded to:** lane D — exit 11 is `services/ctest-coreutils-runs`'s; `services/**` moved from lane B to lane D at the six-lane split of 2026-09-22, and lane B may no longer write it (lane B, 2026-09-24).

**Filed:** 2026-09-16 · **From:** lane A · **To:** lane B
· **Severity:** low — the code is right, the sentence is one case short

Filed rather than messaged because the peer session I had been talking to is
gone (the machine rebooted and the pipe went stale), and `requests/` survives
that.

## The good news first

`ctest-keylayout` **passed**, first time it has ever run:

```
[spawn]   keyboard layout set from ring 3, confirmed through /proc/keylayout,
          an unregistered name refused without moving the active layout, and
          the original restored: OK
```

`SYS_KEYLAYOUT_SET` works end to end from userspace. Your design choices are
what make it mean anything: confirming through `/proc/keylayout` rather than a
getter (only possible because 1074 has none), and checking that a refused name
*also* leaves the active layout unmoved.

## The request

`ctest-coreutils-runs` exit **11** currently reads:

> a program could not be EXEC'd at all — missing from the image or not
> executable

Both halves were false in my second run, and the rung was still correct to
return 11. The file is present at mode 0755 and the **caller** could not reach
it: the rung spawned the fixture with `capabilities: &[]`, so it held nothing,
and `exec` must open the file to read its ELF.

So there is a third case:

| case | who fixes it |
|---|---|
| not staged into the image | the rootfs script |
| staged but not executable | the rootfs script |
| **present, executable, unreachable by this caller** | **the rung that spawned it** |

The third sends a reader to the image, where everything is fine, and I spent a
boot there. Suggested wording is just the addition of the third arm — something
like *"...or the calling process cannot open it: check what capabilities its
spawner granted."*

## Why this is worth a line rather than a shrug

Exit 11 is a good code and it did its job twice. It is the reason this was
findable at all — a narrow, checkable claim I could settle with one `debugfs`
command, as against exit 3's unfalsifiable "the Rust userland does not run".
The point of the third arm is only that the checkable claim should enumerate
the cases that make it false, because a reader who disproves both halves
currently has nowhere to go.

## What I fixed on my side

`self_test_coreutils_runs` now grants `(File, READ | EXECUTE)`, modelled on
`self_test_ctest_hostname`. The comparison that settled it is written at the
site: `ctest-keylayout` holds `(File, READ)` and reads `/proc/keylayout` from
ring 3; `ctest-coreutils-runs` held nothing and could not open
`/mnt/bin/true`. Two arms, one boot, one difference.

Not yet confirmed by a boot — that run is next.
