------------------------------------------------------------------------------
--  Virtqueue descriptor bookkeeping -- SPARK, proved free of run-time errors.
--
--  WHAT THIS IS FOR
--
--  A virtio split virtqueue has a descriptor table in memory the device can
--  see.  The driver hands the device a chain of descriptors, the device
--  completes it and writes the chain's head index into the *used ring* -- and
--  the used ring is written by the device, not by us.  kernel/src/virtio/
--  queue.rs then took that index and walked the chain with it, using it to
--  index the descriptor table through a raw pointer.  Nothing bounded it.  A
--  device reporting a head of 65535 walks the kernel a megabyte past the end
--  of a 16 KiB frame.  See known-issues.md B-VIRTIO-UNVALIDATED-USED-ID.
--
--  So this index arithmetic is safety-critical driver logic in the sense
--  design.txt (lines 84-95) means: it is what stands between a misbehaving
--  device and kernel memory.  That is why it is here, in SPARK, rather than in
--  Rust -- Rust's type system does not stop `virt_base.add(idx * 16)` inside an
--  unsafe block from going anywhere, and this module's whole job is to prove
--  the index cannot.
--
--  THE DIVISION OF LABOUR WITH RUST
--
--  This package owns *indices and topology*, in kernel-private memory.  It
--  never dereferences anything and has no notion of an address.  Rust keeps the
--  MMIO and DMA -- it owns the pointer and does the volatile writes -- and asks
--  this package for every index decision.  The halves meet only at the C entry
--  points below.
--
--  Keeping the chain links here, rather than reading them back out of the
--  descriptor table, is the substantive change.  The old code kept the free
--  list in `desc.next` *inside the device-visible table*, so the structure
--  deciding which memory to touch next lived where a device could scribble on
--  it.  Here the links are a private array and the descriptor table becomes a
--  write-only rendering of state whose authoritative copy the device cannot
--  reach.
--
--  WHAT IS ACTUALLY PROVED
--
--  `gnatprove` discharges absence of run-time errors: no overflow, no array
--  index outside its range, on every path, for every input -- including inputs
--  no test supplies, which is the point.  Because the exported operations take
--  raw `U16`s and every path through them is checked, "the device sent
--  nonsense" is not a special case; it is the same proof.
--
--  THE C BOUNDARY IS UNCONSTRAINED ON PURPOSE
--
--  Every exported subprogram takes plain `U16`, never a constrained subtype
--  like `Queue_Id`.  A constrained parameter would push the range check onto
--  the *caller* and SPARK would discharge it by assuming the caller complies --
--  an assumption a C caller never agreed to and a hostile device has no reason
--  to honour.  Taking the wide type and validating in the body puts the check
--  on the side that can be proved to perform it.
--
--  NO SIDE-EFFECTING FUNCTIONS
--
--  Everything that changes state is a procedure with an `out` parameter, never
--  a function returning a status.  SPARK functions must be free of side
--  effects; making these functions would need `Side_Effects => True` and buy
--  nothing but a tidier C signature.
------------------------------------------------------------------------------

package Virtqueue_Descriptors with
  SPARK_Mode     => On,
  Abstract_State => Pool,
  Initializes    => Pool
