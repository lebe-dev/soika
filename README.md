# soika

![soika dashboard](screenshot1.png)

soika is a lightweight, Sentry-compatible error tracker for collecting and
triaging application errors. The Rust backend and web interface ship together
as a single binary, storing data in SQLite (the default) or PostgreSQL.

## Getting started

```sh
docker run -d \
  --name soika \
  -p 8080:8080 \
  -e SECRET_KEY="$(openssl rand -hex 32)" \
  -v soika-data:/app/data \
  tinyops/soika:0.10.0
```

Open [http://localhost:8080](http://localhost:8080) and create the initial
Owner account on the setup page.

To use PostgreSQL instead of the bundled SQLite file, point `DATABASE_URL` at
an existing database:

```sh
docker run -d \
  --name soika \
  -p 8080:8080 \
  -e SECRET_KEY="$(openssl rand -hex 32)" \
  -e DATABASE_URL="postgres://soika:secret@db.example.com:5432/soika" \
  tinyops/soika:0.10.0
```

## Features

- Accepts events from Sentry-compatible SDKs.
- Groups errors and displays issues, events, stacktraces, tags, and context.
- Supports password login, OAuth/OIDC single sign-on, and passkeys.
- Provides team-based project access and issue triage in the web interface.
- Runs as one binary with embedded frontend assets; stores data in an SQLite
  file or in PostgreSQL 15+ (chosen by the `DATABASE_URL` scheme).
- Supports English and Russian localizations (`en`, `ru`).
- Blazing fast, built with Rust and SvelteKit.
- Low resource usage — example Docker stats snapshot:

  ```text
  CONTAINER ID   NAME   CPU %   MEM USAGE / LIMIT   MEM %   NET I/O         BLOCK I/O       PIDS
  3cc6acffa7a7   soika  0.00%   17.01MiB / 128MiB   13.29%  725MB / 135MB   49.9MB / 289MB  11
  ```

## Docs

- [Installation and configuration](docs/OPERATIONS.md)
- [Frequently asked questions](docs/FAQ.md)
- [Outgoing webhooks](docs/WEBHOOKS.md)
- [Helm chart](helm-chart/README.md)
- [User interface](docs/UI.md)
- [Development](DEV.md)

## License

[MIT](LICENSE.md)

[Declarative project and team management (GitOps)](docs/OPERATIONS.md#configuration-as-code-gitops).
