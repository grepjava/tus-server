import type { Upload, UploadEvent, WebhookConfig, WebhookDelivery, NewWebhookConfig, UpdateWebhookConfig } from './types';

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

export const api = {
  // Uploads
  listUploads:      ()                        => get<Upload[]>('/uploads'),
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
};
