# onport — see what's listening on your ports

## Project overview

`onport` is a cross-platform CLI tool that instantly shows what processes are listening on which ports. It replaces the mental gymnastics of `lsof -i`, `ss -tlnp`, `netstat -an`, and `Get-NetTCPConnection` with a single, beautiful, memorable command.

```
$ onport
 PORT   PROTO  PROCESS          PID    USER   STATE
   22   tcp    sshd            1204   root    LISTEN
   80   tcp    traefik         8832   root    LISTEN   [docker: traefik]
  443   tcp    traefik         8832   root    LISTEN   [docker: traefik]
 3000   tcp    node           14523   hadi    LISTEN
 5432   tcp    postgres        9102   root    LISTEN   [docker: scolx-db]
 8080   tcp    nginx          14201   www     LISTEN

$ onport 3000
 PORT   PROTO  PROCESS   PID    USER   STATE
 3000   tcp    node     14523   hadi   LISTEN

  Kill this process? [y/N]

$ onport :5432 :3000
 PORT   PROTO  PROCESS    PID    USER   STATE
 3000   tcp    node      14523   hadi   LISTEN
 5432   tcp    postgres   9102   root   LISTEN   [docker: scolx-db]
```

## Tech stack

- Language: Rust
- CLI framework: `clap` (derive API)
- Colored output: `tabled` for table rendering + `owo-colors` for coloring
- Cross-platform process info: `sysinfo` crate
- Docker detection: `bollard` crate (async Docker API client)
- Async runtime: `tokio` (needed for bollard)

## Target platforms

- **Linux** (primary): Read `/proc/net/tcp`, `/proc/net/tcp6`, `/proc/net/udp`, `/proc/net/udp6`. Resolve PIDs via `/proc/{pid}/fd` → socket inode mapping, or use `netlink` via `netlink-packet-sock-diag` crate for a cleaner approach. Process names from `/proc/{pid}/comm`, users from `/proc/{pid}/status`.
- **macOS**: Use `libproc` bindings or shell out to `lsof -iTCP -sTCP:LISTEN -nP` and parse. The `libproc` crate provides Rust bindings.
- **Windows**: Use `GetExtendedTcpTable` / `GetExtendedUdpTable` from the Win32 `iphlpapi` API. The `windows` crate provides bindings. Process names via `OpenProcess` + `QueryFullProcessImageName`.

## CLI interface

```
onport                      # Show all listening ports
onport 3000                 # Show what's on port 3000
onport :3000 :8080          # Show multiple specific ports
onport --tcp                # TCP only
onport --udp                # UDP only
onport --all                # Show all states, not just LISTEN
onport -k 3000              # Kill whatever is on port 3000 (with confirmation)
onport -kf 3000             # Force kill (SIGKILL / taskkill /F) without confirmation
onport --docker             # Show Docker container names for containerized processes
onport --json               # JSON output for scripting
onport --no-color           # Disable colors
onport -w / --watch         # Live-updating view (refresh every 2s)
```

### Argument parsing rules

- Bare numbers are treated as ports: `onport 3000` = `onport :3000`
- Colon prefix is optional: `onport :3000` and `onport 3000` are equivalent
- Multiple ports: `onport 3000 8080 5432`
- Port ranges (stretch goal): `onport 3000-3010`

## Core architecture

```
src/
├── main.rs              # CLI parsing, entry point
├── platform/
│   ├── mod.rs           # Platform trait definition
│   ├── linux.rs         # Linux /proc/net + netlink implementation
│   ├── macos.rs         # macOS lsof/libproc implementation
│   └── windows.rs       # Windows iphlpapi implementation
├── docker.rs            # Docker container name resolution (optional, graceful fallback)
├── types.rs             # PortEntry, Protocol, State enums
├── output/
│   ├── mod.rs           # Output trait
│   ├── table.rs         # Pretty table output
│   ├── json.rs          # JSON output
│   └── watch.rs         # Live-updating watch mode
└── kill.rs              # Process kill logic (SIGTERM/SIGKILL on Unix, taskkill on Windows)
```

### Core types

```rust
pub struct PortEntry {
    pub port: u16,
    pub protocol: Protocol,
    pub state: SocketState,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub user: Option<String>,
    pub local_addr: IpAddr,
    pub remote_addr: Option<SocketAddr>,   // For established connections (--all mode)
    pub docker_container: Option<String>,  // Container name if inside Docker
}

pub enum Protocol {
    Tcp,
    Udp,
}

pub enum SocketState {
    Listen,
    Established,
    TimeWait,
    CloseWait,
    SynSent,
    SynRecv,
    Other(String),
}
```

### Platform trait

```rust
pub trait PlatformProvider {
    fn list_sockets(&self) -> Result<Vec<PortEntry>>;
}
```

Each platform module implements this trait. The main binary selects the right implementation at compile time via `#[cfg(target_os = "...")]`.

## Output design

The output should be immediately scannable. Design principles:

- **Port numbers right-aligned** for easy scanning
- **Process names left-aligned, truncated to 16 chars** if needed
- **Color coding**: green for LISTEN, yellow for ESTABLISHED, red for TIME_WAIT/CLOSE_WAIT
- **Docker containers** shown in square brackets `[docker: name]` in a muted color
- **No header clutter** — just the table, clean
- **Sort by port number** by default

When filtering to a single port (`onport 3000`), show more detail:
- Full command line (not just process name)
- Process start time
- Open file descriptors count
- Prompt to kill with `y/N`

## Docker integration

Docker detection is **optional and graceful**. If Docker socket is not available, skip silently — never error. Implementation:

