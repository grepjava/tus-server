<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';

  interface MetricSample {
    labels: Record<string, string>;
    value: number;
  }
  interface Metric {
    name: string;
    help: string;
    type: string;
    samples: MetricSample[];
  }

  let metrics    = $state<Metric[]>([]);
  let raw        = $state('');
  let loading    = $state(true);
  let error      = $state<string | null>(null);
  let lastFetch  = $state<Date | null>(null);
  let showRaw    = $state(false);
  let grafanaUrl = $state<string | null>(null);

  async function loadGrafanaUrl() {
    try {
      const settings = await api.listSettings();
      const entry = settings.find(s => s.key === 'GRAFANA_URL');
      grafanaUrl = (entry && entry.value) ? entry.value : null;
    } catch { grafanaUrl = null; }
  }

  function parseMetrics(text: string): Metric[] {
    const result: Metric[] = [];
    let current: Metric | null = null;

    for (const line of text.split('\n')) {
      if (line.startsWith('# HELP ')) {
        const rest = line.slice(7);
        const sp = rest.indexOf(' ');
        const name = sp > -1 ? rest.slice(0, sp) : rest;
        const help = sp > -1 ? rest.slice(sp + 1) : '';
        current = { name, help, type: '', samples: [] };
        result.push(current);
      } else if (line.startsWith('# TYPE ')) {
        const parts = line.slice(7).split(' ');
        if (current && parts[0] === current.name) current.type = parts[1] ?? '';
      } else if (line === '# EOF' || line === '') {
        // skip
      } else if (!line.startsWith('#')) {
        // data line: metric_name{labels} value
        const brace = line.indexOf('{');
        const space = line.lastIndexOf(' ');
        if (space < 0) continue;
        const valueStr = line.slice(space + 1);
        const value = parseFloat(valueStr);
        if (isNaN(value)) continue;

        let metricName: string;
        let labels: Record<string, string> = {};

        if (brace > -1 && brace < space) {
          metricName = line.slice(0, brace);
          const labelStr = line.slice(brace + 1, line.indexOf('}'));
          for (const pair of labelStr.split(',')) {
            const eq = pair.indexOf('=');
            if (eq > -1) {
              labels[pair.slice(0, eq).trim()] = pair.slice(eq + 1).replace(/^"|"$/g, '');
            }
          }
        } else {
          metricName = line.slice(0, space);
        }

        // match to current metric (OpenMetrics appends _total for counters)
        const target = result.find(m =>
          metricName === m.name || metricName === m.name + '_total'
        );
        if (target) target.samples.push({ labels, value });
        else if (current) current.samples.push({ labels, value });
      }
    }

    return result.filter(m => m.samples.length > 0);
  }

  function fmt(n: number): string {
    if (n >= 1e9)  return `${(n / 1e9).toFixed(2)} GB`;
    if (n >= 1e6)  return `${(n / 1e6).toFixed(1)} M`;
    if (n >= 1e3)  return `${(n / 1e3).toFixed(1)} K`;
    return String(Math.round(n));
  }

  function fmtBytes(n: number): string {
    if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(2)} GB`;
    if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(1)} MB`;
    if (n >= 1024)      return `${(n / 1024).toFixed(1)} KB`;
    return `${n} B`;
  }

  function sampleValue(m: Metric, labelFilter?: Record<string, string>): number | null {
    const s = m.samples.find(s => {
      if (!labelFilter) return true;
      return Object.entries(labelFilter).every(([k, v]) => s.labels[k] === v);
    });
    return s?.value ?? null;
  }

  // Extract key metrics for summary cards
  let summary = $derived.by(() => {
    const byName = Object.fromEntries(metrics.map(m => [m.name, m]));
    const get = (n: string, lf?: Record<string, string>) => {
      const m = byName[n] ?? byName[n + '_total'];
      return m ? (sampleValue(m, lf) ?? 0) : 0;
    };
    return {
      created:    get('tus_uploads_created'),
      completed:  get('tus_uploads_completed'),
      failures:   get('tus_processing_failures'),
      bytes:      get('tus_bytes_uploaded'),
      active:     get('tus_active_uploads'),
      processing: get('tus_processing_uploads'),
      webhookOk:  get('tus_webhook_deliveries', { outcome: 'success' }),
      webhookErr: get('tus_webhook_deliveries', { outcome: 'failure' }),
    };
  });

  async function load() {
    try {
      raw = await api.metrics();
      metrics = parseMetrics(raw);
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
      lastFetch = new Date();
    }
  }

  onMount(() => {
    load();
    loadGrafanaUrl();
    const t = setInterval(load, 15_000);
    return () => clearInterval(t);
  });

  function reltime(d: Date) {
    const s = Math.floor((Date.now() - d.getTime()) / 1000);
    return s < 5 ? 'just now' : `${s}s ago`;
  }
