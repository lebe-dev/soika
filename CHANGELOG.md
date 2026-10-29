# Changelog

## 0.9.0

- Manage projects, teams and team membership declaratively from a YAML file,
  independently from UI management. Existing projects retain their DSNs and history.
- Assign team roles through email grants and exact OIDC group paths, including
  users joining later through SSO. Matching grants automatically approve users.
- Preserve removed projects as read-only by default, or explicitly choose permanent
  deletion. Restore read-only projects with the same DSN by adding them back.
- Display management sources, synchronization status, pending grants and orphan
  warnings in the UI. Mute settings and DSN regeneration remain available.
- Team deletion preserves projects and supports an explicit atomic transfer.
- Enforce SSO for non-Owner accounts, including old sessions and invite acceptance.
  The first SSO login to a local non-Owner account removes its password, passkeys
  and previous sessions. Owners retain their password fallback.
- **Breaking:** SSO and passkeys cannot be enabled together. Disable one before
  upgrading. Management defaults to the UI, so GitOps remains opt-in.
