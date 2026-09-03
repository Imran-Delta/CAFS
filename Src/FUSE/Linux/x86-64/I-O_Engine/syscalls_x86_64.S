/*
 * syscalls_x86_64.S — raw Linux x86-64 syscalls for device access.
 * No libc wrappers: direct `syscall` instruction, kernel 6.12+ ABI
 * (these six syscalls have been stable for well over a decade; 6.12
 * is a support-floor statement, not a reason to use a newer
 * mechanism like io_uring here).
 *
 * System V AMD64 C ABI (caller -> here): rdi, rsi, rdx, rcx, r8, r9
 * Linux syscall ABI (here -> kernel):     rdi, rsi, rdx, r10, r8, r9
 * The one difference that matters: the 4th argument moves from rcx to
 * r10 before every `syscall` below. rcx/r11 are clobbered by the
 * `syscall` instruction itself, which is exactly why the ABI avoids
 * rcx for arg 4.
 *
 * Return convention: matches the raw kernel convention, not errno.
 * On error, rax holds -errno (a small negative number); the C side
 * checks `ret < 0` and interprets `-ret` as the errno value itself.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
    .text

/* Linux x86-64 syscall numbers used here. */
#define SYS_close       3
#define SYS_fstat       5
#define SYS_pread64     17
#define SYS_pwrite64    18
#define SYS_ioctl       16
#define SYS_fdatasync   75
#define SYS_openat      257

/* int64_t cafs_sys_openat(int dirfd, const char *path, int flags, int mode); */
    .globl cafs_sys_openat
    .type cafs_sys_openat,@function
cafs_sys_openat:
    mov     %ecx, %r10d         /* arg4 mode: rcx -> r10 */
    mov     $SYS_openat, %eax
    syscall
    ret
    .size cafs_sys_openat, . - cafs_sys_openat

/* int64_t cafs_sys_pread64(int fd, void *buf, size_t count, off_t offset); */
    .globl cafs_sys_pread64
    .type cafs_sys_pread64,@function
cafs_sys_pread64:
    mov     %rcx, %r10          /* arg4 offset: rcx -> r10 */
    mov     $SYS_pread64, %eax
    syscall
    ret
    .size cafs_sys_pread64, . - cafs_sys_pread64

/* int64_t cafs_sys_pwrite64(int fd, const void *buf, size_t count, off_t offset); */
    .globl cafs_sys_pwrite64
    .type cafs_sys_pwrite64,@function
cafs_sys_pwrite64:
    mov     %rcx, %r10          /* arg4 offset: rcx -> r10 */
    mov     $SYS_pwrite64, %eax
    syscall
    ret
    .size cafs_sys_pwrite64, . - cafs_sys_pwrite64

/* int64_t cafs_sys_close(int fd); */
    .globl cafs_sys_close
    .type cafs_sys_close,@function
cafs_sys_close:
    mov     $SYS_close, %eax
    syscall
    ret
    .size cafs_sys_close, . - cafs_sys_close

/* int64_t cafs_sys_fdatasync(int fd); */
    .globl cafs_sys_fdatasync
    .type cafs_sys_fdatasync,@function
cafs_sys_fdatasync:
    mov     $SYS_fdatasync, %eax
    syscall
    ret
    .size cafs_sys_fdatasync, . - cafs_sys_fdatasync

/* int64_t cafs_sys_ioctl(int fd, unsigned long request, void *argp); */
    .globl cafs_sys_ioctl
    .type cafs_sys_ioctl,@function
cafs_sys_ioctl:
    mov     $SYS_ioctl, %eax
    syscall
    ret
    .size cafs_sys_ioctl, . - cafs_sys_ioctl

/* int64_t cafs_sys_fstat(int fd, void *statbuf); — raw kernel `struct
 * stat` layout (144 bytes, x86-64), not glibc's; the C side picks
 * st_mode/st_size out of it by fixed offset. See cafs_io_layout.h-
 * adjacent comment in mount.c for the exact offsets used. */
    .globl cafs_sys_fstat
    .type cafs_sys_fstat,@function
cafs_sys_fstat:
    mov     $SYS_fstat, %eax
    syscall
    ret
    .size cafs_sys_fstat, . - cafs_sys_fstat

    .section .note.GNU-stack,"",@progbits
