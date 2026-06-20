<?php
/**
 * SiteGraph API client with caching + outage fallback (WP1.9 / Whitepaper §13).
 *
 * Two layers:
 *   - a short-lived transient (fresh cache, performance), and
 *   - a never-expiring option backup (last known good, used when the API is down).
 *
 * The trust promise: a website never breaks just because the SaaS is unreachable.
 */

if (!defined('ABSPATH')) {
    exit;
}

class SiteGraph_Client {

    /** Fresh-cache lifetime in seconds. */
    const FRESH_TTL = 60;

    /**
     * Configured API base URL + token.
     *
     * @return array{api_url:string, token:string}
     */
    public static function settings() {
        $o = get_option('sitegraph_settings', []);
        return [
            'api_url' => isset($o['api_url']) ? rtrim((string) $o['api_url'], '/') : '',
            'token'   => isset($o['token']) ? (string) $o['token'] : '',
        ];
    }

    /**
     * Fetch published content for a channel/module, with cache + fallback.
     *
     * @param string $channel Channel slug.
     * @param string $module  Content type key.
     * @return array Decoded delivery body (`items`), possibly flagged `_stale`.
     */
    public static function fetch($channel, $module) {
        $s = self::settings();
        if ($s['api_url'] === '' || $s['token'] === '') {
            return ['items' => [], 'error' => 'not_configured'];
        }

        $base_key  = 'sitegraph_' . md5($s['api_url'] . '|' . $channel . '|' . $module);
        $stale_key = $base_key . '_stale';

        // 1) Fresh cache hit.
        $fresh = get_transient($base_key);
        if (is_array($fresh)) {
            return $fresh;
        }

        // 2) Fetch live.
        $url = $s['api_url'] . '/v1/' . rawurlencode($channel) . '/content/' . rawurlencode($module);
        $res = wp_remote_get($url, [
            'timeout' => 5,
            'headers' => ['Authorization' => 'Bearer ' . $s['token']],
        ]);

        if (!is_wp_error($res) && (int) wp_remote_retrieve_response_code($res) === 200) {
            $body = json_decode(wp_remote_retrieve_body($res), true);
            if (is_array($body)) {
                set_transient($base_key, $body, self::FRESH_TTL);
                // Persistent backup for outages (autoload off).
                update_option($stale_key, $body, false);
                return $body;
            }
        }

        // 3) Outage fallback: last known good (Whitepaper §13).
        $stale = get_option($stale_key, false);
        if (is_array($stale)) {
            $stale['_stale'] = true;
            return $stale;
        }

        return ['items' => [], 'error' => 'unavailable'];
    }
}
