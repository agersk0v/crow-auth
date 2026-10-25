#!/usr/bin/env bash
# Checks database-backed service startup and HTTP health, then cleans up the process.
set -euo pipefail

: "${DATABASE_URL:?Set DATABASE_URL to a disposable PostgreSQL database}"
# Disposable smoke configuration only; never use these public fixture keys in deployment.
export CROW_ORIGIN=https://crow.localhost:3000
export BROWSER_SIGNING_KEY=BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc
export MAIL_ENCRYPTION_KEY=CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg
export MAIL_FROM=crow@example.test
export SMTP_HOST=127.0.0.1
export SMTP_PORT=1025
export MAIL_LOCAL_CAPTURE=true
unset SMTP_USERNAME SMTP_PASSWORD
export PASSWORD_BLOCKLIST_PATH=tests/fixtures/compromised-passwords.txt
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
    curl --fail --silent --max-time 2 http://127.0.0.1:3000/register >"$smoke_dir/register"
    grep -q 'name="csrf"' "$smoke_dir/register"
    grep -q 'action="/api/hosted/register"' "$smoke_dir/register"
    curl --fail --silent --max-time 2 http://127.0.0.1:3000/verify-email >"$smoke_dir/resend"
    grep -q 'action="/api/hosted/resend-verification"' "$smoke_dir/resend"
    echo 'PostgreSQL startup, HTTP health and hosted-form smoke checks passed.'
    exit 0
  fi
  sleep 1
done

echo 'Service did not become healthy within 30 attempts.' >&2
exit 1
