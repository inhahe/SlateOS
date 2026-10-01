/*
 * SlateOS: <obstack.h> -- GNU's object stacks, which musl has none of.
 *
 * An obstack holds objects one after another in chunks it gets from the
 * program's allocation function; one object at a time grows at its end,
 * and freeing an object frees everything made after it. The interface is
 * mostly these macros, which a program expands: they do what glibc 2.39's
 * installed header's do -- where a new chunk is asked for and for how
 * much, how an object is aligned when it is finished, what obstack_free
 * does without a call -- and the struct is glibc's, field for field, for
 * a program that reads one (posix/src/obstack.rs; design-decisions 1162).
 * glibc 2.39 installs the older form of the interface, with int lengths,
 * and so does this.
 */

/* A system header, as the musl ones it extends are: warnings in it are not
 * the program's. (scripts/check-libc-overlay.py defines the macro, to hold
 * it to -Wall -Wextra itself.) */
#ifndef _SLATEOS_OVERLAY_WARNINGS
#pragma GCC system_header
#endif

#ifndef _OBSTACK_H
#define _OBSTACK_H 1

/* The type of a pointer difference, without <stddef.h>'s names where the
 * compiler knows it. */
#ifdef __PTRDIFF_TYPE__
# define PTR_INT_TYPE __PTRDIFF_TYPE__
#else
# include <stddef.h>
# define PTR_INT_TYPE ptrdiff_t
#endif

/* P rounded up to the next multiple of A + 1, counted from B (A + 1 a power
 * of two): an object's start, aligned. */
#define __BPTR_ALIGN(B, P, A) ((B) + (((P) - (B) + (A)) & ~(A)))

/* The same, counted from address 0 where a pointer fits the difference type,
 * as it does here. */
#define __PTR_ALIGN(B, P, A)						\
  __BPTR_ALIGN (sizeof (PTR_INT_TYPE) < sizeof (void *) ? (B) : (char *) 0, \
		P, A)

#include <string.h>

#ifndef __attribute_pure__
# if defined __GNUC__ || defined __clang__
#  define __attribute_pure__ __attribute__ ((__pure__))
# else
#  define __attribute_pure__
# endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* The head of each chunk; the chunk's objects begin at contents. */
struct _obstack_chunk
{
  char *limit;                  /* one past the chunk's end */
  struct _obstack_chunk *prev;  /* the chunk before, or NULL */
  char contents[4];
};

/* An obstack: its chunks, and the object growing in the last of them. */
struct obstack
{
  long chunk_size;              /* the size new chunks are asked for in */
  struct _obstack_chunk *chunk; /* the current chunk */
  char *object_base;            /* where the growing object begins */
  char *next_free;              /* where its next byte goes */
  char *chunk_limit;            /* one past the current chunk's end */
  union
  {
    PTR_INT_TYPE tempint;
    void *tempptr;
  } temp;                       /* the portable macros' scratch */
  int alignment_mask;           /* what an object's address is a multiple of, less 1 */
  /* Called with extra_arg first when use_extra_arg is set. */
  struct _obstack_chunk *(*chunkfun) (void *, long);
  void (*freefun) (void *, struct _obstack_chunk *);
  void *extra_arg;
  unsigned use_extra_arg : 1;
  /* The current chunk may hold an empty object, which a new chunk must
   * not lose by freeing this one. */
  unsigned maybe_empty_object : 1;
  /* Unused; kept as glibc keeps it. */
  unsigned alloc_failed : 1;
};

extern void _obstack_newchunk (struct obstack *, int);
extern int _obstack_begin (struct obstack *, int, int,
			   void *(*) (long), void (*) (void *));
extern int _obstack_begin_1 (struct obstack *, int, int,
			     void *(*) (void *, long),
			     void (*) (void *, void *), void *);
extern int _obstack_memory_used (struct obstack *) __attribute_pure__;

/* The function obstack_free calls for an object outside the current chunk;
 * a program may name another, as glibc's header lets it. */
#ifndef __obstack_free
# define __obstack_free obstack_free
#endif
extern void __obstack_free (struct obstack *, void *);

