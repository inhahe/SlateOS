## sysinfo: an environment variable could write a heading of the system report

`apps/sysinfo/src/main.rs`. Fixed in `dab9fab26`. Two bugs of one cause, and
the cause is the interesting part.

`export_text` writes a report whose grammar puts headings at column 0 and data
indented by two spaces. It chose between them like this:

```rust
} else if prop.value.is_empty() {
    out.push_str(&format!("{}\n", prop.name));   // column 0
} else {
    out.push_str(&format!("  {}: {}\n", ...));   // indented
}
```

The empty-value branch exists so the file can emit its own sub-headings —
`Property::new("--- CPU Features ---", "")`. But `props_env_vars` builds a
`Property` directly from each environment pair, and `FOO=` is a legal and
ordinary environment variable. So a variable named `--- Display Outputs ---`
with an empty value printed itself at column 0, byte-identical to the heading
the report writes for the display section.

**This one needed no control characters at all.** Every other finding in this
audit required the payload to smuggle in a newline; folding was therefore a
complete fix for them. Folding does nothing here — there is nothing in the
string to fold. That is worth remembering as a class: *a value can forge
structure without containing any structural character, if the format infers
structure from something other than the value's text.* Here the inference was
from the value's **emptiness**.

The detail-pane renderer had the same bug in its own dialect:

```rust
let is_section = prop.name.starts_with("---");
```

so a variable named `---x` was drawn bold and in the accent colour.

### The fix, and why it is not "escape the name"

Two consumers were each re-deriving *is this row structure?* from the strings.
The strings are environment variables, PCI vendor names and process names —
the one place the answer cannot live. Escaping or folding the name only
narrows the set of strings that happen to fool the inference; it leaves the
inference.

So the distinction is now recorded at construction by the code that knows it:
`PropertyKind::{Heading, Blank, Field}`, with `Property::heading` for the three
sub-headings this file writes, `Property::blank` for the ten separators, and
`Property::new` for data. `Field` rows are always indented — including when
their value is empty, which now means nothing beyond an empty value.

This is the same shape as the fileassoc finding recorded above ("a format with
two writers has no invariant, only a coincidence"), reflected: there, one
format had two *writers* that drifted; here, one format had two *readers* both
inventing an invariant that was never written down.

`Property::new` folding both halves is a second, independent benefit: it closes
the ordinary newline vector for all fourteen `props_*` functions at once —
PCI descriptions, driver paths, process names — rather than at each call site.

### Multiplicity is the new position

sysinfo had no unit tests; there are now seven. The headline one did not catch
its own bug on the first draft, and the reason is the same lesson as
"a substring count is a `contains` in disguise" wearing yet another disguise.

It forged headings that duplicate ones the clean report already contains —
deliberately, because a forgery *identical* to a real heading is the strongest
form of the attack. It then asked, of each column-0 line in the hostile report,
"is this a line the clean report also produced?" The answer was yes, and it
passed.

The assertion has to compare column-0 lines as a **multiset**. Set membership
discards multiplicity exactly as `contains` discards position. Running the
break — reinstating the emptiness guess — now fails
`an_empty_environment_variable_is_not_a_section_heading`, which is the test
named after the bug; before the fix only a bystander test caught it.

Three breaks were run against the final code (reinstate the emptiness guess;
stop folding in `Property::new`; make `Property::new` return `Heading`). All
three are caught, each by at least two named tests.
