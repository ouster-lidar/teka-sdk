(function (global) {
    'use strict';

    function apiPath(id, path) {
        var p = path.charAt(0) === '/' ? path : '/' + path;
        return '/api/v1/app/' + encodeURIComponent(id) + p;
    }

    function mcAppApi(defaultId) {
        var appHost = document.querySelector('.app-panel-host');
        var appId =
            (appHost && appHost.getAttribute('data-app-id')) ||
            defaultId ||
            '';

        function appPagePath(id, path) {
            var p = path.charAt(0) === '/' ? path : '/' + path;
            if (p === '/' || p === '') {
                return '/app/' + encodeURIComponent(id);
            }
            return '/app/' + encodeURIComponent(id) + p;
        }

        function api(path) {
            if (!appId) {
                return '';
            }
            if (global.appApi) {
                return global.appApi(appId, path);
            }
            return apiPath(appId, path);
        }

        function appApi(id, path) {
            if (global.appApi) {
                return global.appApi(id, path);
            }
            return apiPath(id, path);
        }

        return { appId: appId, api: api, appApi: appApi, appPage: appPagePath };
    }

    function streamMessageType(d) {
        return d && d.type != null ? String(d.type).toUpperCase() : '';
    }

    function refetchOnStatus(d, refetch) {
        var t = streamMessageType(d);
        if (t !== 'SNAPSHOT' && t !== 'STATUS') return;
        refetch();
    }

    function fetchApp(url, opts) {
        opts = opts || {};
        opts.credentials = 'include';
        return fetch(url, opts);
    }

    function fetchAppJson(url) {
        return fetchApp(url).then(function (r) { return r.ok ? r.json() : null; });
    }

    /** POST/PUT JSON with credentials; resolves parsed JSON body. */
    function fetchAppJsonPost(url, body, method) {
        return fetchApp(url, {
            method: method || 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: body !== undefined ? JSON.stringify(body) : undefined
        }).then(function (r) { return r.json(); });
    }

    /** Show or clear an inline alert element (id string or Element). */
    function showInlineError(elOrId, msg) {
        var el = typeof elOrId === 'string' ? document.getElementById(elOrId) : elOrId;
        if (!el) return;
        el.textContent = msg || '';
        el.style.display = msg ? 'block' : 'none';
    }

    function connectStatusStream(appId, streamPath, refetch) {
        if (!global.appWebSocket) return;
        global.appWebSocket(appId, streamPath, function (d) {
            refetchOnStatus(d, refetch);
        });
    }

    function appWs(appId, path, onMessage, options) {
        if (global.appWebSocket) {
            return global.appWebSocket(appId, path, onMessage, options);
        }
        if (!global.connectWebSocket) return null;
        var p = path.charAt(0) === '/' ? path : '/' + path;
        return global.connectWebSocket(apiPath(appId, p), onMessage, options);
    }

    /** Version chip HTML for jet-version-value spans (Jetstream sysupdate/install). */
    function versionValueHtml(value, variant) {
        var esc = global.escapeHtml || function (s) { return s; };
        var cls = 'jet-version-value' + (variant ? ' jet-version-value--' + variant : '');
        return '<span class="' + cls + '">' + esc(value || '(unknown)') + '</span>';
    }

    /** Sentence-case operator label (first word capitalized; rest lowercase). */
    function formatChipLabel(value) {
        if (value == null || value === '') return '';
        var s = String(value).trim();
        if (!s) return '';
        var colon = s.indexOf(':');
        if (colon >= 0) {
            var head = formatChipLabel(s.slice(0, colon));
            var tail = s.slice(colon + 1).trim();
            if (!tail) return head + ':';
            return head + ': ' + tail.toLowerCase();
        }
        if (/^[A-Z0-9]{2,4}$/.test(s)) return s;
        return s.split(/\s+/).map(function (word, i) {
            if (/^[A-Z0-9]{2,4}$/.test(word)) return word;
            if (i === 0) return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
            return word.toLowerCase();
        }).join(' ');
    }

    /** Map wire enum tokens (RUNNING, FALLBACK) to operator sentence case. */
    function formatEnumLabel(value) {
        if (value == null || value === '') return '—';
        var s = String(value).trim();
        if (!s) return '—';
        if (/^[A-Z][A-Z0-9_]*$/.test(s)) {
            if (!s.includes('_') && /^[A-Z0-9]{2,4}$/.test(s)) return s;
            var label = s.replace(/_/g, ' ').toLowerCase();
            return label.charAt(0).toUpperCase() + label.slice(1);
        }
        return formatChipLabel(s);
    }

    /** Service header one-liner: status.summary, else fallback, else 'Disabled'. */
    function formatServiceSummary(status, fallback) {
        if (status && status.summary) return status.summary;
        if (fallback) return fallback;
        return 'Disabled';
    }

    global.mcAppApi = mcAppApi;
    global.refetchOnStatus = refetchOnStatus;
    global.fetchApp = fetchApp;
    global.fetchAppJson = fetchAppJson;
    global.fetchAppJsonPost = fetchAppJsonPost;
    global.showInlineError = showInlineError;
    global.connectStatusStream = connectStatusStream;
    global.connectResourceStream = connectStatusStream;
    global.connectAppStream = connectStatusStream;
    global.appWs = appWs;
    global.versionValueHtml = versionValueHtml;
    global.formatChipLabel = formatChipLabel;
    global.formatEnumLabel = formatEnumLabel;
    global.formatServiceSummary = formatServiceSummary;
})(typeof window !== 'undefined' ? window : this);
