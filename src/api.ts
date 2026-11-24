import { invoke } from "@tauri-apps/api/core";
import type { Config, ProviderKind, ProviderReport } from "./types";

// Thin typed wrappers over the Rust commands in src-tauri/src/lib.rs.
export const api = {
  getConfig: () => invoke<Config>("get_config"),
  saveConfig: (config: Config) => invoke<Config>("save_config", { config }),
  getUsage: () => invoke<ProviderReport[]>("get_usage"),
  refreshNow: () => invoke<ProviderReport[]>("refresh_now"),
  hasSecret: (providerId: string) => invoke<boolean>("has_secret", { providerId }),
  setSecret: (providerId: string, secret: string) => invoke<void>("set_secret", { providerId, secret }),
  deleteSecret: (providerId: string) => invoke<void>("delete_secret", { providerId }),
  defaultCredentialsPath: (kind: ProviderKind) => invoke<string | null>("default_credentials_path", { kind }),
  openSettings: () => invoke<void>("open_settings"),
  edgeLayout: (width: number, height: number) => invoke<void>("edge_layout", { width, height }),
  edgeDragStart: () => invoke<void>("edge_drag_start"),
  edgeDrag: (dx: number) => invoke<void>("edge_drag", { dx }),
  edgeDragEnd: () => invoke<void>("edge_drag_end"),
  setTrayLabels: (labels: { toggle: string; refresh: string; settings: string; quit: string }) =>
    invoke<void>("set_tray_labels", { labels }),
};

export const EVENTS = {
  configChanged: "config-changed",
  usageUpdated: "usage-updated",
} as const;
