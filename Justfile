# soika — task runner
#
# `just` lists recipes. The default DATABASE_URL points at a local sqlite file
# so `sqlx::migrate!` works at build/run time without external services.

set dotenv-load

version := `cat Cargo.toml | grep version | head -1 | cut -d " " -f 3 | tr -d "\""`
imageName := 'tinyops/soika'

DATABASE_URL := env_var_or_default("DATABASE_URL", "sqlite://soika.db")
export SECRET_KEY := env_var_or_default("SECRET_KEY", "dev-secret-change-me")

IMAGE := "tinyops/soika"
TAG := env_var_or_default("TAG", "latest")

# List available recipes.
default:
    @just --list

# Format the code.
fmt:
    cargo fmt --all
    cd frontend && yarn prettier --write .

# Check formatting without writing changes.
fmt-check:
    cargo fmt --all -- --check

format: fmt

# Build the workspace (debug). Run `just dist` first for fresh embedded assets.
build:
    cargo build

# --- Frontend (SvelteKit + Tailwind + shadcn-svelte, yarn) ---

frontend-build:
    cd frontend && yarn build

# Type-check the frontend (svelte-check).
frontend-check:
    cd frontend && yarn run check

# Lint the frontend: formatting (prettier) + type/Svelte checks (svelte-check).
lint-frontend:
    cd frontend && yarn lint
    cd frontend && yarn run check

# Build the embedded frontend assets (alias used by `release`).
dist: frontend-build

# --- Dependencies ---
bump-backend-deps:
    cargo update

bump-frontend-deps:
    cd frontend && yarn upgrade

bump-deps: bump-backend-deps && bump-frontend-deps

# Run the server (loads .env if present).
run:
    cargo run

# Run the whole test suite, or a focused test: `just test <name>`.
test-backend name="":
    cargo test {{ name }}

test-frontend name="":
    cd frontend && yarn vitest run {{ if name != "" { name } else { "" } }}

# Run the backend tests with an LCOV coverage report (read by SonarQube via
# `sonar.rust.lcov.reportPaths`). Requires `cargo install cargo-llvm-cov`.
# `--remap-path-prefix` makes source paths workspace-relative so they resolve
# inside the dockerized sonar-scanner (which mounts the repo at /usr/src).
test-backend-coverage:
    cargo llvm-cov --lcov --remap-path-prefix --output-path lcov.info

# Run the frontend tests with a V8 coverage report. The lcov paths are emitted
# relative to `frontend/`; rewrite them to be repo-root-relative so the
# dockerized sonar-scanner (repo mounted at /usr/src) resolves them to the
# indexed `frontend/src/...` files.
test-frontend-coverage:
    cd frontend && yarn vitest run --coverage
    sed -i.bak 's#^SF:#SF:frontend/#' frontend/coverage/lcov.info && rm -f frontend/coverage/lcov.info.bak

test: test-backend && test-frontend

# Lint with clippy, denying warnings.
lint: fmt
    cargo clippy --all-targets --all-features -- -D warnings

# Type-check without producing artifacts.
check:
    cargo check --all-targets

run-backend:
    cargo run

run-frontend:
    cd frontend && yarn && npm run dev -- --port=4200

# Seed the local database with demo teams, projects, and issues.
init-env:
    bash scripts/seed_data.sh

start-env:
    docker compose -f docker-compose-dev.yml up -d --build --force-recreate

stop-env:
    docker compose -f docker-compose-dev.yml down

# Push the image to Docker Hub (`tinyops/soika`).
docker-push:
    docker push {{ IMAGE }}:{{ TAG }}

# Remove build artifacts.
clean:
    cargo clean

build-image: test && lint
    docker build --progress=plain --platform linux/amd64 -t {{ imageName }}:{{ version }} .

push-image:
    docker push {{ imageName }}:{{ version }}

release: build-image && push-image

ssh:
    ssh kaiman

# Forward MailCrab web UI (port 1080) from kaiman to localhost:1080.
mailcrab:
    ssh -N -L 1080:localhost:1080 kaiman

stop:
    lsof -ti :4200 | xargs kill -9
    lsof -ti :18080 | xargs kill -9

# --- Deploy ---
deploy:
    ssh kaiman "cd /opt/soika && sed -i 's|{{ imageName }}:[^\"]*|{{ imageName }}:{{ version }}|' docker-compose.yml && docker compose pull && docker compose down && docker compose up -d"

# --- SonarQube (static analysis) ---
# Compose stack lives in sonarqube.yml (SonarQube Community Build + PostgreSQL).
# The scanner runs as a one-shot `docker run` and reads sonar-project.properties.
sonarComposeFile := "sonarqube.yml"
# Host URL as seen from *inside* the scanner container. host.docker.internal reaches the
# host's published port 9000 on Docker Desktop and (via --add-host) on Linux.
sonarHostUrl := env_var_or_default("SONAR_HOST_URL", "http://host.docker.internal:9000")

# Start SonarQube + PostgreSQL (UI at http://localhost:9000, admin/admin on first login)
sonar-up:
    docker compose -f {{ sonarComposeFile }} up -d
    @echo "==> SonarQube starting at http://localhost:9000 (first boot ~1-2 min). Login admin/admin, then:"
    @echo "==> 1. Set a strong password."
    @echo "==> 2. Create a token (My Account -> Security) and put it in .env as SONAR_TOKEN=sqp_xxx."

# Stop SonarQube (named volumes keep the DB + analysis history)
sonar-down:
    docker compose -f {{ sonarComposeFile }} down

# Wipe SonarQube including all data volumes
sonar-clean:
    docker compose -f {{ sonarComposeFile }} down -v

# Run the scanner against the running instance. Reads SONAR_TOKEN from .env.
# Regenerates the backend + frontend coverage reports first so SonarQube reads
# fresh `lcov.info` / `frontend/coverage/lcov.info` (otherwise coverage shows 0%).
sonar-scan: test-backend-coverage test-frontend-coverage
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -z "${SONAR_TOKEN:-}" ]; then
        echo "error: SONAR_TOKEN is not set." >&2
        echo "  Generate a token at {{ sonarHostUrl }} -> My Account -> Security," >&2
        echo "  then add it to .env:  SONAR_TOKEN=sqp_xxx" >&2
        exit 1
    fi
    docker run --rm \
        --add-host=host.docker.internal:host-gateway \
        -e SONAR_HOST_URL="{{ sonarHostUrl }}" \
        -e SONAR_TOKEN="$SONAR_TOKEN" \
        -v "$PWD:/usr/src" \
        sonarsource/sonar-scanner-cli:latest \
        -Dsonar.projectVersion="{{ version }}"
