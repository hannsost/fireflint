# FireFlint – Phase 0/1

End-to-end-Gerüst: **Rust-Core + PostgreSQL + Content-Typen (Standort/Team/Job) + interne CRUD-API mit Auth & Rollen + öffentliche Delivery-API + Web Component.**

Beweist die Kern-Story: *Inhalte einmal pflegen → kontrolliert auf mehreren Websites ausspielen* (Whitepaper §17/§24).

Stand: Phase 1 weitgehend umgesetzt — WP1.2–1.10 (Validierung, Auth/Rollen,
Admin-API + OpenAPI + Audit, Versionierung, Delivery-Härtung, Admin-UI, Adapter
Web Component / WordPress / Next.js). Offen: WP1.11/1.12 (Pilot-Onboarding,
Integrationstests/Monitoring/Backups). Parallel: Storage-Operator + Data
Residency (O0–O4.1) unter `operator/`, `deploy/operator/`, `crates/platform/`;
Foundation-Module als Rust-Crates unter `crates/foundation/`. Details:
`../entwicklungsplan-phase-1-2.md` (Umsetzungsstand) und
`modules/OPERATOR-DEVELOPMENT-PLAN.md`.

> **Auth (WP1.3):** Admin-Routen liegen unter `/api/orgs/:org_id/…` und erfordern
> ein Access-JWT. Die Delivery-API erfordert einen API-Token. Ein Demo-Mandant
> wird beim Start automatisch angelegt:
> Login `owner@demo.test` / `demo1234`, Delivery-Token `sg_demo_token`,
> Org-ID `00000000-0000-0000-0000-000000000001`.

## Voraussetzungen
- Rust (stable) + Cargo
- Docker (für PostgreSQL)

## Starten

```bash
# 1. Datenbank starten
docker compose -f deploy/docker-compose.yml up -d

# 2. Env setzen (oder deploy/.env.example kopieren)
export DATABASE_URL="postgres://sitegraph:sitegraph@localhost:5432/sitegraph"

# 3. Core starten (führt Migrationen + Demo-Seed automatisch aus)
cargo run

# 4. Demo öffnen: adapters/web-component/demo.html im Browser
#    (zwei Websites, dieselbe Datenquelle, ein lokaler Override)
```

## Die Demo-Story (90 Sekunden)

1. `demo.html` zeigt zwei „Websites". Beide lesen denselben Standort (mit
   Delivery-Token `sg_demo_token`). Karriere zeigt Darmstadt mit lokalem
   Override (anderes Telefon/Zeiten).
2. Einen neuen Standort anlegen und auf beiden Channels veröffentlichen:

```bash
ORG=00000000-0000-0000-0000-000000000001

# einloggen -> Access-Token holen
TOKEN=$(curl -s -X POST localhost:8080/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"owner@demo.test","password":"demo1234"}' | jq -r .access_token)

# anlegen (Status: draft)
curl -s -X POST localhost:8080/api/orgs/$ORG/content/standort \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"data":{"name":"Berlin","adresse":"Unter den Linden 1, 10117 Berlin","telefon":"030-300"}}'
# -> liefert die neue id zurück, z.B. <ID>

# beiden Websites zuweisen
curl -s -X PUT localhost:8080/api/orgs/$ORG/content/standort/<ID>/channels \
  -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"assignments":[
        {"website_id":"00000000-0000-0000-0000-000000000100"},
        {"website_id":"00000000-0000-0000-0000-000000000101"}]}'

# veröffentlichen
curl -s -X POST localhost:8080/api/orgs/$ORG/content/standort/<ID>/publish \
  -H "authorization: Bearer $TOKEN"
```

3. `demo.html` neu laden → Berlin erscheint auf **beiden** Websites.
4. API stoppen, neu laden → die Web Component zeigt dank Cache+Fallback die
   letzte bekannte Version statt eines Fehlers (Whitepaper §13/§23).

## API-Überblick

Auth: `Authorization: Bearer <access_token>` für alle `/api/*`-Routen.

| Methode | Pfad | Zweck |
|---|---|---|
| GET  | `/healthz` | Health-Check |
| GET  | `/openapi.json` | OpenAPI 3.1 Dokument (alle Endpunkte) |
| POST | `/auth/login` \| `/auth/refresh` | Login / Token erneuern |
| GET  | `/api/me` | Eigener User + Memberships |
| GET/POST | `/api/orgs/:org/websites` | Channels listen / anlegen |
| GET/POST | `/api/orgs/:org/content/:type` | Inhalte listen / anlegen |
| PUT/DELETE | `/api/orgs/:org/content/:type/:id` | Inhalt ändern / löschen |
| PUT | `/api/orgs/:org/content/:type/:id/channels` | Channel-Zuweisung + Overrides |
| POST | `/api/orgs/:org/content/:type/:id/publish` \| `/unpublish` | Status setzen |
| GET | `/api/orgs/:org/content/:type/:id/versions` | Versionsverlauf |
| POST | `/api/orgs/:org/content/:type/:id/revert/:version_id` | Version wiederherstellen |
| GET/POST | `/api/orgs/:org/tokens` | API-Tokens listen / erstellen |
| GET | `/api/orgs/:org/audit` | Audit-Log (Owner) |
| GET | `/v1/:channel/content/:type` | **Delivery** (public, Token nötig, nur published) |