/* Called when a chunk cannot be had. It must not return: the default
 * prints "memory exhausted" and exits with obstack_exit_failure. */
extern void (*obstack_alloc_failed_handler) (void);
extern int obstack_exit_failure;

#define obstack_base(h) ((void *) (h)->object_base)
#define obstack_chunk_size(h) ((h)->chunk_size)
#define obstack_next_free(h) ((h)->next_free)
#define obstack_alignment_mask(h) ((h)->alignment_mask)

/* The program defines obstack_chunk_alloc and obstack_chunk_free. */
#define obstack_init(h)							\
  _obstack_begin ((h), 0, 0,						\
		  (void *(*) (long)) obstack_chunk_alloc,		\
		  (void (*) (void *)) obstack_chunk_free)

#define obstack_begin(h, size)						\
  _obstack_begin ((h), (size), 0,					\
		  (void *(*) (long)) obstack_chunk_alloc,		\
		  (void (*) (void *)) obstack_chunk_free)

#define obstack_specify_allocation(h, size, alignment, chunkfun, freefun) \
  _obstack_begin ((h), (size), (alignment),				\
		  (void *(*) (long)) (chunkfun),			\
		  (void (*) (void *)) (freefun))

#define obstack_specify_allocation_with_arg(h, size, alignment, chunkfun, freefun, arg) \
  _obstack_begin_1 ((h), (size), (alignment),				\
		    (void *(*) (void *, long)) (chunkfun),		\
		    (void (*) (void *, void *)) (freefun), (arg))

#define obstack_chunkfun(h, newchunkfun)				\
  ((h)->chunkfun = (struct _obstack_chunk *(*) (void *, long)) (newchunkfun))

#define obstack_freefun(h, newfreefun)					\
  ((h)->freefun = (void (*) (void *, struct _obstack_chunk *)) (newfreefun))

#define obstack_1grow_fast(h, achar) (*((h)->next_free)++ = (achar))

#define obstack_blank_fast(h, n) ((h)->next_free += (n))

#define obstack_memory_used(h) _obstack_memory_used (h)

#if defined __GNUC__ || defined __clang__

/* Each argument evaluated once, with no use of the temp slot. */

# define obstack_object_size(OBSTACK)					\
  __extension__								\
    ({ struct obstack const *__o = (OBSTACK);				\
       (unsigned) (__o->next_free - __o->object_base); })

# define obstack_room(OBSTACK)						\
  __extension__								\
    ({ struct obstack const *__o = (OBSTACK);				\
       (unsigned) (__o->chunk_limit - __o->next_free); })

# define obstack_make_room(OBSTACK, length)				\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       int __len = (length);						\
       if (__o->chunk_limit - __o->next_free < __len)			\
	 _obstack_newchunk (__o, __len);				\
       (void) 0; })

# define obstack_empty_p(OBSTACK)					\
  __extension__								\
    ({ struct obstack const *__o = (OBSTACK);				\
       (__o->chunk->prev == 0						\
	&& __o->next_free == __PTR_ALIGN ((char *) __o->chunk,		\
					  __o->chunk->contents,		\
					  __o->alignment_mask)); })

# define obstack_grow(OBSTACK, where, length)				\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       int __len = (length);						\
       if (__o->next_free + __len > __o->chunk_limit)			\
	 _obstack_newchunk (__o, __len);				\
       memcpy (__o->next_free, where, __len);				\
       __o->next_free += __len;						\
       (void) 0; })

# define obstack_grow0(OBSTACK, where, length)				\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       int __len = (length);						\
       if (__o->next_free + __len + 1 > __o->chunk_limit)		\
	 _obstack_newchunk (__o, __len + 1);				\
       memcpy (__o->next_free, where, __len);				\
       __o->next_free += __len;						\
       *(__o->next_free)++ = 0;						\
       (void) 0; })

# define obstack_1grow(OBSTACK, datum)					\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       if (__o->next_free + 1 > __o->chunk_limit)			\
	 _obstack_newchunk (__o, 1);					\
       obstack_1grow_fast (__o, datum);					\
       (void) 0; })

/* These two take the object, and the obstack's alignment, to be good
 * enough for a pointer or an int. */
