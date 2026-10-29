<script lang="ts">
  import { t } from '$lib/i18n';
  import type { AuthenticationStatus } from '$lib/api';
  import * as Card from '$lib/components/ui/card';
  import { Badge } from '$lib/components/ui/badge';
  let { authentication: auth }: { authentication: AuthenticationStatus } = $props();
</script>

<div class="space-y-6">
  <p class="text-muted-foreground text-sm">{$t('auth_config_readonly')}</p>
  <div class="grid gap-6 lg:grid-cols-2">
    <Card.Root>
      <Card.Header>
        <Card.Title>{$t('auth_sso')}</Card.Title>
        <Card.Description>{$t(auth.sso ? 'auth_sso_on' : 'auth_sso_off')}</Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4 text-sm">
        <Badge variant={auth.sso ? 'secondary' : 'outline'}
          >{$t(auth.sso ? 'enabled' : 'disabled')}</Badge
        >
        {#if auth.sso}
          <dl class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
            <dt class="text-muted-foreground">{$t('auth_provider')}</dt>
            <dd class="break-words">{auth.sso.provider_name}</dd>
            <dt class="text-muted-foreground">{$t('auth_domains')}</dt>
            <dd class="break-words">
              {auth.sso.allowed_email_domains.length
                ? auth.sso.allowed_email_domains.join(', ')
                : $t('auth_any_domain')}
            </dd>
            <dt class="text-muted-foreground">{$t('auth_approval')}</dt>
            <dd>{$t(auth.sso.require_approval ? 'auth_required' : 'auth_not_required')}</dd>
          </dl>
          <p class="text-muted-foreground">{$t('auth_owner_sso')}</p>
        {/if}
      </Card.Content>
    </Card.Root>
    <Card.Root>
      <Card.Header
        ><Card.Title>{$t('auth_password')}</Card.Title><Card.Description
          >{$t('auth_password_' + auth.password_login)}</Card.Description
        ></Card.Header
      >
      <Card.Content
        ><Badge variant={auth.password_login === 'disabled' ? 'outline' : 'secondary'}
          >{$t(auth.password_login === 'disabled' ? 'disabled' : 'enabled')}</Badge
        ></Card.Content
      >
    </Card.Root>
    <Card.Root>
      <Card.Header
        ><Card.Title>{$t('auth_passkeys')}</Card.Title><Card.Description
          >{$t(auth.passkeys ? 'auth_passkeys_on' : 'auth_passkeys_off')}</Card.Description
        ></Card.Header
      >
      <Card.Content class="space-y-4 text-sm">
        <Badge variant={auth.passkeys ? 'secondary' : 'outline'}
          >{$t(auth.passkeys ? 'enabled' : 'disabled')}</Badge
        >
        {#if auth.passkeys}
          <dl class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
            <dt class="text-muted-foreground">{$t('auth_rp_id')}</dt>
            <dd class="break-all">{auth.passkeys.rp_id}</dd>
            <dt class="text-muted-foreground">{$t('auth_origin')}</dt>
            <dd class="break-all">{auth.passkeys.rp_origin}</dd>
            <dt class="text-muted-foreground">{$t('auth_passkey_limit')}</dt>
            <dd>{auth.passkeys.max_per_user}</dd>
          </dl>
        {/if}
      </Card.Content>
    </Card.Root>
    <Card.Root>
      <Card.Header><Card.Title>{$t('auth_access')}</Card.Title></Card.Header>
      <Card.Content class="space-y-4 text-sm">
        <dl class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
          <dt class="text-muted-foreground">{$t('allow_public_sign_up')}</dt>
          <dd>{$t(auth.public_signup ? 'enabled' : 'disabled')}</dd>
          <dt class="text-muted-foreground">{$t('auth_setup')}</dt>
          <dd>{$t(auth.admin_setup_enabled ? 'enabled' : 'disabled')}</dd>
          <dt class="text-muted-foreground">{$t('auth_login_protection')}</dt>
          <dd>{$t(auth.login_protection_enabled ? 'enabled' : 'disabled')}</dd>
        </dl>
        {#if auth.yaml_teams}<p class="text-muted-foreground">{$t('auth_yaml_access')}</p>{/if}
      </Card.Content>
    </Card.Root>
  </div>
</div>
