#!/usr/bin/env bash
# scripts/pg-e2e.sh — Run Postgres E2E tests with an ephemeral Postgres instance.
#
# Usage:
#   nix develop .#e2e --command bash scripts/pg-e2e.sh
#
# Or if postgres is already available:
#   bash scripts/pg-e2e.sh
#
# The script creates a temporary Postgres data directory, starts the server
# on an unused port, runs tests, and cleans up everything on exit.

set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────
PG_USER="${PG_USER:-atrg_test}"
PG_DB="${PG_DB:-atrg_test}"
PG_PORT="${PG_PORT:-15432}"
PGDATA="$(mktemp -d "${TMPDIR:-/tmp}/atrg-pg-e2e.XXXXXX")"

# ── Cleanup on exit ──────────────────────────────────────────────────
cleanup() {
    echo ""
    echo "🧹 Cleaning up..."
    if [ -f "$PGDATA/postmaster.pid" ]; then
        pg_ctl -D "$PGDATA" -m fast stop 2>/dev/null || true
    fi
    rm -rf "$PGDATA"
    echo "✓ Postgres data directory removed"
}
trap cleanup EXIT INT TERM

# ── Check prerequisites ──────────────────────────────────────────────
if ! command -v initdb &>/dev/null; then
    echo "ERROR: initdb not found. Run inside the Nix e2e shell:"
    echo "  nix develop .#e2e --command bash scripts/pg-e2e.sh"
    exit 1
fi

if ! command -v cargo &>/dev/null; then
    echo "ERROR: cargo not found."
    exit 1
fi

# ── Start Postgres ───────────────────────────────────────────────────
echo "🐘 Starting ephemeral Postgres on port $PG_PORT..."
echo "   Data dir: $PGDATA"

# Initialize the data directory
initdb -D "$PGDATA" --no-locale --encoding=UTF8 -U "$PG_USER" >/dev/null 2>&1

# Configure for local trust auth (no password needed)
cat > "$PGDATA/pg_hba.conf" <<EOF
local   all   all                 trust
host    all   all   127.0.0.1/32  trust
host    all   all   ::1/128       trust
EOF

# Start the server
pg_ctl -D "$PGDATA" -l "$PGDATA/server.log" \
    -o "-p $PG_PORT -k $PGDATA" \
    start >/dev/null 2>&1

# Wait for readiness
for i in $(seq 1 30); do
    if pg_isready -h 127.0.0.1 -p "$PG_PORT" -U "$PG_USER" >/dev/null 2>&1; then
        echo "   ✓ Postgres ready (took ${i}s)"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "   ✗ Postgres failed to start within 30s"
        cat "$PGDATA/server.log"
        exit 1
    fi
    sleep 1
done

# Create the test database
createdb -h 127.0.0.1 -p "$PG_PORT" -U "$PG_USER" "$PG_DB" 2>/dev/null || true

# ── Run tests ────────────────────────────────────────────────────────
export TEST_DATABASE_URL="postgres://${PG_USER}@127.0.0.1:${PG_PORT}/${PG_DB}"

echo ""
echo "🧪 Running Postgres E2E tests..."
echo "   TEST_DATABASE_URL=$TEST_DATABASE_URL"
echo ""

cargo test \
    --package atrg-db \
    --test postgres_e2e \
    --features postgres \
    -- --test-threads=1 \
    "$@"

RESULT=$?

echo ""
if [ $RESULT -eq 0 ]; then
    echo "✅ All Postgres E2E tests passed"
else
    echo "❌ Some Postgres E2E tests failed (exit code: $RESULT)"
fi

exit $RESULT
