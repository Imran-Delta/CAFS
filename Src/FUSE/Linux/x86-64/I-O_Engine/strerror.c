/*
 * strerror.c — cafs_strerror().
 *
 * SPDX-License-Identifier: BSD-3-Clause
 * Copyright (c) 2026, Imran Bin Gifary (System Delta)
 */
#include "cafs_io.h"

const char *cafs_strerror(cafs_status_t status)
{
    switch (status) {
    case CAFS_OK:                return "success";
    case CAFS_ERR_OPEN:           return "device open failed";
    case CAFS_ERR_IO:              return "read/write syscall failed";
    case CAFS_ERR_TOO_SMALL:       return "device too small for the anchor region";
    case CAFS_ERR_BAD_MAGIC:       return "structure magic mismatch";
    case CAFS_ERR_CHECKSUM:        return "structure checksum mismatch";
    case CAFS_ERR_NOMEM:            return "out of memory";
    case CAFS_ERR_READONLY:         return "operation not permitted on a readonly mount";
    case CAFS_ERR_MISALIGNED:       return "unaligned I/O rejected by mount policy";
    case CAFS_ERR_NOT_MOUNTED:      return "called before mount() or after close()";
    case CAFS_ERR_INVALID_ARG:      return "invalid argument";
    case CAFS_ERR_UNSUPPORTED:      return "requested option unsupported on this device";
    default:                         return "unknown cafs_status_t";
    }
}
