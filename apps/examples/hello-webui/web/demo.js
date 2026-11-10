(function () {
    'use strict';

    const dashboard = document.getElementById('hw-dashboard');
    if (!dashboard) return;

    function refresh() {
        dashboard.querySelectorAll('[data-base]').forEach(function (card) {
            let value = Number(card.dataset.base);
            const values = Array.from({ length: 24 }, function () {
                value = Math.max(0, Math.min(Number(card.dataset.max || Infinity),
                    value + (Math.random() - 0.5) * Number(card.dataset.spread)));
                return value;
            });
            const low = Math.min(...values);
            const range = Math.max(...values) - low || 1;
            card.querySelector('output').textContent = value.toFixed(Number(card.dataset.decimals)) + card.dataset.unit;
            card.querySelector('polyline').setAttribute('points', values.map(function (sample, index) {
                return `${4 + index * 10},${64 - 56 * (sample - low) / range}`;
            }).join(' '));
        });
        dashboard.querySelector('[data-updated]').textContent = 'Updated ' + new Date().toLocaleTimeString();
    }

    dashboard.querySelector('button').addEventListener('click', refresh);
    refresh();
})();
