/* SPDX-License-Identifier: MIT */
/*
 * What the C half's own files share among themselves, beside Linux's
 * declarations (slx.h is the interface with the Rust half). It names Linux
 * types, so it is included after Linux's headers.
 */
#ifndef LX_EMUL_H
#define LX_EMUL_H

/* lx_emul.c: the first task's record, which `current` names while it runs. */
extern struct task_struct lx_main_task;
/* lx_emul.c: the record `current` names between tasks, Linux's idle task's. */
extern struct task_struct init_task;
/* lx_sched.c: the first task's handle, to wake it by. */
extern int lx_main_handle;

/* lx_emul.c: jiffies brought up to the monotonic clock, read now or `now_ns`. */
void lx_emul_jiffies_now(void);
void lx_emul_jiffies_at(u64 now_ns);
/* lx_emul.c: when, by the monotonic clock, the current jiffy began. */
u64 lx_emul_jiffy_began_ns(void);
/* lx_softirq.c: softirqs, tasklets and timers made ready; 0, or -ENOMEM. */
int lx_softirq_init(void);
/* lx_sched.c: the RCU callbacks queued so far, run: every task is switched out. */
void lx_rcu_run(void);
/* lx_sched.c: whether any RCU callback waits to run. */
bool lx_rcu_pending(void);
/* lx_sched.c: Linux's kthreadd started, as a task; 0, or -ENOMEM. */
int lx_start_kthreadd(void);
/* lx_emul.c: when the running task was switched to, by the monotonic clock. */
extern u64 lx_switched_ns;
/* lx_emul.c: the running task's run time counted up to `now_ns`. */
void lx_emul_account(u64 now_ns);
/* lx_fs.c: the objects Linux's own sysfs set-up makes; 0, or -ENOMEM. */
int lx_fs_init(void);
/* lx_emul.c: the first argument "<key><value>"'s value, or NULL. */
const char *lx_arg(const char *key);
/* lx_emul.c: whether the initcall at `fn`, answering `ret`, ends the process. */
bool lx_initcall_fatal(const void *fn, int ret);
/* lx_mm.c: Linux's direct map laid over the heap; 0, or -ENOMEM. And the
 * pages of `size` bytes kmalloc handed out at `p` marked slab memory. */
int lx_mm_init(void);
void lx_mm_mark_slab(const void *p, size_t size);
/* lx_mm.c: the pages of `size` bytes at `p`, handed out whole, said to be no slab's. */
void lx_mm_mark_pages(const void *p, size_t size);
/* lx_pci.c: whether a driver took every PCI function the process was given,
 * once probing is done; each it did not is said. And of the functions given,
 * at most LX_PCI_FUNCTIONS_MAX: `dev`'s place among them (-1 for none), and
 * the handle of the one at `i`. And whether `dev` is the device whose
 * arenas back the DMA window (the first function given). */
#define LX_PCI_FUNCTIONS_MAX 8
struct pci_dev;
struct device;
bool lx_pci_check_bound(void);
int lx_pci_index(const struct pci_dev *dev);
long lx_pci_handle(int i);
bool lx_pci_dma_device(const struct device *dev);
/* lx_irq.c: the interrupts that came since the last turn, handled; which
 * the loop is to wait for when idle, told the kit (just before it waits);
 * and how many times the handler of interrupt `irq` has run, for the tests. */
void lx_irq_poll(void);
void lx_irq_listen(void);
/* lx_irq.c: whether an interrupt owed while its handler was disabled waits
 * to be run now that it is enabled -- the next turn must come at once. */
bool lx_irq_due(void);
/* lx_hrtimer.c: the high-resolution timers due, run; and when the earliest
 * queued one is due by the monotonic clock reading `now_ns` (KTIME_MAX for
 * none). */
void lx_hrtimer_run(void);
u64 lx_hrtimer_next(u64 now_ns);
/* lx_softirq.c: run at the end of every turn once set, the clock reading
 * `now_ns` -- by lx_nic.c, to see to a card served to the network stack. */
extern void (*lx_emul_turn_hook)(u64 now_ns);
unsigned long lx_irq_count(unsigned int irq);
/* lx_irq.c: how many interrupts on `irq` were dropped, no handler requested. */
unsigned long lx_irq_dropped(unsigned int irq);
/* lx_irq.c: whether the kernel has interrupts of `dev`'s not yet taken,
 * looked at as lx_intx_poll() looks (slx_pci_irq_ready), for the tests. */
