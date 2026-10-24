# CAFS licensing research

Working notes, 2026-10-23. Not legal advice. Items tagged **[verify]** were not
confirmed against primary text; everything else was read from the sources in
section 8 during research. Re-check before the first public binary release.

## 1. Decisions so far

- Userspace (`Src/FUSE`, shared code in top-level `Src`, tools): **BSD-3-Clause**.
  Goal: usable by anyone without forced publishing; the author's name stays in notices.
- `Src/Kernel` (Linux kernel code + Windows kernel helper): **GPL-2.0-only**.
  Goal: improvements by companies reach users as source.
- Linux: userspace only (FUSE) for now. No kernel module planned.
- Windows: userspace via WinFsp. A kernel helper (TRIM) is optional and built as a
  separate binary. No Microsoft signing, no Partner Center.
- Distribution: a zip installer, or a GitHub binary branch that an installer reads.
- Solo author for 5+ years, so one copyright holder today.

## 2. Proposed licence map

| Path | Licence | Notes |
|---|---|---|
| `Src/FUSE/**`, `Src/smart_handler.py`, shared `Src/*` | BSD-3-Clause | default |
| Shared on-disk format and ABI headers | BSD-3-Clause; for the Linux module use `GPL-2.0-only WITH Linux-syscall-note OR BSD-3-Clause` | userspace must be able to include them freely |
| `Src/Kernel/Linux` | GPL-2.0-only | matches the kernel |
| `Src/Kernel/Windows` | GPL-2.0-only (chosen; see 4.6 for the checks it needs) | separate binary |
| `LICENSES/`, per-file SPDX headers | all of the above | use REUSE lint in CI |

Enforcement: exclude `Src/Kernel` from the Cargo workspace, fail CI on any path
dependency from BSD crates into `Src/Kernel`, run `reuse lint`.

## 3. What "distribution" means

- Not distribution: building and running binaries on your own machines. GPLv3's
  definition of propagation excludes running a program and modifying a private copy.
- Distribution: any copy that reaches someone else. GPLv3 counts making a copy
  available to the public. A public zip, a GitHub binary branch, a release asset, and
  an installer that downloads from them all count, free or not.
- GPL-2.0-only has its own wording (section 3); the notes here use the v3 text.
  **[verify]** read GPLv2 section 3 before the first release.

| What you ship | Obligation |
|---|---|
| BSD-3 userspace binaries | Reproduce the copyright notice, conditions and disclaimer in the documentation shipped with the binaries. |
| GPL-2.0-only kernel binaries | Provide the matching source. Same place, or clear directions next to the binary; keep it available as long as the binaries are. Tag the exact commit and note it beside the binary. |
| One zip with both | An aggregate of separate programs does not extend the GPL to the others. Keep GPL binaries unlinked, in their own folder, with their own licence file. |
| WinFsp installer | Only if you bundle it. Recommended: do not bundle, link to winfsp.dev. |
| Microsoft redistributables | Only what the kit's REDIST list allows (see 4.6). Avoid shipping any. |

Practical release rule: every binary release is built from a git tag; the tag, commit
hash, and a `SOURCE.txt` with the repository URL sit next to the binaries; the
commit hash is embedded in the binary's version output.

## 4. Component by component

### 4.1 BSD-3-Clause
- Three conditions: keep the notice in source, reproduce it in docs shipped with
  binaries, no use of the authors' names to endorse derived products.
- Code comments are not the attribution mechanism. Put
  `Copyright (c) <year> <name>` and an SPDX line in every file.
- No express patent grant (unlike Apache-2.0). **[verify]**
- Anyone, including closed-source vendors, may fork and keep changes private.
- BSD-3 code may be compiled into a GPL-2.0-only module (distributed as GPL). The
  reverse is not allowed: nothing from `Src/Kernel` may move into BSD code without the
  author of that code consenting.

### 4.2 GPL-2.0-only (`Src/Kernel`)
- Incompatible with GPLv3 and Apache-2.0 code. **[verify]** So no WinFsp-derived code
  and no GPLv3 Rust crates inside `Src/Kernel`.
- Linux accepts module licence strings including "Dual BSD/GPL". As sole author you may
  dual-license your own kernel files `GPL-2.0-only OR BSD-3-Clause`, but that lets others
  reuse them under BSD, which defeats the copyleft goal.
- A kernel module with a non-GPL-compatible licence taints the kernel and cannot use
  GPL-only symbols.

### 4.3 WinFsp
- Licence: GPLv3 with a special exception for free/open-source software; a
  commercial licence is also sold (as seen: individuals 2400 USD per 3 years;
  organizations 6000 USD per 3 years for up to 10 developers). **[verify]** current prices.
- The exception grants two permissions: link against the WinFsp DLL
  (x64/x86/arm64/msil), and distribute unmodified official WinFsp installers.
