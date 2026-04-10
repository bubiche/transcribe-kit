#!/usr/bin/env bash
#
# Download the llama-server binary for the current (or specified) platform.
# Places it in src-tauri/binaries/ with the Tauri sidecar target-triple suffix.
#
# Usage:
#   ./scripts/download-llama-server.sh            # auto-detect platform
#   ./scripts/download-llama-server.sh <target>    # explicit target triple
#
# Supported targets:
#   aarch64-apple-darwin
#   x86_64-apple-darwin
#   x86_64-unknown-linux-gnu
#   x86_64-pc-windows-msvc

set -euo pipefail

# Pin to a specific llama.cpp release for reproducible builds.
LLAMA_CPP_VERSION="b8724"

REPO="ggml-org/llama.cpp"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARIES_DIR="${SCRIPT_DIR}/../src-tauri/binaries"

detect_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Darwin)
            case "$arch" in
                arm64) echo "aarch64-apple-darwin" ;;
                x86_64) echo "x86_64-apple-darwin" ;;
                *) echo "Unsupported macOS architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        Linux)
            case "$arch" in
                x86_64) echo "x86_64-unknown-linux-gnu" ;;
                *) echo "Unsupported Linux architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*)
            echo "x86_64-pc-windows-msvc"
            ;;
        *)
            echo "Unsupported OS: $os" >&2; exit 1
            ;;
    esac
}

# Map target triple to release archive name
archive_name_for_target() {
    local target="$1"
    case "$target" in
        aarch64-apple-darwin)
            echo "llama-${LLAMA_CPP_VERSION}-bin-macos-arm64.tar.gz" ;;
        x86_64-apple-darwin)
            echo "llama-${LLAMA_CPP_VERSION}-bin-macos-x64.tar.gz" ;;
        x86_64-unknown-linux-gnu)
            echo "llama-${LLAMA_CPP_VERSION}-bin-ubuntu-x64.tar.gz" ;;
        x86_64-pc-windows-msvc)
            echo "llama-${LLAMA_CPP_VERSION}-bin-win-cpu-x64.zip" ;;
        *)
            echo "No archive mapping for target: $target" >&2; exit 1 ;;
    esac
}

# The binary name inside the archive
server_binary_in_archive() {
    local target="$1"
    case "$target" in
        x86_64-pc-windows-msvc)
            echo "llama-server.exe" ;;
        *)
            echo "llama-server" ;;
    esac
}

# Output filename with Tauri sidecar suffix
output_filename() {
    local target="$1"
    case "$target" in
        x86_64-pc-windows-msvc)
            echo "llama-server-${target}.exe" ;;
        *)
            echo "llama-server-${target}" ;;
    esac
}

main() {
    local target="${1:-$(detect_target)}"
    local archive_name output_name server_bin
    archive_name="$(archive_name_for_target "$target")"
    output_name="$(output_filename "$target")"
    server_bin="$(server_binary_in_archive "$target")"

    local output_path="${BINARIES_DIR}/${output_name}"

    if [ -f "$output_path" ]; then
        echo "Already exists: ${output_path}"
        exit 0
    fi

    local download_url="https://github.com/${REPO}/releases/download/${LLAMA_CPP_VERSION}/${archive_name}"

    echo "Downloading llama-server ${LLAMA_CPP_VERSION} for ${target}..."
    echo "  URL: ${download_url}"

    mkdir -p "$BINARIES_DIR"

    TMPDIR_CLEANUP="$(mktemp -d)"
    trap 'rm -rf "$TMPDIR_CLEANUP"' EXIT
    local tmpdir="$TMPDIR_CLEANUP"

    local archive_path="${tmpdir}/${archive_name}"
    curl -fSL --progress-bar -o "$archive_path" "$download_url"

    echo "Extracting ${server_bin}..."
    case "$archive_name" in
        *.tar.gz)
            # Binary is at llama-<version>/llama-server inside the tarball
            tar -xzf "$archive_path" -C "$tmpdir"
            local extracted="${tmpdir}/llama-${LLAMA_CPP_VERSION}/${server_bin}"
            if [ ! -f "$extracted" ]; then
                echo "ERROR: Expected ${extracted} not found in archive. Contents:" >&2
                tar -tzf "$archive_path" | head -20 >&2
                exit 1
            fi
            cp "$extracted" "$output_path"
            ;;
        *.zip)
            unzip -q "$archive_path" -d "$tmpdir/extracted"
            local extracted
            extracted="$(find "$tmpdir/extracted" -name "$server_bin" -type f | head -1)"
            if [ -z "$extracted" ]; then
                echo "ERROR: ${server_bin} not found in archive." >&2
                exit 1
            fi
            cp "$extracted" "$output_path"
            ;;
    esac

    chmod +x "$output_path"
    echo "Installed: ${output_path}"
    ls -lh "$output_path"
}

main "$@"
