// SPDX-License-Identifier: MIT
/*
 * Netlink, as a driver process has it: no sockets, and one user -- the
 * layer.
 *
 * Linux's 802.11 stack is driven through generic netlink: cfg80211's
 * nl80211 registers its family at start-up, answers commands (scan,
 * connect, keys) and, as wiphys come and go and scans end, multicasts what
 * happened. A driver process has no netlink sockets (net/netlink is not
 * compiled in). So:
 *
 * - A family is registered as Linux registers it -- given an id and its
 *   multicast groups' numbers, so that its messages are built as Linux
 *   builds them -- and kept, for the layer to find by name.
 * - The layer asks a command as a netlink socket of its own would
 *   (lx_genl_request(), lx_genl_doit()): the request's attributes parsed by
 *   the command's policy, the family's pre_doit and post_doit around its
 *   handler, as genetlink runs one. Its replies come back to the layer.
 * - A message multicast to a group the layer listens to is kept for it
 *   (lx_genl_listen(), lx_genl_event()); so is one sent to the layer's own
 *   port that answers none of its requests -- a management frame it
 *   registered for (lx_genl_unicast()); a frame's transmit status is
 *   nl80211's to multicast, to "mlme". To any other group, or to any other
 *   socket, a message is freed, answered as Linux answers a send no one
 *   receives. The routing netlink's notifications (rtnl_notify and the
 *   rest) go to no one.
 *
 * The layer's are nl80211's ABI, the interface wpa_supplicant and iwd
 * drive Linux's WiFi through, which stays put from one release to the next
 * where cfg80211's insides do not. Permission checks (GENL_ADMIN_PERM) are
 * not made: the layer is the process's one caller, and what the process
 * may do is the kernel's to say, by the capabilities it holds.
 *
 * Left out, and so generated stops: dumps, and what only a netlink
 * socket's own request reaches -- strict checking of a dump, a namespace
 * looked up by its id for a caller.
 */
#include <linux/netlink.h>
#include <linux/rtnetlink.h>
#include <linux/skbuff.h>
#include <linux/slab.h>
#include <linux/wait.h>
#include <net/genetlink.h>
#include <net/netlink.h>

#include "lx_emul.h"

/*
 * Family ids, and multicast group numbers, handed out in order past those
 * Linux reserves (nlctrl's and two older families'). Their values matter
 * only to a listener -- the layer itself, here.
 */
static u16 lx_genl_next_id = GENL_ID_PMCRAID + 1;
static unsigned int lx_genl_next_mcgrp = GENL_ID_PMCRAID + 1;

/* The families registered, so that the layer can find one by its name. */
#define LX_GENL_FAMILIES 8
static struct genl_family *lx_genl_families[LX_GENL_FAMILIES];

int genl_register_family(struct genl_family *family)
{
	int i;

	for (i = 0; i < LX_GENL_FAMILIES && lx_genl_families[i]; i++)
		;
	if (WARN(i == LX_GENL_FAMILIES, "lx-genl: more than %d families\n", LX_GENL_FAMILIES))
		return -ENOMEM;
	family->id = lx_genl_next_id++;
	family->mcgrp_offset = family->n_mcgrps ? lx_genl_next_mcgrp : 0;
	lx_genl_next_mcgrp += family->n_mcgrps;
	lx_genl_families[i] = family;
	return 0;
}

int genl_unregister_family(const struct genl_family *family)
{
	int i;

	for (i = 0; i < LX_GENL_FAMILIES; i++)
		if (lx_genl_families[i] == family)
			lx_genl_families[i] = NULL;
	return 0;
}

const struct genl_family *lx_genl_family(const char *name)
{
	int i;

	for (i = 0; i < LX_GENL_FAMILIES; i++)
		if (lx_genl_families[i] && !strcmp(lx_genl_families[i]->name, name))
			return lx_genl_families[i];
	return NULL;
}

/*
 * The layer's own requests, as a netlink socket of its own would send them:
 * from LX_GENL_PORTID, each with the next sequence number. Replies to that
 * port -- carrying the sequence number of the request that runs -- go to
 * the queue it gave.
 */
#define LX_GENL_PORTID 1
static u32 lx_genl_seq;
static struct sk_buff_head *lx_genl_replies;
static u32 lx_genl_reply_seq;
/*
 * One doit at a time: the reply target above is the one doit's under way,
 * and the layer has more than one task asking -- a WiFi process's own and
 * its station's keeper (lx_wifi.c), which installs keys from a worker.
 */
static DEFINE_MUTEX(lx_genl_doit_lock);

