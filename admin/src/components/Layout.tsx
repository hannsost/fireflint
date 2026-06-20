import { NavLink, Outlet } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import { useAuth, useOrg } from "../auth/AuthContext";
import type { ContentType } from "../types";

export function Layout() {
  const org = useOrg();
  const { user, memberships, logout } = useAuth();
  const role = memberships.find((m) => m.org_id === org)?.role;

  const { data: types } = useQuery({
    queryKey: ["content-types", org],
    queryFn: () => api.get<ContentType[]>(`/api/orgs/${org}/content-types`),
  });

  const navClass = ({ isActive }: { isActive: boolean }) => (isActive ? "active" : "");

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">SiteGraph</div>
        <nav className="nav">
          <div className="nav-section">Inhalte</div>
          {types?.map((t) => (
            <NavLink key={t.id} to={`/content/${t.key}`} className={navClass}>
              {t.name}
            </NavLink>
          ))}
          <div className="nav-section">Verwaltung</div>
          <NavLink to="/websites" className={navClass}>
            Websites
          </NavLink>
          <NavLink to="/tokens" className={navClass}>
            API-Tokens
          </NavLink>
        </nav>
        <div className="spacer" />
        <div className="user-chip">
          <span>{user?.email}</span>
          {role && <span className="badge role">{role}</span>}
        </div>
        <button onClick={logout}>Abmelden</button>
      </aside>
      <main className="main">
        <Outlet />
      </main>
    </div>
  );
}
