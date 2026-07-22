#!/usr/bin/env sh
set -eu

repository="${INSTADOWN_REPO:-__REPOSITORY__}"
install_dir="${INSTADOWN_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
  Linux) platform="linux" ;;
  Darwin) platform="macos" ;;
  *)
    echo "[error] Unsupported operating system: $(uname -s)" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) architecture="x86_64" ;;
  arm64 | aarch64) architecture="aarch64" ;;
  *)
    echo "[error] Unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

if [ "$platform" = "linux" ] && [ "$architecture" != "x86_64" ]; then
  echo "[error] Linux releases currently support x86_64 only" >&2
  exit 1
fi

archive="instadown-${platform}-${architecture}.tar.gz"
url="https://github.com/${repository}/releases/latest/download/${archive}"
temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT INT TERM

echo "Installing instadown from ${repository}..."
curl --fail --location --silent --show-error "$url" --output "$temp_dir/$archive"
tar -xzf "$temp_dir/$archive" -C "$temp_dir"
mkdir -p "$install_dir"
install -m 755 "$temp_dir/instadown" "$install_dir/instadown"

echo "Installed instadown to $install_dir/instadown"
echo "Make sure yt-dlp and ffmpeg are installed before downloading media."

