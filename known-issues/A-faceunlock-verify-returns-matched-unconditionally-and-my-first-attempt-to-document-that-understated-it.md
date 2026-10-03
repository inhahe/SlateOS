### [A] `faceunlock::verify()` returns Matched unconditionally, and my first attempt to document that understated it -- 2026-09-17

**Status:** OPEN

**In short:** the face-unlock check does not check anything. For any enrolled
user it returns "matched" without comparing a face, a template, or a number
-- the code says so in a comment. Nothing calls it today, so nothing is
unlocked wrongly; the danger is entirely in what the next caller would
believe.

```rust
// Simulate match (always matches enrolled user).
enrollment.verify_count += 1;
enrollment.last_verified_ns = now;
state.total_matches += 1;
Ok(VerifyResult::Matched)
```

The only branches that can refuse are `!state.enabled` (CameraError), an
absent enrolment (NoEnrollment), and the liveness flag when the caller itself
passes `is_live: false`. So the decision is the *caller's* to make, and a
caller passing `is_live: true` for an enrolled id is always authenticated.

**The part worth recording is the draft I nearly shipped.** Correcting the
module doc, I wrote that `verify()` "compares stored numbers, not a face".
That is more accurate than the original claim and still wrong in the
dangerous direction -- it describes a weak comparison where there is none.
Reading the function before writing the sentence is what caught it.

Lane C stated this rule this morning and I had quoted it hours earlier:
**an overstatement is caught the first time somebody tries the feature; an
understatement is believed, so nobody tries.** Their example was
`apps/whiteboard` telling users their work could not be saved while Ctrl+S
was writing an SVG. Mine would have been a security function described as
weak when it is absent -- and a reader who believes it is weak looks for a
stronger comparison, not for a missing one.

So the note in `faceunlock.rs` now leads with `verify()` ALWAYS SUCCEEDS, and
records that the first draft softened it. A doc that reads as diligent and
still misleads is worse than the blunt original, because it has already
spent the reader's suspicion.

**Fixed alongside** (docs only, dd-950 -- the claim is the shape of the
intended work, so it is restated as intent rather than deleted): `sealing`
leads with NOT ENFORCED YET, `authbroker` with "records credentials; does not
broker anything", `filevault` with "encrypts nothing; a folder marked as a
vault is stored in plaintext". `diskencrypt` deliberately untouched -- its
doc already says "management", "encryption *status*" and "settings panel
interface", and correcting accurate text is the sweep I declined, not the fix
I justified.

**Not fixed:** the implementations. Real face recognition needs a camera
stack; seal enforcement needs the VFS write path and a capability story.
Those are features, and dd-950's point is that these modules are the outline
of them rather than dead weight.