1. Try connecting to `/var/run/docker.sock` (Linux/macOS) or named pipe (Windows)
2. If connected, list all containers and their port mappings
3. For each PortEntry, check if the PID matches a containerized process by:
   - Matching port mappings from `docker inspect`
   - Or checking if PID's cgroup contains a Docker container ID (Linux only, via `/proc/{pid}/cgroup`)
4. If matched, set `docker_container` to the container name

The `--docker` flag makes this explicit, but even without it, Docker info is shown if the socket is available. Use `--no-docker` to disable.

## Kill functionality

Safety is critical:

- `onport -k 3000`: Show what's on port 3000, ask "Kill process `node` (PID 14523)? [y/N]"
- `onport -kf 3000`: Skip confirmation, send SIGKILL (Linux/macOS) or `taskkill /F` (Windows)
- Default signal is SIGTERM (graceful), with a note: "Process still running after 3s? Use `onport -kf 3000`"
- Never kill PID 1, kernel threads, or the current shell process
- If the port is owned by a Docker container, suggest `docker stop <name>` instead of killing the PID

## Watch mode

`onport -w` or `onport --watch`:

- Clear screen and redraw every 2 seconds
- Highlight new ports in green, disappeared ports in red (for one refresh cycle)
- Show a timestamp in the header
- Exit with `q` or Ctrl+C
- Uses `crossterm` for terminal manipulation

## Build and distribution

### Cargo.toml essentials

```toml
[package]
name = "onport"
version = "0.1.0"
edition = "2021"
description = "See what's listening on your ports"
license = "MIT"
repository = "https://github.com/<username>/onport"
keywords = ["port", "network", "process", "lsof", "cli"]
categories = ["command-line-utilities", "network-programming"]

[dependencies]
clap = { version = "4", features = ["derive"] }
tabled = "0.17"
owo-colors = "4"
sysinfo = "0.33"
tokio = { version = "1", features = ["rt", "net"], optional = true }
bollard = { version = "0.18", optional = true }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
crossterm = "0.28"

[features]
default = ["docker"]
docker = ["dep:tokio", "dep:bollard"]

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_NetworkManagement_IpHelper",
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_Security",
] }

[profile.release]
lto = true
strip = true
codegen-units = 1
```

### CI/CD (GitHub Actions)

- Build on push to main and PRs
- Release workflow triggered by tags (`v*`)
- Build matrix: `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`
- Publish to crates.io on release
- Upload prebuilt binaries to GitHub Releases
- Include SHA256 checksums

### Installation methods (for README)

```bash
# Cargo
cargo install onport  # note: `onport` might be taken on crates.io, check first

# Homebrew (create formula after first release)
brew install onport

# Arch Linux (AUR, create PKGBUILD)
yay -S onport

# Pre-built binary
curl -sSL https://github.com/<user>/onport/releases/latest/download/onport-linux-x86_64.tar.gz | tar xz
sudo mv onport /usr/local/bin/
```

## README structure

This is critical — the README sells the tool. Structure:

1. **One-liner + badge row**: "See what's listening on your ports" + CI badge + crates.io badge + license badge
2. **Hero screenshot**: Animated GIF showing `onport` → beautiful table → `onport -k 3000` → kill confirmation. This is the MOST important element.
3. **Installation**: cargo install, brew, AUR, binary download
4. **Usage examples**: 5-6 common use cases with output
5. **Comparison table**: `onport` vs `lsof` vs `ss` vs `netstat` — show the command you'd have to type for each tool to accomplish the same thing
6. **Benchmarks** (optional): Time to list all ports vs lsof
7. **Configuration**: Environment variables if any (e.g., `PLS_NO_DOCKER=1`)
8. **Contributing**: Standard Rust contributing guide
9. **License**: MIT

## Week-by-week plan

### Week 1: Core engine + Linux
- [ ] Scaffold project with clap CLI
- [ ] Implement Linux `/proc/net/tcp` parser
- [ ] PID resolution via `/proc/{pid}/fd` inode matching
- [ ] Process name + user resolution
- [ ] Basic table output with colors
- [ ] Port filtering (single and multiple)
- [ ] JSON output mode

### Week 2: Cross-platform + Docker + kill
- [ ] macOS implementation (lsof parsing or libproc)
- [ ] Windows implementation (iphlpapi)
- [ ] Docker container name resolution
- [ ] Kill functionality with confirmation prompt
- [ ] Watch mode
- [ ] UDP support

### Week 3: Polish + ship
- [ ] Record terminal GIF for README (use `vhs` by charmbracelet or `asciinema`)
- [ ] Write README with comparison table
- [ ] Set up GitHub Actions CI/CD
- [ ] Build prebuilt binaries for all platforms
- [ ] Test on actual Linux, macOS, Windows machines
- [ ] Publish to crates.io
- [ ] Create AUR PKGBUILD
- [ ] Write Show HN post
- [ ] Post to r/rust, r/commandline, r/programming

## Crate name

The crate and binary name is `onport`. Publish to crates.io as `onport`.

## Stretch goals (post v0.1)

- [ ] Port ranges: `onport 3000-3010`
- [ ] Process tree: show parent process chain
- [ ] Kubernetes pod resolution (similar to Docker)
- [ ] Remote mode: `onport --host 192.168.1.50` via SSH
- [ ] TUI mode with ratatui (interactive, sortable, filterable)
- [ ] Bandwidth per port (like bandwhich but port-focused)
- [ ] Shell completions (bash, zsh, fish, PowerShell)
- [ ] `onport why 3000` — explain why a port is in use (e.g., "Port 3000 is commonly used by Node.js dev servers")
