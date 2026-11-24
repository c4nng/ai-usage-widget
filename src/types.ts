// Mirrors the Rust types serialized over IPC (src-tauri/src/config.rs, usage-core/src/model.rs).

export type ProviderKind = "claude_subscription" | "codex_subscription" | "anthropic_api" | "openai_api";

export const PROVIDER_KINDS: ProviderKind[] = ["claude_subscription", "codex_subscription", "anthropic_api", "openai_api"];

export const isApiKind = (kind: ProviderKind) => kind === "anthropic_api" || kind === "openai_api";

export interface ProviderConfig {
  id: string;
  kind: ProviderKind;
  label: string;
  enabled: boolean;
  credentialsPath?: string | null;
  monthlyBudget?: number | null;
}

export type Theme = "auto" | "dark" | "light";
export type Placement = "edge" | "desktop";

export interface Config {
  refreshMinutes: number;
  autostart: boolean;
  placement: Placement;
  aboveWindows: boolean;
  theme: Theme;
  opacity: number;
  providers: ProviderConfig[];
}

export interface Meter {
  key: string;
  label: string;
  percent: number | null;
  /** Unix seconds. */
  resetsAt: number | null;
  spentUsd: number | null;
  budgetUsd: number | null;
}

export interface Usage {
  plan: string | null;
  meters: Meter[];
}

export type ErrorCode =
  | "not_configured"
  | "credentials_missing"
  | "token_expired"
  | "unauthorized"
  | "rate_limited"
  | "network"
  | "bad_response";

export interface ProviderError {
  code: ErrorCode;
  message: string;
}

export interface ProviderReport {
  id: string;
  label: string;
  kind: ProviderKind;
  /** Unix seconds. */
  fetchedAt: number;
  usage: Usage | null;
  error: ProviderError | null;
  /** `usage` is from an earlier fetch because the latest one failed. */
  stale: boolean;
}
