(function () {
    'use strict';

    var appApi = tekaAppApi('hello-sub');
    var appId = appApi.appId;
    var FILTER_KEY = 'hello-sub.subscriber.feedFilters';
    var draftUrls = [];
    var draftDirty = false;
    var lastStatus = null;
    var feedEventsCache = [];
    /** @type {Object.<string, boolean>} */
    var filterEnabled = {};
    /** @type {Object.<string, boolean>} */
    var expandedEvents = {};

    function api(path) {
        return appApi.api(path);
    }

    function showError(msg) {
        window.showInlineError('sub-error', msg);
    }

    function escapeHtml(s) {
        return (window.escapeHtml || function (x) { return String(x); })(s);
    }

    function shortPublisherLabel(url) {
        if (!url) return '?';
        try {
            var u = new URL(url);
            var parts = u.pathname.replace(/\/+$/, '').split('/');
            var app = parts[parts.length - 1] || '';
            return app ? (u.hostname + '/' + app) : u.hostname;
        } catch (_) {
            return url.length > 40 ? url.slice(0, 37) + '…' : url;
        }
    }

    function loadFilters() {
        try {
            var raw = localStorage.getItem(FILTER_KEY);
            if (raw) filterEnabled = JSON.parse(raw) || {};
        } catch (_) {
            filterEnabled = {};
        }
    }

    function saveFilters() {
        try { localStorage.setItem(FILTER_KEY, JSON.stringify(filterEnabled)); } catch (_) {}
    }

    function ensureFiltersFor(urls) {
        urls.forEach(function (url) {
            if (typeof filterEnabled[url] !== 'boolean') filterEnabled[url] = true;
        });
        Object.keys(filterEnabled).forEach(function (url) {
            if (urls.indexOf(url) < 0) delete filterEnabled[url];
        });
        saveFilters();
    }

    function renderFilters(urls) {
        var el = document.getElementById('feed-filters');
        if (!el) return;
        ensureFiltersFor(urls);
        if (!urls.length) {
            el.innerHTML = '';
            return;
        }
        var allOn = urls.every(function (u) { return filterEnabled[u]; });
        var html = '<label class="hello-filter-chip">' +
            '<input type="checkbox" id="filter-all" ' + (allOn ? 'checked' : '') + '>' +
            '<span>All</span></label>';
        urls.forEach(function (url, i) {
            html += '<label class="hello-filter-chip" title="' + escapeHtml(url) + '">' +
                '<input type="checkbox" data-filter-url="' + escapeHtml(url) + '" ' +
                (filterEnabled[url] ? 'checked' : '') + '>' +
                '<span>' + escapeHtml(shortPublisherLabel(url)) + '</span></label>';
        });
        el.innerHTML = html;
        var allCb = document.getElementById('filter-all');
        if (allCb) {
            allCb.addEventListener('change', function () {
                var on = !!allCb.checked;
                urls.forEach(function (u) { filterEnabled[u] = on; });
                saveFilters();
                renderFilters(urls);
                renderEvents();
            });
        }
        el.querySelectorAll('input[data-filter-url]').forEach(function (cb) {
            cb.addEventListener('change', function () {
                filterEnabled[cb.getAttribute('data-filter-url')] = !!cb.checked;
                saveFilters();
                renderFilters(urls);
                renderEvents();
            });
        });
    }

    function eventKey(ev) {
        return String(ev.publisher_url || '') + '|' + String(ev.id) + '|' + String(ev.at || '');
    }

    function renderEvents(events) {
        var el = document.getElementById('events-list');
        if (events) feedEventsCache = events.slice();
        var shown = feedEventsCache.filter(function (ev) {
            var url = ev.publisher_url || '';
            if (!url) return true;
            if (typeof filterEnabled[url] === 'boolean') return filterEnabled[url];
            return true;
        });
        if (!shown.length) {
            el.innerHTML = '<div class="stream-item"><span class="stream-meta">No mirrored events yet</span></div>';
            return;
        }
        el.innerHTML = shown.map(function (ev) {
            var key = eventKey(ev);
            var open = !!expandedEvents[key];
            return '<div class="stream-item hello-feed-event' + (open ? ' hello-feed-event--expanded' : '') + '" data-event-key="' + escapeHtml(key) + '">' +
                '<div class="hello-feed-event-text">' +
                '<span class="stream-meta">' +
                '<span class="hello-event-source" title="' + escapeHtml(ev.publisher_url || '') + '">' +
                escapeHtml(shortPublisherLabel(ev.publisher_url)) + '</span>' +
                '<span class="hello-event-kind">' + escapeHtml(ev.kind) + '</span>' +
                '#' + ev.id + ' · ' + escapeHtml(ev.at) +
                '</span>' +
                '<span class="stream-stats hello-feed-event-msg">' + escapeHtml(ev.message) + '</span>' +
                '</div>' +
                '<button type="button" class="btn btn-secondary btn-small hello-feed-expand" aria-expanded="' +
                (open ? 'true' : 'false') + '">' + (open ? 'Collapse' : 'Expand') + '</button>' +
                '</div>';
        }).join('');
        el.querySelectorAll('.hello-feed-event').forEach(function (row) {
            var btn = row.querySelector('.hello-feed-expand');
            if (!btn) return;
            var key = row.getAttribute('data-event-key') || '';
            btn.addEventListener('click', function () {
                var open = !row.classList.contains('hello-feed-event--expanded');
                expandedEvents[key] = open;
                row.classList.toggle('hello-feed-event--expanded', open);
                btn.setAttribute('aria-expanded', open ? 'true' : 'false');
                btn.textContent = open ? 'Collapse' : 'Expand';
            });
        });
    }

    function publisherMeta(url) {
        var pubs = (lastStatus && lastStatus.publishers) || [];
        for (var i = 0; i < pubs.length; i++) {
            if (pubs[i].publisher_url === url) return pubs[i];
        }
        return null;
    }

    function renderPublishersList() {
        var el = document.getElementById('publishers-list');
        if (!el) return;
        if (!draftUrls.length) {
            el.innerHTML = '<div class="hello-publisher-empty">No publishers configured. Add a URL below.</div>';
            return;
        }
        el.innerHTML = draftUrls.map(function (url, idx) {
            var meta = publisherMeta(url);
            var connected = meta && meta.connected;
            var gen = '—';
            if (meta && meta.peer_state && typeof meta.peer_state.generator_running === 'boolean') {
                gen = meta.peer_state.generator_running ? 'on' : 'off';
            }
            var err = (meta && meta.last_error) || '';
            return '<div class="hello-publisher-row">' +
                '<div class="hello-publisher-body">' +
                '<span class="app-meta-chip ' + (connected ? 'app-meta-chip--usage' : 'app-meta-chip--state') + '">' +
                (connected ? 'connected' : 'waiting') + '</span>' +
                '<code class="hello-mono hello-publisher-url" title="' + escapeHtml(url) + '">' +
                escapeHtml(url) + '</code>' +
                '<span class="hello-publisher-gen">Generator: ' + escapeHtml(gen) + '</span>' +
                (err ? '<span class="hello-publisher-err">' + escapeHtml(err) + '</span>' : '') +
                '</div>' +
                '<button type="button" class="btn btn-secondary btn-small hello-publisher-remove" data-remove-idx="' + idx + '">Remove</button>' +
                '</div>';
        }).join('');
        el.querySelectorAll('[data-remove-idx]').forEach(function (btn) {
            btn.addEventListener('click', function () {
                var i = parseInt(btn.getAttribute('data-remove-idx'), 10);
                if (!isFinite(i)) return;
                var removed = draftUrls[i];
                draftUrls.splice(i, 1);
                if (removed) {
                    feedEventsCache = feedEventsCache.filter(function (ev) {
                        return ev.publisher_url !== removed;
                    });
                    renderEvents();
                }
                renderPublishersList();
                renderFilters(draftUrls.slice());
                // Persist immediately so Refresh / reload cannot restore the peer.
                postConfig(draftUrls.slice());
            });
        });
    }

    function applyStatus(status) {
        if (!status) return;
        lastStatus = status;
        document.getElementById('events-received').textContent = String(status.events_received || 0);
        var countEl = document.getElementById('publisher-count');
        if (countEl) countEl.textContent = String(status.publisher_count || 0);
        var chip = document.getElementById('conn-chip');
        var n = status.connected_count || 0;
        var m = status.publisher_count || 0;
        chip.textContent = n + '/' + m + ' connected';
        chip.className = 'app-meta-chip ' + (n > 0 ? 'app-meta-chip--usage' : 'app-meta-chip--state');

        var urls = (status.publishers || []).map(function (p) { return p.publisher_url; });
        if (!draftDirty) {
            draftUrls = urls.slice();
        }
        renderPublishersList();
        renderFilters(draftUrls.slice());
    }

    function applySnapshot(data) {
        if (!data) return;
        if (data.status) applyStatus(data.status);
        renderEvents(data.events || []);
    }

    function refetch() {
        window.fetchAppJson(api('/status')).then(applyStatus);
        window.fetchAppJson(api('/feed')).then(function (body) {
            if (body) renderEvents(body.events || []);
        });
    }

    function postConfig(urls) {
        showError('');
        return window.fetchAppJsonPost(api('/peer-config'), { publisher_urls: urls }).then(function (status) {
            if (!status) {
                showError('Apply failed');
                return;
            }
            draftDirty = false;
            draftUrls = (status.publishers || []).map(function (p) { return p.publisher_url; });
            applyStatus(status);
            return window.fetchAppJson(api('/feed')).then(function (body) {
                if (body) renderEvents(body.events || []);
            });
        }).catch(function () {
            showError('Apply failed');
        });
    }

    loadFilters();

    document.getElementById('btn-add-peer').addEventListener('click', function () {
        showError('');
        var input = document.getElementById('cfg-publisher-url');
        var url = (input.value || '').trim();
        if (!url) {
            showError('Publisher URL is required');
            return;
        }
        if (draftUrls.indexOf(url) >= 0) {
            showError('Publisher already in list (normalize on Apply)');
            return;
        }
        draftUrls.push(url);
        draftDirty = true;
        input.value = '';
        renderPublishersList();
        renderFilters(draftUrls.slice());
    });

    document.getElementById('peer-config-form').addEventListener('submit', function (e) {
        e.preventDefault();
        showError('');
        var input = document.getElementById('cfg-publisher-url');
        var pending = (input.value || '').trim();
        var urls = draftUrls.slice();
        if (pending && urls.indexOf(pending) < 0) {
            urls.push(pending);
            input.value = '';
        }
        draftUrls = urls;
        postConfig(urls);
    });

    document.getElementById('btn-peer-state').addEventListener('click', function () {
        showError('');
        window.fetchAppJsonPost(api('/peer-state'), {}).then(function (body) {
            if (!body || !body.ok) {
                showError((body && body.error) || 'Peer state fetch failed');
                return;
            }
            refetch();
        }).catch(function () {
            showError('Peer state fetch failed');
        });
    });

    window.appWs(appId, '/feed/stream', function (d) {
        if (!d || String(d.type).toLowerCase() !== 'snapshot') return;
        applySnapshot(d.data);
    }, { reconnectDelay: 2000 });

    refetch();
})();
