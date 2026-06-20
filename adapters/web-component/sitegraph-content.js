// SiteGraph Web Component (no-build skeleton version, WP1.8).
//
// Usage:
//   <sitegraph-content api="http://localhost:8080" channel="hauptseite"
//                      module="standort" token="sg_demo_token">
//   </sitegraph-content>
//
// Renders published content for a channel from the Delivery API. The token
// authorizes read access to the channel (WP1.3). Includes a localStorage cache
// + fallback so the website keeps showing the last known version if the API is
// unreachable (Whitepaper §13/§23 — trust killer #1).

class SiteGraphContent extends HTMLElement {
  static get observedAttributes() {
    return ["api", "channel", "module", "token"];
  }

  connectedCallback() {
    this.render();
  }

  attributeChangedCallback() {
    if (this.isConnected) this.render();
  }

  get cacheKey() {
    return `sitegraph:${this.getAttribute("channel")}:${this.getAttribute("module")}`;
  }

  async render() {
    const api = this.getAttribute("api") || "";
    const channel = this.getAttribute("channel");
    const module = this.getAttribute("module");
    const token = this.getAttribute("token");
    if (!channel || !module) {
      this.innerHTML = `<p style="color:#b00">sitegraph: 'channel' und 'module' sind erforderlich</p>`;
      return;
    }

    const url = `${api}/v1/${encodeURIComponent(channel)}/content/${encodeURIComponent(module)}`;
    try {
      const res = await fetch(url, {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body = await res.json();
      const items = body.items || [];
      localStorage.setItem(this.cacheKey, JSON.stringify(items));
      this.paint(items, null);
    } catch (err) {
      // Fallback to last known version.
      const cached = localStorage.getItem(this.cacheKey);
      if (cached) {
        this.paint(JSON.parse(cached), "Live nicht erreichbar – zeige gespeicherte Version.");
      } else {
        this.innerHTML = `<p style="color:#b00">sitegraph: Inhalte nicht verfügbar (${err.message})</p>`;
      }
    }
  }

  paint(items, notice) {
    const esc = (s) =>
      String(s ?? "").replace(/[&<>"]/g, (c) =>
        ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c])
      );

    const cards = items
      .map(
        (it) => `
        <div style="border:1px solid #e2e2e2;border-radius:8px;padding:12px 16px;margin:8px 0;font-family:system-ui,sans-serif">
          <strong style="font-size:1.1em">${esc(it.name)}</strong>
          <div>${esc(it.adresse)}</div>
          ${it.telefon ? `<div>Tel: ${esc(it.telefon)}</div>` : ""}
          ${it.oeffnungszeiten ? `<div style="color:#555">${esc(it.oeffnungszeiten)}</div>` : ""}
        </div>`
      )
      .join("");

    const banner = notice
      ? `<div style="background:#fff6d6;border:1px solid #e6cf66;padding:6px 10px;border-radius:6px;font-size:.85em;margin-bottom:8px">${esc(notice)}</div>`
      : "";

    this.innerHTML =
      banner + (cards || `<p style="color:#777">Keine Inhalte für diesen Channel.</p>`);
  }
}

customElements.define("sitegraph-content", SiteGraphContent);
