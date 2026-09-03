# Makefile — CAFS I/O Engine (V1, Linux x86-64, kernel 6.12+)
#
# SPDX-License-Identifier: BSD-3-Clause
# Copyright (c) 2026, Imran Bin Gifary (System Delta)

CC := gcc
AR := ar

CFLAGS := -Wall -Wextra -O2 -std=c11 -pthread -D_GNU_SOURCE -g
ASFLAGS := -D_GNU_SOURCE

VENDOR := ../../../../vendor/blake3
BLAKE3_DEFS := -DBLAKE3_NO_AVX512 -DBLAKE3_NO_AVX2 -DBLAKE3_NO_SSE41 -DBLAKE3_NO_SSE2
INCLUDES := -I. -I$(VENDOR)

C_SRCS := block_io.c anchor_io.c mount.c check.c remount.c confirm.c strerror.c checksum.c blake3_wrap.c
ASM_SRCS := syscalls_x86_64.S
VENDOR_SRCS := $(VENDOR)/blake3.c $(VENDOR)/blake3_dispatch.c $(VENDOR)/blake3_portable.c

OBJS := $(C_SRCS:.c=.o) $(ASM_SRCS:.S=.o)
VENDOR_OBJS := $(VENDOR_SRCS:.c=.o)

LIB := libcafs_io.a
MKFS := mkfs_cafs
TEST := test/test_pipeline

.PHONY: all clean test

all: $(LIB) $(MKFS)

$(LIB): $(OBJS) $(VENDOR_OBJS)
	$(AR) rcs $@ $^

# mkfs_cafs doesn't need the ctx/mutex machinery — just checksums,
# layout constants, and the raw syscalls.
MKFS_OBJS := mkfs_cafs.o checksum.o blake3_wrap.o syscalls_x86_64.o
$(MKFS): $(MKFS_OBJS) $(VENDOR_OBJS)
	$(CC) $(CFLAGS) $(INCLUDES) -o $@ $(MKFS_OBJS) $(VENDOR_OBJS)

$(TEST): test/test_pipeline.c $(LIB) $(MKFS)
	$(CC) $(CFLAGS) $(INCLUDES) -o $@ test/test_pipeline.c $(LIB)

test: $(TEST)
	./$(TEST)

%.o: %.c
	$(CC) $(CFLAGS) $(INCLUDES) -c -o $@ $<

%.o: %.S
	$(CC) $(ASFLAGS) -c -o $@ $<

$(VENDOR)/%.o: $(VENDOR)/%.c
	$(CC) $(CFLAGS) $(BLAKE3_DEFS) -c -o $@ $<

clean:
	rm -f $(OBJS) $(VENDOR_OBJS) $(LIB) $(MKFS) $(MKFS_OBJS) $(TEST) test/*.o test/*.img
