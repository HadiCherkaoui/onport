# onport

> See what's listening on your ports.

![onport demo](demo.gif)

`onport` is a cross-platform CLI tool that instantly shows what processes are listening on which ports. One command replaces `lsof -i`, `ss -tlnp`, `netstat -an`, and `Get-NetTCPConnection`.

> **Downloading?** Pre-built binaries are available on GitLab — see the
> [latest release](https://gitlab.cherkaoui.ch/HadiCherkaoui/onport/-/releases)
> for Linux, Windows, macOS, and FreeBSD downloads.

## Features

- **Instant**: Shows all listening ports in under 50ms
- **Beautiful**: Colored, aligned, scannable table
- **Cross-platform**: Linux, macOS, Windows, FreeBSD
- **Port ranges**: `onport 3000-3002` or `onport 80 3000-3002`
- **Docker-aware**: Automatically labels ports with container names
- **Watch mode**: Live-updating display with new/gone highlighting
- **Kill mode**: Kill a process by port with a single command
- **Single-port detail view**: Full command line, start time, open FDs, process tree
- **Service names**: Well-known ports (ssh, http, https, postgres, redis, etc.) shown in SERVICE column and in JSON output
- **REMOTE column**: Remote address visible in both table and watch mode
- **Filtering**: Filter by process name, username, PID, or IP version (IPv4/IPv6)
- **Sorting**: Sort output by port, PID, name, user, state, or protocol
- **Wide mode**: Show full untruncated process names
- **Watch interval**: Control watch refresh rate with `--interval`
- **Shell completions**: Generate completions for bash, zsh, fish, and PowerShell
- **Graceful degradation**: Missing info shows `?`, never crashes

## Installation

### Pre-built binaries (fastest)

```bash
# Linux / macOS — one-liner
curl -sSfL https://gitlab.cherkaoui.ch/HadiCherkaoui/onport/-/raw/main/install.sh | bash

# Windows — PowerShell one-liner
irm https://gitlab.cherkaoui.ch/HadiCherkaoui/onport/-/raw/main/install.ps1 | iex
```

Or download a binary directly from the
[latest release](https://gitlab.cherkaoui.ch/HadiCherkaoui/onport/-/releases).

### cargo (crates.io)

```bash
cargo install onport
```

### From source

```bash
# clone and build
git clone https://gitlab.cherkaoui.ch/HadiCherkaoui/onport
cd onport
cargo build --release
```

## Platform support

| Platform | Tested | CI build |
|----------|--------|----------|
| Linux x86_64 | ✅ Tested | musl static binary |
| Linux ARM64 | ✅ Tested | GNU binary |
| Windows x86_64 | ✅ Tested | mingw cross-compile |
| macOS x86_64 | ⚠️ Untested | cross-compiled via cargo-zigbuild |
| macOS ARM64 | ⚠️ Untested | cross-compiled via cargo-zigbuild |
| FreeBSD x86_64 | ⚠️ Build from source (`cargo build --release`) | zig sysroot missing libexecinfo |

## Usage

```bash
# Show all listening ports
onport

# Filter to specific ports
onport 3000
onport 3000 8080
onport :3000 :8080

# Port ranges
onport 3000-3002
onport 80 3000-3002

# Show all states (not just LISTEN)
onport --all

# TCP or UDP only
onport --tcp
onport --udp

# JSON output for scripting
onport --json

# Disable colors
onport --no-color

# Skip Docker enrichment (faster when Docker socket is slow)
onport --no-docker

# Kill the process on a port
onport --kill 3000
onport -k 3000

# Force kill (no confirmation prompt)
onport --kill --force 3000
onport -k -f 3000

# Send a specific signal when killing
onport -k --signal HUP 8080
onport -k --signal 9 3000

# Live-updating watch mode (press q to quit)
onport --watch
onport -w
onport -w 3000

# Watch mode with custom refresh interval (seconds, minimum 0.5)
onport -w --interval 1.0
onport -w -i 5

# Filter by process name (case-insensitive substring)
onport --name nginx
onport -n node

# Filter by username (case-insensitive substring)
onport --user root
onport -u SYSTEM

# Filter by PID
onport --pid 1234

# Show only IPv4 or IPv6 sockets
onport --ipv4
onport -4
onport --ipv6
onport -6

# Sort results
onport --sort name         # sort by process name
onport --sort pid          # sort by PID
onport --sort user         # sort by username
onport --sort state        # sort by socket state
onport --sort proto        # sort by protocol
onport --sort port         # sort by port number (default)

# Show full process names without truncation
onport --wide
onport -W

# Generate shell completions
onport --completions bash > ~/.bash_completion.d/onport
onport --completions zsh > ~/.zsh/completions/_onport
onport --completions fish > ~/.config/fish/completions/onport.fish
onport --completions powershell > $PROFILE.CurrentUserAllHosts
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--tcp` | | Show only TCP sockets |
| `--udp` | | Show only UDP sockets |
| `--all` | | Show all socket states, not just LISTEN |
| `--json` | | Output as JSON for scripting |
| `--no-color` | | Disable colored output |
| `--no-docker` | | Disable Docker container name detection |
| `--kill` | `-k` | Kill the process on the specified port |
| `--force` | `-f` | Force kill without confirmation (with --kill) |
| `--signal <SIG>` | | Signal to send: name (HUP, TERM, KILL) or number (9). Only with --kill |
| `--watch` | `-w` | Live-updating watch mode (press q to quit) |
| `--interval <SECS>` | `-i` | Refresh interval for watch mode in seconds (default: 2.0, minimum: 0.5) |
| `--name <NAME>` | `-n` | Filter by process name (case-insensitive substring) |
| `--user <USER>` | `-u` | Filter by username (case-insensitive substring) |
| `--pid <PID>` | | Filter by PID |
| `--ipv4` | `-4` | Show only IPv4 sockets |
| `--ipv6` | `-6` | Show only IPv6 sockets |
| `--sort <FIELD>` | | Sort by: `port` (default), `pid`, `name`, `user`, `state`, `proto` |
| `--wide` | `-W` | Disable process name truncation (show full names) |
| `--completions <SHELL>` | | Generate shell completions: `bash`, `zsh`, `fish`, `powershell`, `elvish` |

## Example Output

Standard listing:

```
 PORT   SERVICE    PROTO  ADDRESS          PROCESS          PID    USER       STATE        REMOTE
   22   ssh        tcp    *                sshd             1204   root       LISTEN       —
   80   http       tcp    *                nginx           14201   www        LISTEN       —
 3000   —          tcp    *                node            14523   hadi       LISTEN       —
 5432   postgres   tcp    *                postgres         9102   postgres   LISTEN       —       [docker: my-postgres]
 8080   http-alt   tcp    127.0.0.1        traefik          8832   root       LISTEN       —       [docker: traefik]
```

Single-port detail view (`onport 5432`):

```
 PORT   SERVICE    PROTO  ADDRESS          PROCESS          PID    USER       STATE        REMOTE
 5432   postgres   tcp    *                postgres         9102   postgres   LISTEN       —       [docker: my-postgres]

  Command:    postgres -D /var/lib/postgresql/data
  Started:    3h 22m ago
  Open FDs:   47
  Tree:       systemd → docker-containerd-shim → postgres

  Kill this process? [y/N]
```

Filtered by name (`onport --name nginx --sort port`):

```
 PORT   SERVICE    PROTO  ADDRESS   PROCESS   PID    USER   STATE
   80   http       tcp    *         nginx      8801   www    LISTEN
  443   https      tcp    *         nginx      8801   www    LISTEN
```

## Well-Known Service Names

The SERVICE column shows standard names for common ports:

| Port | Service | Port | Service |
|------|---------|------|---------|
| 22 | ssh | 3306 | mysql |
| 25 | smtp | 3389 | rdp |
| 53 | dns | 5432 | postgres |
| 80 | http | 5672 | amqp |
| 443 | https | 6379 | redis |
| 1433 | mssql | 8080 | http-alt |
| 2049 | nfs | 9200 | elastic |
| 27017 | mongodb | 6443 | kube-api |

Ports without a standard name show `—`.

## Comparison

| Task | onport | lsof | ss | netstat |
|------|--------|------|----|---------|
| All listening ports | `onport` | `lsof -iTCP -sTCP:LISTEN -nP` | `ss -tlnp` | `netstat -tlnp` |
| What's on port 3000 | `onport 3000` | `lsof -i :3000` | `ss -tlnp sport = :3000` | `netstat -tlnp \| grep 3000` |
| Kill process on port | `onport -k 3000` | N/A | N/A | N/A |
| JSON output | `onport --json` | N/A | `ss -tlnp -H -O` | N/A |
| All connections | `onport --all` | `lsof -i` | `ss -tanp` | `netstat -tanp` |
| Live watch | `onport -w` | N/A | N/A | N/A |
| Filter by name | `onport -n nginx` | N/A | N/A | N/A |
| Filter by user | `onport -u root` | N/A | N/A | N/A |
| Sort by field | `onport --sort name` | N/A | N/A | N/A |

## License

GPL-3.0-or-later — Copyright (C) 2026 Hadi Cherkaoui
