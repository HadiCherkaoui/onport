# CLAUDE.md — onport project

## What is this project?

`onport` is a cross-platform Rust CLI tool that shows what processes are listening on which ports. It replaces `lsof -i`, `ss -tlnp`, `netstat -an`, and PowerShell's `Get-NetTCPConnection` with one beautiful, instant command.

## Project principles

- **One command, zero config.** `onport` with no arguments shows everything. No flags needed for the common case.
- **Beautiful by default.** Colored, aligned, scannable table output. Looks good in any terminal.
- **Cross-platform.** Linux (primary), macOS, Windows. Platform-specific code lives in `src/platform/`.
- **Graceful degradation.** Docker not available? Skip silently. Can't resolve a PID? Show "?" instead of erroring. Never crash on missing info.
- **Fast.** Should complete in under 50ms for the common case. No unnecessary allocations or syscalls.

## Architecture

```
src/
├── main.rs              # CLI parsing (clap derive), entry point
├── platform/
│   ├── mod.rs           # PlatformProvider trait + platform selection
│   ├── linux.rs         # /proc/net/* parsing + inode→PID resolution
│   ├── macos.rs         # lsof-based parsing
│   ├── windows.rs       # Win32 iphlpapi API
│   └── freebsd.rs       # sockstat output parsing
├── docker.rs            # Optional Docker container name resolution via bollard
├── types.rs             # PortEntry, Protocol, SocketState, ProcessDetails structs/enums
├── process_detail.rs    # On-demand process detail resolution (cmdline, start time, fd count)
├── output/
│   ├── mod.rs           # Output format selection + render_details
│   ├── table.rs         # Pretty colored table via tabled + owo-colors
│   ├── json.rs          # JSON output via serde_json
│   └── watch.rs         # Live-updating watch mode via crossterm
└── kill.rs              # Process termination (SIGTERM/SIGKILL on Unix, taskkill on Windows)
```

## Key technical decisions

- **Linux socket enumeration**: Use `/proc/net/tcp` and `/proc/net/tcp6` parsing (not netlink) for simplicity in v0.1. Each line gives local_address:port, remote_address:port, state, and inode. Map inode → PID by scanning `/proc/{pid}/fd/` symlinks for `socket:[inode]` matches.
- **Docker detection**: Use `bollard` crate behind a `docker` feature flag. Connect to Docker socket, list containers, match container names via published port mapping. If socket unavailable, skip silently. Pass `--no-docker` at runtime to skip enrichment entirely (useful for speed or when the Docker socket is slow to respond).
- **Table rendering**: Use `tabled` crate with `owo-colors` for terminal coloring. Right-align port numbers, left-align everything else.
- **CLI framework**: `clap` with derive macros. Keep the arg struct flat and simple.

## Code style

- Use `anyhow` for error handling throughout
- Prefer iterators and collect over manual loops
- Platform-specific code behind `#[cfg(target_os = "...")]` at the module level, not scattered through the codebase
- No `unwrap()` in non-test code — always handle errors gracefully
- Every public function gets a doc comment

## Testing approach

- Unit tests for each platform's parser (using sample /proc/net/tcp content for Linux)
- Integration tests that run the actual binary and check output format
- Platform-specific tests behind `#[cfg]` attributes
- Mock Docker responses for Docker integration tests

## Build notes

- `cargo build` for dev, `cargo build --release` for production
- `cargo build --no-default-features` to build without Docker support
- Release profile uses LTO + strip for smallest binary
- CI builds for: x86_64-linux-gnu, x86_64-apple-darwin, aarch64-apple-darwin, x86_64-windows-msvc, x86_64-unknown-freebsd
