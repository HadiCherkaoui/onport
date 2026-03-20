# onport

> See what's listening on your ports.

`onport` is a cross-platform CLI tool that instantly shows what processes are listening on which ports. One command replaces `lsof -i`, `ss -tlnp`, `netstat -an`, and `Get-NetTCPConnection`.

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

# Show all states (not just LISTEN)
onport --all

# TCP only
onport --tcp

# JSON output for scripting
onport --json

# Disable colors
onport --no-color
```

## Example Output

```
 PORT   PROTO  PROCESS          PID    USER   STATE
   22   tcp    sshd            1204   root    LISTEN
   80   tcp    nginx          14201   www     LISTEN
 3000   tcp    node           14523   hadi    LISTEN
 5432   tcp    postgres        9102   root    LISTEN
 8080   tcp    traefik         8832   root    LISTEN
```

## Comparison

| Task | onport | lsof | ss | netstat |
|------|--------|------|----|---------|
| All listening ports | `onport` | `lsof -iTCP -sTCP:LISTEN -nP` | `ss -tlnp` | `netstat -tlnp` |
| What's on port 3000 | `onport 3000` | `lsof -i :3000` | `ss -tlnp sport = :3000` | `netstat -tlnp \| grep 3000` |
| JSON output | `onport --json` | N/A | `ss -tlnp -H -O` | N/A |
| All connections | `onport --all` | `lsof -i` | `ss -tanp` | `netstat -tanp` |

## License

GPL-3.0-or-later — Copyright (C) 2026 Hadi Cherkaoui
