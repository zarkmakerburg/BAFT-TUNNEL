#!/usr/bin/env bash
# Reproducible build of the pinned BAFT engine (ADR-0019, ADR-0020).
# Usage: scripts/build-baft-engine.sh OUT_DIR [SRC_DIR]
# Builds linux/amd64 from the exact commit in release/baft-engine.pin with the pinned Go
# toolchain, writes OUT_DIR/baft, OUT_DIR/baft.sha256 and the licence bundle in
# OUT_DIR/licenses/. No network access is needed on user hosts; this runs in CI/release.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=/dev/null
. "$here/release/baft-engine.pin"
out="${1:?usage: build-baft-engine.sh OUT_DIR [SRC_DIR]}"
src="${2:-}"
if [[ -z "$src" ]]; then
  src="$(mktemp -d)"
  git clone --quiet "$BAFT_REPO" "$src"
fi
git -C "$src" checkout --quiet "$BAFT_SHA"
[[ "$(git -C "$src" rev-parse HEAD)" == "$BAFT_SHA" ]] || { echo "source is not at the pinned commit" >&2; exit 1; }
have="$(cd "$src" && go env GOVERSION | sed 's/^go//')"
[[ "$have" == "$GO_VERSION" ]] || { echo "Go $have found, $GO_VERSION is pinned" >&2; exit 1; }
mkdir -p "$out/licenses"
(
  cd "$src"
  CGO_ENABLED=0 GOOS=linux GOARCH=amd64 GOFLAGS=-mod=readonly \
    go build -trimpath -buildvcs=false -ldflags='-s -w -buildid=' -o "$out/baft" ./cmd/baft
  cp LICENSE COPYRIGHT THIRD_PARTY_LICENSES.md "$out/licenses/"
)
printf 'BAFT source: %s\nCommit: %s\nLicense: AGPL-3.0-only (see LICENSE)\nThe complete corresponding source is at the URL and commit above.\n' \
  "$BAFT_REPO" "$BAFT_SHA" > "$out/licenses/SOURCE.txt"
(cd "$out" && sha256sum baft | tee baft.sha256)
