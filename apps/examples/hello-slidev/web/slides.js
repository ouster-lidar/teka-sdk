(function () {
    'use strict';

    var appApi = jetstreamAppApi('hello-slidev');
    var deckUrl = '/api/v1/app/hello-slidev/deck/index.html';

    function api(path) {
        return appApi.api(path);
    }

    var toolbar = document.getElementById('hs-toolbar');
    var toggle = document.getElementById('hs-guest-access');
    var hint = document.getElementById('hs-guest-hint');
    var frame = document.getElementById('hs-slidev-frame');
    var busy = false;

    function setHint(msg) {
        if (!hint) return;
        hint.textContent = msg || '';
        hint.hidden = !msg;
    }

    function showDeck() {
        if (!frame || frame.getAttribute('src') === deckUrl) return;
        frame.src = deckUrl;
    }

    function hideDeck() {
        if (!frame || frame.getAttribute('src') === 'about:blank') return;
        frame.src = 'about:blank';
    }

    function applyState(data) {
        if (!toggle || !toolbar) return;
        var editable = !!(data && data.editable);
        var enabled = !!(data && data.enabled);
        toggle.checked = enabled;
        toggle.disabled = !editable || busy;
        toolbar.hidden = false;
        if (editable || enabled) {
            showDeck();
        } else {
            hideDeck();
        }
        if (!editable) {
            setHint(
                enabled
                    ? 'Viewing as guest.'
                    : 'Sign in to view the deck or change guest access.'
            );
            return;
        }
        setHint(
            enabled
                ? 'Guests can open this page and use presenter fullscreen.'
                : 'Guest access is off; only signed-in users can view the deck.'
        );
    }

    function loadState() {
        return fetchAppJson(api('/guest-access')).then(function (data) {
            applyState(data || { enabled: false, editable: false });
            return data;
        });
    }

    function saveState(enabled) {
        if (!toggle || busy) return Promise.resolve();
        busy = true;
        toggle.disabled = true;
        return fetchApp(api('/guest-access'), {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ enabled: enabled }),
        })
            .then(function (r) {
                return r.ok ? r.json() : null;
            })
            .then(function (data) {
                if (data) applyState(data);
            })
            .finally(function () {
                busy = false;
                loadState();
            });
    }

    if (toggle) {
        toggle.addEventListener('change', function () {
            saveState(toggle.checked);
        });
    }

    if (frame) {
        frame.addEventListener('load', function () {
            frame.classList.add('hs-slidev-frame--ready');
        });
    }

    loadState();
})();
