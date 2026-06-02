// Client-side stacktrace extraction from a raw Sentry event payload.
//
// The backend stores and returns each event's payload exactly as the SDK sent
// it (`SoikaEvent.payload`, typed `unknown`). soika does NO server-side
// demangling or source-map resolution — the viewer renders frames exactly as
// reported. This module mirrors the backend's `NormalizedEvent` resolution
// (src/domain/stacktrace.rs) so the in-app frame is shown first with
// expand/collapse for the full trace and any source-context lines.

/** A single stack frame, mirroring the Sentry frame interface. */
export interface Frame {
  module?: string;
  function?: string;
  filename?: string;
  abs_path?: string;
  lineno?: number;
  colno?: number;
  in_app?: boolean;
  context_line?: string;
  pre_context: string[];
  post_context: string[];
}

/** A parsed stacktrace. Frames are stored oldest-first (crashing frame last). */
export interface Stacktrace {
  frames: Frame[];
}

/** A single exception entry: type + value + optional stacktrace. */
export interface ExceptionEntry {
  type?: string;
  value?: string;
  module?: string;
  stacktrace?: Stacktrace;
}

/** The display-relevant slice of an event payload. */
export interface ParsedEvent {
  exceptionType?: string;
  exceptionValue?: string;
  message?: string;
  level?: string;
  platform?: string;
  exceptions: ExceptionEntry[];
  stacktrace?: Stacktrace;
}

type Json = Record<string, unknown>;

function asObject(value: unknown): Json | undefined {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value as Json;
  return undefined;
}

function stringField(obj: Json | undefined, key: string): string | undefined {
  const v = obj?.[key];
  if (typeof v !== 'string') return undefined;
  const trimmed = v.trim();
  return trimmed.length > 0 ? trimmed : undefined;
}

function intField(obj: Json, key: string): number | undefined {
  const v = obj[key];
  if (typeof v === 'number' && Number.isFinite(v)) return Math.trunc(v);
  if (typeof v === 'string') {
    const n = Number.parseInt(v.trim(), 10);
    if (Number.isFinite(n)) return n;
  }
  return undefined;
}

function stringArray(obj: Json, key: string): string[] {
  const v = obj[key];
  if (!Array.isArray(v)) return [];
  return v.filter((x): x is string => typeof x === 'string');
}

function parseFrame(raw: unknown): Frame {
  const obj = asObject(raw) ?? {};
  return {
    module: stringField(obj, 'module'),
    function: stringField(obj, 'function'),
    filename: stringField(obj, 'filename'),
    abs_path: stringField(obj, 'abs_path'),
    lineno: intField(obj, 'lineno'),
    colno: intField(obj, 'colno'),
    in_app: typeof obj.in_app === 'boolean' ? obj.in_app : undefined,
    context_line: stringField(obj, 'context_line'),
    pre_context: stringArray(obj, 'pre_context'),
    post_context: stringArray(obj, 'post_context')
  };
}

function parseStacktrace(raw: unknown): Stacktrace | undefined {
  const obj = asObject(raw);
  if (!obj) return undefined;
  const frames = Array.isArray(obj.frames) ? obj.frames.map(parseFrame) : [];
  if (frames.length === 0) return undefined;
  return { frames };
}

function parseExceptions(payload: Json): ExceptionEntry[] {
  const values = asObject(payload.exception)?.values;
  if (!Array.isArray(values)) return [];
  return values.map((raw) => {
    const obj = asObject(raw) ?? {};
    return {
      type: stringField(obj, 'type'),
      value: stringField(obj, 'value'),
      module: stringField(obj, 'module'),
      stacktrace: parseStacktrace(obj.stacktrace)
    };
  });
}

/** Resolve the most relevant stacktrace, mirroring the backend resolution order. */
function resolveStacktrace(payload: Json, exceptions: ExceptionEntry[]): Stacktrace | undefined {
  const thrown = exceptions[exceptions.length - 1];
  if (thrown?.stacktrace) return thrown.stacktrace;
  for (const exc of exceptions) {
    if (exc.stacktrace) return exc.stacktrace;
  }

  const threads = asObject(payload.threads)?.values;
  if (Array.isArray(threads)) {
    const crashed = threads.find((t) => asObject(t)?.crashed === true);
    for (const thread of [crashed, ...threads]) {
      const st = parseStacktrace(asObject(thread)?.stacktrace);
      if (st) return st;
    }
  }

  return parseStacktrace(payload.stacktrace);
}

/** A formatted message string from `message` (string or object) or `logentry`. */
function parseMessage(payload: Json): string | undefined {
  const message = payload.message;
  if (typeof message === 'string') {
    const trimmed = message.trim();
    if (trimmed) return trimmed;
  }
  const fromMessage = messageObject(asObject(message));
  if (fromMessage) return fromMessage;
  return messageObject(asObject(payload.logentry));
}

function messageObject(obj: Json | undefined): string | undefined {
  return stringField(obj, 'formatted') ?? stringField(obj, 'message');
}

/** Parse the display-relevant slice of a raw Sentry event payload. */
export function parseEvent(payload: unknown): ParsedEvent {
  const obj = asObject(payload) ?? {};
  const exceptions = parseExceptions(obj);
  const thrown = exceptions[exceptions.length - 1];
  return {
    exceptionType: thrown?.type,
    exceptionValue: thrown?.value,
    message: parseMessage(obj),
    level: stringField(obj, 'level'),
    platform: stringField(obj, 'platform'),
    exceptions,
    stacktrace: resolveStacktrace(obj, exceptions)
  };
}

/** True when the frame is explicitly marked application code (matches backend). */
export function isInApp(frame: Frame): boolean {
  return frame.in_app === true;
}

/** Whether the frame carries any source-context lines. */
export function hasSourceContext(frame: Frame): boolean {
  return (
    frame.context_line !== undefined ||
    frame.pre_context.length > 0 ||
    frame.post_context.length > 0
  );
}

/** The most specific file label available (`filename`, else `abs_path`). */
export function frameFile(frame: Frame): string | undefined {
  return frame.filename ?? frame.abs_path;
}

/** A best-effort location string for display: `module in function` etc. */
export function frameLocation(frame: Frame): string | undefined {
  if (frame.function) {
    if (frame.module) return `${frame.module} in ${frame.function}`;
    const file = frameFile(frame);
    if (file) return `${file} in ${frame.function}`;
    return frame.function;
  }
  const file = frameFile(frame);
  if (file) return frame.lineno !== undefined ? `${file}:${frame.lineno}` : file;
  return frame.module;
}

/** The most relevant frame: last in-app frame, else the last frame overall. */
export function mostRelevantFrame(st: Stacktrace): Frame | undefined {
  for (let i = st.frames.length - 1; i >= 0; i--) {
    if (isInApp(st.frames[i])) return st.frames[i];
  }
  return st.frames[st.frames.length - 1];
}

/**
 * Frames ordered for display: crashing (most relevant) frame first, then the
 * rest oldest-last. We reverse the SDK's oldest-first ordering so the viewer
 * leads with the in-app frame the user most likely cares about.
 */
export function framesForDisplay(st: Stacktrace): Frame[] {
  return [...st.frames].reverse();
}