</script>

<div class="page-header">
  <h1>Metrics</h1>
  <div class="actions">
    {#if lastFetch}<span class="last">Updated {reltime(lastFetch)}</span>{/if}
    {#if grafanaUrl}
      <a href={grafanaUrl} target="_blank" rel="noopener" class="btn btn-grafana">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" style="vertical-align:-2px;margin-right:4px"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 14H9V8h2v8zm4 0h-2V8h2v8z"/></svg>
        Open in Grafana
      </a>
    {:else}
      <a href="/settings#GRAFANA_URL" class="btn btn-grafana-dim" title="Configure GRAFANA_URL in Settings">
        Connect Grafana
      </a>
    {/if}
    <button class="btn" onclick={load} disabled={loading}>↻ Refresh</button>
    <button class="btn" onclick={() => showRaw = !showRaw}>{showRaw ? 'Hide' : 'Show'} raw</button>
  </div>
</div>

{#if error}
  <div class="alert">{error}</div>
{:else if loading}
  <div class="empty">Loading metrics…</div>
{:else}
  <!-- ── summary cards ── -->
  <div class="cards">
    <div class="card blue">
      <span class="card-n">{fmt(summary.created)}</span>
      <span class="card-l">Uploads created</span>
    </div>
    <div class="card green">
      <span class="card-n">{fmt(summary.completed)}</span>
      <span class="card-l">Uploads completed</span>
    </div>
    <div class="card cyan">
      <span class="card-n">{fmtBytes(summary.bytes)}</span>
      <span class="card-l">Bytes uploaded</span>
    </div>
    <div class="card yellow">
      <span class="card-n">{fmt(summary.active)}</span>
      <span class="card-l">Active uploads</span>
    </div>
    <div class="card amber">
      <span class="card-n">{fmt(summary.processing)}</span>
      <span class="card-l">Processing</span>
    </div>
    <div class="card red">
      <span class="card-n">{fmt(summary.failures)}</span>
      <span class="card-l">Processing failures</span>
    </div>
    <div class="card green">
      <span class="card-n">{fmt(summary.webhookOk)}</span>
      <span class="card-l">Webhooks delivered</span>
    </div>
    <div class="card red">
      <span class="card-n">{fmt(summary.webhookErr)}</span>
      <span class="card-l">Webhook failures</span>
    </div>
  </div>

  <!-- ── all metrics table ── -->
  <h2>All metrics</h2>
  <div class="metrics-list">
    {#each metrics as m}
      <div class="metric">
        <div class="metric-header">
          <span class="metric-name">{m.name}</span>
          <span class="metric-type">{m.type}</span>
        </div>
        {#if m.help}<div class="metric-help">{m.help}</div>{/if}
        <div class="samples">
          {#each m.samples as s}
            <div class="sample">
              {#if Object.keys(s.labels).length > 0}
                <span class="sample-labels">
                  {#each Object.entries(s.labels) as [k, v]}
                    <span class="label-pair"><span class="lk">{k}</span>=<span class="lv">"{v}"</span></span>
                  {/each}
                </span>
              {/if}
              <span class="sample-value">{s.value}</span>
            </div>
          {/each}
        </div>
      </div>
    {/each}
  </div>

  {#if showRaw}
    <h2>Raw output</h2>
    <pre class="raw">{raw}</pre>
  {/if}
{/if}

<style>
  .page-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem; flex-wrap: wrap; gap: 0.75rem; }
  h1 { font-size: 1.25rem; font-weight: 700; }
  h2 { font-size: 0.85rem; font-weight: 600; color: #64748b; text-transform: uppercase; letter-spacing: 0.06em; margin: 1.5rem 0 0.75rem; }
  .actions { display: flex; align-items: center; gap: 0.5rem; }
  .last { font-size: 0.75rem; color: #475569; }
  .btn { padding: 0.375rem 0.875rem; background: #2d3148; color: #e2e8f0; border: 1px solid #3d4263; border-radius: 6px; cursor: pointer; font-size: 0.8rem; }
  .btn:hover { background: #363b5a; text-decoration: none; }
  .btn:disabled { opacity: 0.5; cursor: default; }
  .btn-grafana { background: #7e2626; color: #fca5a5; border-color: #ef4444; font-weight: 600; }
  .btn-grafana:hover { background: #991b1b; text-decoration: none; }
  .btn-grafana-dim { background: #1e2130; color: #475569; border-color: #2d3148; }
  .btn-grafana-dim:hover { background: #252a3d; color: #64748b; text-decoration: none; }

  .alert { background: #3b1a1a; border: 1px solid #7f1d1d; padding: 0.75rem 1rem; border-radius: 6px; margin-bottom: 1rem; font-size: 0.875rem; }
  .empty { color: #64748b; padding: 2rem 0; }

  /* ── summary cards ── */
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 0.75rem; margin-bottom: 1.5rem; }
  .card {
    background: #1e2130; border: 1px solid #2d3148; border-radius: 8px;
    padding: 0.875rem 1rem; display: flex; flex-direction: column; gap: 0.25rem;
    position: relative; overflow: hidden;
  }
  .card::after { content: ''; position: absolute; top: 0; left: 0; right: 0; height: 2px; }
  .card.blue   { --c: #3b82f6; } .card.blue::after   { background: #3b82f6; }
  .card.green  { --c: #4ade80; } .card.green::after  { background: #4ade80; }
  .card.cyan   { --c: #22d3ee; } .card.cyan::after   { background: #22d3ee; }
  .card.yellow { --c: #facc15; } .card.yellow::after { background: #facc15; }
  .card.amber  { --c: #fb923c; } .card.amber::after  { background: #fb923c; }
  .card.red    { --c: #f87171; } .card.red::after    { background: #f87171; }
  .card-n { font-size: 1.4rem; font-weight: 700; color: var(--c, #e2e8f0); line-height: 1; }
  .card-l { font-size: 0.68rem; text-transform: uppercase; letter-spacing: 0.05em; color: #64748b; }

  /* ── metrics list ── */
  .metrics-list { display: flex; flex-direction: column; gap: 0.5rem; }
  .metric { background: #1e2130; border: 1px solid #2d3148; border-radius: 8px; padding: 0.875rem 1rem; }
  .metric-header { display: flex; align-items: center; gap: 0.75rem; margin-bottom: 0.2rem; }
  .metric-name { font-family: monospace; font-size: 0.85rem; color: #e2e8f0; font-weight: 600; }
  .metric-type { font-size: 0.65rem; text-transform: uppercase; letter-spacing: 0.05em; color: #475569; background: #2d3148; padding: 0.1rem 0.35rem; border-radius: 3px; }
  .metric-help { font-size: 0.775rem; color: #64748b; margin-bottom: 0.5rem; }

  .samples { display: flex; flex-direction: column; gap: 0.2rem; }
  .sample { display: flex; align-items: center; justify-content: space-between; font-size: 0.8rem; padding: 0.2rem 0; }
  .sample-labels { display: flex; gap: 0.5rem; flex-wrap: wrap; font-family: monospace; }
  .label-pair { font-size: 0.72rem; }
  .lk { color: #94a3b8; }
  .lv { color: #7dd3fc; }
  .sample-value { font-family: monospace; font-weight: 700; color: #e2e8f0; }

  .raw { background: #161824; border: 1px solid #2d3148; border-radius: 8px; padding: 1rem; font-size: 0.75rem; font-family: monospace; color: #94a3b8; overflow-x: auto; white-space: pre; line-height: 1.6; }
</style>
