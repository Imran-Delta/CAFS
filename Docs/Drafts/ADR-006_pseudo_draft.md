# ADR-005 (DRAFT): SMART Handler — Communication with C

**Status**: Accepted (2026-08-31)

## Context

The C mntproc and Python SMART Handler must communicate. The design needs a simple, cross-platform IPC method that supports passing JSON data.

## Decision

Use **`stdin`/`stdout` pipes** with `fork()` + `exec()`.

- C writes JSON input to Python's `stdin`.
- Python reads `stdin`, processes data, writes JSON result to `stdout`.
- C reads `stdout`, parses result.

## Rationale

- **Cross-platform**: Works on Linux and Windows.
- **No persistent state**: Python process exits after each call — no IPC state to manage.
- **Simple**: No sockets, no shared memory, no locking.
- **Adequate**: SMART runs every 60 seconds — process spawn overhead is acceptable.

## Consequences

- **Positive**: Simple, portable, easy to debug.
- **Negative**: Process spawn overhead (~5ms per call — acceptable for 60-second interval).
- **Mitigation**: For hash daemon (high frequency), a separate design will be considered.
