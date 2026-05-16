<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';

  let grafanaUrl = $state<string | null>(null);
  let loading    = $state(true);

  // Grafana kiosk mode hides its own nav/toolbar for a clean embed
  let embedUrl = $derived(
    grafanaUrl
      ? `${grafanaUrl}/d/tuskar-main/tuskar-file-transfer-server?kiosk&refresh=30s&theme=dark`
      : null
  );

  onMount(async () => {
    try {
      const settings = await api.listSettings();
      const entry = settings.find(s => s.key === 'GRAFANA_URL');
      grafanaUrl = (entry?.value && entry.value !== '') ? entry.value : null;
    } catch {
      grafanaUrl = null;
    } finally {
      loading = false;
    }
  });
</script>

<div class="page">
  {#if loading}
    <div class="empty">Loading…</div>
  {:else if embedUrl}
    <iframe
      src={embedUrl}
      title="Tuskar Grafana Dashboard"
      class="frame"
      allowfullscreen
    ></iframe>
    <div class="footer">
      <a href={grafanaUrl} target="_blank" rel="noopener" class="open-link">
        Open Grafana ↗
      </a>
    </div>
  {:else}
    <div class="unconfigured">
      <div class="uc-icon">📊</div>
      <h2>Dashboard not configured</h2>
      <p>Set <code>GRAFANA_URL</code> to your Grafana instance to embed the live dashboard here.</p>
      <p class="hint">If you started Tuskar with <code>docker compose up</code>, Grafana is already running at <code>http://localhost:3001</code>.</p>
      <a href="/settings" class="btn">Configure in Settings →</a>
    </div>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 3.5rem);
    margin: -1.75rem -2rem;
  }

  .frame {
    flex: 1;
    width: 100%;
    border: none;
    background: #0b0d12;
  }

  .footer {
    padding: 0.4rem 1rem;
    background: #0f1117;
    border-top: 1px solid #1e2130;
    text-align: right;
  }
  .open-link {
    font-size: 0.72rem;
    color: #475569;
  }
  .open-link:hover { color: #94a3b8; }

  .empty {
    color: #64748b;
    padding: 3rem 2rem;
  }

  .unconfigured {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    flex: 1;
    padding: 3rem 2rem;
    text-align: center;
  }
  .uc-icon { font-size: 2.5rem; }
  h2 { font-size: 1.1rem; font-weight: 700; color: #e2e8f0; }
  p { font-size: 0.875rem; color: #64748b; max-width: 420px; line-height: 1.5; }
  .hint { font-size: 0.8rem; color: #334155; }
  code { font-family: monospace; background: #1e2130; padding: 0.1rem 0.3rem; border-radius: 3px; color: #94a3b8; }
  .btn {
    margin-top: 0.5rem;
    padding: 0.5rem 1.25rem;
    background: #f97316; color: #0b0d12;
    border: none; border-radius: 6px;
    font-weight: 700; font-size: 0.875rem;
    cursor: pointer; text-decoration: none;
  }
  .btn:hover { background: #fb923c; text-decoration: none; }
</style>
