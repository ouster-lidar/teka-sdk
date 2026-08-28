(function () {
    'use strict';

    var CHAT_PORT = 8080;
    var POLL_MS = 5000;

    var link = document.getElementById('hllm-chat-link');
    var status = document.getElementById('hllm-status');
    if (!link || !status) return;

    var chatUrl =
        window.location.protocol + '//' + window.location.hostname + ':' + CHAT_PORT + '/';
    link.href = chatUrl;

    function setStatus(text, ready) {
        status.textContent = text;
        status.classList.toggle('hllm-status--ok', ready);
        status.classList.toggle('hllm-status--warn', !ready && text !== 'Checking…');
    }

    async function probe() {
        try {
            var ctrl = new AbortController();
            var timer = setTimeout(function () { ctrl.abort(); }, 4000);
            await fetch(chatUrl, { method: 'GET', mode: 'no-cors', cache: 'no-store', signal: ctrl.signal });
            clearTimeout(timer);
            setStatus('Ready', true);
            return true;
        } catch (_) {
            setStatus('Starting…', false);
            return false;
        }
    }

    (async function poll() {
        while (!(await probe())) {
            await new Promise(function (r) { setTimeout(r, POLL_MS); });
        }
    })();
})();
