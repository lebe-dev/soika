<script lang="ts">
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

  let { user, pendingApprovals = 0 }: { user: User; pendingApprovals?: number } = $props();

  type NavItem = { href: string; label: string; icon: typeof LayoutDashboard; adminOnly?: boolean };

  const items: NavItem[] = [
    { href: '/', label: 'Dashboard', icon: LayoutDashboard },
    { href: '/teams', label: 'Teams', icon: Users },
    { href: '/profile', label: 'Profile', icon: UserIcon },
    { href: '/admin', label: 'Admin', icon: Shield, adminOnly: true }
  ];

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
    toast.success('Signed out');
    await goto('/login');
  }
</script>

<header
  class="bg-background/95 supports-[backdrop-filter]:bg-background/60 sticky top-0 z-40 border-b backdrop-blur"
>
  <div class="container flex h-14 items-center gap-6">
    <a href="/" class="flex items-center gap-2 font-semibold tracking-tight">
      <span class="text-primary">soika</span>
    </a>

    <nav class="flex items-center gap-1 text-sm">
      {#each visibleItems as item (item.href)}
        <a
          href={item.href}
          class={cn(
            'flex items-center gap-1.5 rounded-md px-3 py-1.5 font-medium transition-colors',
            isActive(item.href)
              ? 'bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:text-foreground'
          )}
        >
          <item.icon class="size-4" />
          {item.label}
          {#if item.href === '/admin' && pendingApprovals > 0}
            <AlertTriangle
              class="text-primary size-3.5"
              aria-label={`${pendingApprovals} account${pendingApprovals === 1 ? '' : 's'} awaiting approval`}
            />
          {/if}
        </a>
      {/each}
    </nav>

    <div class="ml-auto flex items-center gap-1">
      <Button variant="ghost" size="icon" onclick={toggleMode} aria-label="Toggle theme">
        {#if mode.current === 'dark'}
          <Sun class="size-4" />
        {:else}
          <Moon class="size-4" />
        {/if}
      </Button>

      <DropdownMenu.Root>
        <DropdownMenu.Trigger>
          <Button variant="ghost" size="sm" class="gap-2">
            <UserIcon class="size-4" />
            {user.display_name}
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Content align="end" class="w-48">
          <DropdownMenu.Label>{user.email}</DropdownMenu.Label>
          <DropdownMenu.Separator />
          <DropdownMenu.Item onclick={() => goto('/profile')}>
            <UserIcon class="size-4" />
            Profile
          </DropdownMenu.Item>
          <DropdownMenu.Item onclick={logout}>
            <LogOut class="size-4" />
            Sign out
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Root>
    </div>
  </div>
</header>
