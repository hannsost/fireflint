import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "../api/client";
import { useAuth, useOrg } from "../auth/AuthContext";
import type { Website } from "../types";

export function WebsitesPage() {
  const org = useOrg();
  const qc = useQueryClient();
  const { memberships } = useAuth();
  const isOwner = memberships.find((m) => m.org_id === org)?.role === "owner";

  const { data: websites } = useQuery({
    queryKey: ["websites", org],
    queryFn: () => api.get<Website[]>(`/api/orgs/${org}/websites`),
  });

  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [domain, setDomain] = useState("");
  const [error, setError] = useState<string | null>(null);

  const createMut = useMutation({
    mutationFn: () =>
      api.post(`/api/orgs/${org}/websites`, { name, slug, domain: domain || null }),
    onSuccess: () => {
      setName("");
      setSlug("");
      setDomain("");
      setError(null);
      qc.invalidateQueries({ queryKey: ["websites", org] });
    },
    onError: (e) => setError(e instanceof ApiError ? e.message : "Fehler beim Anlegen."),
  });

  return (
    <>
      <div className="topbar">
        <h1>Websites</h1>
      </div>

      <div className="panel">
        {websites && websites.length === 0 && <p className="muted">Noch keine Websites.</p>}
        {websites && websites.length > 0 && (
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Slug</th>
                <th>Domain</th>
              </tr>
            </thead>
            <tbody>
              {websites.map((w) => (
                <tr key={w.id}>
                  <td>{w.name}</td>
                  <td className="code">{w.slug}</td>
                  <td className="muted">{w.domain ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {isOwner && (
        <div className="panel">
          <h2>Neue Website</h2>
          <div className="field">
            <label htmlFor="w-name">Name</label>
            <input id="w-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="field">
            <label htmlFor="w-slug">Slug</label>
            <input id="w-slug" value={slug} onChange={(e) => setSlug(e.target.value)} placeholder="z.B. karriere" />
          </div>
          <div className="field">
            <label htmlFor="w-domain">Domain (optional)</label>
            <input id="w-domain" value={domain} onChange={(e) => setDomain(e.target.value)} placeholder="karriere.firma.de" />
          </div>
          {error && <p className="error">{error}</p>}
          <div className="actions">
            <button
              className="primary"
              onClick={() => createMut.mutate()}
              disabled={createMut.isPending || !name || !slug}
            >
              Anlegen
            </button>
          </div>
        </div>
      )}
    </>
  );
}
