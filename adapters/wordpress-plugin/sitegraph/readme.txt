=== SiteGraph ===
Contributors: sitegraph
Tags: cms, content, headless, multisite
Requires at least: 6.0
Requires PHP: 7.4
Stable tag: 0.1.0
License: MIT

Bindet zentral in SiteGraph gepflegte Inhalte (Team, Standorte, Jobs …) in
WordPress ein — serverseitig gerendert, mit Cache und Ausfall-Fallback.

== Beschreibung ==

SiteGraph pflegt Inhalte einmal zentral und spielt sie auf mehreren Websites aus.
Dieses Plugin holt die für einen Channel veröffentlichten Inhalte über die
Delivery-API und zeigt sie an — per Shortcode oder Gutenberg-Block.

* Serverseitiges Rendering (SEO-freundlich, funktioniert ohne JavaScript)
* Kurzzeit-Cache (Transient) für Performance
* Persistenter Fallback: Bei API-Ausfall bleibt die zuletzt bekannte Version
  sichtbar — die Website bricht nicht.

== Einrichtung ==

1. Plugin aktivieren.
2. Einstellungen → SiteGraph öffnen und API-URL + Delivery-Token eintragen.
3. Inhalte einbinden:
   * Shortcode: [sitegraph module="job" channel="karriere"]
   * Block: „SiteGraph Inhalt" einfügen, Modul + Channel wählen.

== Hinweise ==

* Der Token autorisiert nur Lesezugriff auf die Delivery-API. Ein
  channel-gebundener Token kann nur seinen Channel lesen.
* Cache-Dauer: 60 Sekunden (Transient). Der Fallback bleibt darüber hinaus
  erhalten, bis ein erfolgreicher Abruf ihn ersetzt.
