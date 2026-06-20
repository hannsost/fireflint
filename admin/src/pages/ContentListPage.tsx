import { Link, useParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";
import { useOrg } from "../auth/AuthContext";
import { StatusBadge } from "../components/StatusBadge";
import type { ContentObject, ContentType } from "../types";

export function ContentListPage() {
  const { typeKey } = useParams<{ typeKey: string }>();
  const org = useOrg();

  const { data: types } = useQuery({
    queryKey: ["content-types", org],
    queryFn: () => api.get<ContentType[]>(`/api/orgs/${org}/content-types`),
  });
  const type = types?.find((t) => t.key === typeKey);
  const primaryField = type?.schema.fields?.[0]?.key;

  const {
    data: items,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["content", org, typeKey],
    queryFn: () => api.get<ContentObject[]>(`/api/orgs/${org}/content/${typeKey}`),
    enabled: !!typeKey,
  });

  const title = (o: ContentObject) =>
    (primaryField && (o.data[primaryField] as string)) || o.id.slice(0, 8);

  return (
    <>
      <div className="topbar">
        <h1>{type?.name ?? typeKey}</h1>
        <Link to={`/content/${typeKey}/new`}>
          <button className="primary">Neu anlegen</button>
        </Link>
      </div>

      <div className="panel">
        {isLoading && <p className="muted">Lädt…</p>}
        {error && <p className="error">Konnte Inhalte nicht laden.</p>}
        {items && items.length === 0 && (
          <p className="muted">Noch keine Einträge. Lege den ersten an.</p>
        )}
        {items && items.length > 0 && (
          <table>
            <thead>
              <tr>
                <th>Titel</th>
                <th>Status</th>
                <th>Aktualisiert</th>
              </tr>
            </thead>
            <tbody>
              {items.map((o) => (
                <tr key={o.id}>
                  <td>
                    <Link to={`/content/${typeKey}/${o.id}`}>{title(o)}</Link>
                  </td>
                  <td>
                    <StatusBadge status={o.status} />
                  </td>
                  <td className="muted">
                    {new Date(o.updated_at).toLocaleString("de-DE")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}
