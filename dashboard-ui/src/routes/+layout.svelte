<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { getSession, setSession } from '$lib/session.svelte';

  let { children } = $props();

  let authChecked = $state(false);

  const nav = [
    { href: '/',           label: 'Uploads',    icon: '⬆',  adminOnly: false },
    { href: '/contexts',   label: 'Contexts',   icon: '⬡',   adminOnly: true  },
    { href: '/webhooks',   label: 'Webhooks',   icon: '🔗',  adminOnly: false },
    { href: '/audit',      label: 'Audit Log',  icon: '📋',  adminOnly: false },
    { href: '/metrics',    label: 'Metrics',    icon: '📊',  adminOnly: false },
    { href: '/dashboard',  label: 'Dashboard',  icon: '📈',  adminOnly: false },
    { href: '/health',     label: 'Health',     icon: '❤',   adminOnly: false },
    { href: '/settings',   label: 'Settings',   icon: '⚙',   adminOnly: true  },
    { href: '/users',      label: 'Users',      icon: '👤',  adminOnly: true  },
  ];

  function isActive(href: string) {
    if (href === '/') return page.url.pathname === '/';
    return page.url.pathname.startsWith(href);
  }

  let session = $derived(getSession());
  let visibleNav = $derived(nav.filter(n => !n.adminOnly || session?.role === 'admin'));

  onMount(async () => {
    if (page.url.pathname.startsWith('/login')) {
      authChecked = true;
      return;
    }
    try {
      const s = await api.me();
      setSession(s);
      authChecked = true;
    } catch {
      // Don't set authChecked — keep splash visible until navigation lands on /login.
      // The template's first branch checks pathname so the login page renders correctly.
      await goto('/login', { replaceState: true });
      authChecked = true;
    }
  });

  async function logout() {
    await api.logout().catch(() => {});
    setSession(null);
    goto('/login');
  }
</script>

{#if page.url.pathname === '/login'}
  {@render children()}
{:else if !authChecked}
  <div class="splash">
    <img src="/tuskar.png" alt="Tuskar" class="splash-logo" />
  </div>
{:else}
  <div class="shell">
    <aside class="sidebar">
      <a href="/" class="brand">
        <img src="/tuskar.png" alt="Tuskar" class="logo" />
      </a>

      <nav>
        {#each visibleNav as item}
          <a href={item.href} class="nav-item" class:active={isActive(item.href)}>
            <span class="nav-icon">{item.icon}</span>
            <span class="nav-label">{item.label}</span>
          </a>
        {/each}
      </nav>

      <div class="sidebar-footer">
        {#if session}
          <div class="user-row">
            <span class="user-name">{session.username}</span>
            <span class="user-role">{session.role}</span>
          </div>
          <button class="logout-btn" onclick={logout}>Sign out</button>
        {/if}
        <span class="version">TUS 1.0.0</span>
      </div>
    </aside>

    <div class="content-area">
      <main>
        {@render children()}
      </main>
    </div>
  </div>
{/if}

<style>
  :global(*, *::before, *::after) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(body) {
    font-family: system-ui, -apple-system, 'Segoe UI', sans-serif;
    background: #0b0d12;
    color: #e2e8f0;
    min-height: 100vh;
  }
  :global(a) { color: #f97316; text-decoration: none; }
  :global(a:hover) { text-decoration: none; color: #fb923c; }
  :global(::-webkit-scrollbar) { width: 6px; height: 6px; }
  :global(::-webkit-scrollbar-track) { background: #0b0d12; }
  :global(::-webkit-scrollbar-thumb) { background: #2d3148; border-radius: 3px; }

  /* splash screen while checking auth */
  .splash {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: #0b0d12;
  }
  .splash-logo {
    width: 64px; height: 64px;
    object-fit: contain;
    border-radius: 12px;
    opacity: 0.6;
    animation: pulse 1.5s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; transform: scale(0.96); }
    50%       { opacity: 0.9; transform: scale(1.04); }
  }

  .shell { display: flex; min-height: 100vh; }

  /* ── sidebar ── */
  .sidebar {
    width: 220px;
    min-width: 220px;
    background: #0f1117;
    border-right: 1px solid #1e2130;
    display: flex;
    flex-direction: column;
    position: sticky;
    top: 0;
    height: 100vh;
    overflow-y: auto;
  }

  .brand {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid #1e2130;
    text-decoration: none;
  }
  .brand:hover { text-decoration: none; }

  .logo {
    width: 100%;
    height: auto;
    object-fit: contain;
    border-radius: 8px;
    display: block;
  }

  .brand-text {
    font-size: 1.1rem;
    font-weight: 800;
    letter-spacing: -0.01em;
    color: #f97316;
  }

  nav {
    flex: 1;
    padding: 0.75rem 0.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.625rem;
    padding: 0.5rem 0.625rem;
    border-radius: 6px;
    font-size: 0.875rem;
    color: #64748b;
    transition: background 0.12s, color 0.12s;
    text-decoration: none;
  }
  .nav-item:hover { background: #1e2130; color: #94a3b8; text-decoration: none; }
  .nav-item.active { background: #1e2130; color: #f97316; }

  .nav-icon { font-size: 1rem; width: 1.25rem; text-align: center; flex-shrink: 0; }
  .nav-label { font-weight: 500; }

  .sidebar-footer {
    padding: 0.875rem 1rem;
    border-top: 1px solid #1e2130;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .user-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.375rem;
  }
  .user-name {
    font-size: 0.78rem;
    font-weight: 600;
    color: #94a3b8;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .user-role {
    font-size: 0.62rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0.1rem 0.35rem;
    border-radius: 3px;
    background: #1e2130;
    color: #475569;
    flex-shrink: 0;
  }

  .logout-btn {
    width: 100%;
    padding: 0.35rem 0.5rem;
    background: transparent;
    border: 1px solid #2d3148;
    border-radius: 5px;
    color: #64748b;
    font-size: 0.75rem;
    cursor: pointer;
    text-align: left;
    transition: background 0.12s, color 0.12s;
  }
  .logout-btn:hover { background: #1e2130; color: #94a3b8; }

  .version {
    font-size: 0.7rem;
    color: #334155;
    font-family: monospace;
  }

  /* ── content ── */
  .content-area {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  main {
    flex: 1;
    padding: 1.75rem 2rem;
    max-width: 1200px;
    width: 100%;
    display: flex;
    flex-direction: column;
  }

  @media (max-width: 640px) {
    .sidebar { width: 56px; min-width: 56px; }
    .brand-text, .nav-label, .user-row, .logout-btn, .version { display: none; }
    .brand { padding: 0.875rem; justify-content: center; }
    .logo { max-width: 32px; border-radius: 6px; }
    .nav-item { justify-content: center; padding: 0.5rem; }
    main { padding: 1rem; }
  }
</style>
