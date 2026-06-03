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

# Check formatting without writing changes.
fmt-check:
    cargo fmt --all -- --check

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

# --- Deploy ---
deploy:
    ssh kaiman "cd /opt/soika && sed -i 's|{{ imageName }}:[^\"]*|{{ imageName }}:{{ version }}|' docker-compose.yml && docker compose pull && docker compose down && docker compose up -d"
