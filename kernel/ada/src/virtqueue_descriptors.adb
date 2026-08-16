------------------------------------------------------------------------------
--  Body.  See the spec for what this is for and why it is in SPARK.
--
--  TWO CHOICES WORTH KNOWING ABOUT BEFORE READING
--
--  1. Every array access is guarded by an explicit `< Size` test at the point
--     of use, rather than by a quantified invariant over the link array.
--
--     A quantified predicate ("every link is No_Descriptor or in range") would
--     also discharge the proof, and would be the tidier thing to write.  The
--     explicit tests are chosen deliberately: an invariant is a promise about
--     what *this package* writes, and this is a kernel, where nothing prevents
--     an unrelated wild write from landing in the middle of these arrays.  A
--     proof resting on the invariant would then be sound about the code and
--     wrong about the machine.  Re-checking at each step costs one compare and
--     is true regardless of how the array came to hold what it holds.
--
--     It also keeps every verification condition local, which is why this
--     proves at level 1 in seconds instead of needing a loop invariant.
--
--  2. Free_Chain validates the whole chain before freeing any of it.
--
--     The alternative -- free as you walk, stop when you hit something bad --
--     leaves the queue in a state that is neither the old one nor a good one,
--     with half a chain returned to the free list, after being handed input a
--     device chose.  Validating first makes rejection total: `Freed = 0` means
--     nothing moved.  It costs a second walk of a chain that is at most a few
--     descriptors long, on the completion path, which is not where the time
--     goes.
------------------------------------------------------------------------------

package body Virtqueue_Descriptors with
  SPARK_Mode     => On,
  Refined_State  => (Pool => Queues)