is
   --  Plain modular types rather than Interfaces.Unsigned_16: the ZFP runtime
   --  is a single system.ads (kernel/ada/rts/), so package Interfaces does not
   --  exist here.  Modular types are compiler-intrinsic and need no runtime.
   type U8  is mod 2 **  8 with Size =>  8;
   type U16 is mod 2 ** 16 with Size => 16;

   --  Compile-time bounds on statically allocated state.  256 covers every
   --  queue size the in-tree drivers negotiate (blk, net, gpu, sound) and 16
   --  covers their total queue count with room spare.  There is no allocator in
   --  this package and no path that could need one.
   Max_Queues      : constant := 16;
   Max_Descriptors : constant := 256;

   --  Returned where a descriptor index is expected but none exists.  Equal to
   --  the virtio spec's chain terminator and outside any queue's index range by
   --  construction, so it can never be mistaken for a real descriptor.
   No_Descriptor : constant U16 := 16#FFFF#;

   --  Status codes shared with the Rust side.  kernel/src/ada.rs keeps a
   --  matching enum; the round-trip self-test catches any drift.
   Status_Ok            : constant U8 := 0;
   Status_Bad_Queue     : constant U8 := 1;  -- queue id out of range
   Status_Bad_Size      : constant U8 := 2;  -- size 0, > Max, or not a 2**n
   Status_Bad_Index     : constant U8 := 3;  -- descriptor index >= queue size
   Status_Not_Allocated : constant U8 := 4;  -- index is not currently in use
   Status_Exhausted     : constant U8 := 5;  -- no free descriptors

   ---------------------------------------------------------------------------
   --  Queries.  Declared first because the contracts below are written in
   --  terms of Size_Of.
   ---------------------------------------------------------------------------

   --  Number of descriptors in the queue; 0 if the id is invalid or the queue
   --  has not been initialised.  0 makes every other operation reject, so an
   --  uninitialised queue fails closed.
   function Size_Of (Queue : U16) return U16 with
     Global => (Input => Pool),
     Post   => Size_Of'Result <= Max_Descriptors,
     Export => True, Convention => C,
     External_Name => "vqd_size_of";

   --  How many descriptors are currently free.
   --
   --  The bound here is Max_Descriptors, not Size_Of (Queue), even though the
   --  tighter one is true.  Proving `Free_Cnt <= Size` needs it carried as an
   --  invariant across every operation, and the body deliberately has no record
   --  predicate (see the body's note).  Nothing depends on the tighter bound:
   --  it is a statistic for the Rust side's backpressure check, not a value
   --  that becomes an index.
   function Free_Count (Queue : U16) return U16 with
     Global => (Input => Pool),
     Post   => Free_Count'Result <= Max_Descriptors,
     Export => True, Convention => C,
     External_Name => "vqd_free_count";

   --  1 if `Index` is a currently-allocated descriptor of this queue, else 0.
   function Is_Allocated (Queue : U16; Index : U16) return U8 with
     Global => (Input => Pool),
     Post   => Is_Allocated'Result in 0 | 1,
     Export => True, Convention => C,
     External_Name => "vqd_is_allocated";

   ---------------------------------------------------------------------------
   --  Lifecycle
   ---------------------------------------------------------------------------

   --  Initialise queue `Queue` with `Size` descriptors, all free.  Also the
   --  reset path: virtio drivers reset a queue after a request times out,
   --  because a timed-out request leaves descriptors owned by the device.
   --  Idempotent.
   procedure Initialize (Queue : U16; Size : U16; Status : out U8) with
     Global => (In_Out => Pool),
     Post   => Status in Status_Ok | Status_Bad_Queue | Status_Bad_Size,
     Export => True, Convention => C,
     External_Name => "vqd_initialize";

   --  Return `Queue` to the uninitialised state, in which every operation on
   --  it rejects.  This is not Initialize with a size of zero -- that is
   --  refused, because a driver asking for a zero-sized queue has a bug -- it
   --  is the deliberate teardown a driver performs when its device goes away.
   --
   --  Without it there is no way back to fail-closed once a queue has been
   --  used.  A hot-unplugged device would leave its queue looking valid, and
   --  the next completion naming one of its stale descriptor indices would be
   --  answered as though the queue were still live.  Post-condition says so
   --  directly: afterwards the queue has no descriptors, so `Free_Chain` and
   --  `Allocate` reject on the size test before reaching anything else.
   procedure Reset (Queue : U16) with
     Global => (In_Out => Pool),
     Post   => Size_Of (Queue) = 0,
     Export => True, Convention => C,
     External_Name => "vqd_reset";

   ---------------------------------------------------------------------------
   --  Allocation
   ---------------------------------------------------------------------------

   --  Take a descriptor off the free list.  `Index` is No_Descriptor if the
   --  queue id is invalid, the queue is uninitialised, or nothing is free --
   --  the caller cannot tell those apart and does not need to; all three mean
   --  "do not submit".
   --
   --  The postcondition is the load-bearing one: the result is either
   --  No_Descriptor or a genuine index into this queue.  Rust's unsafe pointer
   --  arithmetic is justified by it.
   procedure Allocate (Queue : U16; Index : out U16) with
     Global => (In_Out => Pool),
     Post   => Index = No_Descriptor or else Index < Size_Of (Queue),
     Export => True, Convention => C,
     External_Name => "vqd_allocate";

   --  Record, in the private link array, that descriptor `From` chains to
   --  descriptor `To`.  Both must be allocated descriptors of this queue.
   procedure Link (Queue : U16; From : U16; To : U16; Status : out U8) with
     Global => (In_Out => Pool),
     Post   => Status in Status_Ok | Status_Bad_Queue | Status_Bad_Index
                       | Status_Not_Allocated,
     Export => True, Convention => C,
     External_Name => "vqd_link";

   --  Mark `Index` as the last descriptor of its chain.
   procedure Terminate_Chain (Queue : U16; Index : U16; Status : out U8) with
     Global => (In_Out => Pool),
     Post   => Status in Status_Ok | Status_Bad_Queue | Status_Bad_Index
                       | Status_Not_Allocated,
     Export => True, Convention => C,
     External_Name => "vqd_terminate_chain";

   ---------------------------------------------------------------------------
   --  Completion
   ---------------------------------------------------------------------------

   --  Free the chain whose head is `Head`, returning its descriptors to the
   --  free list.  `Freed` is the number returned; **0 means the chain was
   --  rejected and nothing changed**.
   --
   --  This is the entry point fed device-controlled data.  `Head` comes from
   --  the used ring, so it is attacker-controlled in the threat model, and each
   --  way it can be wrong is answered here rather than by the caller:
   --
   --    * out of range for this queue          -> rejected, 0
   --    * in range but not allocated           -> rejected, 0 (double completion)
   --    * a chain longer than the queue        -> rejected, 0 (cycle)
   --
   --  The walk is a bounded `for` loop rather than `loop ... end loop`, so
   --  termination is structural instead of something a prover must establish:
   --  a cyclic chain cannot hang the kernel even if one were constructed.
   procedure Free_Chain (Queue : U16; Head : U16; Freed : out U16) with
     Global => (In_Out => Pool),
     Post   => Freed <= Size_Of (Queue),
     Export => True, Convention => C,
     External_Name => "vqd_free_chain";

end Virtqueue_Descriptors;
