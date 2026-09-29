(function () {
    'use strict';

    var appApi = jetstreamAppApi('hello-pub');
    var appId = appApi.appId;

    function api(path) {
        return appApi.api(path);
    }

    function showError(msg) {
        window.showInlineError('pub-error', msg);
    }

    function escapeHtml(s) {
        return (window.escapeHtml || function (x) { return String(x); })(s);
    }

    function clampInterval(n) {
        n = parseInt(n, 10);
        if (isNaN(n)) return 5;
        if (n < 1) return 1;
        if (n > 10) return 10;
        return n;
    }

    function publisherUrl() {
        return window.location.origin.replace(/\/$/, '') + '/api/v1/app/hello-pub';
    }

    function fillPublisherUrl() {
        var el = document.getElementById('publisher-url');
        if (el) el.textContent = publisherUrl();
    }

    /** Works on http://.local — Clipboard API needs a secure context. */
    function copyText(text) {
        if (navigator.clipboard && navigator.clipboard.writeText && window.isSecureContext) {
            return navigator.clipboard.writeText(text);
        }
        return new Promise(function (resolve, reject) {
            var ta = document.createElement('textarea');
            ta.value = text;
            ta.setAttribute('readonly', '');
            ta.style.position = 'fixed';
            ta.style.top = '0';
            ta.style.left = '-9999px';
            document.body.appendChild(ta);
            ta.focus();
            ta.select();
            try {
                if (document.execCommand('copy')) {
                    resolve();
                } else {
                    reject(new Error('execCommand copy failed'));
                }
            } catch (e) {
                reject(e);
            } finally {
                document.body.removeChild(ta);
            }
        });
    }

    function copyPublisherUrl() {
        var url = publisherUrl();
        showError('');
        copyText(url).then(function () {
            flashButton(document.getElementById('btn-copy-url'), 'Copied', 1500);
        }).catch(function () {
            showError('Copy failed — select the URL and copy manually');
        });
    }

    /** @type {Object.<string, boolean>} */
    var expandedEvents = {};

    function eventKey(ev) {
        return String(ev.id) + '|' + String(ev.at || '');
    }

    function renderEvents(events) {
        var el = document.getElementById('events-list');
        if (!el) return;
        if (!events || !events.length) {
            el.innerHTML = '<div class="stream-item"><span class="stream-meta">No events yet</span></div>';
            return;
        }
        el.innerHTML = events.map(function (ev) {
            var key = eventKey(ev);
            var open = !!expandedEvents[key];
            return '<div class="stream-item hello-feed-event' + (open ? ' hello-feed-event--expanded' : '') +
                '" data-event-key="' + escapeHtml(key) + '">' +
                '<div class="hello-feed-event-row">' +
                '<div class="hello-feed-event-text">' +
                '<span class="stream-meta">' +
                '<span class="hello-event-kind">' + escapeHtml(ev.kind) + '</span>' +
                '#' + escapeHtml(String(ev.id)) + ' · ' + escapeHtml(ev.at) +
                '</span>' +
                '<span class="stream-stats hello-feed-event-msg">' + escapeHtml(ev.message) + '</span>' +
                '</div>' +
                '<button type="button" class="btn btn-secondary btn-small hello-feed-expand" aria-expanded="' +
                (open ? 'true' : 'false') + '">' + (open ? 'Collapse' : 'Expand') + '</button>' +
                '</div></div>';
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

    function applyState(state) {
        if (!state) return;
        document.getElementById('total-published').textContent = String(state.total_published || 0);
        var intervalEl = document.getElementById('generator-interval');
        if (intervalEl && document.activeElement !== intervalEl) {
            intervalEl.value = String(clampInterval(state.generator_interval_secs || 5));
        }
        var toggle = document.getElementById('generator-toggle');
        if (toggle) toggle.checked = !!state.generator_running;
    }

    function applySnapshot(data) {
        if (!data) return;
        if (data.state) applyState(data.state);
        renderEvents(data.events || []);
    }

    function refetch() {
        window.fetchAppJson(api('/state')).then(function (state) {
            applyState(state);
        });
        window.fetchAppJson(api('/events')).then(function (body) {
            if (body) renderEvents(body.events || []);
        });
    }

    function postGenerator(partial) {
        showError('');
        var running = document.getElementById('generator-toggle').checked;
        var interval = clampInterval(document.getElementById('generator-interval').value);
        var body = {
            running: partial.running !== undefined ? !!partial.running : running,
            interval_secs: partial.interval_secs !== undefined ? partial.interval_secs : interval
        };
        return window.fetchAppJsonPost(api('/generator'), body).then(function (state) {
            if (!state || state.error) {
                showError((state && state.error && state.error.message) || 'Generator update failed');
                refetch();
                return;
            }
            applyState(state);
        }).catch(function () {
            showError('Generator update failed');
            refetch();
        });
    }

    function flashButton(btn, label, ms) {
        if (!btn) return;
        var prev = btn.textContent;
        btn.textContent = label;
        clearTimeout(btn._flashTimer);
        btn._flashTimer = setTimeout(function () { btn.textContent = prev; }, ms || 1500);
    }

    function publishEvent(e) {
        if (e) e.preventDefault();
        showError('');
        var message = document.getElementById('event-message').value;
        var body = {};
        if (message && message.trim()) body.message = message.trim();
        window.fetchAppJsonPost(api('/events'), body).then(function (ev) {
            if (!ev || ev.error) {
                showError((ev && ev.error && ev.error.message) || 'Publish failed');
                return;
            }
            flashButton(document.getElementById('btn-publish'), 'Published', 1200);
            refetch();
        }).catch(function () {
            showError('Publish failed');
        });
    }

    document.getElementById('publish-form').addEventListener('submit', publishEvent);
    document.getElementById('btn-copy-url').addEventListener('click', copyPublisherUrl);

    document.getElementById('generator-toggle').addEventListener('change', function (e) {
        postGenerator({ running: !!e.target.checked });
    });

    document.getElementById('generator-interval').addEventListener('change', function (e) {
        var secs = clampInterval(e.target.value);
        e.target.value = String(secs);
        postGenerator({ interval_secs: secs });
    });

    window.appWs(appId, '/events/stream', function (d) {
        if (!d || String(d.type).toLowerCase() !== 'snapshot') return;
        applySnapshot(d.data);
    }, { reconnectDelay: 2000 });

    fillPublisherUrl();
    refetch();
})();
