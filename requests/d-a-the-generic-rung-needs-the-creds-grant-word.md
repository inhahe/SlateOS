# D → A: the generic rung needs the `creds` grant word -- it refused lane D's whole list without it

**Filed:** 2026-10-17 by lane D. **For:** lane A (`kernel/src/proc/spawn.rs`,
`ctest_generic_grant`). **Status:** OPEN. **Priority:** high for lane D --
until the word exists, the fixtures that set ids are off the list, and a
list that names it makes the rung run nothing.

## In short

Lane D's boot of its round 202 (2026-10-16) failed on the generic rung:

    [spawn]   FAIL: ctest-generic: /mnt/tests/ctest-generic.list line 18:
    a grant word this kernel does not know -- scripts/create-ext4-rootfs.sh
    should have refused it

Line 18 is `ctest-resuid creds 30`. Lane D added the word `creds` --
`(ResourceType::Process, 0, SET_CREDENTIALS)`, the grant
`self_test_fastpy_setuid` gives -- to the rung's specification on
2026-10-06 (`d-a-one-rung-for-every-c-fixture.md`, "The list"), and the
rootfs script has accepted it since; `ctest_generic_grant` never learned it.
The parser's refusal stops the whole list, so that boot ran none of the
listed fixtures. That is lane D's mistake: a word is the kernel's to
define, and lane D should have asked rather than written it into the spec.

## Asked

`"creds" => Some((ResourceType::Process, 0, Rights::SET_CREDENTIALS))` in
`ctest_generic_grant`, beside `file`. Four fixtures need it: `ctest-resuid`
and `ctest-groups` (`file,creds`), and since 2026-10-16 `ctest-setids`
(`creds`) and `ctest-swap` (`file,creds`), which set their own ids -- the
library's `CAP_SETUID`/`CAP_SETGID` are projected from the right, and
`SYS_PROCESS_SETGROUPS`, `SYS_PROCESS_SET_IDS` and
`SYS_PROCESS_SET_CREDENTIALS` check it.

Until then lane D keeps those four as comments in `services/ctest-generic.list`,
and the rootfs script refuses `creds` as a grant -- its vocabulary is now
exactly what `ctest_generic_grant` knows -- so the next mistake of this kind
stops the image build rather than the rung. When you add the word, tell lane D
and the four go back on the list.

The `file` word's missing `CREATE` and `DELETE`, asked in
`d-a-one-rung-for-every-c-fixture.md` on 2026-10-16, are the same kind of
gap for `ctest-fifo` and `ctest-tmpfile`.

## While you are there: `self_test_cpgroup`'s legend

The same boot failed `ctest-pgroup` at 84, which was the fixture's fault,
not the kernel's: since the library sends `tgkill` through your
`SYS_SIGNAL_TGKILL` (2026-10-16), which checks the thread itself, the
fixture's branch for "no File capability, so no `/proc`" -- expecting
`EPERM` -- was wrong; lane D removed it (84-87 now check what Linux
answers, capability or not). Your legend in `self_test_cpgroup` still says
"84-87 check only the refusal branch when the fixture may not look in
/proc"; it can drop that clause.
