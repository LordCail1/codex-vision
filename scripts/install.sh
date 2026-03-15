#!/usr/bin/env bash
set -euo pipefail

repo="LordCail1/codex-vision"
bin_dir="${BIN_DIR:-$HOME/.local/bin}"

os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64)
    asset="codex-vision-x86_64-unknown-linux-gnu.tar.gz"
    ;;
  Darwin:arm64)
    asset="codex-vision-aarch64-apple-darwin.tar.gz"
    ;;
  Darwin:x86_64)
    asset="codex-vision-x86_64-apple-darwin.tar.gz"
    ;;
  *)
    echo "Unsupported platform: ${os} ${arch}" >&2
    echo "Download a release manually from https://github.com/${repo}/releases" >&2
    exit 1
    ;;
esac

url="https://github.com/${repo}/releases/latest/download/${asset}"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

echo "Downloading ${asset}..."
curl -fsSL "${url}" -o "${tmp_dir}/${asset}"
tar -xzf "${tmp_dir}/${asset}" -C "${tmp_dir}"

mkdir -p "${bin_dir}"
install "${tmp_dir}/codex-vision" "${bin_dir}/codex-vision"

echo "Installed codex-vision to ${bin_dir}/codex-vision"
echo "Add ${bin_dir} to PATH if needed, then run: codex-vision doctor"
