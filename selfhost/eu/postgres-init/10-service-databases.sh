#!/bin/sh
# Runs once, when the data volume is first initialised: one database and one
# login role per service in POSTGRES_SERVICE_DATABASES. Each role's password
# comes from <NAME>_DB_PASSWORD in secrets.env (upper-cased). Passwords go to
# psql as variables, never on the command line.
set -eu
for db in ${POSTGRES_SERVICE_DATABASES:-}; do
  var=$(printf '%s_DB_PASSWORD' "$db" | tr 'a-z' 'A-Z')
  eval "password=\${$var:-}"
  if [ -z "$password" ]; then
    echo "postgres-init: $var is not set; skipping $db" >&2
    continue
  fi
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname postgres \
    -v role="$db" -v password="$password" <<'SQL'
SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'role', :'password') \gexec
SELECT format('CREATE DATABASE %I OWNER %I', :'role', :'role') \gexec
SQL
done
