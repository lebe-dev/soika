<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { auth, ApiError, type User } from '$lib/api';
  import { authStore } from '$lib/stores/auth.svelte';
  import { toast } from '$lib/components/ui/sonner';
  import { Button } from '$lib/components/ui/button';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
  import { cn } from '$lib/utils';
  import LayoutDashboard from '@lucide/svelte/icons/layout-dashboard';
  import Users from '@lucide/svelte/icons/users';
  import Shield from '@lucide/svelte/icons/shield';
  import UserIcon from '@lucide/svelte/icons/user';
  import LogOut from '@lucide/svelte/icons/log-out';

  let { user }: { user: User } = $props();

  type NavItem = { href: string; label: string; icon: typeof LayoutDashboard; adminOnly?: boolean };

  const items: NavItem[] = [
    { href: '/', label: 'Dashboard', icon: LayoutDashboard },
    { href: '/teams', label: 'Teams', icon: Users },
    { href: '/profile', label: 'Profile', icon: UserIcon },
    { href: '/admin', label: 'Admin', icon: Shield, adminOnly: true }
  ];

  const visibleItems = $derived(items.filter((item) => !item.adminOnly || user.is_admin));

  function isActive(href: string): boolean {
    const path = $page.url.pathname;
    if (href === '/') return path === '/';
    return path === href || path.startsWith(`${href}/`);
  }

  async function logout() {
    try {
      await auth.logout();
    } catch (err) {
      // Even if the server call fails, drop local state and bounce to login.
      if (!(err instanceof ApiError)) throw err;
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
        </a>
      {/each}
    </nav>

    <div class="ml-auto">
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
