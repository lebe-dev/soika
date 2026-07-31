// Thin, exception-safe wrapper around window.localStorage. Storage can throw
// (private browsing, quota exceeded, disabled, or simply absent during SSR
// prerendering) — every call is best-effort and never lets a storage failure
// interrupt the caller.

export function readJson<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? null : (JSON.parse(raw) as T);
  } catch {
    return null;
  }
}

export function writeJson<T>(key: string, value: T): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Best-effort: ignore storage failures.
  }
}