is
   subtype Queue_Index is U16 range 0 .. Max_Queues - 1;
   subtype Desc_Index  is U16 range 0 .. Max_Descriptors - 1;

   type Desc_State  is (Free, Allocated);
   type State_Array is array (Desc_Index) of Desc_State;
   type Link_Array  is array (Desc_Index) of U16;

   --  Deliberately no record predicate.  `Free_Cnt <= Size` and `Size <=
   --  Max_Descriptors` are both true by construction, but stating them as a
   --  Dynamic_Predicate would make them checked on assignment, and these
   --  records are updated a component at a time -- Initialize sets Size before
   --  it sets Free_Cnt, so the predicate would be transiently false through no
   --  fault of the logic.  The safety property that actually matters (no index
   --  ever leaves its range) is established by the tests at each point of use
   --  and needs no invariant; see note 1 above and Size_Of below.
   type Queue_Record is record
      Size     : U16 := 0;
      Head     : U16 := 0;
      Free_Cnt : U16 := 0;
      States   : State_Array := (others => Free);
      Links    : Link_Array  := (others => 0);
   end record;

   type Queue_Array is array (Queue_Index) of Queue_Record;

   --  Every default above is the zero bit pattern, so the table itself lands in
   --  .bss (12384 bytes) rather than .data.  GNAT nonetheless emits an
   --  elaboration body, `virtqueue_descriptors___elabb`, that zeroes it
   --  explicitly -- checked with objdump, it is a rep-stos loop over all 16
   --  records.  On a kernel whose loader already zeroes .bss that is redundant,
   --  but kernel/src/ada.rs calls it during init anyway: "redundant" is a
   --  property of today's loader, and a package that silently depends on
   --  someone else's zeroing is a package that breaks when that changes.
   --
   --  `Size = 0` is the uninitialised marker, and because every operation tests
   --  against Size, an uninitialised queue fails closed rather than needing a
   --  separate "valid" flag that could disagree with it.
   Queues : Queue_Array;

   ---------------------------------------------------------------------------
   --  Queries
   --
   --  Both are *expression* functions, and both clamp what they read.  Two
   --  separate reasons, and they reinforce each other:
   --
   --  Expression, because SPARK treats an expression function's body as an
   --  implicit postcondition, so `Size_Of (Queue)` is transparent at every call
   --  site -- including in Allocate's postcondition, which is stated in terms
   --  of it.  Written as an ordinary function body it would be opaque, and the
   --  contract that justifies Rust's pointer arithmetic could not be proved.
   --
   --  Clamping, because it discharges `Size <= Max_Descriptors` *without
   --  assuming anything about the stored value*.  The alternative was a
   --  constrained component subtype, which would give the prover the same fact
   --  by promising that only this package ever writes the field -- the promise
   --  note 1 above declines to make.  Here the bound is a property of the
   --  reader, not of the writer: a Size the kernel somehow corrupted to 300
   --  reads back as 0, and 0 makes every operation reject.  Fails closed, and
   --  the proof stays true of the machine rather than only of the code.
   ---------------------------------------------------------------------------

   function Size_Of (Queue : U16) return U16 is
     (if Queue > Queue_Index'Last then 0
      elsif Queues (Queue_Index (Queue)).Size > Max_Descriptors then 0
      else Queues (Queue_Index (Queue)).Size);

   function Free_Count (Queue : U16) return U16 is
     (if Queue > Queue_Index'Last then 0
      elsif Queues (Queue_Index (Queue)).Free_Cnt > Max_Descriptors then 0
      else Queues (Queue_Index (Queue)).Free_Cnt);

   --  There is deliberately no separate Valid_Queue predicate.  "Is this queue
   --  usable?" and "how big is it?" are the same question here -- Size_Of folds
   --  in the id range test, the initialised test and the clamp, and returns 0
   --  for all three failures.  Every entry point below therefore opens with a
   --  single `Size_Of` and tests `Size = 0`, so there is no second condition
   --  that could drift out of step with the first.
   function Is_Allocated (Queue : U16; Index : U16) return U8 is
      --  Read the size once, through the clamp, and test against that -- never
      --  against Q.Size directly.  Index < Size and Size <= Max_Descriptors is
      --  what puts Index inside Desc_Index, and it is the clamp that supplies
      --  the second half.
      Size : constant U16 := Size_Of (Queue);
   begin
      if Size = 0 or else Index >= Size then
         return 0;
      end if;

      if Queues (Queue_Index (Queue)).States (Desc_Index (Index)) = Allocated
      then
         return 1;
      end if;
      return 0;
   end Is_Allocated;

   ---------------------------------------------------------------------------
   --  Lifecycle
   ---------------------------------------------------------------------------

   procedure Initialize (Queue : U16; Size : U16; Status : out U8) is
   begin
      if Queue > Queue_Index'Last then
         Status := Status_Bad_Queue;
         return;
      end if;

      --  Size must be a power of two in 1 .. Max_Descriptors.  Virtio requires
      --  a power of two, and the ring-index masking in the Rust half depends on
      --  it; rejecting here means that half never has to wonder.
      if Size = 0
        or else Size > Max_Descriptors
        or else (Size and (Size - 1)) /= 0
      then
         Status := Status_Bad_Size;
         return;
      end if;

      declare
         Q : Queue_Record renames Queues (Queue_Index (Queue));
      begin
         Q.Size     := Size;
         Q.States   := (others => Free);
         Q.Links    := (others => 0);

         --  Free list is 0 -> 1 -> ... -> Size-1 -> No_Descriptor.  Built in
         --  ascending order so a fresh queue hands out descriptor 0 first,
         --  which makes boot-time traces reproducible.
         for I in Desc_Index range 0 .. Desc_Index (Size - 1) loop
            if I = Size - 1 then
               Q.Links (I) := No_Descriptor;
            else
               Q.Links (I) := I + 1;
            end if;
         end loop;

         Q.Head     := 0;
         Q.Free_Cnt := Size;
      end;

      Status := Status_Ok;
   end Initialize;

   procedure Reset (Queue : U16) is
   begin
      if Queue > Queue_Index'Last then
         return;   -- nothing to reset; Size_Of already reads 0 for this id
      end if;

      --  Size is the whole of the state that matters: with Size = 0 every entry
      --  point rejects before it reads States or Links.  They are cleared
      --  anyway, so that a torn-down queue does not sit in memory holding a map
      --  of descriptors marked Allocated -- state that is inert today only
      --  because of a test in another subprogram, which is the kind of coupling
      --  that stops being true quietly.  The cost is 774 bytes of stores on a
      --  device-teardown path.
      declare
         Q : Queue_Record renames Queues (Queue_Index (Queue));
      begin
         Q.Size     := 0;
         Q.Head     := 0;
         Q.Free_Cnt := 0;
         Q.States   := (others => Free);
         Q.Links    := (others => 0);
      end;
   end Reset;

   ---------------------------------------------------------------------------
   --  Allocation
   ---------------------------------------------------------------------------

   procedure Allocate (Queue : U16; Index : out U16) is
      Size : constant U16 := Size_Of (Queue);
   begin
      Index := No_Descriptor;

      if Size = 0 then
         return;
      end if;

      declare
         Q    : Queue_Record renames Queues (Queue_Index (Queue));
         Head : constant U16 := Q.Head;
      begin
         if Q.Free_Cnt = 0 then
            return;
         end if;

         --  Fail closed if the free-list head is not a real descriptor.  With
         --  Free_Cnt > 0 this should be unreachable; it is checked anyway
         --  because it is the value that becomes an array index, and because
         --  returning No_Descriptor here costs a stalled request whereas
         --  trusting it costs a wild access.
         if Head >= Size then
            return;
         end if;

         declare
            H : constant Desc_Index := Desc_Index (Head);
         begin
            --  Advance the free list before handing the descriptor out.  The
            --  successor is only adopted if it is itself in range or the
            --  terminator, so a damaged link truncates the free list rather
            --  than aiming it somewhere.
            if Q.Links (H) = No_Descriptor or else Q.Links (H) >= Size then
               Q.Head := 0;
            else
               Q.Head := Q.Links (H);
            end if;

            Q.States (H) := Allocated;
            Q.Links  (H) := No_Descriptor;
            Q.Free_Cnt   := Q.Free_Cnt - 1;
            Index        := Head;
         end;
      end;
   end Allocate;

   procedure Link (Queue : U16; From : U16; To : U16; Status : out U8) is
      Size : constant U16 := Size_Of (Queue);
   begin
      if Size = 0 then
         Status := Status_Bad_Queue;
         return;
      end if;

      declare
         Q : Queue_Record renames Queues (Queue_Index (Queue));
      begin
         if From >= Size or else To >= Size then
            Status := Status_Bad_Index;
            return;
         end if;

         --  Both ends must be in use.  Chaining to a free descriptor would
         --  build a chain that Free_Chain will later refuse to free, stranding
         --  the descriptors -- better to refuse to build it.
         if Q.States (Desc_Index (From)) /= Allocated
           or else Q.States (Desc_Index (To)) /= Allocated
         then
            Status := Status_Not_Allocated;
            return;
         end if;

         Q.Links (Desc_Index (From)) := To;
      end;

      Status := Status_Ok;
   end Link;

   procedure Terminate_Chain (Queue : U16; Index : U16; Status : out U8) is
      Size : constant U16 := Size_Of (Queue);
   begin
      if Size = 0 then
         Status := Status_Bad_Queue;
         return;
      end if;

      declare
         Q : Queue_Record renames Queues (Queue_Index (Queue));
      begin
         if Index >= Size then
            Status := Status_Bad_Index;
            return;
         end if;

         if Q.States (Desc_Index (Index)) /= Allocated then
            Status := Status_Not_Allocated;
            return;
         end if;

         Q.Links (Desc_Index (Index)) := No_Descriptor;
      end;

      Status := Status_Ok;
   end Terminate_Chain;

   ---------------------------------------------------------------------------
   --  Completion
   ---------------------------------------------------------------------------

   procedure Free_Chain (Queue : U16; Head : U16; Freed : out U16) is
      Size   : constant U16 := Size_Of (Queue);
      Length : U16 := 0;
   begin
      Freed := 0;

      if Size = 0 then
         return;
      end if;

      declare
         Q     : Queue_Record renames Queues (Queue_Index (Queue));
         Cur   : U16;
         Valid : Boolean := False;
      begin
         --  Pass 1: validate, mutating nothing.  `Head` is device-supplied, so
         --  this pass is the one that matters.
         if Head >= Size then
            return;                          -- out of range for this queue
         end if;
         if Q.States (Desc_Index (Head)) /= Allocated then
            return;                          -- never issued, or already freed
         end if;

         Cur := Head;
         --  Bounded by the queue size: a chain cannot legitimately be longer
         --  than the number of descriptors, so exceeding it means a cycle.  The
         --  bound is what makes termination structural rather than a proof
         --  obligation -- see the spec.  Falling out of the loop without
         --  setting Valid (a chain that never terminated) is therefore a
         --  rejection, which is the behaviour a cycle should get.
         Scan : for Step in U16 range 1 .. Size loop
            pragma Loop_Invariant (Cur < Size);
            pragma Loop_Invariant (Length = Step - 1);

            Length := Length + 1;

            declare
               Next : constant U16 := Q.Links (Desc_Index (Cur));
            begin
               if Next = No_Descriptor then
                  Valid := True;
                  exit Scan;
               end if;

               --  A link that is neither the terminator nor a valid, allocated
               --  descriptor of this queue means the chain is malformed.  Leave
               --  Valid false and stop; the queue is untouched.
               exit Scan when Next >= Size;
               exit Scan when Q.States (Desc_Index (Next)) /= Allocated;

               Cur := Next;
            end;
         end loop Scan;

         if not Valid then
            return;
         end if;

         --  Pass 2: free.  Every index visited here was proved in range by pass
         --  1, but the range tests are repeated rather than assumed, because
         --  the cost is a compare and the alternative is a wild write.
         Cur := Head;
         Release : for Step in U16 range 1 .. Length loop
            pragma Loop_Invariant (Cur < Size);
            --  Q.Size is untouched by this loop, so the clamped read that gave
            --  us Size still gives it.  Stated because Freed's contract is in
            --  terms of Size_Of, which reads the record we are writing to.
            pragma Loop_Invariant (Size_Of (Queue) = Size);

            declare
               C    : constant Desc_Index := Desc_Index (Cur);
               Next : constant U16        := Q.Links (C);
            begin
               Q.States (C) := Free;
               Q.Links  (C) := Q.Head;
               Q.Head       := Cur;
               if Q.Free_Cnt < Size then
                  Q.Free_Cnt := Q.Free_Cnt + 1;
               end if;

               exit Release when Next = No_Descriptor or else Next >= Size;
               Cur := Next;
            end;
         end loop Release;

         Freed := Length;
      end;
   end Free_Chain;

end Virtqueue_Descriptors;