struct sk_buff *lx_genl_request(const struct genl_family *family, u8 cmd)
{
	struct sk_buff *skb = nlmsg_new(NLMSG_DEFAULT_SIZE, GFP_KERNEL);

	if (!skb)
		return NULL;
	if (!genlmsg_put(skb, LX_GENL_PORTID, ++lx_genl_seq, family, NLM_F_REQUEST, cmd)) {
		nlmsg_free(skb);
		return NULL;
	}
	NETLINK_CB(skb).portid = LX_GENL_PORTID;
	return skb;
}

/*
 * The handler `family` has for `cmd`, as one split op the way genetlink
 * makes one of a full or small op: the family's pre_doit and post_doit
 * around it, the op's policy or else the family's. False for none.
 */
static bool lx_genl_doit_op(const struct genl_family *family, u8 cmd, struct genl_split_ops *op)
{
	int i;

	*op = (struct genl_split_ops){};
	for (i = 0; i < family->n_ops; i++) {
		const struct genl_ops *full = &family->ops[i];

		if (full->cmd != cmd || !full->doit)
			continue;
		op->doit = full->doit;
		op->policy = full->policy;
		op->maxattr = full->maxattr;
		op->internal_flags = full->internal_flags;
		op->flags = full->flags;
		op->validate = full->validate;
		break;
	}
	for (i = 0; !op->doit && i < family->n_small_ops; i++) {
		const struct genl_small_ops *small = &family->small_ops[i];

		if (small->cmd != cmd || !small->doit)
			continue;
		op->doit = small->doit;
		op->internal_flags = small->internal_flags;
		op->flags = small->flags;
		op->validate = small->validate;
	}
	if (!op->doit)
		return false;
	/* Each the family's where the op has none, apart, as genl_op_from_full() takes them. */
	if (!op->maxattr)
		op->maxattr = family->maxattr;
	if (!op->policy)
		op->policy = family->policy;
	op->cmd = cmd;
	op->pre_doit = family->pre_doit;
	op->post_doit = family->post_doit;
	return true;
}

/*
 * `req`, a request lx_genl_request() made and the caller filled, run as
 * genetlink runs a doit: its attributes parsed by the op's policy, then the
 * family's pre_doit, the op's doit and the family's post_doit. Each reply
 * it sends is queued on `replies`. The doit's answer, or the parse's or the
 * pre_doit's error; -EOPNOTSUPP for a command the family has no handler
 * for. `req` stays the caller's.
 */
int lx_genl_doit(const struct genl_family *family, struct sk_buff *req,
		 struct sk_buff_head *replies)
{
	struct nlmsghdr *nlh = nlmsg_hdr(req);
	struct genlmsghdr *hdr = nlmsg_data(nlh);
	struct netlink_ext_ack extack = {};
	struct nlattr **attrs = NULL;
	struct genl_split_ops op;
	struct genl_info info;
	int err;

	nlmsg_end(req, nlh);
	if (!lx_genl_doit_op(family, hdr->cmd, &op))
		return -EOPNOTSUPP;
	if (op.maxattr) {
		attrs = kmalloc_array(op.maxattr + 1, sizeof(*attrs), GFP_KERNEL);
		if (!attrs)
			return -ENOMEM;
		err = __nlmsg_parse(nlh, GENL_HDRLEN + family->hdrsize, attrs, op.maxattr, op.policy,
				    op.validate & GENL_DONT_VALIDATE_STRICT ? NL_VALIDATE_LIBERAL :
									      NL_VALIDATE_STRICT,
				    &extack);
		if (err)
			goto out;
	}
	info = (struct genl_info){
		.snd_seq = nlh->nlmsg_seq,
		.snd_portid = nlh->nlmsg_pid,
		.family = family,
		.nlhdr = nlh,
		.genlhdr = hdr,
		.attrs = attrs,
		.extack = &extack,
	};
	write_pnet(&info._net, &init_net);
	mutex_lock(&lx_genl_doit_lock);
	lx_genl_replies = replies;
	lx_genl_reply_seq = nlh->nlmsg_seq;
	err = op.pre_doit ? op.pre_doit(&op, req, &info) : 0;
	if (!err) {
		err = op.doit(req, &info);
		if (op.post_doit)
			op.post_doit(&op, req, &info);
	}
	lx_genl_replies = NULL;
	mutex_unlock(&lx_genl_doit_lock);
out:
	if (err && extack._msg)
		pr_info("lx-genl: %s command %u: %s (%d)\n", family->name, hdr->cmd, extack._msg, err);
	kfree(attrs);
	return err;
}