bool lx_irq_ready(struct pci_dev *dev);

/*
 * lx_netlink.c: the layer as a generic netlink user. A family registered,
 * by its name (NULL for none); a request to its command `cmd`, for the
 * caller to add attributes to (NULL for no memory); the request run as
 * genetlink runs a doit, its replies queued on `replies` -- the handler's
 * answer, the request still the caller's; a multicast group, by its name,
 * listened to (0, or -ENOENT for no such group); and the next message sent
 * to it, waiting up to `timeout` jiffies (NULL for none by then). The next
 * message sent to the layer's port that answers none of its requests -- a
 * management frame it registered for -- or NULL, without waiting; and
 * whether a message is kept on the port or in the group `name` (NULL for
 * the port alone), waiting up to `timeout` jiffies for one to be.
 */
struct genl_family;
struct sk_buff;
struct sk_buff_head;
const struct genl_family *lx_genl_family(const char *name);
struct sk_buff *lx_genl_request(const struct genl_family *family, u8 cmd);
int lx_genl_doit(const struct genl_family *family, struct sk_buff *req,
		 struct sk_buff_head *replies);
int lx_genl_listen(const struct genl_family *family, const char *name);
struct sk_buff *lx_genl_event(const struct genl_family *family, const char *name,
			      unsigned long timeout);
struct sk_buff *lx_genl_unicast(void);
bool lx_genl_wait(const struct genl_family *family, const char *name, unsigned long timeout);
/*
 * lx_netlink.c: the port's queue and the waits at their edges, for a run to
 * check before it relies on them -- `family`'s groups `group` and `other`
 * listened to, and anything kept in them or on the port dropped: 0, or
 * -EPROTO with what was wrong said.
 */
int lx_genl_check(const struct genl_family *family, const char *group, const char *other);

/*
 * lx_wpa.c: WPA2-PSK's keys and the frames of its four-way handshake
 * (IEEE 802.11-2020 12.7), CCMP its cipher.
 */
#define LX_WPA_PMK_LEN 32
#define LX_WPA_NONCE_LEN 32
#define LX_WPA_KCK_LEN 16
#define LX_WPA_KEK_LEN 16
#define LX_WPA_TK_LEN 16
#define LX_WPA_MIC_LEN 16
/* The most key data a handshake frame of the layer's carries, unwrapped. */
#define LX_WPA_KEY_DATA_MAX 256
/* An EAPOL-Key frame's Key Information: version 2 (HMAC-SHA1-128, AES key wrap), and its bits. */
#define LX_WPA_INFO_VERSION 2
#define LX_WPA_INFO_PAIRWISE BIT(3)
#define LX_WPA_INFO_INSTALL BIT(6)
#define LX_WPA_INFO_ACK BIT(7)
#define LX_WPA_INFO_MIC BIT(8)
#define LX_WPA_INFO_SECURE BIT(9)
#define LX_WPA_INFO_ENCRYPTED BIT(12)

/* The PTK, as 12.7.1.3 cuts it for CCMP: KCK, KEK, TK. */
struct lx_wpa_ptk {
	u8 kck[LX_WPA_KCK_LEN];
	u8 kek[LX_WPA_KEK_LEN];
	u8 tk[LX_WPA_TK_LEN];
};

/* An EAPOL-Key frame (12.7.2), RSN key descriptor, behind 802.1X's header. */
struct lx_wpa_eapol_key {
	u8 version;		/* 802.1X's: 2 */
	u8 type;		/* 3, EAPOL-Key */
	__be16 length;		/* of what follows these four bytes */
	u8 descriptor;		/* 2, RSN */
	__be16 info;
	__be16 key_length;
	u8 replay[8];
	u8 nonce[LX_WPA_NONCE_LEN];
	u8 iv[16];
	u8 rsc[8];
	u8 reserved[8];
	u8 mic[LX_WPA_MIC_LEN];
	__be16 data_length;
	u8 data[];
} __packed;

/*
 * The PMK of `passphrase` on the network named by the `ssid_len` bytes at
 * `ssid`; the standard's PRF of `key` over `label` and `data`, `out_len`
 * bytes of it; the PTK of the PMK between the authenticator at `aa` and the
 * supplicant at `spa`, by their nonces -- the same whichever side asks.
 */
void lx_wpa_pmk(const char *passphrase, const u8 *ssid, size_t ssid_len, u8 pmk[LX_WPA_PMK_LEN]);
void lx_wpa_prf(const u8 *key, size_t key_len, const char *label, const u8 *data, size_t data_len,
		u8 *out, size_t out_len);
