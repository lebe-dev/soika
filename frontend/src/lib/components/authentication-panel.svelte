<script lang="ts">
  import { t } from '$lib/i18n';
  import type { AuthenticationStatus } from '$lib/api';
  import * as Card from '$lib/components/ui/card';
  import { Badge } from '$lib/components/ui/badge';
  import InfoTip from '$lib/components/info-tip.svelte';
  let { authentication: auth }: { authentication: AuthenticationStatus } = $props();
</script>

{#snippet term(label: string, tip: string)}
  <dt class="text-muted-foreground flex items-center gap-1.5">{label}<InfoTip text={$t(tip)} /></dt>
{/snippet}

<div class="space-y-6">
  <p class="text-muted-foreground text-sm">{$t('auth_config_readonly')}</p>
  <div class="grid gap-6 lg:grid-cols-2">
    <Card.Root>
      <Card.Header>
        <Card.Title>{$t('auth_sso')}</Card.Title>
        <Card.Description>{$t(auth.sso ? 'auth_sso_on' : 'auth_sso_off')}</Card.Description>
      </Card.Header>
      <Card.Content class="space-y-4 text-sm">
        <Badge variant={auth.sso ? 'enabled' : 'disabled'}
          >{$t(auth.sso ? 'enabled' : 'disabled')}</Badge
        >
        {#if auth.sso}
          <dl class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
            <dt class="text-muted-foreground">{$t('auth_provider')}</dt>
            <dd class="break-words">{auth.sso.provider_name}</dd>
            {@render term($t('auth_domains'), 'tip_auth_domains')}
            <dd class="break-words">
              {auth.sso.allowed_email_domains.length
                ? auth.sso.allowed_email_domains.join(', ')
                : $t('auth_any_domain')}
            </dd>
            {@render term($t('auth_approval'), 'tip_auth_approval')}
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
        ><Badge variant={auth.password_login === 'disabled' ? 'disabled' : 'enabled'}
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
        <Badge variant={auth.passkeys ? 'enabled' : 'disabled'}
          >{$t(auth.passkeys ? 'enabled' : 'disabled')}</Badge
        >
        {#if auth.passkeys}
          <dl class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
            {@render term($t('auth_rp_id'), 'tip_auth_rp_id')}
            <dd class="break-all">{auth.passkeys.rp_id}</dd>
            {@render term($t('auth_origin'), 'tip_auth_origin')}
            <dd class="break-all">{auth.passkeys.rp_origin}</dd>
            {@render term($t('auth_passkey_limit'), 'tip_auth_passkey_limit')}
            <dd>{auth.passkeys.max_per_user}</dd>
          </dl>
        {/if}
      </Card.Content>
    </Card.Root>
    <Card.Root>
      <Card.Header><Card.Title>{$t('auth_access')}</Card.Title></Card.Header>
      <Card.Content class="space-y-4 text-sm">
        <dl class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-3">
          {@render term($t('allow_public_sign_up'), 'tip_allow_signup')}
          <dd>
            <Badge variant={auth.public_signup ? 'enabled' : 'disabled'}
              >{$t(auth.public_signup ? 'enabled' : 'disabled')}</Badge
            >
          </dd>
          {@render term($t('auth_invites'), 'tip_auth_invites')}
          <dd>
            <Badge variant={auth.invites_enabled ? 'enabled' : 'disabled'}
              >{$t(auth.invites_enabled ? 'enabled' : 'disabled')}</Badge
            >
          </dd>
          {@render term($t('auth_setup'), 'tip_auth_setup')}
          <dd>
            <Badge variant={auth.admin_setup_enabled ? 'enabled' : 'disabled'}
              >{$t(auth.admin_setup_enabled ? 'enabled' : 'disabled')}</Badge
            >
          </dd>
          {@render term($t('auth_login_protection'), 'tip_auth_login_protection')}
          <dd>
            <Badge variant={auth.login_protection_enabled ? 'enabled' : 'disabled'}
              >{$t(auth.login_protection_enabled ? 'enabled' : 'disabled')}</Badge
            >
          </dd>
        </dl>
        {#if auth.yaml_teams}<p class="text-muted-foreground">{$t('auth_yaml_access')}</p>{/if}
      </Card.Content>
    </Card.Root>
  </div>
</div>
