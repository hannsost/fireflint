// Typed-ish fetch client with token storage and one-shot refresh (WP1.7).
//
// Tokens live in localStorage. On a 401 the client tries the refresh token
// once; if that fails it clears the session and the caller redirects to login.

const API_URL = (import.meta.env.VITE_API_URL as string) || "http://localhost:8080";

const ACCESS_KEY = "sg.access";
const REFRESH_KEY = "sg.refresh";

export const tokens = {
  get access() {
    return localStorage.getItem(ACCESS_KEY);
  },
  get refresh() {
    return localStorage.getItem(REFRESH_KEY);
  },
  set(access: string, refresh?: string) {
    localStorage.setItem(ACCESS_KEY, access);
    if (refresh) localStorage.setItem(REFRESH_KEY, refresh);
  },
  clear() {
    localStorage.removeItem(ACCESS_KEY);
    localStorage.removeItem(REFRESH_KEY);
  },
};

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

async function rawRequest(path: string, init: RequestInit): Promise<Response> {
  return fetch(`${API_URL}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(tokens.access ? { authorization: `Bearer ${tokens.access}` } : {}),
      ...(init.headers || {}),
    },
  });
}

async function tryRefresh(): Promise<boolean> {
  if (!tokens.refresh) return false;
  const res = await fetch(`${API_URL}/auth/refresh`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ refresh_token: tokens.refresh }),
  });
  if (!res.ok) return false;
  const body = await res.json();
  tokens.set(body.access_token);
  return true;
}

async function parse<T>(res: Response): Promise<T> {
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  const body = text ? JSON.parse(text) : undefined;
  if (!res.ok) {
    throw new ApiError(res.status, body?.error || res.statusText);
  }
  return body as T;
}

export async function request<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  let res = await rawRequest(path, init);
  if (res.status === 401 && tokens.refresh) {
    if (await tryRefresh()) {
      res = await rawRequest(path, init);
    } else {
      tokens.clear();
    }
  }
  return parse<T>(res);
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: "POST", body: body ? JSON.stringify(body) : undefined }),
  put: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: "PUT", body: body ? JSON.stringify(body) : undefined }),
  del: <T>(path: string) => request<T>(path, { method: "DELETE" }),
};

// Login is special: it sets tokens and is not itself authorized.
export async function login(email: string, password: string) {
  const res = await fetch(`${API_URL}/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  const body = await parse<{ access_token: string; refresh_token: string; user: unknown }>(res);
  tokens.set(body.access_token, body.refresh_token);
  return body;
}