# define obstack_ptr_grow(OBSTACK, datum)				\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       if (__o->next_free + sizeof (void *) > __o->chunk_limit)		\
	 _obstack_newchunk (__o, sizeof (void *));			\
       obstack_ptr_grow_fast (__o, datum); })

# define obstack_int_grow(OBSTACK, datum)				\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       if (__o->next_free + sizeof (int) > __o->chunk_limit)		\
	 _obstack_newchunk (__o, sizeof (int));				\
       obstack_int_grow_fast (__o, datum); })

# define obstack_ptr_grow_fast(OBSTACK, aptr)				\
  __extension__								\
    ({ struct obstack *__o1 = (OBSTACK);				\
       void *__p1 = __o1->next_free;					\
       *(const void **) __p1 = (aptr);					\
       __o1->next_free += sizeof (const void *);			\
       (void) 0; })

# define obstack_int_grow_fast(OBSTACK, aint)				\
  __extension__								\
    ({ struct obstack *__o1 = (OBSTACK);				\
       void *__p1 = __o1->next_free;					\
       *(int *) __p1 = (aint);						\
       __o1->next_free += sizeof (int);					\
       (void) 0; })

# define obstack_blank(OBSTACK, length)					\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       int __len = (length);						\
       if (__o->chunk_limit - __o->next_free < __len)			\
	 _obstack_newchunk (__o, __len);				\
       obstack_blank_fast (__o, __len);					\
       (void) 0; })

# define obstack_alloc(OBSTACK, length)					\
  __extension__								\
    ({ struct obstack *__h = (OBSTACK);					\
       obstack_blank (__h, (length));					\
       obstack_finish (__h); })

# define obstack_copy(OBSTACK, where, length)				\
  __extension__								\
    ({ struct obstack *__h = (OBSTACK);					\
       obstack_grow (__h, (where), (length));				\
       obstack_finish (__h); })

# define obstack_copy0(OBSTACK, where, length)				\
  __extension__								\
    ({ struct obstack *__h = (OBSTACK);					\
       obstack_grow0 (__h, (where), (length));				\
       obstack_finish (__h); })

/* The object made a finished one: the next begins after it, aligned, or at
 * the chunk's end if aligning would pass it. */
# define obstack_finish(OBSTACK)					\
  __extension__								\
    ({ struct obstack *__o1 = (OBSTACK);				\
       void *__value = (void *) __o1->object_base;			\
       if (__o1->next_free == __value)					\
	 __o1->maybe_empty_object = 1;					\
       __o1->next_free							\
	 = __PTR_ALIGN (__o1->object_base, __o1->next_free,		\
			__o1->alignment_mask);				\
       if (__o1->next_free - (char *) __o1->chunk			\
	   > __o1->chunk_limit - (char *) __o1->chunk)			\
	 __o1->next_free = __o1->chunk_limit;				\
       __o1->object_base = __o1->next_free;				\
       __value; })

/* OBJ and everything made after it freed: within the current chunk by
 * moving the current object back to it; otherwise by __obstack_free, which
 * frees the chunks after OBJ's -- and everything, for NULL. */
# define obstack_free(OBSTACK, OBJ)					\
  __extension__								\
    ({ struct obstack *__o = (OBSTACK);					\
       void *__obj = (OBJ);						\
       if (__obj > (void *) __o->chunk && __obj < (void *) __o->chunk_limit) \
	 __o->next_free = __o->object_base = (char *) __obj;		\
       else (__obstack_free) (__o, __obj); })

#else /* not GNU C: each argument evaluated once through the temp slot */

# define obstack_object_size(h)						\
  (unsigned) ((h)->next_free - (h)->object_base)

# define obstack_room(h)						\
  (unsigned) ((h)->chunk_limit - (h)->next_free)

# define obstack_empty_p(h)						\
  ((h)->chunk->prev == 0						\
   && (h)->next_free == __PTR_ALIGN ((char *) (h)->chunk,		\
				     (h)->chunk->contents,		\
				     (h)->alignment_mask))

