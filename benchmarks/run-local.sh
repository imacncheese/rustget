#!/usr/bin/env bash
set -euo pipefail
SIZE="${SIZE:-134217728}"
CONNECTIONS="${CONNECTIONS:-16}"
RUNS="${RUNS:-3}"
PORT="${PORT:-59552}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
cargo build --release -p rustget-cli -p rustget-testkit

WORK="$ROOT/.tmp/bench"
mkdir -p "$WORK"
./target/release/rustget-testkit --listen "127.0.0.1:$PORT" --size "$SIZE" --name bench.bin >"$WORK/testkit.log" 2>&1 &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null || true' EXIT
sleep 0.6
URL="http://127.0.0.1:$PORT/files/bench.bin"
RUST_TIMES=()
ARIA_TIMES=()
for i in $(seq 1 "$RUNS"); do
  rm -rf "$WORK/rust-$i"
  start=$(date +%s.%N)
  ./target/release/rustget --local --data-dir "$WORK/data-$i" --download-dir "$WORK/rust-$i" \
    add "$URL" --connections "$CONNECTIONS" --output "$WORK/rust-$i/file.bin" --quiet >/dev/null
  end=$(date +%s.%N)
  RUST_TIMES+=("$(echo "$end - $start" | bc)")
  test "$(stat -c%s "$WORK/rust-$i/file.bin")" -eq "$SIZE"
done
if command -v aria2c >/dev/null; then
  for i in $(seq 1 "$RUNS"); do
    rm -rf "$WORK/aria-$i"; mkdir -p "$WORK/aria-$i"
    start=$(date +%s.%N)
    aria2c --allow-overwrite=true --auto-file-renaming=false --file-allocation=none \
      -x"$CONNECTIONS" -s"$CONNECTIONS" --dir "$WORK/aria-$i" --out file.bin "$URL" >/dev/null 2>&1
    end=$(date +%s.%N)
    ARIA_TIMES+=("$(echo "$end - $start" | bc)")
    test "$(stat -c%s "$WORK/aria-$i/file.bin")" -eq "$SIZE"
  done
fi
python3 - "$SIZE" "${RUST_TIMES[@]}" -- "${ARIA_TIMES[@]}" <<'PY'
import statistics, sys
size = int(sys.argv[1]); parts = sys.argv[2:]; split = parts.index('--')
rust = [float(x) for x in parts[:split]]
aria = [float(x) for x in parts[split+1:]]
print("RustGet:", rust, "median", statistics.median(rust))
if aria: print("aria2:", aria, "median", statistics.median(aria))
PY
