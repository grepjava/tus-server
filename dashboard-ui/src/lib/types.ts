export type UploadStatus =
  | 'Created'
  | 'Uploading'
  | 'Completed'
  | 'Processing'
  | 'Finalized'
  | 'FailedUpload'
  | 'FailedProcessing'
  | 'FailedFinalization'
  | 'Abandoned'
  | 'ConsumedByConcat';

export interface Upload {
  id: string;
  filename: string | null;
  upload_length: number;
  upload_offset: number;
  metadata_json: string | null;
  status: UploadStatus;
  storage_path: string;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
  error_message: string | null;
  length_is_deferred: boolean;
  concat_type: string | null;
  concat_uploads: string | null;
  context_id: string | null;
}

export interface UploadEvent {
  id: string;
  upload_id: string;
  event_type: string;
  message: string | null;
  created_at: string;
}

export interface WebhookConfig {
  id: string;
  name: string;
  url: string;
  has_secret: boolean;
  events: string[];
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface NewWebhookConfig {
  name: string;
  url: string;
  secret: string | null;
  events: string[];
}

export interface UpdateWebhookConfig {
  name: string;
  url: string;
  secret: string | null;
  events: string[];
  enabled: boolean;
}

export interface WebhookDelivery {
  id: string;
  webhook_id: string;
  upload_id: string | null;
  event_type: string;
  payload: string;
  status_code: number | null;
  response_body: string | null;
  error: string | null;
  attempts: number;
  delivered_at: string;
}

export interface AuditEntry {
  id: string;
  created_at: string;
  request_id: string | null;
  actor: string;
  source_ip: string | null;
  method: string;
  path: string;
  upload_id: string | null;
  status_code: number;
}

export interface HealthStatus {
  status: 'ok' | 'degraded';
  db: boolean;
  storage: boolean;
}

export interface User {
  id: string;
  username: string;
  role: 'admin' | 'viewer';
  created_at: string;
}

export interface SessionInfo {
  id: string;
  username: string;
  role: 'admin' | 'viewer';
}

export interface Context {
  id: string;
  slug: string;
  display_name: string;
  storage_prefix: string;
  max_upload_bytes: number | null;
  created_at: string;
  updated_at: string;
}

export interface ContextCreated extends Context {
  api_key: string;
}

export interface SettingEntry {
  key: string;
  label: string;
  description: string;
  category: string;
  input_type: string;
  value: string;
  source: 'default' | 'env' | 'db';
  restart_required: boolean;
  options: string[] | null;
}
