(function () {
    'use strict';

    const canvas = document.getElementById('hw-particles');
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    const dots = Array.from({ length: 24 }, () => ({ x: 0, y: 0, vx: 0, vy: 0, r: 0 }));
    let w = 0;
    let h = 0;

    function resize() {
        const rect = canvas.parentElement.getBoundingClientRect();
        w = canvas.width = Math.floor(rect.width);
        h = canvas.height = Math.floor(rect.height);
        for (const d of dots) {
            d.x = Math.random() * w;
            d.y = Math.random() * h;
            d.vx = (Math.random() - 0.5) * 0.6;
            d.vy = (Math.random() - 0.5) * 0.6;
            d.r = 1 + Math.random() * 2;
        }
    }

    function drawFrame() {
        ctx.clearRect(0, 0, w, h);
        ctx.fillStyle = 'rgba(74, 222, 128, 0.55)';
        for (const d of dots) {
            d.x += d.vx;
            d.y += d.vy;
            if (d.x < 0 || d.x > w) d.vx *= -1;
            if (d.y < 0 || d.y > h) d.vy *= -1;
            ctx.beginPath();
            ctx.arc(d.x, d.y, d.r, 0, Math.PI * 2);
            ctx.fill();
        }
        requestAnimationFrame(drawFrame);
    }

    function animateCounter(el) {
        const target = parseFloat(el.dataset.target || '0', 10);
        const decimals = parseInt(el.dataset.decimals || '0', 10);
        const suffix = el.dataset.suffix || '';
        const start = performance.now();
        function tick(now) {
            const t = Math.min(1, (now - start) / 1200);
            el.textContent = (target * t).toFixed(decimals) + suffix;
            if (t < 1) requestAnimationFrame(tick);
        }
        requestAnimationFrame(tick);
    }

    resize();
    drawFrame();
    window.addEventListener('resize', resize);
    document.querySelectorAll('.hw-stat-value').forEach(animateCounter);
})();