/*
 * The multicast groups the layer listens to: each one's messages kept, in
 * the order sent, until taken (lx_genl_event()).
 */
#define LX_GENL_LISTENS 4
static struct lx_genl_listen {
	unsigned int group;
	struct sk_buff_head queue;
} lx_genl_listens[LX_GENL_LISTENS];
static DECLARE_WAIT_QUEUE_HEAD(lx_genl_events);

/*
 * Messages sent to the layer's port that answer none of its requests --
 * nl80211 unicasts the management frames a port registered for to it --
 * kept, in the order sent, until taken (lx_genl_unicast()). At most
 * LX_GENL_UNSOLICITED_MAX: past that, refused as a socket whose receive
 * queue is full refuses one.
 */
#define LX_GENL_UNSOLICITED_MAX 64
static struct sk_buff_head lx_genl_unsolicited;

static int __init lx_genl_init(void)
{
	skb_queue_head_init(&lx_genl_unsolicited);
	return 0;
}
pure_initcall(lx_genl_init);

struct sk_buff *lx_genl_unicast(void)
{
	return skb_dequeue(&lx_genl_unsolicited);
}

/* Where group `group`'s messages are kept, or NULL when no one listens. */
static struct lx_genl_listen *lx_genl_listener(unsigned int group)
{
	int i;

	for (i = 0; i < LX_GENL_LISTENS; i++)
		if (lx_genl_listens[i].group && lx_genl_listens[i].group == group)
			return &lx_genl_listens[i];
	return NULL;
}

/* `family`'s multicast group named `name`, as a number netlink sends it to; 0 for none. */
static unsigned int lx_genl_group(const struct genl_family *family, const char *name)
{
	int i;

	for (i = 0; i < family->n_mcgrps; i++)
		if (!strcmp(family->mcgrps[i].name, name))
			return family->mcgrp_offset + i;
	return 0;
}

int lx_genl_listen(const struct genl_family *family, const char *name)
{
	unsigned int group = lx_genl_group(family, name);
	int i;

	if (!group)
		return -ENOENT;
	if (lx_genl_listener(group))
		return 0;
	for (i = 0; i < LX_GENL_LISTENS; i++) {
		if (!lx_genl_listens[i].group) {
			skb_queue_head_init(&lx_genl_listens[i].queue);
			lx_genl_listens[i].group = group;
			return 0;
		}
	}
	return -ENOMEM;
}

void lx_genl_unlisten(const struct genl_family *family, const char *name)
{
	struct lx_genl_listen *l = lx_genl_listener(lx_genl_group(family, name));

	if (!l)
		return;
	skb_queue_purge(&l->queue);
	l->group = 0;
}

struct sk_buff *lx_genl_event(const struct genl_family *family, const char *name,
			      unsigned long timeout)
{
	struct lx_genl_listen *l = lx_genl_listener(lx_genl_group(family, name));

	if (!l)
		return NULL;
	wait_event_timeout(lx_genl_events, !skb_queue_empty(&l->queue), timeout);
	return skb_dequeue(&l->queue);
}

/* Whether a message is kept on the layer's port, or in `l`'s group when not NULL. */
static bool lx_genl_pending(const struct lx_genl_listen *l)
{
	return !skb_queue_empty(&lx_genl_unsolicited) || (l && !skb_queue_empty(&l->queue));
}

/*
 * Only the port and the one group the caller takes from: a message waiting
 * in another -- a scan's, say, while a join waits -- would otherwise make
 * every wait return at once, and the caller spin without ever yielding.
 */
bool lx_genl_wait(const struct genl_family *family, const char *name, unsigned long timeout)
{
	struct lx_genl_listen *l = name ? lx_genl_listener(lx_genl_group(family, name)) : NULL;

	wait_event_timeout(lx_genl_events, lx_genl_pending(l), timeout);
	return lx_genl_pending(l);
}

/*
 * A message sent to group `group`: kept when the layer listens to it, freed
 * otherwise -- answered as Linux answers, 0 for delivered and -ESRCH for no
 * listener.
 */
static int lx_genl_deliver(struct sk_buff *skb, unsigned int group)
{
	struct lx_genl_listen *l = lx_genl_listener(group);

	if (!l) {
		kfree_skb(skb);
		return -ESRCH;
	}
	skb_queue_tail(&l->queue, skb);
	wake_up(&lx_genl_events);
	return 0;
}

/*
 * A netlink message's header at the tail of `skb`, `len` bytes of payload
 * room after it, padded to the alignment netlink keeps: the caller
 * (nlmsg_put()) has made sure the room is there.
 */
