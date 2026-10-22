#!/usr/bin/env sh
# Regenerates .sqlx/ so the compile-time checked queries build offline.
set -eu
cd "$(dirname "$0")/.."
mkdir -p target/sqlx
export DATABASE_URL="sqlite://$PWD/target/sqlx/dev.db"
rm -f target/sqlx/dev.db*
sqlx database create
sqlx migrate run --source crates/storage/migrations
SQLX_OFFLINE=false cargo sqlx prepare --workspace -- -p storage --all-targets
