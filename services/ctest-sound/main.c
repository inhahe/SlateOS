/*
 * ctest-sound -- can a native SlateOS program drive the sound card through
 * the ALSA interface a Linux program uses?
 *
 * A program plays sound by opening /dev/snd/pcmC0D0p and talking ALSA to it:
 * ioctls that negotiate a format (HW_PARAMS), prepare the stream, and write
 * frames (WRITEI_FRAMES), with poll telling it when the ring has room. The
 * card's control device, /dev/snd/controlC0, is what alsa-lib opens first,
 * to find the card. Until lane A's device door (SYS_DEVICE_*, 1119-1123) a
 * native program could not open either -- the C library answered ENOENT,
 * and its ioctl answered every request but a terminal's with ENOTTY
 * (requests/e-ad-no-application-can-reach-the-sound-device.md).
 *
 * The checks, in order, follow the calls alsa-lib makes:
 *
 *    1x  the control device opens, speaks a protocol version, and names
 *        card 0
 *    2x  the playback device opens, speaks a protocol version, and is a
 *        character device of ALSA's major number, 116
 *    3x  a format is negotiated: interleaved signed 16-bit stereo at 48 kHz
 *        (HW_REFINE, HW_PARAMS), with a buffer and a period the device chose
 *    4x  the stream is prepared, takes a period of silence, and poll says
 *        when it has room again
 *    5x  what is the device's and what is the descriptor's: a request the
 *        device does not know is ENOTTY; the control device, subscribed to
 *        no events, reads EBADFD and polls as having nothing to report
 *        (Linux 6.6's snd_ctl_read and snd_ctl_poll); neither seeks; FIONBIO,
 *        FIOCLEX and FIONCLEX are the descriptor's, and work
 *
 * Exit codes: 42 is every check passed.
 *    2  a setup call failed     Otherwise <check><step>; no step is 42.
 *
 * On Linux it needs a sound card: without /dev/snd it says so and passes,
 * having nothing to check. On SlateOS the boot test's QEMU has one, so there
 * it must find it.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <sound/asound.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/utsname.h>
#include <unistd.h>

static void emit(const char *s)
{
    size_t n = strlen(s);
    if (n != 0) {
        ssize_t written = write(1, s, n);
        (void)written;
    }
}

static void emit_num(long n)
{
    char buf[24];
    int i = (int)sizeof buf;
    int neg = n < 0;
    unsigned long v = neg ? 0UL - (unsigned long)n : (unsigned long)n;
    buf[--i] = '\0';
    do {
        buf[--i] = (char)('0' + v % 10);
        v /= 10;
    } while (v != 0 && i > 1);
    if (neg) {
        buf[--i] = '-';
    }
    emit(buf + i);
}

static int fail(int code, const char *what)
{
    int e = errno;
    emit("ctest-sound: FAIL ");
    emit_num(code);
    emit(": ");
    emit(what);
    emit(" (errno ");
    emit_num(e);
    emit(")\n");
    return code;
}

static void ok(const char *what)
{
    emit("ctest-sound: ok ");
    emit(what);
    emit("\n");
}

static int on_linux(void)
{
    struct utsname u;
    return uname(&u) == 0 && strcmp(u.sysname, "Linux") == 0;
}

/* hw_params with every value allowed, as alsa-lib's snd_pcm_hw_params_any
 * starts it. */
static void hw_any(struct snd_pcm_hw_params *p)
{
    memset(p, 0, sizeof *p);
    for (int k = SNDRV_PCM_HW_PARAM_FIRST_MASK; k <= SNDRV_PCM_HW_PARAM_LAST_MASK; k++) {
        struct snd_mask *m = &p->masks[k - SNDRV_PCM_HW_PARAM_FIRST_MASK];
        memset(m->bits, 0xff, sizeof m->bits);
    }
    for (int k = SNDRV_PCM_HW_PARAM_FIRST_INTERVAL; k <= SNDRV_PCM_HW_PARAM_LAST_INTERVAL; k++) {
        struct snd_interval *i = &p->intervals[k - SNDRV_PCM_HW_PARAM_FIRST_INTERVAL];
        i->min = 0;
        i->max = ~0U;
    }
    p->rmask = ~0U;
    p->cmask = 0;
    p->info = ~0U;
}