- Conditions: (a) your software uses a licence meeting the Free Software Definition or
  the Open Source Definition; (b) it shows the WinFsp copyright notice (WinFsp, Windows
  File System Proxy, Bill Zissimopoulos) and a link to the WinFsp repository in its UI
  and user-facing documentation; (c) it is not linked or distributed with proprietary
  software.
- BSD-3 qualifies for (a). Print the notice in `--version`/about output and the docs.
- The exception names only the DLL and the installer. Do not vendor WinFsp's headers or
  import library into the repo (they are plain GPLv3). Build against the installed
  WinFsp SDK, or declare your own FFI and load the DLL at runtime.
- Closed-source forks of CAFS on Windows need their own WinFsp commercial licence.
  Say so in the README.
- Recommended: do not bundle the WinFsp installer. Point users to winfsp.dev; then you
  are not distributing WinFsp.

### 4.4 `winfsp` and `winfsp-sys` Rust crates
- Both declare GPL-3.0; the README states GPLv3 and grants no exception. If the
  BSD-3 Windows glue depends on them, the distributed Windows binary must be GPLv3.
- Options: (1) write your own FFI (own declarations, load the DLL at runtime);
  (2) a thin C shim in `Src/FUSE/Windows` compiled against the installed WinFsp SDK with
  the Rust core behind a C ABI; (3) ask the crate author for an exception; (4) accept
  GPLv3 for the Windows glue only.

### 4.5 Windows userspace filesystem options (licence and fit)
| Option | What it is | Licence angle / fit |
|---|---|---|
| WinFsp | kernel driver plus user-mode DLL; native NT-semantics API; FUSE2-style compat layer **[verify]** | GPLv3 + FLOSS exception; best fit for a full filesystem on its own block store |
| Dokan(y) | similar | LGPL library and driver; ruled out for permissions problems |
| ProjFS | Microsoft's projection layer; the provider's items are cached on a local file system | not a standalone filesystem; cannot host CAFS's raw layout; Windows 10 1809+ |
| WSL2 `--mount` | run the Linux FUSE build inside WSL2 with the disk passed through | no Windows driver at all; limits reported: no USB drives, no boot disks, access via a network path. **[verify]** in Microsoft docs |
| Loopback NFS/SMB server | userspace server mounted by Windows' own client | no driver; semantics and performance limits **[verify]** |
| Own IFS driver | write a full Windows filesystem driver | rejected: crash risk, signing |

CAFS stores a dual ACL (POSIX + NT security descriptor), which points at WinFsp's native
API rather than the FUSE-compat layer.

### 4.6 Windows Driver Kit (WDK / EWDK)
- Click-through licence per kit; the current page covers the Windows 11 26H1 EWDK
  (July 2026). Same Distributable Code section as older versions.
- Distributable Code means files listed in the kit's REDIST.TXT files and redistribution
  list. You may distribute its object code. `.lib` files may be shipped only as the result
  of linking them into your program. Downstream parties must accept terms protecting that
  code at least as much. You may not modify or distribute its source so that it becomes
  subject to an Excluded License (one requiring source disclosure or others' right to
  modify).
- The restriction attaches to Microsoft's Distributable Code, not to your own code. A
  GPL-2.0-only helper is not barred by these terms.
- Risk spots: copying sample/Distributable source into `Src/Kernel/Windows`; shipping WDK
  files or tools; statically linking Distributable libraries into a GPL driver. Check the
  REDIST.TXT in the installed kit for any KMDF stub libraries you link. If they are listed,
  add a GPL linking exception for them.
- Sample licences: WDF framework source is MIT; the Rust driver samples are
  Apache-2.0/MIT; check the licence file of any other sample you copy.
- Not read: the rest of the licence text. **[verify]**
- Signing programs (Hardware Dev Center) have separate terms; not needed while unsigned.

### 4.7 Signing, Secure Boot, user friction
- Test-signing works with any certificate, needs administrator rights, Secure Boot off,
  and (with memory integrity enabled) a test-signed binary.
- Production signing: attestation signing needs an EV certificate and a Partner Center
  account; the result is not Windows-certified and cannot ship through Windows Update.
  Self-signed certificates are not accepted for production kernel drivers.
- Precedent: WinBtrfs. Early versions needed Test Mode; current releases are signed by
  the maintainer but may fail under Secure Boot on newer Windows 10/11, with a registry
  workaround or Secure Boot off.
- Decision: do not sign. Document the workaround. The helper is optional, so CAFS works
  without it.

### 4.8 MSVC runtime
- Link the CRT statically so no Microsoft runtime DLLs ship. Reason: the WinFsp condition
  against distribution with proprietary software could arguably cover a bundled
  redistributable. **[verify]** (own interpretation)

### 4.9 Linux userspace
- libfuse: the repository lists GPL-2.0 and LGPL-2.1; the library is the LGPL part
  **[verify]**. Link dynamically and ship the licence text.
