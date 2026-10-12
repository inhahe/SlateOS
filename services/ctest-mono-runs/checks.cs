// What services/ctest-mono-runs asks Mono to do on SlateOS, in order, each
// adding one thing to the one before; one line per check, compared exactly
// by the fixture, so a run that stops says where.
//
//   1. start: the runtime loads mscorlib, JIT-compiles Main and writes a line.
//   2. arithmetic and strings through the JIT: a computed line, not a constant.
//   3. a managed exception thrown and caught: the runtime's unwinder.
//   4. a null dereference caught as NullReferenceException: the hardware
//      fault, delivered as SIGSEGV with a context the runtime's handler
//      rewrites, so that the thread resumes at code that throws. The check
//      no other here makes, and the one most likely to fail first.
//   5. integer division by zero caught as DivideByZeroException: SIGFPE, the
//      same path.
//   6. the collector: allocate far more than the heap starts with, collect,
//      and see that the objects still reachable are intact.
//   7. a second thread started and joined: the runtime's threads, and the
//      collector stopping a thread at a safepoint (cooperative suspend).
//   8. C called by name: DllImport("libc"), which Mono's config maps to
//      libc.so.6. Here that opens the program itself, and dlsym finds the
//      function in the symbol table mono-sgen exports (design-decisions
//      1184): strlen of a marshalled string, and getpid.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;

static class Checks
{
    static int Main()
    {
        Console.WriteLine("mono: started");

        int sum = 0;
        for (int i = 1; i <= 100; i++)
            sum += i;
        Console.WriteLine("mono: sum " + sum.ToString() + " " + string.Concat("ab", "cd").ToUpperInvariant());

        try
        {
            throw new InvalidOperationException("thrown");
        }
        catch (InvalidOperationException e)
        {
            Console.WriteLine("mono: caught " + e.Message);
        }

        try
        {
            object o = Nothing();
            Console.WriteLine("mono: null not caught " + o.GetHashCode().ToString());
        }
        catch (NullReferenceException)
        {
            Console.WriteLine("mono: caught a null reference");
        }

        try
        {
            int zero = Zero();
            Console.WriteLine("mono: division not caught " + (7 / zero).ToString());
        }
        catch (DivideByZeroException)
        {
            Console.WriteLine("mono: caught a division by zero");
        }

        var keep = new List<byte[]>();
        for (int i = 0; i < 2000; i++)
        {
            var block = new byte[64 * 1024];
            block[0] = (byte)i;
            block[block.Length - 1] = (byte)(i * 7);
            if (i % 100 == 0)
                keep.Add(block);
        }
        GC.Collect();
        GC.WaitForPendingFinalizers();
        bool intact = keep.Count == 20;
        for (int k = 0; k < keep.Count && intact; k++)
        {
            int i = k * 100;
            intact = keep[k][0] == (byte)i && keep[k][keep[k].Length - 1] == (byte)(i * 7);
        }
        Console.WriteLine("mono: collected, " + (intact ? "kept intact" : "KEPT DAMAGED"));

        int seen = 0;
        var t = new Thread(() =>
        {
            var junk = new List<object>();
            for (int i = 0; i < 100000; i++)
                junk.Add(new object());
            Interlocked.Exchange(ref seen, junk.Count);
        });
        t.Start();
        GC.Collect();
        t.Join();
        Console.WriteLine("mono: thread joined, " + seen.ToString());

        Console.WriteLine("mono: libc strlen " + strlen("hello").ToString());
        Console.WriteLine("mono: libc getpid " + (getpid() > 0 ? "positive" : "NOT POSITIVE"));

        Console.WriteLine("mono: done");
        return 0;
    }

    [DllImport("libc", CharSet = CharSet.Ansi)]
    static extern UIntPtr strlen(string s);

    [DllImport("libc")]
    static extern int getpid();

    // Kept out of line so the JIT cannot see the null or the zero and fold
    // the fault away.
    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    static object Nothing() { return null; }

    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    static int Zero() { return 0; }
}
