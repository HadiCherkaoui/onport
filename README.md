# onport

> See what's listening on your ports.

`onport` is a cross-platform CLI tool that instantly shows what processes are listening on which ports. One command replaces `lsof -i`, `ss -tlnp`, `netstat -an`, and `Get-NetTCPConnection`.

## Features

- **Instant**: Shows all listening ports in under 50ms
- **Beautiful**: Colored, aligned, scannable table
- **Cross-platform**: Linux, macOS, Windows, FreeBSD
- **Port ranges**: `onport 3000-3002` or `onport 80 3000-3002`
- **Docker-aware**: Automatically labels ports with container names
- **Watch mode**: Live-updating display with new/gone highlighting
- **Kill mode**: Kill a process by port with a single command
- **Single-port detail view**: Full command line, start time, open FDs, process tree
- **Graceful degradation**: Missing info shows `?`, never crashes

## Installation

```bash
# From source
cargo install onport

# Or clone and build
git clone https://github.com/hadihallak/onport
cd onport
cargo build --release
```

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

# Live-updating watch mode (press q to quit)
onport --watch
onport -w
onport -w 3000
```

## Example Output

Standard listing:

```
 PORT   PROTO  ADDRESS          PROCESS          PID    USER       STATE
   22   tcp    *                sshd             1204   root       LISTEN
   80   tcp    *                nginx           14201   www        LISTEN
 3000   tcp    *                node            14523   hadi       LISTEN
 5432   tcp    *                postgres         9102   postgres   LISTEN  [docker: my-postgres]
 8080   tcp    127.0.0.1        traefik          8832   root       LISTEN  [docker: traefik]
```

Single-port detail view (`onport 5432`):

```
 PORT   PROTO  ADDRESS          PROCESS          PID    USER       STATE
 5432   tcp    *                postgres         9102   postgres   LISTEN  [docker: my-postgres]

  Command:    postgres -D /var/lib/postgresql/data
  Started:    3h 22m ago
  Open FDs:   47
  Tree:       systemd → docker-containerd-shim → postgres

  Kill this process? [y/N]
```

## Comparison

| Task | onport | lsof | ss | netstat |
|------|--------|------|----|---------|
| All listening ports | `onport` | `lsof -iTCP -sTCP:LISTEN -nP` | `ss -tlnp` | `netstat -tlnp` |
| What's on port 3000 | `onport 3000` | `lsof -i :3000` | `ss -tlnp sport = :3000` | `netstat -tlnp \| grep 3000` |
| Kill process on port | `onport -k 3000` | N/A | N/A | N/A |
| JSON output | `onport --json` | N/A | `ss -tlnp -H -O` | N/A |
| All connections | `onport --all` | `lsof -i` | `ss -tanp` | `netstat -tanp` |
| Live watch | `onport -w` | N/A | N/A | N/A |

## License

GPL-3.0-or-later — Copyright (C) 2026 Hadi Cherkaoui
