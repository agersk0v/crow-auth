#!/usr/bin/env bash
set -euo pipefail

: "${DATABASE_URL:?Set DATABASE_URL to a disposable PostgreSQL database}"
cargo build --locked

smoke_dir=$(mktemp -d)
service_pid=''
cleanup() {
  if [[ -n "$service_pid" ]]; then
    kill "$service_pid" 2>/dev/null || true
    wait "$service_pid" 2>/dev/null || true
  fi
  rm -rf "$smoke_dir"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

./target/debug/crow-auth >"$smoke_dir/service.log" 2>&1 &
service_pid=$!
for ((attempt = 0; attempt < 30; attempt++)); do
  if ! kill -0 "$service_pid" 2>/dev/null; then
    echo 'Service exited before becoming healthy.' >&2
    exit 1
  fi
  if curl --fail --silent --max-time 1 http://127.0.0.1:3000/health >"$smoke_dir/health"; then
    if ! printf 'ok' | cmp -s - "$smoke_dir/health"; then
      echo 'Unexpected health response.' >&2
      exit 1
    fi
    # Reject a response from another process if our service failed to bind.
    kill -0 "$service_pid"
    echo 'PostgreSQL startup and HTTP health smoke test passed.'
    exit 0
  fi
  sleep 1
done

echo 'Service did not become healthy within 30 attempts.' >&2
exit 1
