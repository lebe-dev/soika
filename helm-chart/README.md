# Soika Helm chart

This chart deploys the single-binary Soika image with SQLite or PostgreSQL 15+.
It uses one replica and a `Recreate` deployment strategy: the retention scheduler,
ingestion rate limiter, and login lockout are per process. The application runs
the selected database's migrations at startup; no migration Job is needed.

## SQLite install (default)

Create a local values file with a public URL and a persistent secret key:

```yaml
envs:
  - name: BIND_ADDR
    value: "0.0.0.0:8080"
  - name: DATABASE_URL
    value: "sqlite:///app/data/soika.db?mode=rwc"
  - name: BASE_URL
    value: "https://errors.example.com"
secrets:
  envs:
    - name: SECRET_KEY
      value: "replace-with-a-random-secret"
ingress:
  enabled: true
  hosts:
    - host: errors.example.com
      paths:
        - path: /
          pathType: Prefix
  tls:
    - hosts: [errors.example.com]
      secretName: errors-tls
```

Generate a key with `openssl rand -hex 32`, keep the values file out of Git,
and install with:

```sh
helm upgrade --install soika ./helm-chart -f my-values.yaml
```

Alternatively, create a Kubernetes Secret with `SECRET_KEY` and set
`secrets.create=false` and `secrets.existingSecret=<name>`. Other sensitive
variables such as `SMTP_PASSWORD` and `OAUTH_CLIENT_SECRET` can go in the same
Secret. List non-sensitive variables in `envs`; the full set is documented in
the root `.env.example`.
When using an existing Secret, restart the Deployment after changing its data;
Helm can only trigger an automatic rollout for a Secret created by this chart.

The image defaults to `tinyops/soika` at the chart's `appVersion`. Use
`image.repository` and `image.tag` for a private registry. `BASE_URL` must
match the external URL for DSNs, email links, secure cookies, and passkeys.
Enable either `ingress` or `httpRoute` for public access. HTTPRoute requires
the Gateway API CRDs and a configured Gateway.

For SQLite, the chart creates a `ReadWriteOnce` PVC at `/app/data`. Use a storage class
with working SQLite file locks; avoid NFS and SMB. Set
`persistence.existingClaim` to reuse a claim. The created PVC has Helm's
`keep` annotation so uninstall does not delete the database. Disabling
`persistence` uses an `emptyDir`, which loses data when the Pod is replaced.
The application also uses a writable temporary directory at `/tmp`.

## PostgreSQL install

Create a PostgreSQL 15+ database separately. Set its URL in a Secret because it
contains a password; replace the default SQLite `DATABASE_URL` in `envs` by
specifying the full list without it:

```yaml
envs:
  - name: BIND_ADDR
    value: "0.0.0.0:8080"
  - name: BASE_URL
    value: "https://errors.example.com"
secrets:
  envs:
    - name: SECRET_KEY
      value: "replace-with-a-random-secret"
    - name: DATABASE_URL
      value: "postgres://soika:password@postgres.example.com:5432/soika?sslmode=require"
persistence:
  enabled: false
```

You can also set `secrets.create: false` and `secrets.existingSecret` to a
preexisting Secret containing both `SECRET_KEY` and `DATABASE_URL`. With
`persistence.enabled: false`, the chart creates no PVC; the `/app/data` mount
is an ephemeral `emptyDir` and does not hold application data on PostgreSQL.
The PostgreSQL database must exist before Soika starts; Soika creates its tables.
Switching from SQLite to PostgreSQL does not copy existing data. Ensure the
selected Soika image includes PostgreSQL support.
