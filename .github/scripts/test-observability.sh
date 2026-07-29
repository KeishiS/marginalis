#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT

mkdir -p \
  "$temporary_directory/good" \
  "$temporary_directory/missing-event" \
  "$temporary_directory/sensitive" \
  "$temporary_directory/header" \
  "$temporary_directory/aliased-sensitive" \
  "$temporary_directory/format-sensitive" \
  "$temporary_directory/unqualified" \
  "$temporary_directory/span-sensitive" \
  "$temporary_directory/span-uri" \
  "$temporary_directory/dynamic-event"
cat >"$temporary_directory/good/log.rs" <<'EOF'
fn log() {
    tracing::info!(
        event = "test.completed",
        reason = "fixture",
        "completed"
    );
}
EOF
cat >"$temporary_directory/missing-event/log.rs" <<'EOF'
fn log() {
    tracing::warn!("missing event");
}
EOF
cat >"$temporary_directory/sensitive/log.rs" <<'EOF'
fn log() {
    tracing::error!(
        event = "test.failed",
        token = "must-not-be-logged",
        "failed"
    );
}
EOF
cat >"$temporary_directory/header/log.rs" <<'EOF'
fn log() {
    tracing::warn!(
        event = "test.rejected",
        received_origin = "https://private.example.test",
        "rejected"
    );
}
EOF
cat >"$temporary_directory/aliased-sensitive/log.rs" <<'EOF'
fn log() {
    tracing::error!(
        event = "test.failed",
        access_token = "must-not-be-logged",
        session_cookie = "must-not-be-logged",
        request_body = "must-not-be-logged",
        "failed"
    );
}
EOF
cat >"$temporary_directory/format-sensitive/log.rs" <<'EOF'
fn log(access_token: &str) {
    tracing::error!(
        event = "test.failed",
        "failed with {}",
        access_token
    );
}
EOF
cat >"$temporary_directory/unqualified/log.rs" <<'EOF'
use tracing::warn;

fn log() {
    warn!(
        event = "test.rejected",
        "rejected"
    );
}
EOF
cat >"$temporary_directory/span-sensitive/log.rs" <<'EOF'
fn span(access_token: &str) {
    let _span = tracing::info_span!(
        "request",
        access_token
    );
}
EOF
cat >"$temporary_directory/span-uri/log.rs" <<'EOF'
fn span(request: &Request) {
    let _span = tracing::info_span!(
        "request",
        path = request.uri().path()
    );
}
EOF
cat >"$temporary_directory/dynamic-event/log.rs" <<'EOF'
fn log(event_name: &str) {
    tracing::info!(
        event = event_name,
        "completed"
    );
}
EOF

bash "$script_dir/check-observability.sh" "$temporary_directory/good"
if bash "$script_dir/check-observability.sh" "$temporary_directory/missing-event" >/dev/null 2>&1; then
  echo "eventのないログを受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/sensitive" >/dev/null 2>&1; then
  echo "記録禁止fieldを持つログを受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/header" >/dev/null 2>&1; then
  echo "HTTP header由来fieldを持つログを受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/aliased-sensitive" >/dev/null 2>&1; then
  echo "別名の記録禁止fieldを持つログを受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/format-sensitive" >/dev/null 2>&1; then
  echo "ログ本文のformat引数にある記録禁止値を受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/unqualified" >/dev/null 2>&1; then
  echo "修飾されていないtracing macroを受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/span-sensitive" >/dev/null 2>&1; then
  echo "spanの記録禁止fieldを受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/span-uri" >/dev/null 2>&1; then
  echo "spanの未正規化request URIを受理しました。" >&2
  exit 1
fi
if bash "$script_dir/check-observability.sh" "$temporary_directory/dynamic-event" >/dev/null 2>&1; then
  echo "動的なevent名を受理しました。" >&2
  exit 1
fi
