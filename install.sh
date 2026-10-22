#!/bin/sh
set -eu

S3="${MYCELIUM_S3:-https://github.com/mycelium-app/mycelium/releases/download}"
CONTROL_PLANE="${1:-${MYCELIUM_CONTROL_PLANE:-}}"

fail() {
  echo "mycelium install: $*" >&2
  exit 1
}

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) fail "unsupported system $(uname -s). Mycelium nodes run on Linux and macOS." ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) fail "unsupported architecture $(uname -m). Use x86_64 or aarch64." ;;
esac

if [ "$(id -u)" -eq 0 ]; then
  scope=system
  bin_dir=/opt/mycelium
  service_user="${SUDO_USER:-root}"
  node_config=/var/lib/mycelium-node/node.toml
else
  scope=user
  bin_dir="$HOME/.local/bin"
  service_user="$(id -un)"
  node_config="${MYCELIUM_NODE_CONFIG:-$HOME/.config/mycelium/node.toml}"
fi
binary="$bin_dir/mycelium"
plist="$HOME/Library/LaunchAgents/app.mycelium.node.plist"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

install_binary() {
  mkdir -p "$bin_dir"
  if [ -n "${MYCELIUM_BINARY:-}" ]; then
    [ -f "$MYCELIUM_BINARY" ] || fail "MYCELIUM_BINARY points at $MYCELIUM_BINARY, which is not a file."
    source_binary="$MYCELIUM_BINARY"
  else
    command -v curl >/dev/null 2>&1 || fail "curl is required. Install curl and run this again."
    command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || fail "sha256sum or shasum is required."
    tag="$(curl -fsSL "$S3/latest")" || fail "could not read $S3/latest. Check your network connection."
    asset="mycelium-$os-$arch"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    curl -fsSL -o "$tmp/$asset" "$S3/$tag/$asset" || fail "could not download $S3/$tag/$asset. There may be no build for $os $arch."
    curl -fsSL -o "$tmp/$asset.sha256" "$S3/$tag/$asset.sha256" || fail "could not download the checksum for $asset."
    expected="$(cut -d' ' -f1 "$tmp/$asset.sha256")"
    [ "$expected" = "$(sha256_of "$tmp/$asset")" ] || fail "checksum mismatch for $asset. Nothing was installed."
    source_binary="$tmp/$asset"
  fi
  cp "$source_binary" "$binary.new"
  chmod 0755 "$binary.new"
  mv -f "$binary.new" "$binary"
  if [ "$scope" = system ]; then
    ln -sf "$binary" /usr/local/bin/mycelium
  fi
}

write_service() {
  if [ "$os" = darwin ]; then
    [ "$scope" = user ] || fail "on macOS run this without sudo."
    mkdir -p "$(dirname "$plist")"
    cat >"$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>app.mycelium.node</string>
  <key>ProgramArguments</key><array><string>$binary</string><string>node</string><string>run</string></array>
  <key>EnvironmentVariables</key><dict><key>PATH</key><string>$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
PLIST
  elif [ "$scope" = system ]; then
    mkdir -p /var/lib/mycelium-node
    chown "$service_user" /var/lib/mycelium-node
    cat >/etc/systemd/system/mycelium-node.service <<UNIT
[Unit]
Description=Mycelium node
After=network-online.target
Wants=network-online.target

[Service]
User=$service_user
Environment=MYCELIUM_NODE_CONFIG=$node_config
ExecStart=$binary node run
Restart=always
RestartSec=3
KillMode=process

[Install]
WantedBy=multi-user.target
UNIT
  else
    mkdir -p "$HOME/.config/systemd/user"
    cat >"$HOME/.config/systemd/user/mycelium-node.service" <<UNIT
[Unit]
Description=Mycelium node
After=network-online.target

[Service]
Environment=MYCELIUM_NODE_CONFIG=$node_config
ExecStart=$binary node run
Restart=always
RestartSec=3
KillMode=process

[Install]
WantedBy=default.target
UNIT
  fi
}

join_if_needed() {
  if [ -f "$node_config" ]; then
    return 0
  fi
  if [ -z "$CONTROL_PLANE" ]; then
    echo "Installed. Join a control plane with: MYCELIUM_NODE_CONFIG=$node_config $binary node join <control plane url>"
    return 1
  fi
  if [ "$scope" = system ] && [ "$service_user" != root ]; then
    su "$service_user" -s /bin/sh -c "MYCELIUM_NODE_CONFIG='$node_config' '$binary' node join '$CONTROL_PLANE'"
  else
    MYCELIUM_NODE_CONFIG="$node_config" "$binary" node join "$CONTROL_PLANE"
  fi
}

start_service() {
  if [ "$os" = darwin ]; then
    launchctl unload "$plist" 2>/dev/null || true
    launchctl load "$plist"
  elif [ "$scope" = system ]; then
    systemctl daemon-reload
    systemctl enable mycelium-node.service
    systemctl restart mycelium-node.service
  else
    systemctl --user daemon-reload
    systemctl --user enable mycelium-node.service
    systemctl --user restart mycelium-node.service
  fi
}

install_binary
write_service
if join_if_needed
then
  start_service
  echo "Mycelium node is running."
fi
