# FireFlint Admin (WP1.7)

React + TypeScript + Vite + TanStack Query. Spricht ausschließlich die FireFlint-API
(`/openapi.json`) — der Core bleibt von der UI getrennt (Whitepaper §22).

## Entwicklung

```bash
npm install
npm run dev        # http://localhost:5173 (erwartet die API auf :8080)
npm run typecheck  # tsc -b --noEmit
npm run build      # tsc + vite build -> dist/
```

API-Basis-URL überschreibbar via `VITE_API_URL` (Default `http://localhost:8080`).
Demo-Login: `owner@demo.test` / `demo1234`.

## Funktionsumfang
- Login mit JWT (Access + Refresh, automatischer Refresh bei 401), Logout
- Layout mit Navigation (Content-Typen dynamisch, Websites, Tokens)
- Content-Liste je Typ mit Status-Badge
- **Schema-getriebenes** Editor-Formular (aus `content_type.schema` generiert)
- Publish / Unpublish, Löschen
- Channel-Zuweisung mit optionalen lokalen Overrides pro Channel (§6)
- Versionsverlauf + Wiederherstellen (WP1.5)
- API-Token-Verwaltung (erstellen, einmalig anzeigen, widerrufen)
- Websites anlegen (Owner)

## Visuelle Selbstprüfung
Gehört zur Definition-of-Done jeder UI-Änderung: `cd ../tools/visual && node shoot.mjs
http://localhost:5173 admin`, dann die Screenshots prüfen (mind. ein Phone + Desktop).
