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

# Run the server (loads .env if present).
run:
    cargo run

# Run the whole test suite, or a focused test: `just test <name>`.
test-backend name="":
    cargo test {{ name }}

test-frontend name="":
    cd frontend && yarn vitest run {{ if name != "" { name } else { "" } }}

test-all: test-backend && test-frontend

# Lint with clippy, denying warnings.
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Format the code.
fmt:
    cargo fmt --all

# Check formatting without writing changes.
fmt-check:
    cargo fmt --all -- --check

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

build-image: test-all && lint
    docker build --progress=plain --platform linux/amd64 -t {{ imageName }}:{{ version }} .

push-image:
    docker push {{ imageName }}:{{ version }}

release: build-image && push-image
