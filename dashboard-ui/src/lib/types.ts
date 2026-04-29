export type UploadStatus =
  | 'Created'
  | 'Uploading'
  | 'Completed'
  | 'Processing'
  | 'Finalized'
  | 'FailedUpload'
  | 'FailedProcessing'
  | 'FailedFinalization'
  | 'Abandoned';

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
  secret: string | null;
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
