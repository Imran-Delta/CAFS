# CAFS Error Identifiers

**Document version:** v1
**Status:** Scheme decided; the actual catalog of codes will grow as `errors.md` (a separate, top-level file) is populated. This document describes the *scheme*, not the catalog itself.

---

## Format

`CA-NNNN` (or `CAFS-NNNN` where the longer form reads better — both are the same namespace, not two separate ones). Sequentially assigned, grouped by subsystem via the leading digit, **not derived from a hash**.

A hash of the human-readable identifier was considered and rejected: a 4-byte hash slice lands outside Windows Event Log's compatibility-recommended ID range (kept under 65,536; a 4-byte value is essentially always larger) essentially every time by construction, and a 4-byte (32-bit) hash space starts colliding around ~65,000 distinct entries by the birthday paradox — not a safe margin for a system explicitly anticipated to grow large enough to need an external reference document. Sequential assignment has zero collision risk by construction and naturally stays inside the compatible range. If the underlying motivation is avoiding coordination overhead between contributors adding errors in different files, the fix is reserved number ranges per subsystem (below), not a hash.

### Reserved ranges

| Range | Subsystem |
|---|---|
| 1000–1999 | Allocator |
| 2000–2999 | WAL |
| 3000–3999 | SMART / handler |
| 4000–4999 | *(reserved, unassigned)* |

Ranges beyond 1xxx are placeholders for future subsystems as they need their own codes — not yet populated with real entries.

## Codes defined so far

| Code | Meaning |
|---|---|
| `CA-1001` | Allocator could not satisfy this allocation attempt (per-attempt ENOSPC — does not necessarily mean the device is full; see Docs/allocator.md Section 7) |
| `CA-1002` | Extent-fragmentation not allowed — the target file carries `extent_limit=1` and the allocator cannot place the block without fragmenting it |
| `CA-1003` | Identifier slots exhausted — a distinct resource from data-block space; reclaiming free blocks does not resolve this |

## Platform surfacing

**Linux**: `dmesg`/syslog/journald — the `CA-NNNN` string appears directly in the message text, no platform-specific translation needed.

**Windows Event Log**: the log's actual queryable Event ID field is numeric only (traditionally kept under 65,536 for compatibility, though the field itself is INT32-capable) — there is no native string-ID field a `CA-1001`-style identifier slots into directly. Resolution: the numeric part (`1001`) is the real Event ID; `CA-1001` appears in the human-readable message text, not as the machine-queryable field. Requires a registered Event "Source" name (done at install time) and ideally a message file for proper text rendering in Event Viewer — both need administrator rights, which is already required for the kernel helper driver install (Docs/io_engine.md), so this adds no new friction.

## Documentation

Top-level `errors.md` is the canonical, version-controlled reference — one entry per code, short description. A read-only external document (Google Doc or similar) is under consideration as a future overflow/discoverability aid if the catalog grows large, but is explicitly a secondary, non-authoritative copy — it lives outside git and won't version alongside code changes the way `errors.md` does.

## Not adopted

**EDQUOT** and **EFBIG** (POSIX's quota-exceeded and file-too-large errors) are deliberately not part of this scheme. Neither has an underlying mechanism in CAFS that could ever produce it — there is no quota system and no independent max-file-size concept anywhere in the design. The nearest thing to EFBIG's role, a hard per-file contiguity requirement, already has its own code (`CA-1002`). Adopting either would be importing POSIX's taxonomy without the feature that makes it real; if quotas or an independent size cap are added later, codes can be assigned then.
