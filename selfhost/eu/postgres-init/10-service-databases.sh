#!/bin/sh
# Runs once, when the data volume is first initialised: one database and one
# login role per service in POSTGRES_SERVICE_DATABASES. Each role's password
# comes from <NAME>_DB_PASSWORD in secrets/postgres.env (upper-cased).
# Identifiers are strictly validated before expansion, and passwords are read
# by psql via \getenv from the environment (never passed on psql's argv).
set -eu
for db in ${POSTGRES_SERVICE_DATABASES:-}; do
  if ! printf '%s\n' "$db" | LC_ALL=C grep -Eq '^[a-z_][a-z0-9_]*$'; then
    echo "postgres-init: invalid database/role identifier '$db'" >&2
    exit 1
  fi
  var=$(printf '%s_DB_PASSWORD' "$db" | tr 'a-z' 'A-Z')
  eval "password=\${$var:-}"
  if [ -z "$password" ]; then
    echo "postgres-init: $var is not set; skipping $db" >&2
    continue
  fi
  PG_INIT_ROLE="$db" PG_INIT_PASSWORD="$password" \
    psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres <<'SQL'
\getenv role PG_INIT_ROLE
\getenv password PG_INIT_PASSWORD
SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'role', :'password') \gexec
SELECT format('CREATE DATABASE %I OWNER %I', :'role', :'role') \gexec
SQL
  unset password
done
