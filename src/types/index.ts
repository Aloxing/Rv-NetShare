// Shared TypeScript types between the Rust backend and the Vue frontend.
// These mirror the structs defined in src-tauri/src/lib.rs.

export type ShareKind = 'file' | 'folder';

export interface ShareSession {
  id: string;
  name: string;
  path: string;
  kind: ShareKind;
  size: number;
  total_bytes: number;
  created_at: number;
}

export interface SiteSession extends ShareSession {
  port: number;
}

export interface AccessRecord {
  id: string;
  share_id: string;
  share_name: string;
  path: string;
  bytes: number;
  timestamp: number;
  peer: string;
  user_agent: string | null;
  status: string;
}

export interface InitialState {
  local_ip: string;
  hostname: string;
  port: number;
  save_dir: string;
  shares: ShareSession[];
  sites: SiteSession[];
  share_port: number | null;
  site_port: number | null;
  ngrok_authtoken: string | null;
  history: AccessRecord[];
}

export type Tab = 'shares' | 'sites' | 'history' | 'settings';

// Result of Tauri's drag-drop event when enabled. On Linux/Windows the
// dropped File objects expose a path property pointing at the real path.
export interface DroppedFile {
  name: string;
  path: string;
}

// Result of `check_paths` and `read_file_meta` on the Rust side.
export interface PathCheck {
  path: string;
  valid: boolean;
  exists: boolean;
  is_dir: boolean;
  is_file: boolean;
  size: number;
  basename: string;
}
