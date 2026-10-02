## 1409. A test build refuses to write settings outside a temporary directory, judged by where, not who

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C &middot; asked for by lane E

**In short:** A test that saved a setting without first borrowing a scratch
configuration folder wrote into the real `~/.config/slateos` of whoever ran the
tests -- it has happened, and left files nobody could account for. Now, in a
test build only, `settingsfile::store` refuses -- by panicking, with the fix in
the message -- to write a settings file anywhere outside the system's temporary
directory. A shipped program cannot see this: it exists only under the
`testing` feature, which only a `[dev-dependencies]` entry turns on.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| What decides a refusal | where the file would go: outside `std::env::temp_dir()` | who asks: a flag `with_scratch_config` sets on the calling thread (lane E's proposal) | the harm is the developer's configuration being written, so that is what is tested for. The flag also refuses writes that harm nothing -- a test that makes its own scratch directory and points `XDG_CONFIG_HOME` at it, as `settingsfile`'s own tests do, and a thread a test starts inside its turn |
| What that costs | a forgetful test that happens to run while another holds a scratch turn writes into *that* one's directory, unrefused | the flag catches it | it fails on every other run, which is found at once; and nothing of the developer's is touched either way |
| Refusal or error | a panic | an `io::Error` | most saves are `keep()` calls that show a failure on screen rather than return it; a `Result` a test may ignore is the failure this exists to end |
| Where it applies | in every crate's tests whenever the feature is on -- including, in a workspace run, crates that do not turn it on themselves, since Cargo unifies features | only crates that ask | a forgetful test is as harmful in a crate that did not ask |

Proved before landing by running the tests of every crate that reaches
`settingsfile` -- effectively every application -- together, with the feature
unified as a workspace run has it.
