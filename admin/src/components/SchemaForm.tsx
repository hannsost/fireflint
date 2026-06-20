import type { FieldDef } from "../types";

type Data = Record<string, unknown>;

interface Props {
  fields: FieldDef[];
  value: Data;
  onChange: (next: Data) => void;
  idPrefix?: string;
  /** Show fields even when empty; used for per-channel overrides. */
  placeholderHint?: (f: FieldDef) => string | undefined;
}

/** Renders form inputs generated from a content type's field definitions (WP1.7). */
export function SchemaForm({ fields, value, onChange, idPrefix = "f", placeholderHint }: Props) {
  const set = (key: string, v: unknown) => onChange({ ...value, [key]: v });

  return (
    <>
      {fields.map((f) => {
        const id = `${idPrefix}-${f.key}`;
        const raw = value[f.key];
        const placeholder = placeholderHint?.(f);
        return (
          <div className="field" key={f.key}>
            <label htmlFor={id}>
              {f.label || f.key}
              {f.required && " *"}
            </label>
            {f.type === "richtext" ? (
              <textarea
                id={id}
                value={raw == null ? "" : String(raw)}
                placeholder={placeholder}
                onChange={(e) => set(f.key, e.target.value)}
              />
            ) : f.type === "number" ? (
              <input
                id={id}
                type="number"
                value={raw == null || raw === "" ? "" : Number(raw)}
                placeholder={placeholder}
                onChange={(e) =>
                  set(f.key, e.target.value === "" ? "" : Number(e.target.value))
                }
              />
            ) : (
              <input
                id={id}
                value={raw == null ? "" : String(raw)}
                placeholder={placeholder}
                onChange={(e) => set(f.key, e.target.value)}
              />
            )}
          </div>
        );
      })}
    </>
  );
}
