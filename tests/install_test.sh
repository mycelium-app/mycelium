#!/bin/sh
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
server_pid=""
trap 'if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; fi; rm -rf "$work"' EXIT

failures=0

check() {
  name="$1"
  shift
  if "$@"; then
    echo "ok   $name"
  else
    echo "FAIL $name"
    failures=$((failures + 1))
  fi
}

mkdir -p "$work/bin" "$work/home"
cat >"$work/bin/systemctl" <<'EOF'
#!/bin/sh
echo "$@" >>"$HOME/systemctl.log"
EOF
chmod +x "$work/bin/systemctl"

cat >"$work/fake-mycelium" <<'EOF'
#!/bin/sh
if [ "$1 $2" = "node join" ]; then
  mkdir -p "$(dirname "$MYCELIUM_NODE_CONFIG")"
  echo "control_plane = \"$3\"" >"$MYCELIUM_NODE_CONFIG"
fi
EOF
chmod +x "$work/fake-mycelium"

run_install() {
  HOME="$work/home" PATH="$work/bin:$PATH" sh "$root/install.sh" "$@"
}

reset_home() {
  rm -rf "${work:?}/home"
  mkdir -p "$work/home"
}

local_binary_joins_and_starts_service() {
  reset_home
  MYCELIUM_BINARY="$work/fake-mycelium" run_install https://control.example >/dev/null
  test -x "$work/home/.local/bin/mycelium" &&
    grep -q "ExecStart=$work/home/.local/bin/mycelium node run" "$work/home/.config/systemd/user/mycelium-node.service" &&
    grep -q 'https://control.example' "$work/home/.config/mycelium/node.toml" &&
    grep -q -- '--user restart mycelium-node.service' "$work/home/systemctl.log"
}

without_control_plane_installs_but_does_not_start() {
  reset_home
  output="$(MYCELIUM_BINARY="$work/fake-mycelium" run_install)"
  test -x "$work/home/.local/bin/mycelium" &&
    echo "$output" | grep -q 'node join' &&
    ! test -e "$work/home/systemctl.log"
}

existing_credential_skips_join() {
  reset_home
  mkdir -p "$work/home/.config/mycelium"
  echo 'token = "kept"' >"$work/home/.config/mycelium/node.toml"
  MYCELIUM_BINARY="$work/fake-mycelium" run_install https://control.example >/dev/null
  grep -q 'token = "kept"' "$work/home/.config/mycelium/node.toml"
}

missing_local_binary_is_reported() {
  reset_home
  if MYCELIUM_BINARY="$work/nope" run_install https://control.example 2>"$work/err"; then
    return 1
  fi
  grep -q 'is not a file' "$work/err"
}

serve_release() {
  tag="$1"
  asset="mycelium-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"
  mkdir -p "$work/s3/$tag"
  printf '%s' "$tag" >"$work/s3/latest"
  cp "$work/fake-mycelium" "$work/s3/$tag/$asset"
  sha256sum "$work/s3/$tag/$asset" | cut -d' ' -f1 >"$work/s3/$tag/$asset.sha256"
  if [ "${2:-}" = corrupt ]; then
    echo 'tampered' >>"$work/s3/$tag/$asset"
  fi
  port=18931
  (cd "$work/s3" && python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1) &
  server_pid=$!
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    curl -fs "http://127.0.0.1:$port/latest" >/dev/null 2>&1 && return 0
    python3 -c 'import time; time.sleep(0.1)'
  done
  return 1
}

stop_release() {
  kill "$server_pid" 2>/dev/null || true
  server_pid=""
  rm -rf "$work/s3"
}

download_verifies_and_installs_latest_release() {
  reset_home
  serve_release v9.9.9
  MYCELIUM_S3="http://127.0.0.1:18931" run_install https://control.example >/dev/null
  status=$?
  stop_release
  test "$status" -eq 0 && test -x "$work/home/.local/bin/mycelium"
}

checksum_mismatch_installs_nothing() {
  reset_home
  serve_release v9.9.9 corrupt
  if MYCELIUM_S3="http://127.0.0.1:18931" run_install https://control.example 2>"$work/err"; then
    stop_release
    return 1
  fi
  stop_release
  grep -q 'checksum mismatch' "$work/err" && ! test -e "$work/home/.local/bin/mycelium"
}

check "a local binary is installed, joined and started as a user service" local_binary_joins_and_starts_service
check "without a control plane it installs and says how to join" without_control_plane_installs_but_does_not_start
check "an existing credential is kept and no join runs" existing_credential_skips_join
check "a missing local binary is reported" missing_local_binary_is_reported
check "the latest release is downloaded, verified and installed" download_verifies_and_installs_latest_release
check "a checksum mismatch installs nothing" checksum_mismatch_installs_nothing

exit "$failures"