struct nlmsghdr *__nlmsg_put(struct sk_buff *skb, u32 portid, u32 seq, int type, int len,
			     int flags)
{
	int size = nlmsg_msg_size(len);
	struct nlmsghdr *nlh = skb_put(skb, NLMSG_ALIGN(size));

	nlh->nlmsg_len = size;
	nlh->nlmsg_type = type;
	nlh->nlmsg_flags = flags;
	nlh->nlmsg_seq = seq;
	nlh->nlmsg_pid = portid;
	memset((char *)nlmsg_data(nlh) + len, 0, NLMSG_ALIGN(size) - size);
	return nlh;
}

/*
 * A generic netlink message's headers: the netlink one, typed by the
 * family's id, then the family's command and version. Where its payload
 * starts -- past the family's own header, if it has one -- or NULL when
 * `skb` has no room.
 */
void *genlmsg_put(struct sk_buff *skb, u32 portid, u32 seq, const struct genl_family *family,
		  int flags, u8 cmd)
{
	struct nlmsghdr *nlh = nlmsg_put(skb, portid, seq, family->id,
					 GENL_HDRLEN + family->hdrsize, flags);
	struct genlmsghdr *hdr;

	if (!nlh)
		return NULL;
	hdr = nlmsg_data(nlh);
	hdr->cmd = cmd;
	hdr->version = family->version;
	hdr->reserved = 0;
	return (char *)hdr + GENL_HDRLEN;
}

/* Sent to a group in every namespace: there is one namespace. */
int genlmsg_multicast_allns(const struct genl_family *family, struct sk_buff *skb, u32 portid,
			    unsigned int group)
{
	if (WARN_ON_ONCE(group >= family->n_mcgrps)) {
		nlmsg_free(skb);
		return -EINVAL;
	}
	return lx_genl_deliver(skb, family->mcgrp_offset + group);
}

/* Sent to a group (nlmsg_multicast() and genlmsg_multicast_netns() come here). */
int netlink_broadcast_filtered(struct sock *ssk, struct sk_buff *skb, __u32 portid, __u32 group,
			       gfp_t allocation, netlink_filter_fn filter, void *filter_data)
{
	return lx_genl_deliver(skb, group);
}

/*
 * Sent to one socket. To the layer's own: a reply to the request of its
 * that runs -- that request's sequence number -- queued with its replies;
 * anything else kept for it (lx_genl_unicast()), or refused, -ENOBUFS, when
 * LX_GENL_UNSOLICITED_MAX are. Linux answers a delivery with the bytes
 * sent. There is no other socket: -ECONNREFUSED.
 */
int netlink_unicast(struct sock *ssk, struct sk_buff *skb, __u32 portid, int nonblock)
{
	unsigned int len = skb->len;

	if (portid != LX_GENL_PORTID) {
		kfree_skb(skb);
		return -ECONNREFUSED;
	}
	if (lx_genl_replies && nlmsg_hdr(skb)->nlmsg_seq == lx_genl_reply_seq) {
		skb_queue_tail(lx_genl_replies, skb);
		return len;
	}
	if (skb_queue_len(&lx_genl_unsolicited) >= LX_GENL_UNSOLICITED_MAX) {
		pr_info_ratelimited("lx-genl: a message to the layer's port refused: %d kept already\n",
				    LX_GENL_UNSOLICITED_MAX);
		kfree_skb(skb);
		return -ENOBUFS;
	}
	skb_queue_tail(&lx_genl_unsolicited, skb);
	wake_up(&lx_genl_events);
	return len;
}

/* Told when a netlink socket is released: none ever is. */
int netlink_register_notifier(struct notifier_block *nb)
{
	return 0;
}

int netlink_unregister_notifier(struct notifier_block *nb)
{
	return 0;
}

/* A request's extended error message, traced: there is no tracing. */
void do_trace_netlink_extack(const char *msg)
{
}

/* The routing netlink's notifications: no one listens to them either. */
void rtnl_notify(struct sk_buff *skb, struct net *net, u32 pid, u32 group,
		 const struct nlmsghdr *nlh, gfp_t flags)
{
	kfree_skb(skb);
}

int rtnl_unicast(struct sk_buff *skb, struct net *net, u32 pid)
{
	kfree_skb(skb);
	return -ECONNREFUSED;
}

void rtnl_set_sk_err(struct net *net, u32 group, int error)
{
}

