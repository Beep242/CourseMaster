// Cross-app bridge login — the same mechanism StockMan, TruthSeeker, and
// BPass already use: PortFolio is the source of identity, and this app
// never collects a password of its own. We silently ask PortFolio "is
// there already a signed-in session in this browser?" by exchanging its
// session cookie for a short-lived JWT scoped to CourseMaster
// (aud: "coursemaster"). No token ever appears in a URL.
export interface BridgeUser {
  id: string;
  email: string;
  role: "admin" | "member";
}

export interface BridgeSession {
  accessToken: string;
  user: BridgeUser;
}

const PORTFOLIO_URL = import.meta.env.VITE_PORTFOLIO_URL ?? "https://iambeep.com";
const APP_NAME = "coursemaster";
const STORAGE_KEY = "coursemaster-bridge-session";

export class BridgeAuthError extends Error {}

export async function exchangeBridgeSession(): Promise<BridgeSession> {
  const response = await fetch(`${PORTFOLIO_URL}/auth/token?app=${APP_NAME}`, {
    method: "POST",
    credentials: "include",
  });
  if (!response.ok) {
    throw new BridgeAuthError("No active Beep.dev session — sign in there first");
  }
  return response.json();
}

/// `?next=` is PortFolio's own cross-app login handoff (already used by
/// TruthSeeker/BPass — see PortFolio's `public/script.js` NEXT_ALLOWED_PREFIXES),
/// an exact-prefix allow-list rather than an open redirect. Without it,
/// logging in just leaves the user sitting on iambeep.com with no way back.
export function portfolioLoginUrl(): string {
  const next = encodeURIComponent(window.location.origin);
  return `${PORTFOLIO_URL}/login.html?next=${next}`;
}

export function loadStoredSession(): BridgeSession | null {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as BridgeSession;
  } catch {
    return null;
  }
}

export function storeSession(session: BridgeSession): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(session));
}

export function clearSession(): void {
  localStorage.removeItem(STORAGE_KEY);
}

/// Returns a usable session, refreshing from PortFolio when nothing is
/// cached yet. A cached token that's actually expired is caught by the API
/// returning 401 — the caller (see `api.ts`) clears the cache and retries
/// once rather than this function trying to pre-parse JWT expiry itself.
export async function ensureBridgeSession(): Promise<BridgeSession> {
  const cached = loadStoredSession();
  if (cached) return cached;
  const fresh = await exchangeBridgeSession();
  storeSession(fresh);
  return fresh;
}