void lx_wpa_ptk(const u8 pmk[LX_WPA_PMK_LEN], const u8 *aa, const u8 *spa, const u8 *anonce,
		const u8 *snonce, struct lx_wpa_ptk *ptk);
/*
 * RFC 3394's key wrap of the `len` bytes at `plain` (16 or more, a multiple
 * of 8) by `kek` into `len` + 8 bytes at `out`; its unwrap of the `len`
 * bytes at `wrapped` into `len` - 8 at `out` -- -EBADMSG, `out` cleared,
 * when they were not wrapped by `kek`. -EINVAL for a length that cannot be.
 */
int lx_wpa_wrap(const u8 kek[LX_WPA_KEK_LEN], const u8 *plain, size_t len, u8 *out);
int lx_wpa_unwrap(const u8 kek[LX_WPA_KEK_LEN], const u8 *wrapped, size_t len, u8 *out);
/*
 * An EAPOL-Key frame built in the `room` bytes at `buf`: its Key
 * Information `info`, replay counter `replay`, nonce (NULL for none) and
 * the `data_len` bytes of key data at `data` -- signed by `kck` when it is
 * not NULL. Its length, or -ENOSPC.
 */
int lx_wpa_key_frame(u8 *buf, size_t room, u16 info, u64 replay, const u8 *nonce,
		     const u8 *data, size_t data_len, const u8 *kck);
/*
 * The `len` bytes at `buf` read as an EAPOL-Key frame -- its MIC checked by
 * `kck` when that is not NULL: the frame, or NULL for one that is not one,
 * or whose MIC does not hold.
 */
const struct lx_wpa_eapol_key *lx_wpa_key_parse(const u8 *buf, size_t len, const u8 *kck);
/*
 * The `len` bytes of key data at `data`, padded as 12.7.2 pads them and
 * wrapped by `kek` into the `room` bytes at `out`: their length, or -ENOSPC.
 */
int lx_wpa_key_data_wrap(const u8 kek[LX_WPA_KEK_LEN], const u8 *data, size_t len, u8 *out,
			 size_t room);
/*
 * The group key in key data (its GTK KDE): the key, its id and its length
 * through `keyid` and `gtk_len` -- or NULL when there is none. And the KDE
 * built at `out`, for the `gtk_len`-byte key at `gtk` with id `keyid`: its
 * length, 8 + `gtk_len`.
 */
const u8 *lx_wpa_find_gtk(const u8 *data, size_t len, u8 *keyid, size_t *gtk_len);
size_t lx_wpa_gtk_kde(u8 *out, u8 keyid, const u8 *gtk, size_t gtk_len);
/* lx_wpa.c at its edges, against published vectors: 0, or -EBADMSG with what was wrong said. */
int lx_wpa_check(void);

/*
 * Hardirq context, as Linux enters it for an interrupt -- interrupts off,
 * the count raised (<linux/hardirq.h>'s __irq_enter_raw()) -- and left: for
 * the devices' handlers and the turn's irq_work alike, so that both run in
 * the one context.
 */
#define lx_hardirq_enter(flags)		\
	do {				\
		local_irq_save(flags);	\
		__irq_enter_raw();	\
	} while (0)
#define lx_hardirq_exit(flags)		\
	do {				\
		__irq_exit_raw();	\
		local_irq_restore(flags);	\
	} while (0)

/* Warnings said (lx_emul.c), and threads' records not yet freed, daemons
 * (kthreadd, idle workers) not counted (lx_sched.c): for the hello driver's
 * tests, and the runs' ending (lx_end_run). */
extern unsigned long lx_warnings;
unsigned long lx_live_kthreads(void);

/*
 * lx_emul.c: a run of the layer's own task ended, `err` its outcome and
 * `who` its task's name for the log -- the process ended with it: a failure,
 * its error said, as a failed initcall; a success in which Linux code warned anywhere in the
 * process, as warned (SLX_EXIT_WARNED), a bug the run must not pass; else 0.
 */
void lx_end_run(const char *who, int err) __attribute__((noreturn));

/*
 * lx_nic.c's wait before it serves its device, a weak default that answers
 * 0 at once; lx_wifi.c's, given "join=", until its network is joined: 0, or
 * why the device cannot be served.
 */
int lx_nic_await_link_layer(void);

#endif /* LX_EMUL_H */