# define obstack_make_room(h, length)					\
  ((h)->temp.tempint = (length),					\
   (((h)->next_free + (h)->temp.tempint > (h)->chunk_limit)		\
    ? (_obstack_newchunk ((h), (h)->temp.tempint), 0) : 0))

# define obstack_grow(h, where, length)					\
  ((h)->temp.tempint = (length),					\
   (((h)->next_free + (h)->temp.tempint > (h)->chunk_limit)		\
    ? (_obstack_newchunk ((h), (h)->temp.tempint), 0) : 0),		\
   memcpy ((h)->next_free, where, (h)->temp.tempint),			\
   (h)->next_free += (h)->temp.tempint)

# define obstack_grow0(h, where, length)				\
  ((h)->temp.tempint = (length),					\
   (((h)->next_free + (h)->temp.tempint + 1 > (h)->chunk_limit)		\
    ? (_obstack_newchunk ((h), (h)->temp.tempint + 1), 0) : 0),		\
   memcpy ((h)->next_free, where, (h)->temp.tempint),			\
   (h)->next_free += (h)->temp.tempint,					\
   *((h)->next_free)++ = 0)

# define obstack_1grow(h, datum)					\
  ((((h)->next_free + 1 > (h)->chunk_limit)				\
    ? (_obstack_newchunk ((h), 1), 0) : 0),				\
   obstack_1grow_fast (h, datum))

# define obstack_ptr_grow(h, datum)					\
  ((((h)->next_free + sizeof (char *) > (h)->chunk_limit)		\
    ? (_obstack_newchunk ((h), sizeof (char *)), 0) : 0),		\
   obstack_ptr_grow_fast (h, datum))

# define obstack_int_grow(h, datum)					\
  ((((h)->next_free + sizeof (int) > (h)->chunk_limit)			\
    ? (_obstack_newchunk ((h), sizeof (int)), 0) : 0),			\
   obstack_int_grow_fast (h, datum))

# define obstack_ptr_grow_fast(h, aptr)					\
  (((const void **) ((h)->next_free += sizeof (void *)))[-1] = (aptr))

# define obstack_int_grow_fast(h, aint)					\
  (((int *) ((h)->next_free += sizeof (int)))[-1] = (aint))

# define obstack_blank(h, length)					\
  ((h)->temp.tempint = (length),					\
   (((h)->chunk_limit - (h)->next_free < (h)->temp.tempint)		\
    ? (_obstack_newchunk ((h), (h)->temp.tempint), 0) : 0),		\
   obstack_blank_fast (h, (h)->temp.tempint))

# define obstack_alloc(h, length)					\
  (obstack_blank ((h), (length)), obstack_finish ((h)))

# define obstack_copy(h, where, length)					\
  (obstack_grow ((h), (where), (length)), obstack_finish ((h)))

# define obstack_copy0(h, where, length)				\
  (obstack_grow0 ((h), (where), (length)), obstack_finish ((h)))

# define obstack_finish(h)						\
  (((h)->next_free == (h)->object_base					\
    ? (((h)->maybe_empty_object = 1), 0)				\
    : 0),								\
   (h)->temp.tempptr = (h)->object_base,				\
   (h)->next_free							\
     = __PTR_ALIGN ((h)->object_base, (h)->next_free,			\
		    (h)->alignment_mask),				\
   (((h)->next_free - (char *) (h)->chunk				\
     > (h)->chunk_limit - (char *) (h)->chunk)				\
    ? ((h)->next_free = (h)->chunk_limit) : 0),				\
   (h)->object_base = (h)->next_free,					\
   (h)->temp.tempptr)

# define obstack_free(h, obj)						\
  ((h)->temp.tempint = (char *) (obj) - (char *) (h)->chunk,		\
   ((((h)->temp.tempint > 0						\
      && (h)->temp.tempint < (h)->chunk_limit - (char *) (h)->chunk))	\
    ? (void) ((h)->next_free = (h)->object_base				\
	      = (h)->temp.tempint + (char *) (h)->chunk)		\
    : (__obstack_free) ((h), (h)->temp.tempint + (char *) (h)->chunk)))

#endif /* not GNU C */

#ifdef __cplusplus
}
#endif

#endif /* _OBSTACK_H */