Schreibende Aktionen (anlegen/ändern/löschen/publish/revert/channels/website/token)
werden im Audit-Log protokolliert (WP1.4). Die vollständige API-Beschreibung
liegt maschinenlesbar unter `/openapi.json`.

**Delivery-Härtung (WP1.6):** Antworten tragen `ETag` + `Cache-Control: public,
max-age=60`; ein passendes `If-None-Match` liefert `304`. Rate-Limits:
120 Delivery-Requests/min pro Token, 10 Login-Versuche/min pro IP (`429` bei
Überschreitung). Limiter sind in-memory (Single-Instance) — verteilt kommt in
Phase 2 (WP2.2).

**Rollen:** Owner (alles inkl. Websites/Tokens), Editor (Content CRUD + Publish),
Viewer (nur lesen). **Delivery-Token:** org-weit oder channel-gebunden — ein
channel-gebundener Token kann andere Channels nicht lesen.

Content-Typen im Seed: `standort`, `team`, `job` (ESP-Scope). Beim Anlegen/Ändern
wird `data` gegen das Typ-Schema validiert (Pflichtfelder + Basistypen, WP1.2).

## Struktur

```
crates/sitegraph-domain  # Entitäten, Rollen, Override-Merge, Validierung (kein IO)
crates/sitegraph-db      # sqlx-Pool, Migrationen, Repositories (org_id-scoped)
crates/sitegraph-api     # axum-Router, Admin- + Delivery-API; auth.rs (argon2/JWT/Token)
crates/sitegraph-bin     # Config, Migrate, Demo-Bootstrap, Server-Start
crates/foundation/*      # Foundation-Module als Rust-Crates (Party, Identity&Access,
                         #   Event&Audit, Work, Time&Resource, Asset) — isoliert,
                         #   noch nicht in den Core verdrahtet (Ports + Reference + Tests)
crates/platform/data-residency # Data-Residency-Verträge; via Seam in sitegraph-db (O4.1)
migrations/              # 0001 Schema, 0002 Seed, 0003 Versionen, 0004 Typen, 0005 Auth, 0006 Audit
admin/                   # Admin-UI (React/TS/Vite) — spricht die API
adapters/web-component/  # no-build Web Component + demo.html
adapters/wordpress-plugin/ # WordPress-Plugin (Shortcode + Block, Cache + Fallback)
adapters/next-sdk/       # @sitegraph/next (typisiertes SDK) + Next.js-Beispiel-App
operator/                # FireFlint Storage Operator (CRDs, Controller, Bin) — O0–O3
deploy/operator/         # Kustomize + Helm + CRDs + Beispiele für den Operator
docs/                    # ADR, Gefahrenmodell, Operator-Support-Matrix
modules/                 # optionale Domänenmodule (TS-Sandboxes) + Planungs-/Handoff-Docs
tools/visual/            # Multi-Viewport-Screenshots zur UI-Selbstprüfung
```

## Optionale Modulgerüste

- `modules/commerce` — modularer B2C/B2B/B2G-Commerce
- `modules/forms-workflow` — Formulare, Einreichungen und Workflows
- `modules/edi` — EDI-Gateway für EDIFACT, X12, Peppol und XRechnung
- `modules/privacy-dsgvo` — Datenschutz-Governance über Content, Commerce, Forms und EDI

Beide Bereiche sind vom Rust-Core isoliert. Ihre Szenariotests dienen als
fachliche Akzeptanzkriterien für eine spätere Integration.

## Nächste Schritte (Phase 1)
Erledigt: WP1.2 (Validierung), WP1.3 (Auth/Rollen/Tokens), WP1.4 (Audit-Log +
OpenAPI), WP1.5 (Versionierung), WP1.6 (Delivery-Caching + Rate-Limiting),
WP1.7 (Admin-UI — siehe `admin/`), WP1.9 (WordPress-Plugin — siehe
`adapters/wordpress-plugin/`), WP1.10 (Next.js-SDK — siehe `adapters/next-sdk/`).
Offen: WP1.12 (Integrationstests gegen Test-DB).
Siehe `../entwicklungsplan-phase-1-2.md`.
