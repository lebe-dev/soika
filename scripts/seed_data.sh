#!/usr/bin/env bash
set -euo pipefail

DB="${DATABASE_URL:-sqlite://soika.db}"
DB_FILE="${DB#sqlite://}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if ! command -v sqlite3 &>/dev/null; then
    echo "error: sqlite3 not found" >&2
    exit 1
fi

if [[ ! -f "$DB_FILE" ]]; then
    echo "error: database file '$DB_FILE' not found — run the app first to apply migrations" >&2
    exit 1
fi

echo "Seeding $DB_FILE ..."

sqlite3 "$DB_FILE" < "$SCRIPT_DIR/seed_data.sql"

if command -v python3 &>/dev/null; then
    DATABASE_URL="$DB" python3 "$SCRIPT_DIR/seed_events.py"
else
    echo "warning: python3 not found, skipping event stack traces"
fi

echo ""
sqlite3 "$DB_FILE" "
SELECT 'teams:    ' || count(*) FROM teams;
SELECT 'projects: ' || count(*) FROM projects;
SELECT 'issues:   ' || count(*) FROM issues;
SELECT 'events:   ' || count(*) FROM events;
"