/* A message of `family`'s, of no command, as one might be sent to the layer. */
static struct sk_buff *lx_genl_test_msg(const struct genl_family *family)
{
	struct sk_buff *skb = nlmsg_new(NLMSG_DEFAULT_SIZE, GFP_KERNEL);
	void *hdr = skb ? genlmsg_put(skb, 0, 0, family, 0, 0) : NULL;

	if (!hdr) {
		nlmsg_free(skb);
		return NULL;
	}
	genlmsg_end(skb, hdr);
	return skb;
}

/* Everything kept on the layer's port, and in group `name`, freed: how many. */
static unsigned int lx_genl_drain(const struct genl_family *family, const char *name)
{
	struct sk_buff *skb;
	unsigned int n = 0;

	while ((skb = lx_genl_unicast()) || (skb = lx_genl_event(family, name, 0))) {
		kfree_skb(skb);
		n++;
	}
	return n;
}

#define LX_GENL_CHECK(cond, what)                                       \
	do {                                                            \
		if (!(cond)) {                                          \
			pr_err("lx-genl: check failed: %s\n", what);    \
			goto fail;                                      \
		}                                                       \
	} while (0)

int lx_genl_check(const struct genl_family *family, const char *group, const char *other)
{
	struct sk_buff *a = NULL, *b = NULL, *skb;
	unsigned int n;
	int i;

	if (lx_genl_listen(family, group) || lx_genl_listen(family, other)) {
		pr_err("lx-genl: check failed: the groups \"%s\" and \"%s\" could not be listened to\n",
		       group, other);
		return -EPROTO;
	}
	/* What came before is not the check's. */
	lx_genl_drain(family, group);
	lx_genl_drain(family, other);
	LX_GENL_CHECK(!lx_genl_wait(family, group, 1) && !lx_genl_unicast(),
		      "a wait, nothing sent, saw something");
	/* Two messages to the port, outside any request: kept, and taken in order. */
	a = lx_genl_test_msg(family);
	b = lx_genl_test_msg(family);
	if (!a || !b) {
		kfree_skb(a);
		kfree_skb(b);
		LX_GENL_CHECK(false, "no memory for the messages");
	}
	LX_GENL_CHECK(netlink_unicast(NULL, a, LX_GENL_PORTID, 0) > 0 &&
		      netlink_unicast(NULL, b, LX_GENL_PORTID, 0) > 0,
		      "a message to the layer's port, outside a request, was refused");
	LX_GENL_CHECK(lx_genl_wait(family, NULL, 0), "a wait for the port missed its message");
	skb = lx_genl_unicast();
	LX_GENL_CHECK(skb == a, "the port's first message was not taken first");
	kfree_skb(skb);
	skb = lx_genl_unicast();
	LX_GENL_CHECK(skb == b, "the port's second message was not taken second");
	kfree_skb(skb);
	LX_GENL_CHECK(!lx_genl_unicast(), "the port gave more than it was sent");
	/* As many as the port keeps, then one refused. */
	for (i = 0; i <= LX_GENL_UNSOLICITED_MAX; i++) {
		int sent;

		skb = lx_genl_test_msg(family);
		LX_GENL_CHECK(skb, "no memory for the messages");
		sent = netlink_unicast(NULL, skb, LX_GENL_PORTID, 0);
		LX_GENL_CHECK(i < LX_GENL_UNSOLICITED_MAX ? sent > 0 : sent == -ENOBUFS,
			      i < LX_GENL_UNSOLICITED_MAX ? "a message the port had room for was refused" :
							    "a message past the port's limit was not refused");
	}
	n = lx_genl_drain(family, group);
	LX_GENL_CHECK(n == LX_GENL_UNSOLICITED_MAX, "the port did not keep as many as its limit");
	/* A message in one group: a wait for the other does not see it, one for its own does. */
	skb = lx_genl_test_msg(family);
	LX_GENL_CHECK(skb, "no memory for the messages");
	LX_GENL_CHECK(!lx_genl_deliver(skb, lx_genl_group(family, other)),
		      "a message to a group listened to was not kept");
	LX_GENL_CHECK(!lx_genl_wait(family, group, 1), "a wait for one group woke for another's message");
	LX_GENL_CHECK(lx_genl_wait(family, other, 0), "a wait for a group missed its message");
	LX_GENL_CHECK(lx_genl_drain(family, other) == 1, "the group kept other than the one message");
	pr_info("lx-genl: the port's queue, its limit and the groups' waits: as they should be\n");
	return 0;
fail:
	lx_genl_drain(family, group);
	lx_genl_drain(family, other);
	return -EPROTO;
}