/* Allow only `value` for the mask parameter `param`. */
static void mask_only(struct snd_pcm_hw_params *p, int param, unsigned value)
{
    struct snd_mask *m = &p->masks[param - SNDRV_PCM_HW_PARAM_FIRST_MASK];
    memset(m->bits, 0, sizeof m->bits);
    m->bits[value >> 5] |= 1U << (value & 31);
}

/* Allow only `value` for the interval parameter `param`. */
static void interval_only(struct snd_pcm_hw_params *p, int param, unsigned value)
{
    struct snd_interval *i = &p->intervals[param - SNDRV_PCM_HW_PARAM_FIRST_INTERVAL];
    i->min = value;
    i->max = value;
    i->openmin = 0;
    i->openmax = 0;
    i->integer = 1;
    i->empty = 0;
}

static unsigned interval_min(const struct snd_pcm_hw_params *p, int param)
{
    return p->intervals[param - SNDRV_PCM_HW_PARAM_FIRST_INTERVAL].min;
}

/* Frames of 16-bit stereo silence: comfortably more than one period. */
static short silence[2 * 8192];

int main(void)
{
    /* 1. */
    int ctl = open("/dev/snd/controlC0", O_RDWR | O_CLOEXEC);
    if (ctl < 0) {
        if (on_linux() && (errno == ENOENT || errno == ENODEV)) {
            emit("ctest-sound: no sound card on this Linux; nothing to check\n");
            return 42;
        }
        return fail(11, "open /dev/snd/controlC0");
    }
    int version = 0;
    if (ioctl(ctl, SNDRV_CTL_IOCTL_PVERSION, &version) != 0
        || SNDRV_PROTOCOL_MAJOR(version) != 2) {
        return fail(12, "SNDRV_CTL_IOCTL_PVERSION is a 2.x protocol");
    }
    struct snd_ctl_card_info card;
    memset(&card, 0, sizeof card);
    if (ioctl(ctl, SNDRV_CTL_IOCTL_CARD_INFO, &card) != 0 || card.card != 0) {
        return fail(13, "SNDRV_CTL_IOCTL_CARD_INFO names card 0");
    }
    ok("the control device opens, and names card 0");

    /* 2. */
    int pcm = open("/dev/snd/pcmC0D0p", O_RDWR | O_CLOEXEC);
    if (pcm < 0) {
        return fail(21, "open /dev/snd/pcmC0D0p");
    }
    version = 0;
    if (ioctl(pcm, SNDRV_PCM_IOCTL_PVERSION, &version) != 0
        || SNDRV_PROTOCOL_MAJOR(version) != 2) {
        return fail(22, "SNDRV_PCM_IOCTL_PVERSION is a 2.x protocol");
    }
    /* The minor number is the kernel's to choose (CONFIG_SND_DYNAMIC_MINORS
     * numbers them as they come); the major is ALSA's. */
    struct stat st;
    if (fstat(pcm, &st) != 0 || !S_ISCHR(st.st_mode) || major(st.st_rdev) != 116) {
        return fail(23, "fstat: a character device, major 116");
    }
    ok("the playback device opens, and is ALSA's character device");

    /* 3. */
    struct snd_pcm_hw_params hw;
    hw_any(&hw);
    mask_only(&hw, SNDRV_PCM_HW_PARAM_ACCESS, SNDRV_PCM_ACCESS_RW_INTERLEAVED);
    mask_only(&hw, SNDRV_PCM_HW_PARAM_FORMAT, SNDRV_PCM_FORMAT_S16_LE);
    mask_only(&hw, SNDRV_PCM_HW_PARAM_SUBFORMAT, SNDRV_PCM_SUBFORMAT_STD);
    interval_only(&hw, SNDRV_PCM_HW_PARAM_CHANNELS, 2);
    interval_only(&hw, SNDRV_PCM_HW_PARAM_RATE, 48000);
    if (ioctl(pcm, SNDRV_PCM_IOCTL_HW_REFINE, &hw) != 0) {
        return fail(31, "SNDRV_PCM_IOCTL_HW_REFINE of S16_LE stereo 48 kHz");
    }
    if (ioctl(pcm, SNDRV_PCM_IOCTL_HW_PARAMS, &hw) != 0) {
        return fail(32, "SNDRV_PCM_IOCTL_HW_PARAMS");
    }
    unsigned period = interval_min(&hw, SNDRV_PCM_HW_PARAM_PERIOD_SIZE);
    unsigned buffer = interval_min(&hw, SNDRV_PCM_HW_PARAM_BUFFER_SIZE);
    if (period == 0 || buffer < period || period > sizeof silence / sizeof silence[0] / 2) {
        emit("ctest-sound: period ");
        emit_num((long)period);
        emit(" buffer ");
        emit_num((long)buffer);
        emit("\n");
        return fail(33, "the device chose a period and a buffer of it");
    }
    ok("S16_LE stereo at 48 kHz is negotiated");

    /* 4. */
    struct snd_pcm_sw_params sw;
    memset(&sw, 0, sizeof sw);
    sw.tstamp_mode = SNDRV_PCM_TSTAMP_NONE;
    sw.period_step = 1;
    sw.avail_min = period;
    sw.start_threshold = period;
    sw.stop_threshold = buffer;
    sw.boundary = buffer;
    while (sw.boundary * 2 <= 0x7fffffffUL - buffer) {
        sw.boundary *= 2;
    }
    if (ioctl(pcm, SNDRV_PCM_IOCTL_SW_PARAMS, &sw) != 0) {
        return fail(41, "SNDRV_PCM_IOCTL_SW_PARAMS");
    }
    if (ioctl(pcm, SNDRV_PCM_IOCTL_PREPARE) != 0) {
        return fail(43, "SNDRV_PCM_IOCTL_PREPARE");
    }
    struct snd_xferi xfer;
    memset(&xfer, 0, sizeof xfer);
    xfer.buf = silence;
    xfer.frames = period;
    if (ioctl(pcm, SNDRV_PCM_IOCTL_WRITEI_FRAMES, &xfer) != 0 || xfer.result <= 0) {
        return fail(44, "SNDRV_PCM_IOCTL_WRITEI_FRAMES of a period of silence");
    }
    struct pollfd p = { .fd = pcm, .events = POLLOUT };
    if (poll(&p, 1, 3000) != 1 || (p.revents & POLLOUT) == 0) {
        return fail(45, "poll says the ring has room again");
    }
    if (ioctl(pcm, SNDRV_PCM_IOCTL_DROP) != 0) {
        return fail(46, "SNDRV_PCM_IOCTL_DROP");
    }
    ok("a prepared stream takes a period, and poll says when it has room");

    /* 5. */
    errno = 0;
    if (ioctl(pcm, TCGETS, &version) != -1 || errno != ENOTTY) {
        return fail(51, "a terminal request on the sound device is ENOTTY");
    }
    /* No SNDRV_CTL_IOCTL_SUBSCRIBE_EVENTS was asked: there are no events to
     * read, which snd_ctl_read answers EBADFD before it looks at the count,
     * and nothing for snd_ctl_poll to report. */
    char c;
    errno = 0;
    if (read(ctl, &c, 1) != -1 || errno != EBADFD) {
        return fail(52, "a read from a control device subscribed to no events is EBADFD");
    }
    struct pollfd cp = { .fd = ctl, .events = POLLIN | POLLOUT };
    if (poll(&cp, 1, 0) != 0 || cp.revents != 0) {
        return fail(53, "an unsubscribed control device polls as having nothing to report");
    }
    errno = 0;
    if (lseek(pcm, 0, SEEK_SET) != -1 || errno != ESPIPE) {
        return fail(54, "the sound device does not seek: ESPIPE");
    }
    int on = 1;
    if (ioctl(pcm, FIONBIO, &on) != 0 || (fcntl(pcm, F_GETFL) & O_NONBLOCK) == 0) {
        return fail(55, "FIONBIO sets O_NONBLOCK");
    }
    if (ioctl(pcm, FIONCLEX) != 0 || (fcntl(pcm, F_GETFD) & FD_CLOEXEC) != 0
        || ioctl(pcm, FIOCLEX) != 0 || (fcntl(pcm, F_GETFD) & FD_CLOEXEC) == 0) {
        return fail(56, "FIONCLEX and FIOCLEX clear and set close-on-exec");
    }
    if (close(pcm) != 0 || close(ctl) != 0) {
        return fail(57, "close");
    }
    ok("the device's requests are the device's, and the descriptor's the descriptor's");

    emit("ctest-sound: every check passed\n");
    return 42;
}
