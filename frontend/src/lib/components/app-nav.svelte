<script lang="ts">
  import { t } from '$lib/i18n';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { auth, type User } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { toast } from '$lib/components/ui/sonner';
  import { Button } from '$lib/components/ui/button';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { cn } from '$lib/utils';
  import { toggleMode, mode } from 'mode-watcher';
  import LayoutDashboard from '@lucide/svelte/icons/layout-dashboard';
  import Users from '@lucide/svelte/icons/users';
  import Shield from '@lucide/svelte/icons/shield';
  import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
  import UserIcon from '@lucide/svelte/icons/user';
  import LogOut from '@lucide/svelte/icons/log-out';
  import Sun from '@lucide/svelte/icons/sun';
  import Moon from '@lucide/svelte/icons/moon';
  import Menu from '@lucide/svelte/icons/menu';
  import BellOff from '@lucide/svelte/icons/bell-off';
  import Avatar from '$lib/components/avatar.svelte';

  let { user: initialUser, pendingApprovals = 0 }: { user: User; pendingApprovals?: number } =
    $props();

  // Read the live user from the auth store so nav state (e.g. the
  // notifications-off bell) reacts to profile updates without a page reload.
  // The store is seeded by the layout load; fall back to the prop until then.
  const user = $derived(authStore.user ?? initialUser);

  type NavItem = { href: string; label: string; icon: typeof LayoutDashboard; adminOnly?: boolean };

  const items: NavItem[] = $derived([
    { href: '/', label: $t('dashboard'), icon: LayoutDashboard },
    { href: '/teams', label: $t('teams'), icon: Users },
    { href: '/profile', label: $t('profile'), icon: UserIcon },
    { href: '/admin', label: $t('admin'), icon: Shield, adminOnly: true }
  ]);

  // Admin-only nav items are visible to instance managers (Owner | Manager).
  const canManageInstance = $derived(
    user.instance_role === 'owner' || user.instance_role === 'manager'
  );
  const visibleItems = $derived(items.filter((item) => !item.adminOnly || canManageInstance));

  function isActive(href: string): boolean {
    const path = $page.url.pathname;
    if (href === '/') return path === '/';
    return path === href || path.startsWith(`${href}/`);
  }

  async function logout() {
    // Always drop local session state and bounce to login, regardless of the
    // outcome of the server call. A network failure (TypeError) or an API error
    // must not leave the user appearing signed in — clearing locally is the
    // resilient behaviour.
    try {
      await auth.logout();
    } catch {
      // Ignore: server-side logout is best-effort; local clear is what matters.
    }
    authStore.clear();
    toast.success($t('signed_out'));
    await goto('/login', { invalidateAll: true });
  }
</script>

<header
  class="bg-background/90 supports-[backdrop-filter]:bg-background/75 sticky top-0 z-40 border-b backdrop-blur-md backdrop-saturate-150"
>
  <div class="container flex h-14 items-center gap-2 sm:gap-7">
    <!-- Mobile: hamburger menu collapses the primary nav links. -->
    <DropdownMenu.Root>
      <DropdownMenu.Trigger class="md:hidden">
        <Button variant="ghost" size="icon" aria-label={$t('open_navigation')}>
          <Menu class="size-5" />
          {#if pendingApprovals > 0 && canManageInstance}
            <AlertTriangle class="text-primary size-3" />
          {/if}
        </Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="start" class="w-48">
        {#each visibleItems as item (item.href)}
          <DropdownMenu.Item
            onclick={() => goto(item.href)}
            class={cn(isActive(item.href) && 'bg-accent')}
          >
            <item.icon class="size-4" />
            {item.label}
            {#if item.href === '/admin' && pendingApprovals > 0}
              <AlertTriangle class="text-primary ml-auto size-3.5" />
            {/if}
          </DropdownMenu.Item>
        {/each}
      </DropdownMenu.Content>
    </DropdownMenu.Root>

    <a href="/" class="flex items-center gap-2.5 text-[16px] font-semibold tracking-[-0.02em]">
      <svg viewBox="0 0 22 22" class="size-[22px]" aria-hidden="true">
        <rect width="22" height="22" rx="6.5" class="fill-primary" />
        <path
          d="M5.5 14c2.6-4.8 6.8-6.4 11-5.6-1.6 3.8-5.4 6.4-11 5.6z"
          class="fill-primary-foreground"
        />
      </svg>
      soika
    </a>

    <!-- Desktop: full inline nav. -->
    <nav class="hidden items-stretch gap-0.5 self-stretch text-sm md:flex">
      {#each visibleItems as item (item.href)}
        <a
          href={item.href}
          aria-current={isActive(item.href) ? 'page' : undefined}
          class={cn(
            'flex items-center gap-1.5 px-2.5 transition-colors',
            isActive(item.href)
              ? 'text-foreground font-medium shadow-[inset_0_-2px_0_var(--primary)]'
              : 'text-muted-foreground hover:text-foreground'
          )}
        >
          {item.label}
          {#if item.href === '/admin' && pendingApprovals > 0}
            <AlertTriangle
              class="text-primary size-3.5"
              aria-label={$t('awaiting_approval_count', { values: { count: pendingApprovals } })}
            />
          {/if}
        </a>
      {/each}
    </nav>

    <div class="ml-auto flex items-center gap-1">
      {#if !user.notifications_enabled}
        <Button
          variant="ghost"
          size="icon"
          onclick={() => goto('/profile')}
          aria-label={$t('email_notifications_are_off')}
          title={$t('email_notifications_are_off')}
        >
          <BellOff class="size-4 text-orange-500" />
        </Button>
      {/if}

      <Button variant="ghost" size="icon" onclick={toggleMode} aria-label={$t('toggle_theme')}>
        {#if mode.current === 'dark'}
          <Sun class="size-4" />
        {:else}
          <Moon class="size-4" />
        {/if}
      </Button>

      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          <Button variant="ghost" size="icon" aria-label={$t('account_menu')}>
            <Avatar name={user.display_name} id={user.id} size="sm" />
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="w-48">
          <DropdownMenu.Label>{user.email}</DropdownMenu.Label>
          <DropdownMenu.Separator />
          <DropdownMenu.Item onclick={() => goto('/profile')}>
            <UserIcon class="size-4" />
            {$t('profile')}
          </DropdownMenu.Item>
          <DropdownMenu.Item onclick={logout}>
            <LogOut class="size-4" />
            {$t('sign_out')}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </div>
  </div>
</header>
