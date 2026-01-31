/** Session entry stored in the session index */
export interface SessionEntry {
  sessionId: string;
  updatedAt: number;
  sessionFile: string;
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
  model: string;
  compactionCount: number;
  memoryFlushAt?: number;
  memoryFlushCompactionCount?: number;
  label?: string;
  channel?: string;
}

/** JSONL message types in a session transcript */
export type TranscriptLineType = "session" | "message" | "tool_result" | "compaction";

/** Session header (first line of JSONL file) */
export interface SessionHeader {
  type: "session";
  version: string;
  sessionId: string;
  timestamp: string;
}

/** Message entry in transcript */
export interface TranscriptMessage {
  type: "message";
  timestamp: string;
  message: {
    role: "user" | "assistant" | "system" | "tool";
    content: Array<{ type: string; text?: string; [key: string]: unknown }>;
    usage?: {
      inputTokens: number;
      outputTokens: number;
    };
  };
}

/** Compaction marker in transcript */
export interface CompactionMarker {
  type: "compaction";
  timestamp: string;
  compactionCount: number;
  summary: string;
  preservedMessages: number;
}

/** Any line in a JSONL transcript */
export type TranscriptLine = SessionHeader | TranscriptMessage | CompactionMarker;

/** Session state for the current active session */
export interface SessionState {
  sessionId: string;
  entry: SessionEntry;
  messages: TranscriptMessage[];
  isActive: boolean;
}

/** Session configuration */
export interface SessionConfig {
  /** Max tokens before triggering compaction (default: 100000) */
  maxContextTokens: number;
  /** Tokens to preserve from the end during compaction (default: 20000) */
  preserveRecentTokens: number;
  /** Enable memory flush before compaction (default: true) */
  memoryFlushEnabled: boolean;
}

export const DEFAULT_SESSION_CONFIG: SessionConfig = {
  maxContextTokens: 100000,
  preserveRecentTokens: 20000,
  memoryFlushEnabled: true,
};
