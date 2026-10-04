# Decision traces: finding where the encoder and libvpx part

The encoder's frames are checked byte for byte against `vpxenc`'s
(`tests/encoder.rs`). When a frame differs, the bytes say little about why:
one block that picked another mode changes every byte after it. A decision
trace says which block, and which candidate inside its search, went the
other way.

Both encoders write the same lines -- libvpx through the `VP9T` statements
`libvpx_trace_patch.py` adds to a copy of its source, the port through
`src/enc/trace.rs` (compiled into tests only) -- and `compare_traces.py`
reports the first line that differs, with the frame and superblock it is in.

## The lines

| Line | Written | Holds |
|---|---|---|
| `F` | once a frame, before its blocks | quantiser, multipliers, reference flags, noise estimate, scene change, cyclic refresh's state, partition thresholds, filter and transform modes |
| `S` | once a superblock (inter frames) | how it was partitioned (`p=` `C` copied, `E` the 64x64 early exit, `P` copied after the source SAD, `V` variance), its content state, low-variance flags, chroma sensitivity, skin, the vector it handed down |
| `L` | per low-variance check | the stale mode info `set_low_temp_var_flag` reads |
| `P` | once a block, before its search | the references searched, the low-variance skip, the intra penalty |
| `R` | per reference searched | nearest and near vectors, their context, the predicted vector's SAD |
| `M` | per inter candidate | its mode, vector, filter, transform, rate, distortion, cost, early stop |
| `N` | per motion search | the vector found and its rate |
| `I` | per intra candidate | its mode, transform, rate, distortion, cost |
| `B` | once a block, after its search | what was picked |
| `U` | once a block, under cyclic refresh | the segment it ended in |
| `E` | once a frame, after it is coded | its size, the refreshed segments' counts, golden refresh, low motion, buffer level |
| `G` | once a superblock (frames partitioned by search) | the content state, and the vector the superblock's prediction is estimated at |
| `Q` | per network prediction (the same) | the block and the network's score |
| `X` | per block the partition search finished (the same) | what it kept -- whole (`part=0`) or cut (`part=3`) -- and at what cost, or `none` |

libvpx also logs its key frames' superblocks; the port does not, since a key
frame's decisions are proved by its bytes. The comparer drops them.

## Doing it

In WSL (or any Linux), with libvpx v1.17.0's source in `~/vp9ref/libvpx`:

    cp -r ~/vp9ref/libvpx ~/vp9ref/libvpx-trace
    python3 tools/trace/libvpx_trace_patch.py ~/vp9ref/libvpx-trace
    mkdir ~/vp9ref/build-trace && cd ~/vp9ref/build-trace
    ../libvpx-trace/configure --target=generic-gnu --enable-vp9 --disable-vp9-highbitdepth
    make -j8

Encode the reference input with the command in
`tests/data/encoder/README.md`, run as `VP9TRACE=libvpx.trace
~/vp9ref/build-trace/vpxenc ...`. Its output must be the reference file
byte for byte (`cmp`): the trace statements only print.

Then the port's trace, from `gui/video/vp9`. `VP9_TRACE_INPUT` picks the
reference -- `rt8` (the default), `rt8cut` or `rt8small` -- and
`VP9_TRACE_FRAMES=N` stops after N frames:

    VP9_TRACE=rust.trace cargo test --release --target x86_64-pc-windows-gnu \
      -p vp9 --lib own_decisions_traced -- --ignored

and compare:

    python tools/trace/compare_traces.py libvpx.trace rust.trace --context 20

The two traces are identical on all three references: 365,835 lines on
`rt8.ivf`, 867,893 on `rt8cut.ivf`, 152,823 on `rt8small.ivf`.
