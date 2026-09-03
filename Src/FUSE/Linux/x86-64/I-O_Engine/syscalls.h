/*
 * syscalls.h — prototypes for syscalls_x86_64.S.
 *
 * All six return the raw kernel convention: >=0 on success, -errno on
 * failure. None of these touch libc's errno.
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#ifndef CAFS_SYSCALLS_H
#define CAFS_SYSCALLS_H

#include <stdint.h>
#include <stddef.h>

extern int64_t cafs_sys_openat(int dirfd, const char *path, int flags, int mode);
extern int64_t cafs_sys_pread64(int fd, void *buf, size_t count, int64_t offset);
extern int64_t cafs_sys_pwrite64(int fd, const void *buf, size_t count, int64_t offset);
extern int64_t cafs_sys_close(int fd);
extern int64_t cafs_sys_fdatasync(int fd);
extern int64_t cafs_sys_ioctl(int fd, unsigned long request, void *argp);
extern int64_t cafs_sys_fstat(int fd, void *statbuf);

#define CAFS_AT_FDCWD (-100)

#endif /* CAFS_SYSCALLS_H */
