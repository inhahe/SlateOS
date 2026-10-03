## 867. An instance-level extension command gets a fan-out policy per command, and `VK_EXT_debug_utils` gets "everyone, all or nothing"

**Date:** 2026-09-22 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** A computer can have more than one graphics driver installed. Our
Vulkan loader hides that: a program asks for one "connection to the graphics
system" and gets a single handle that secretly stands for one connection per
driver. That works until the program calls a function through that handle,
because then somebody has to decide *which driver actually does the work*. For
most functions there is no single right answer, and it differs per function.
This records the answer for the first such function pair we implement — the one
that subscribes a program to warning messages from the drivers — and, more
importantly, records that "which driver answers" is a decision to be made once
per function rather than a rule that can be written once for all of them.

### What forced the decision

§803 forwards extension commands the loader has never heard of, in three
instructions of assembly, with no knowledge of their signatures. That works for
a command whose first argument is a `VkPhysicalDevice` — one GPU, therefore one
driver, therefore one right answer and no decision to make. Overwrite argument
0 with the driver's own handle and jump.

It cannot work for a command whose first argument is a `VkInstance`. That handle
is the loader's own object standing in front of *several* driver instances at
once, so there is no single driver to substitute. The question "who answers?"
has to be answered, and the answer is different for different commands:

| shape of command | plausible policies |
|---|---|
| a subscription (`vkCreateDebugUtilsMessengerEXT`) | everyone, so no driver's diagnostics go missing |
| a surface (`vkCreateXlibSurfaceKHR`) | the loader owns one object every driver understands |
| a query | first driver that answers, or a merge |

No mechanism can pick between those, which is why §803's trampoline pool stops
where it does. The roadmap and `known-issues.md` both described this as "a
policy, not a forwarding rule", and this entry is the first policy.

### The policy chosen for the messenger pair

**Every driver that offers the command gets one, and the loader's handle stands
for the whole set.** A debug messenger is a *subscription*: the program is
asking to be told about anything any driver finds noteworthy, so leaving a
driver out would silently narrow what it hears.

Two rules give the policy its edges, and they are deliberately not the same
rule:

* **A driver that does not offer the command is not a failure.** It simply does
  not implement `VK_EXT_debug_utils`, which is a legitimate thing for a driver
  to be. It contributes nothing, exactly as a driver with no GPUs contributes
  nothing to `vkEnumeratePhysicalDevices`.
* **A driver that offers the command and then fails, fails the whole call.**
  Everything already built is destroyed and the driver's own error is returned.

### The alternative, and why it lost

The obvious alternative was to reuse `instance::outcome`, the rule the loader
already applies to `vkCreateInstance`: *any driver succeeding is a success*.
Reusing it would have been one line and would have kept one policy in the crate
instead of two.

It is the wrong rule here, and the difference is what the command means. For
`vkCreateInstance`, a driver that cannot participate is a driver the program was
never going to use — dropping it loses nothing the program can observe. For a
messenger, a driver that cannot participate is **a source of warnings going
quiet**, and the C API gives the program no way to find out: it receives one
handle and a success code, with no count and no list. It would be subscribed to
a subset of its drivers, believe it was subscribed to all of them, and have no
way to ask. That is the "instrument that fails toward clean" shape recorded as
known-issues.md shape 23 — under-reporting that deletes its own evidence.

Rolling back also matches what Vulkan already promises for a create command: on
failure the program's handle is untouched and nothing is left allocated, which
is what lets a caller keep a previous messenger in the same variable.

The cost of the choice is real and worth stating: **a machine with one broken
driver among three now gets no messenger at all, where the lenient rule would
have given it two working ones.** That is the right trade only because the
failure is loud — the program is handed an error code it must handle — whereas
the lenient rule's failure is silent. A loud failure can be diagnosed; a quiet
subscription cannot.

### Two smaller calls that came with it

**The loader answers null when no driver implements the extension.** It would
have been simpler to always hand back a function pointer and let the call fail.
But a non-null answer from `vkGetInstanceProcAddr` *is* the claim that the
command exists, and null is what the C API already means by "no such entry
point". Handing out a pointer that can only fail would be a new instance of the
exact defect this whole line of work exists to close — advertising an extension
whose entry points answer null.

**§802's "declare no Vulkan structures" survives this command.** The
create-info pointer is passed to each driver untouched; the loader never reads
a field, so it needs no declaration of
`VkDebugUtilsMessengerCreateInfoEXT`. This is worth recording because it is
*not* generally true of what remains: a surface-creation command is one the
loader has to read to know which platform it is for, and that is the command
that will finally force the structure declarations. The messenger pair being
free of them is a property of this command, not a reprieve.

### What this does not settle

The surface family is still open, and it is now clear that it is blocked on
more than a policy. See `known-issues.md`
`C-VKLOADER-ADVERTISES-EXTENSIONS-WHOSE-ENTRY-POINTS-IT-ANSWERS-NULL-FOR`.