- The FUSE kernel header may be redistributed under either the GPL or a BSD licence
  (per the macFUSE licence text). A BSD-3 daemon speaking the protocol directly can use it
  under BSD terms.
- TRIM needs no kernel code: the `BLKDISCARD` ioctl works from userspace on a block device
  opened for writing, typically as root, with sector-aligned ranges.
- The kernel is GPL-2.0-only; there is no kernel work planned.

### 4.10 Other platforms (deferred)
- macFUSE: BSD-style, with a clause restricting binary redistribution bundled with
  commercial software. FUSE-T: free for non-commercial use, commercial licence needed for
  commercial use or bundling; its libfuse is LGPL. FreeBSD ships a FUSE kernel driver
  (fusefs) in base.

## 5. TRIM context (why a kernel helper exists)

- Without TRIM the SSD cannot see blocks CAFS has freed. It treats every LBA ever written
  as live, so its effective spare area shrinks to the factory over-provisioning. Capacity
  stays correct (the Zone Table knows free blocks); the cost is write amplification, wear
  and sustained speed.
- Mitigation without any driver: a `reserve_percent` config that never allocates the tail
  of the device. Rule of thumb for uniform random writes (from memory, unverified):
  write amplification is roughly (1 + OP) / (2 x OP).
- Windows: Microsoft documents the trim action of the data-set-management IOCTL as
  unsupported for user-mode callers. Test user-mode ATA/NVMe pass-through in a VM first;
  the helper may be unnecessary. Some USB bridges drop TRIM entirely. **[verify]**

## 6. Open questions

1. `winfsp` crate (GPLv3 Windows glue) vs own FFI vs C shim.
2. Are KMDF stub libraries in the kit's REDIST list?
3. GPLv2 section 3 wording for the binary-branch release process.
4. Does a bundled Microsoft runtime breach the WinFsp "no proprietary software" condition?
5. Licence of any WDK sample you copy.
6. Whether a solo individual can enrol for attestation signing (only if signing is ever reconsidered).
7. Contribution policy before the first PR: DCO sign-off, inbound = outbound per folder,
   and whether you want the right to relicense. Sole authorship allows relicensing today;
   outside contributions without a policy remove that.
8. Third-party notices: a `THIRD-PARTY-NOTICES` file for Rust std (MIT/Apache) and every
   crate; run a licence audit tool in CI.
9. Clean-room rule: studying btrfs/XFS/bcachefs formats is fine; no verbatim GPL code in
   the BSD core (including AI-drafted code).
10. Legal review before a wide public release.

## 7. Repository layout (as of 2026-10-23)

```
Src/FUSE/Linux/x86-64/I-O_Engine   Rust crate (anchor, block_io, check, checksum,
                                   confirm, error, layout, mount, remount, mkfs_cafs)
Src/FUSE/Windows                   placeholder
Src/Kernel/Linux                   placeholder
Src/Kernel/Windows                 placeholder
Src/smart_handler.py
```
Nothing is shared yet: the format code sits inside the Linux x86-64 crate. Move it to a
shared top-level crate; keep kernel C definitions in sync with a conformance test.

## 8. Sources

- WinFsp licence text (copy): https://gitverse.ru/valemobil/kvm-guest-drivers-windows/content/master/viofs/LICENSE (verify against github.com/winfsp/winfsp License.txt)
- WinFsp project and pricing: https://winfsp.dev/ and https://github.com/winfsp/winfsp
- winfsp crates: https://docs.rs/crate/winfsp/latest/source/Cargo.toml, https://docs.rs/crate/winfsp-sys/0.2.2+winfsp-2.0/source/Cargo.toml.orig, https://docs.rs/crate/winfsp/latest/source/README.md
- Enterprise WDK licence (VS 2026): https://learn.microsoft.com/en-us/legal/windows/hardware/enterprise-wdk-license-2026
- WDK licence (VS 2022): https://learn.microsoft.com/en-us/legal/windows/hardware/enterprise-wdk-license-2022
- Test-signing: https://learn.microsoft.com/en-sg/WINDOWS-HARDWARE/DRIVERS/install/the-testsigning-boot-configuration-option
- Trim action not supported in user mode: https://learn.microsoft.com/en-us/windows/desktop/api/WinIoCtl/ns-winioctl-device_manage_data_set_attributes_output
- WinBtrfs: https://github.com/maharmstone/btrfs ; news article: https://korben.info/en/winbtrfs-windows-driver-mounts-linux-disks-wakes-up.html
- ProjFS: https://learn.microsoft.com/windows/win32/api/projectedfslib/nf-projectedfslib-prjupdatefileifneeded
- macFUSE / FUSE-T licences: see their repositories' LICENSE files
- libfuse licences: https://github.com/libfuse/libfuse
- blkdiscard: https://manpages.ubuntu.com/manpages/noble/man8/blkdiscard.8.html
