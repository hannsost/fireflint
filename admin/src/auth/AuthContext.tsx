import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { api, login as apiLogin, tokens } from "../api/client";
import type { Membership, User } from "../types";

interface AuthState {
  user: User | null;
  memberships: Membership[];
  activeOrg: string | null;
  loading: boolean;
  login: (email: string, password: string) => Promise<void>;
  logout: () => void;
}

const AuthCtx = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [memberships, setMemberships] = useState<Membership[]>([]);
  const [activeOrg, setActiveOrg] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const loadMe = useCallback(async () => {
    if (!tokens.access) {
      setLoading(false);
      return;
    }
    try {
      const me = await api.get<{ user: User; memberships: Membership[] }>("/api/me");
      setUser(me.user);
      setMemberships(me.memberships);
      setActiveOrg(me.memberships[0]?.org_id ?? null);
    } catch {
      tokens.clear();
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadMe();
  }, [loadMe]);

  const login = useCallback(
    async (email: string, password: string) => {
      await apiLogin(email, password);
      setLoading(true);
      await loadMe();
    },
    [loadMe],
  );

  const logout = useCallback(() => {
    tokens.clear();
    setUser(null);
    setMemberships([]);
    setActiveOrg(null);
  }, []);

  const value = useMemo(
    () => ({ user, memberships, activeOrg, loading, login, logout }),
    [user, memberships, activeOrg, loading, login, logout],
  );

  return <AuthCtx.Provider value={value}>{children}</AuthCtx.Provider>;
}

export function useAuth() {
  const ctx = useContext(AuthCtx);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}

/** The active org id, asserted present (use inside authed routes). */
export function useOrg(): string {
  const { activeOrg } = useAuth();
  if (!activeOrg) throw new Error("no active org");
  return activeOrg;
}
