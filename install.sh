#!/usr/bin/env sh
# SignalDock Runtime Installer
# Usage: curl -fsSL https://signaldock.io/install | sh
# Options (env vars):
#   SIGNALDOCK_VERSION  — override version (default: latest)
#   SIGNALDOCK_INSTALL_DIR — install directory (default: /usr/local/bin, or ~/.local/bin if no sudo)

set -e

REPO="CleoAgent/signaldock-runtime"
BINARY="signaldock"

# ── helpers ─────────────────────────────────────────────────────────────────

say()  { printf '%s\n' "$*"; }
err()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"; }

# ── detect platform ──────────────────────────────────────────────────────────

detect_platform() {
  local os arch

  case "$(uname -s)" in
    Linux)  os="linux" ;;
    Darwin) os="darwin" ;;
    MINGW*|MSYS*|CYGWIN*) os="windows" ;;
    *) err "Unsupported OS: $(uname -s)" ;;
  esac

  case "$(uname -m)" in
    x86_64|amd64) arch="x64" ;;
    arm64|aarch64) arch="arm64" ;;
    *) err "Unsupported architecture: $(uname -m)" ;;
  esac

  # Validate supported combinations
  case "${os}-${arch}" in
    linux-x64|darwin-x64|darwin-arm64|windows-x64) ;;
    *) err "No prebuilt binary for ${os}-${arch}. See https://github.com/${REPO}/releases" ;;
  esac

  printf '%s-%s' "$os" "$arch"
}

# ── resolve version ──────────────────────────────────────────────────────────

resolve_version() {
  if [ -n "${SIGNALDOCK_VERSION}" ]; then
    printf '%s' "${SIGNALDOCK_VERSION}"
    return
  fi

  need curl
  # Fetch the redirect target of the "latest" release URL to get the tag
  local tag
  tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest" \
    | sed 's|.*/tag/||')

  [ -n "$tag" ] || err "Could not determine latest version. Set SIGNALDOCK_VERSION explicitly."
  printf '%s' "${tag#v}"
}

# ── pick install directory ────────────────────────────────────────────────────

pick_install_dir() {
  if [ -n "${SIGNALDOCK_INSTALL_DIR}" ]; then
    printf '%s' "${SIGNALDOCK_INSTALL_DIR}"
    return
  fi

  if [ -w "/usr/local/bin" ] || sudo -n true 2>/dev/null; then
    printf '/usr/local/bin'
  else
    printf '%s/.local/bin' "${HOME}"
  fi
}

# ── download ─────────────────────────────────────────────────────────────────

download() {
  local url="$1" dest="$2"
  need curl
  curl -fsSL --progress-bar "$url" -o "$dest"
}

# ── main ──────────────────────────────────────────────────────────────────────

main() {
  say "SignalDock Runtime Installer"
  say "----------------------------"

  local platform version install_dir ext url dest

  platform=$(detect_platform)
  say "Platform: ${platform}"

  version=$(resolve_version)
  say "Version:  v${version}"

  install_dir=$(pick_install_dir)
  say "Install:  ${install_dir}"

  # Windows gets an .exe suffix
  case "$platform" in
    windows-*) ext=".exe" ;;
    *) ext="" ;;
  esac

  url="https://github.com/${REPO}/releases/download/v${version}/signaldock-${platform}${ext}"
  dest="${install_dir}/${BINARY}${ext}"

  # Create install dir if needed
  if [ ! -d "$install_dir" ]; then
    mkdir -p "$install_dir" || sudo mkdir -p "$install_dir"
  fi

  say ""
  say "Downloading: ${url}"

  local tmp
  tmp="$(mktemp)"
  trap 'rm -f "$tmp"' EXIT

  download "$url" "$tmp"

  # Install (may need sudo for /usr/local/bin)
  if [ -w "$install_dir" ]; then
    mv "$tmp" "$dest"
    chmod +x "$dest"
  else
    sudo mv "$tmp" "$dest"
    sudo chmod +x "$dest"
  fi

  say ""
  say "Installed: ${dest}"
  say ""

  # Verify
  if command -v "$BINARY" >/dev/null 2>&1; then
    say "Run 'signaldock --help' to get started."
  else
    say "Add ${install_dir} to your PATH to use signaldock:"
    say "  export PATH=\"\$PATH:${install_dir}\""
  fi
}

main "$@"
