# The video player's test films

## `red-then-blue.webm`

A WebM film of 16 by 16 pixels, ten frames a second for 0.8 seconds: four
red frames, then four blue -- so that a test can tell which picture is on
screen from its colour, and so where the clock is. VP9, lossless, a key frame
every four frames (at 0 and 0.4 seconds), so that a seek between them has to
decode from the one before it. Title "Red then blue". 756 bytes.

Made with ffmpeg (2026-03-09, git 9b7439c31b, with libvpx):

```sh
ffmpeg -f lavfi -i "color=c=red:s=16x16:r=10:d=0.4" \
       -f lavfi -i "color=c=blue:s=16x16:r=10:d=0.4" \
       -filter_complex "[0:v][1:v]concat=n=2:v=1[v]" -map "[v]" \
       -c:v libvpx-vp9 -lossless 1 -g 4 -keyint_min 4 -pix_fmt yuv420p -an \
       -metadata title="Red then blue" red-then-blue.webm
```

As ffprobe reads it:

| Frame | Time (s) | Key frame | Colour |
|---|---|---|---|
| 0 | 0.0 | yes | red |
| 1 | 0.1 | | red |
| 2 | 0.2 | | red |
| 3 | 0.3 | | red |
| 4 | 0.4 | yes | blue |
| 5 | 0.5 | | blue |
| 6 | 0.6 | | blue |
| 7 | 0.7 | | blue |

Each frame lasts 0.1 seconds; the file says it plays for 0.8.
