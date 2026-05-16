import type { Upload, UploadEvent, WebhookConfig, WebhookDelivery, NewWebhookConfig, UpdateWebhookConfig, AuditEntry, HealthStatus, SettingEntry, User, SessionInfo, Context, ContextCreated } from './types';

const BASE = '/api';

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

async function post<T = void>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: body ? { 'Content-Type': 'application/json' } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  if (res.status === 204) return undefined as T;
  return res.json();
}

async function put<T = void>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  if (res.status === 204) return undefined as T;
  return res.json();
}

async function del(path: string): Promise<void> {
  const res = await fetch(`${BASE}${path}`, { method: 'DELETE' });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
}

function pageQuery(params?: { limit?: number; offset?: number }): string {
  const q = new URLSearchParams();
  if (params?.limit != null)  q.set('limit',  String(params.limit));
  if (params?.offset != null) q.set('offset', String(params.offset));
  const s = q.toString();
  return s ? `?${s}` : '';
}

export const api = {
  // Uploads
  listUploads:      (p?: { limit?: number; offset?: number }) => get<Upload[]>(`/uploads${pageQuery(p)}`),
  getUpload:        (id: string)              => get<Upload>(`/uploads/${id}`),
  getEvents:        (id: string)              => get<UploadEvent[]>(`/uploads/${id}/events`),
  retryProcessing:  (id: string)              => post(`/uploads/${id}/retry-processing`),
  markAbandoned:    (id: string)              => post(`/uploads/${id}/mark-abandoned`),
  deleteUpload:     (id: string)              => del(`/uploads/${id}`),
  purgeUploads:     (ids: string[])           => post<{ deleted: number }>('/uploads/purge', { ids }),
  streamEvents:     (id: string)              => new EventSource(`${BASE}/uploads/${id}/stream`),

  // Webhooks
  listWebhooks:         ()                                        => get<WebhookConfig[]>('/webhooks'),
  createWebhook:        (body: NewWebhookConfig)                  => post<WebhookConfig>('/webhooks', body),
  updateWebhook:        (id: string, body: UpdateWebhookConfig)   => put<WebhookConfig>(`/webhooks/${id}`, body),
  deleteWebhook:        (id: string)                              => del(`/webhooks/${id}`),
  listDeliveries:       (id: string)                              => get<WebhookDelivery[]>(`/webhooks/${id}/deliveries`),

  // Health
  health: () => get<HealthStatus>('/health'),

  // Metrics (Prometheus text format from root /metrics endpoint)
  metrics: () => fetch('/metrics').then(r => {
    if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
    return r.text();
  }),

  // Audit log
  listAudit: (p?: { limit?: number; offset?: number }) => get<AuditEntry[]>(`/audit${pageQuery(p)}`),

  // Settings
  listSettings:   ()                                    => get<SettingEntry[]>('/settings'),
  updateSetting:  (key: string, value: string)          => put<SettingEntry>(`/settings/${key}`, { value }),
  deleteSetting:  (key: string)                         => del(`/settings/${key}`),

  // Auth
  login:   (username: string, password: string) => post<SessionInfo>('/auth/login', { username, password }),
  logout:  ()                                   => post('/auth/logout'),
  me:      ()                                   => get<SessionInfo>('/auth/me'),

  // Users
  listUsers:      ()                                                     => get<User[]>('/users'),
  createUser:     (username: string, password: string, role: string)     => post<User>('/users', { username, password, role }),
  deleteUser:     (id: string)                                           => del(`/users/${id}`),
  changePassword: (id: string, newPassword: string, currentPassword?: string) =>
    put(`/users/${id}/password`, { new_password: newPassword, current_password: currentPassword }),

  // Contexts
  listContexts:    ()                                                                       => get<Context[]>('/contexts'),
  createContext:   (slug: string, display_name: string, max_upload_bytes?: number | null)  => post<ContextCreated>('/contexts', { slug, display_name, max_upload_bytes }),
  updateContext:   (id: string, body: { display_name?: string; max_upload_bytes?: number | null }) => put<Context>(`/contexts/${id}`, body),
  deleteContext:   (id: string)                                                             => del(`/contexts/${id}`),
  rotateContextKey:(id: string)                                                             => post<{ api_key: string }>(`/contexts/${id}/rotate-key`),
};
