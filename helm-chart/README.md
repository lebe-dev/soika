# Soika Helm chart

This chart deploys the single-binary Soika image with a persistent SQLite volume.
It uses one replica and a `Recreate` deployment strategy so upgrades do not run
two processes against the same database. The application runs migrations at
startup; no migration Job is needed.

## Install

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

The chart creates a `ReadWriteOnce` PVC at `/app/data`. Use a storage class
with working SQLite file locks; avoid NFS and SMB. Set
`persistence.existingClaim` to reuse a claim. The created PVC has Helm's
`keep` annotation so uninstall does not delete the database. Disabling
`persistence` uses an `emptyDir`, which loses data when the Pod is replaced.
The application also uses a writable temporary directory at `/tmp`.
