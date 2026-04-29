<script lang="ts">
  import { page } from '$app/state';
  let { children } = $props();
  const nav = [
    { href: '/',          label: 'Uploads' },
    { href: '/webhooks',  label: 'Webhooks' },
  ];
</script>

<div class="shell">
  <header>
    <a href="/" class="brand">TUS Server</a>
    <nav>
      {#each nav as item}
        <a href={item.href} class:active={page.url.pathname === item.href || (item.href !== '/' && page.url.pathname.startsWith(item.href))}>
          {item.label}
        </a>
      {/each}
    </nav>
  </header>
  <main>
    {@render children()}
  </main>
</div>

<style>
  :global(*, *::before, *::after) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(body) { font-family: system-ui, sans-serif; background: #0f1117; color: #e2e8f0; min-height: 100vh; }
  :global(a) { color: #60a5fa; text-decoration: none; }
  :global(a:hover) { text-decoration: underline; }

  .shell { display: flex; flex-direction: column; min-height: 100vh; }

  header {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 0 1.5rem;
    height: 3rem;
    background: #1e2130;
    border-bottom: 1px solid #2d3148;
  }

  .brand { font-weight: 700; font-size: 1rem; color: #e2e8f0; }

  nav { display: flex; gap: 0.25rem; }
  nav a {
    padding: 0.25rem 0.75rem;
    border-radius: 4px;
    font-size: 0.875rem;
    color: #94a3b8;
    transition: background 0.15s, color 0.15s;
  }
  nav a:hover { background: #2d3148; color: #e2e8f0; text-decoration: none; }
  nav a.active { background: #2d3148; color: #e2e8f0; }

  main { padding: 1.5rem; max-width: 1100px; width: 100%; margin: 0 auto; }
</style>
