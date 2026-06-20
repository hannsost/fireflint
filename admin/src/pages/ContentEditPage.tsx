import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "../api/client";
import { useOrg } from "../auth/AuthContext";
import { SchemaForm } from "../components/SchemaForm";
import { StatusBadge } from "../components/StatusBadge";
import type {
  ChannelAssignment,
  ContentObject,
  ContentType,
  ContentVersion,
  FieldDef,
  Website,
} from "../types";

type Data = Record<string, unknown>;
interface ChannelState {
  selected: boolean;
  overrides: Data;
}

export function ContentEditPage() {
  const { typeKey, id } = useParams<{ typeKey: string; id: string }>();
  const isNew = !id;
  const org = useOrg();
  const qc = useQueryClient();
  const navigate = useNavigate();

  const { data: types } = useQuery({
    queryKey: ["content-types", org],
    queryFn: () => api.get<ContentType[]>(`/api/orgs/${org}/content-types`),
  });
  const type = types?.find((t) => t.key === typeKey);
  const fields = type?.schema.fields ?? [];

  const { data: object } = useQuery({
    queryKey: ["content-obj", org, id],
    queryFn: () => api.get<ContentObject>(`/api/orgs/${org}/content/${typeKey}/${id}`),
    enabled: !isNew,
  });

  const [data, setData] = useState<Data>({});
  useEffect(() => {
    if (object) setData(object.data);
  }, [object]);

  const [saveError, setSaveError] = useState<string | null>(null);

  const saveMut = useMutation({
    mutationFn: () =>
      isNew
        ? api.post<ContentObject>(`/api/orgs/${org}/content/${typeKey}`, { data })
        : api.put<ContentObject>(`/api/orgs/${org}/content/${typeKey}/${id}`, { data }),
    onSuccess: (obj) => {
      setSaveError(null);
      qc.invalidateQueries({ queryKey: ["content", org, typeKey] });
      if (isNew) navigate(`/content/${typeKey}/${obj.id}`);
      else qc.invalidateQueries({ queryKey: ["content-obj", org, id] });
    },
    onError: (e) =>
      setSaveError(e instanceof ApiError ? e.message : "Speichern fehlgeschlagen."),
  });

  const statusMut = useMutation({
    mutationFn: (action: "publish" | "unpublish") =>
      api.post(`/api/orgs/${org}/content/${typeKey}/${id}/${action}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["content-obj", org, id] });
      qc.invalidateQueries({ queryKey: ["content", org, typeKey] });
    },
  });

  const deleteMut = useMutation({
    mutationFn: () => api.del(`/api/orgs/${org}/content/${typeKey}/${id}`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["content", org, typeKey] });
      navigate(`/content/${typeKey}`);
    },
  });

  return (
    <>
      <div className="topbar">
        <h1>
          {isNew ? "Neu" : "Bearbeiten"}: {type?.name ?? typeKey}
        </h1>
        {object && <StatusBadge status={object.status} />}
      </div>

      <div className="panel">
        <h2>Inhalt</h2>
        <SchemaForm fields={fields} value={data} onChange={setData} idPrefix="main" />
        {saveError && <p className="error">{saveError}</p>}
        <div className="actions">
          <button className="primary" onClick={() => saveMut.mutate()} disabled={saveMut.isPending}>
            {saveMut.isPending ? "Speichern…" : "Speichern"}
          </button>
          {object && object.status === "draft" && (
            <button onClick={() => statusMut.mutate("publish")} disabled={statusMut.isPending}>
              Veröffentlichen
            </button>
          )}
          {object && object.status === "published" && (
            <button onClick={() => statusMut.mutate("unpublish")} disabled={statusMut.isPending}>
              Zurückziehen
            </button>
          )}
          {object && (
            <button
              className="danger"
              onClick={() => {
                if (confirm("Diesen Eintrag wirklich löschen?")) deleteMut.mutate();
              }}
            >
              Löschen
            </button>
          )}
        </div>
      </div>

      {!isNew && id && <ChannelsPanel org={org} typeKey={typeKey!} id={id} fields={fields} data={data} />}
      {!isNew && id && <VersionsPanel org={org} typeKey={typeKey!} id={id} onReverted={setData} />}
      {isNew && (
        <p className="hint">Channels &amp; Versionen erscheinen nach dem ersten Speichern.</p>
      )}
    </>
  );
}

// --- Channel assignment (with per-channel overrides, Whitepaper §6) ---------

function ChannelsPanel({
  org,
  typeKey,
  id,
  fields,
  data,
}: {
  org: string;
  typeKey: string;
  id: string;
  fields: FieldDef[];
  data: Data;
}) {
  const qc = useQueryClient();
  const { data: websites } = useQuery({
    queryKey: ["websites", org],
    queryFn: () => api.get<Website[]>(`/api/orgs/${org}/websites`),
  });
  const { data: current } = useQuery({
    queryKey: ["channels", org, id],
    queryFn: () =>
      api.get<{ assignments: ChannelAssignment[] }>(
        `/api/orgs/${org}/content/${typeKey}/${id}/channels`,
      ),
  });

  const [state, setState] = useState<Record<string, ChannelState>>({});
  useEffect(() => {
    if (!websites) return;
    const next: Record<string, ChannelState> = {};
    for (const w of websites) {
      const assigned = current?.assignments.find((a) => a.website_id === w.id);
      next[w.id] = {
        selected: !!assigned,
        overrides: (assigned?.overrides as Data) ?? {},
      };
    }
    setState(next);
  }, [websites, current]);

  const saveMut = useMutation({
    mutationFn: () => {
      const assignments: ChannelAssignment[] = (websites ?? [])
        .filter((w) => state[w.id]?.selected)
        .map((w) => {
          const ov = state[w.id].overrides;
          const hasKeys = Object.values(ov).some((v) => v !== "" && v != null);
          return { website_id: w.id, overrides: hasKeys ? ov : null };
        });
      return api.put(`/api/orgs/${org}/content/${typeKey}/${id}/channels`, { assignments });
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: ["channels", org, id] }),
  });

  const toggle = (wid: string) =>
    setState((s) => ({ ...s, [wid]: { ...s[wid], selected: !s[wid].selected } }));
  const setOverrides = (wid: string, overrides: Data) =>
    setState((s) => ({ ...s, [wid]: { ...s[wid], overrides } }));

  return (
    <div className="panel">
      <h2>Channels</h2>
      <p className="hint">
        Wähle, auf welchen Websites dieser Eintrag erscheint. Pro Channel kannst du
        einzelne Felder lokal überschreiben.
      </p>
      {websites?.map((w) => {
        const cs = state[w.id];
        if (!cs) return null;
        return (
          <div key={w.id} style={{ borderTop: "1px solid var(--border)", paddingTop: "0.7rem", marginTop: "0.7rem" }}>
            <label className="row" style={{ fontWeight: 600 }}>
              <input
                type="checkbox"
                style={{ width: "auto" }}
                checked={cs.selected}
                onChange={() => toggle(w.id)}
              />
              {w.name} <span className="muted code">{w.slug}</span>
            </label>
            {cs.selected && (
              <details style={{ marginTop: "0.5rem" }}>
                <summary className="muted">Lokale Overrides (optional)</summary>
                <div style={{ marginTop: "0.6rem" }}>
                  <SchemaForm
                    fields={fields}
                    value={cs.overrides}
                    onChange={(ov) => setOverrides(w.id, ov)}
                    idPrefix={`ov-${w.id}`}
                    placeholderHint={(f) => {
                      const base = data[f.key];
                      return base == null ? undefined : `Standard: ${String(base)}`;
                    }}
                  />
                </div>
              </details>
            )}
          </div>
        );
      })}
      <div className="actions">
        <button className="primary" onClick={() => saveMut.mutate()} disabled={saveMut.isPending}>
          {saveMut.isPending ? "Speichern…" : "Channel-Zuweisung speichern"}
        </button>
      </div>
    </div>
  );
}

// --- Version history + revert (WP1.5) --------------------------------------

function VersionsPanel({
  org,
  typeKey,
  id,
  onReverted,
}: {
  org: string;
  typeKey: string;
  id: string;
  onReverted: (data: Data) => void;
}) {
  const qc = useQueryClient();
  const { data } = useQuery({
    queryKey: ["versions", org, id],
    queryFn: () =>
      api.get<{ versions: ContentVersion[] }>(
        `/api/orgs/${org}/content/${typeKey}/${id}/versions`,
      ),
  });

  const revertMut = useMutation({
    mutationFn: (versionId: string) =>
      api.post<ContentObject>(`/api/orgs/${org}/content/${typeKey}/${id}/revert/${versionId}`),
    onSuccess: (obj) => {
      onReverted(obj.data);
      qc.invalidateQueries({ queryKey: ["content-obj", org, id] });
      qc.invalidateQueries({ queryKey: ["versions", org, id] });
    },
  });

  const versions = data?.versions ?? [];

  return (
    <div className="panel">
      <h2>Versionen</h2>
      {versions.length === 0 && <p className="muted">Noch keine Versionen.</p>}
      {versions.length > 0 && (
        <table>
          <thead>
            <tr>
              <th>Zeitpunkt</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {versions.map((v, i) => (
              <tr key={v.id}>
                <td>
                  {new Date(v.created_at).toLocaleString("de-DE")}
                  {i === 0 && <span className="muted"> (aktuell)</span>}
                </td>
                <td>
                  {i !== 0 && (
                    <button onClick={() => revertMut.mutate(v.id)} disabled={revertMut.isPending}>
                      Wiederherstellen
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
