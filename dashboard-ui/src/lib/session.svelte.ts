import type { SessionInfo } from './types';

let _session = $state<SessionInfo | null>(null);

export function getSession(): SessionInfo | null {
  return _session;
}

export function setSession(s: SessionInfo | null) {
  _session = s;
}
