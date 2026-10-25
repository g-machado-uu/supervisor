#!/usr/bin/env bash
# Install Supervisor for the current user on Linux (no root needed).
#
#   ./install.sh                      build, install, keep the current data file
#   ./install.sh --path=~/Documents/  also use ~/Documents/data.svt as the data file
#   ./install.sh --path=~/sync/x.svt  ...or a specific file name
#   ./install.sh --uninstall          remove the binary, launcher and icon (keeps data)
#
# Installs:
#   ~/.local/bin/supervisor
#   ~/.local/share/applications/supervisor.desktop   (Rofi drun, app menus)
#   ~/.local/share/icons/hicolor/*/apps/supervisor.{svg,png}

set -euo pipefail

APP=supervisor
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
APPS_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor"
# Must match storage.rs (directories::ProjectDirs "Supervisor" on Linux).
DEFAULT_DATA="$DATA_HOME/$APP/data.svt"
LAST_FILE="$CONFIG_HOME/$APP/last_file.txt"
KEY_FILE="$CONFIG_HOME/$APP/key.bin"

data_path=""
uninstall=0

usage() {
    sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --path=*) data_path="${1#--path=}" ;;
        --path)
            [[ $# -ge 2 ]] || { echo "error: --path needs a value" >&2; exit 1; }
            data_path="$2"; shift ;;
        --uninstall) uninstall=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown option '$1'" >&2; usage >&2; exit 1 ;;
    esac
    shift
done

refresh_caches() {
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$ICON_DIR" 2>/dev/null || true
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS_DIR" 2>/dev/null || true
}

if [[ $uninstall -eq 1 ]]; then
    rm -f "$BIN_DIR/$APP" "$APPS_DIR/$APP.desktop"
    rm -f "$ICON_DIR"/scalable/apps/$APP.svg "$ICON_DIR"/*/apps/$APP.png
    refresh_caches
    echo "Removed Supervisor. Your data file, key file (if any) and $LAST_FILE were left in place."
    exit 0
fi

# --- Resolve the data file ------------------------------------------------
if [[ -n "$data_path" ]]; then
    # The shell doesn't expand ~ after "--path=", so do it here.
    case "$data_path" in
        "~") data_path="$HOME" ;;
        "~/"*) data_path="$HOME/${data_path#\~/}" ;;
    esac
    # A directory (existing, or written with a trailing slash) gets data.svt inside it.
    if [[ "$data_path" == */ || -d "$data_path" ]]; then
        data_path="${data_path%/}/data.svt"
    fi
    data_path="$(realpath -m -- "$data_path")"
    mkdir -p -- "$(dirname -- "$data_path")"
elif [[ -s "$LAST_FILE" ]]; then
    data_path="$(head -n1 "$LAST_FILE")"
else
    data_path="$DEFAULT_DATA"
fi

# --- Build -----------------------------------------------------------------
binary="$SRC_DIR/target/release/$APP"
if command -v cargo >/dev/null; then
    echo "==> Building release binary"
    cargo build --release --manifest-path "$SRC_DIR/Cargo.toml"
elif [[ ! -x "$binary" ]]; then
    echo "error: cargo not found and no prebuilt $binary" >&2
    exit 1
fi

# --- Install ---------------------------------------------------------------
echo "==> Installing"
install -Dm755 "$binary" "$BIN_DIR/$APP"

install -Dm644 "$SRC_DIR/packaging/$APP.svg" "$ICON_DIR/scalable/apps/$APP.svg"
# PNGs too, for launchers/themes that don't render SVG.
renderer=""
if command -v magick >/dev/null; then renderer=magick
elif command -v convert >/dev/null; then renderer=convert
fi
if [[ -n "$renderer" ]]; then
    for size in 48 64 128 256; do
        mkdir -p "$ICON_DIR/${size}x${size}/apps"
        "$renderer" -background none "$SRC_DIR/packaging/$APP.svg" -resize "${size}x${size}" \
            "$ICON_DIR/${size}x${size}/apps/$APP.png" 2>/dev/null || true
    done
elif command -v rsvg-convert >/dev/null; then
    for size in 48 64 128 256; do
        mkdir -p "$ICON_DIR/${size}x${size}/apps"
        rsvg-convert -w "$size" -h "$size" "$SRC_DIR/packaging/$APP.svg" \
            -o "$ICON_DIR/${size}x${size}/apps/$APP.png" || true
    done
fi

# Absolute Exec path: Hyprland's exec environment may not include ~/.local/bin.
mkdir -p "$APPS_DIR"
sed -e "s|^Exec=.*|Exec=$BIN_DIR/$APP|" -e "s|^Icon=.*|Icon=$APP|" \
    "$SRC_DIR/packaging/$APP.desktop" > "$APPS_DIR/$APP.desktop"
chmod 644 "$APPS_DIR/$APP.desktop"

# The app opens the last-used file, so pointing that at the chosen path makes
# both the launcher and a bare `supervisor` in a terminal use it.
# Owner-only: this folder also holds the key file, if you add one.
mkdir -p -m 700 "$(dirname -- "$LAST_FILE")"
printf '%s' "$data_path" > "$LAST_FILE"

refresh_caches

# --- Report ----------------------------------------------------------------
cat <<EOF

Supervisor installed.

  Binary:    $BIN_DIR/$APP
  Launcher:  $APPS_DIR/$APP.desktop
  Icon:      $ICON_DIR/scalable/apps/$APP.svg
  Data file: $data_path
EOF
if [[ -e "$data_path" ]]; then
    echo "             (existing file – unlock it with your password)"
else
    echo "             (doesn't exist yet – set a password on first launch to create it)"
fi
echo "             Backup: $data_path.bak (kept after each save)"
if [[ -e "$KEY_FILE" ]]; then
    echo "  Key file:  $KEY_FILE"
fi

case ":$PATH:" in
    *":$BIN_DIR:"*)
        echo
        echo "$BIN_DIR is already on your PATH – run 'supervisor' from a terminal." ;;
    *)
        cat <<EOF

$BIN_DIR is not on your PATH. Add it for your shell, then open a new terminal:

  bash:  echo 'export PATH="\$HOME/.local/bin:\$PATH"' >> ~/.bashrc
  zsh:   echo 'export PATH="\$HOME/.local/bin:\$PATH"' >> ~/.zshrc
  fish:  fish_add_path ~/.local/bin
EOF
        ;;
esac

cat <<EOF

Launching:
  Rofi:      rofi -show drun -show-icons   (search "Supervisor")
  Hyprland:  bind = \$mainMod, S, exec, $BIN_DIR/$APP
  Window class (for Hyprland windowrules): supervisor
EOF
