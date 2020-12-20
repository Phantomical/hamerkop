
#include <unistd.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <signal.h>

// These syscall numbers copied from liburing.
#ifdef __alpha__
/*
 * alpha is the only exception, all other architectures
 * have common numbers for new system calls.
 */
# ifndef __NR_io_uring_setup
#  define __NR_io_uring_setup       535
# endif
# ifndef __NR_io_uring_enter
#  define __NR_io_uring_enter       536
# endif
# ifndef __NR_io_uring_register
#  define __NR_io_uring_register    537
# endif
#else /* !__alpha__ */
# ifndef __NR_io_uring_setup
#  define __NR_io_uring_setup       425
# endif
# ifndef __NR_io_uring_enter
#  define __NR_io_uring_enter       426
# endif
# ifndef __NR_io_uring_register
#  define __NR_io_uring_register    427
# endif
#endif

struct io_uring_params;

int io_uring_register(
    int fd,
    unsigned opcode,
    const void* arg,
    unsigned nr_args
) {
    return syscall(__NR_io_uring_register, fd, opcode, arg, nr_args);
}

int io_uring_setup(
    unsigned entries,
    struct io_uring_params* p
) {
    return syscall(__NR_io_uring_setup, entries, p);
}

int io_uring_enter(
    int fd,
    unsigned to_submit,
    unsigned min_complete,
    unsigned flags,
    sigset_t* sig
) {
    return syscall(__NR_io_uring_enter, fd, to_submit, min_complete, flags, sig, _NSIG / 8);
}
