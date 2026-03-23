#!/usr/bin/env bash
set -euo pipefail

# install.sh — onport installer for Linux and macOS
# Usage: curl -sSfL https://gitlab.cherkaoui.ch/HadiCherkaoui/onport/-/raw/main/install.sh | bash
# Or:    INSTALL_DIR=/usr/local/bin bash install.sh

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

REPO_API="https://gitlab.cherkaoui.ch/api/v4/projects/HadiCherkaoui%2Fonport"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()  { printf '\033[1;34m[onport]\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m[onport]\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m[onport]\033[0m %s\n' "$*" >&2; }
die()   { printf '\033[1;31m[onport] error:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux"  ;;
        Darwin*) echo "macos"  ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            die "Windows detected — please use install.ps1 instead." ;;
        *)
            die "Unsupported OS: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)       echo "x86_64"  ;;
        aarch64|arm64)      echo "aarch64" ;;
        *)
            die "Unsupported architecture: $(uname -m)" ;;
    esac
}

OS="$(detect_os)"
ARCH="$(detect_arch)"

# Map OS+ARCH to the binary name produced by the CI pipeline.
# Darwin aarch64 is also known as arm64; normalise to aarch64 here.
BINARY="onport-${OS}-${ARCH}"

info "Detected platform: ${OS}/${ARCH}  →  binary: ${BINARY}"

# ---------------------------------------------------------------------------
# Fetch the latest release tag from the GitLab Releases API
# ---------------------------------------------------------------------------

info "Fetching latest release tag from GitLab…"

RELEASES_JSON="$(curl -sSfL "${REPO_API}/releases")" \
    || die "Failed to fetch releases from ${REPO_API}/releases"

# The response is a JSON array; grab the first tag_name value.
# We deliberately avoid requiring jq or python — plain grep + sed suffices.
VERSION="$(printf '%s' "$RELEASES_JSON" \
    | grep -o '"tag_name":"[^"]*"' \
    | head -1 \
    | sed 's/"tag_name":"//;s/"//')"

[[ -n "$VERSION" ]] || die "Could not parse a release tag from the API response."

info "Latest version: ${VERSION}"

# ---------------------------------------------------------------------------
# Download the binary
# ---------------------------------------------------------------------------

DOWNLOAD_URL="${REPO_API}/packages/generic/onport/${VERSION}/${BINARY}"
info "Downloading from: ${DOWNLOAD_URL}"

# Write to a temp file so we don't clobber the destination on a failed download.
TMP_FILE="$(mktemp)"
# Ensure the temp file is removed on exit (success or failure).
trap 'rm -f "$TMP_FILE"' EXIT

curl -sSfL "$DOWNLOAD_URL" -o "$TMP_FILE" \
    || die "Download failed. Check that '${BINARY}' exists for version '${VERSION}'."

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

# Create the install directory if it doesn't exist.
mkdir -p "$INSTALL_DIR"

DEST="${INSTALL_DIR}/onport"
mv "$TMP_FILE" "$DEST"
chmod +x "$DEST"

ok "Installed onport ${VERSION} → ${DEST}"

# ---------------------------------------------------------------------------
# PATH hint
# ---------------------------------------------------------------------------

# Check whether INSTALL_DIR is already on PATH.
case ":${PATH}:" in
    *":${INSTALL_DIR}:"*)
        # Already on PATH — nothing to do.
        ;;
    *)
        warn "'${INSTALL_DIR}' is not in your PATH."
        warn "Add the following line to your shell config (~/.bashrc, ~/.zshrc, etc.):"
        warn ""
        warn "    export PATH=\"\$HOME/.local/bin:\$PATH\""
        warn ""
        warn "Then restart your shell or run:  source ~/.bashrc"
        ;;
esac

# ---------------------------------------------------------------------------
# Verify installation
# ---------------------------------------------------------------------------

info "Verifying installation…"
"$DEST" --version 2>/dev/null && ok "onport is ready to use." || warn "Could not run '$DEST --version'. Check the binary is compatible with this system."
