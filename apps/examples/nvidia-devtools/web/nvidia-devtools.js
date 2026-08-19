(function () {
    'use strict';

    var mc = mcAppApi('nvidia-devtools');
    var SSH_PORT = 2223;
    var WINDOW_SEC = 60;
    var SMOOTH_FACTOR = 0.16;

    var METRICS = [
        {
            id: 'ram_pct',
            label: 'RAM',
            color: '#f59e0b',
            value: function (s) { return s.ram_pct; },
            text: function (s) { return s.ram_pct.toFixed(1) + '%'; },
            sub: function (s) { return s.ram_used_mb + ' / ' + s.ram_total_mb + ' MB'; },
            min: 0,
            max: 100
        },
        {
            id: 'cpu_pct',
            label: 'CPU',
            color: '#38bdf8',
            value: function (s) { return s.cpu_pct; },
            text: function (s) { return s.cpu_pct.toFixed(1) + '%'; },
            sub: function () { return 'avg active cores'; },
            min: 0,
            max: 100
        },
        {
            id: 'gpu_pct',
            label: 'GPU',
            color: '#4ade80',
            value: function (s) { return s.gpu_pct; },
            text: function (s) { return s.gpu_pct.toFixed(1) + '%'; },
            sub: function () { return 'GR3D_FREQ'; },
            min: 0,
            max: 100
        },
        {
            id: 'gpu_temp_c',
            label: 'GPU temp',
            color: '#f87171',
            value: function (s) { return s.gpu_temp_c; },
            text: function (s) { return s.gpu_temp_c.toFixed(1) + '°C'; },
            sub: function () { return 'gpu@ sensor'; },
            min: 30,
            max: 90
        },
        {
            id: 'power_mw',
            label: 'Power',
            color: '#a78bfa',
            value: function (s) { return s.power_mw / 1000; },
            text: function (s) { return (s.power_mw / 1000).toFixed(2) + ' W'; },
            sub: function (s) { return s.power_mw + ' mW VDD_IN'; },
            min: 0,
            max: 30
        }
    ];

    function api(path) {
        return mc.api(path);
    }

    function sshCmd(user) {
        return 'ssh -p ' + SSH_PORT + ' ' + user + '@' + (window.location.hostname || 'device');
    }

    function fillSshCommands() {
        document.getElementById('ndt-ssh-js').textContent = sshCmd('js');
        document.getElementById('ndt-ssh-root').textContent = sshCmd('root');
    }

    function showError(msg) {
        window.showInlineError('ndt-error', msg);
        window.showInlineError('ndt-success', '');
    }

    function showSuccess(msg) {
        showError('');
        window.showInlineError('ndt-success', msg);
    }

    function copyText(text) {
        if (navigator.clipboard && navigator.clipboard.writeText && window.isSecureContext) {
            return navigator.clipboard.writeText(text);
        }
        return new Promise(function (resolve, reject) {
            var ta = document.createElement('textarea');
            ta.value = text;
            ta.setAttribute('readonly', '');
            ta.style.position = 'fixed';
            ta.style.left = '-9999px';
            document.body.appendChild(ta);
            ta.select();
            try {
                if (document.execCommand('copy')) resolve();
                else reject(new Error('copy failed'));
            } catch (e) {
                reject(e);
            } finally {
                document.body.removeChild(ta);
            }
        });
    }

    function flashButton(btn, label) {
        if (!btn) return;
        var prev = btn.textContent;
        btn.textContent = label;
        clearTimeout(btn._flashTimer);
        btn._flashTimer = setTimeout(function () {
            btn.textContent = prev;
        }, 1500);
    }

    function loadKeys() {
        showError('');
        window.fetchAppJson(api('/keys')).then(function (body) {
            if (!body) {
                showError('Could not load authorized keys.');
                return;
            }
            document.getElementById('ndt-keys-js').value = body.js || '';
            document.getElementById('ndt-keys-root').value = body.root || '';
        }).catch(function () {
            showError('Could not load authorized keys.');
        });
    }

    function saveKeys() {
        showError('');
        var btn = document.getElementById('ndt-save');
        btn.disabled = true;
        window.fetchApp(api('/keys'), {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                js: document.getElementById('ndt-keys-js').value,
                root: document.getElementById('ndt-keys-root').value
            })
        })
            .then(function (r) {
                if (r.status === 204 || r.ok) {
                    showSuccess('Authorized keys saved.');
                    return loadKeys();
                }
                return r.json().then(function (body) {
                    var err = body && body.error;
                    var msg = (err && (err.message || err)) || 'Save failed';
                    throw new Error(msg);
                });
            })
            .catch(function (e) {
                showError(e.message || 'Save failed.');
            })
            .finally(function () {
                btn.disabled = false;
            });
    }

    function wireCopyButton(id, user) {
        document.getElementById(id).addEventListener('click', function () {
            copyText(sshCmd(user))
                .then(function () {
                    flashButton(document.getElementById(id), 'Copied');
                })
                .catch(function () {
                    showError('Copy failed — select the command and copy manually.');
                });
        });
    }

    function buildMetricCards() {
        var root = document.getElementById('ndt-metrics');
        root.innerHTML = METRICS.map(function (m) {
            return '<div class="ndt-metric" id="ndt-metric-' + m.id + '">' +
                '<span class="ndt-metric-label">' + m.label + '</span>' +
                '<span class="ndt-metric-value">—</span>' +
                '<span class="ndt-metric-sub"></span>' +
                '</div>';
        }).join('');
    }

    function setMonitorStatus(state, text) {
        var el = document.getElementById('ndt-monitor-status');
        el.textContent = text;
        el.className = 'ndt-monitor-status ndt-monitor-status--' + state;
    }

    function formatSampleAge(tsMs) {
        var ageSec = Math.max(0, Math.round((Date.now() - tsMs) / 1000));
        if (ageSec === 0) return 'now';
        return '−' + ageSec + 's';
    }

    function TegraCharts() {
        var canvas = document.getElementById('ndt-chart');
        var wrap = document.getElementById('ndt-chart-wrap');
        var tooltip = document.getElementById('ndt-chart-tooltip');
        var ctx = canvas.getContext('2d');
        var samples = [];
        var smooth = {};
        var layout = { padL: 42, padR: 10, padT: 8, padB: 18, width: 0, height: 0, rowH: 0, rowGap: 6 };
        var hoverIndex = null;
        var rafId = 0;

        METRICS.forEach(function (m) {
            smooth[m.id] = [];
        });

        function syncSmoothLengths() {
            METRICS.forEach(function (m) {
                var arr = smooth[m.id];
                while (arr.length > samples.length) arr.pop();
                while (arr.length < samples.length) {
                    var idx = arr.length;
                    var v = m.value(samples[idx]);
                    arr.push(arr.length ? arr[arr.length - 1] : v);
                }
            });
        }

        function tweenSmooth() {
            var dirty = false;
            METRICS.forEach(function (m) {
                var arr = smooth[m.id];
                for (var i = 0; i < samples.length; i++) {
                    var target = m.value(samples[i]);
                    var cur = arr[i];
                    var next = cur + (target - cur) * SMOOTH_FACTOR;
                    if (Math.abs(next - cur) > 0.02) dirty = true;
                    arr[i] = next;
                }
            });
            return dirty;
        }

        function push(sample) {
            samples.push(sample);
            while (samples.length > WINDOW_SEC) {
                samples.shift();
            }
            syncSmoothLengths();
            updateCards(sample);
            document.getElementById('ndt-tegra-raw').textContent = sample.raw || '';
            if (hoverIndex !== null && hoverIndex >= samples.length) {
                hoverIndex = samples.length - 1;
            }
        }

        function updateCards(sample) {
            METRICS.forEach(function (m) {
                var card = document.getElementById('ndt-metric-' + m.id);
                if (!card) return;
                card.querySelector('.ndt-metric-value').textContent = m.text(sample);
                card.querySelector('.ndt-metric-sub').textContent = m.sub(sample);
            });
        }

        function plotX(idx, plotW) {
            if (samples.length <= 1) return layout.padL;
            return layout.padL + (idx / (samples.length - 1)) * plotW;
        }

        function plotY(value, y0, rowH, minV, maxV) {
            var norm = (value - minV) / Math.max(0.001, maxV - minV);
            norm = Math.max(0, Math.min(1, norm));
            return y0 + rowH - norm * (rowH - 4) - 2;
        }

        function strokeSmoothLine(xAt, yAt, color) {
            var n = samples.length;
            if (n < 2) return;

            ctx.beginPath();
            ctx.moveTo(xAt(0), yAt(0));
            for (var i = 0; i < n - 1; i++) {
                var x0 = xAt(i);
                var y0 = yAt(i);
                var x1 = xAt(i + 1);
                var y1 = yAt(i + 1);
                var mx = (x0 + x1) / 2;
                var my = (y0 + y1) / 2;
                ctx.quadraticCurveTo(x0, y0, mx, my);
            }
            ctx.lineTo(xAt(n - 1), yAt(n - 1));
            ctx.strokeStyle = color;
            ctx.lineWidth = 1.75;
            ctx.lineJoin = 'round';
            ctx.lineCap = 'round';
            ctx.stroke();
        }

        function draw() {
            var width = canvas.clientWidth;
            var height = canvas.clientHeight;
            if (!width || !height) return;

            layout.width = width;
            layout.height = height;
            var plotW = width - layout.padL - layout.padR;
            var rowH = (height - layout.padT - layout.padB - layout.rowGap * (METRICS.length - 1)) / METRICS.length;
            layout.rowH = rowH;

            ctx.clearRect(0, 0, width, height);

            METRICS.forEach(function (m, i) {
                var y0 = layout.padT + i * (rowH + layout.rowGap);
                var vals = smooth[m.id];

                ctx.fillStyle = '#64748b';
                ctx.font = '11px ui-monospace, monospace';
                ctx.textAlign = 'right';
                ctx.textBaseline = 'middle';
                ctx.fillText(m.label, layout.padL - 8, y0 + rowH * 0.5);

                ctx.strokeStyle = '#1e293b';
                ctx.lineWidth = 1;
                ctx.strokeRect(layout.padL, y0, plotW, rowH);

                if (vals.length < 2) return;

                strokeSmoothLine(
                    function (idx) { return plotX(idx, plotW); },
                    function (idx) { return plotY(vals[idx], y0, rowH, m.min, m.max); },
                    m.color
                );

                if (hoverIndex !== null && hoverIndex < vals.length) {
                    var hx = plotX(hoverIndex, plotW);
                    var hy = plotY(vals[hoverIndex], y0, rowH, m.min, m.max);
                    ctx.fillStyle = m.color;
                    ctx.beginPath();
                    ctx.arc(hx, hy, 3.5, 0, Math.PI * 2);
                    ctx.fill();
                    ctx.strokeStyle = '#f8fafc';
                    ctx.lineWidth = 1;
                    ctx.stroke();
                }
            });

            if (hoverIndex !== null && samples.length > 0) {
                var cx = plotX(hoverIndex, plotW);
                ctx.save();
                ctx.strokeStyle = 'rgba(148, 163, 184, 0.55)';
                ctx.lineWidth = 1;
                ctx.setLineDash([4, 4]);
                ctx.beginPath();
                ctx.moveTo(cx, layout.padT);
                ctx.lineTo(cx, height - layout.padB);
                ctx.stroke();
                ctx.restore();
            }

            ctx.fillStyle = '#475569';
            ctx.font = '10px ui-monospace, monospace';
            ctx.textAlign = 'center';
            ctx.textBaseline = 'top';
            ctx.fillText('−60s', layout.padL, height - layout.padB + 2);
            ctx.fillText('now', width - layout.padR, height - layout.padB + 2);
        }

        function showTooltip(idx, clientX, clientY) {
            var sample = samples[idx];
            if (!sample) {
                tooltip.hidden = true;
                return;
            }

            var rows = METRICS.map(function (m) {
                return '<div class="ndt-chart-tooltip-row">' +
                    '<span style="color:' + m.color + '">' + m.label + '</span>' +
                    '<span>' + m.text(sample) + '</span>' +
                    '</div>';
            }).join('');

            tooltip.innerHTML =
                '<div class="ndt-chart-tooltip-title">' + formatSampleAge(sample.ts_ms) + '</div>' +
                rows;
            tooltip.hidden = false;

            var rect = wrap.getBoundingClientRect();
            var left = clientX - rect.left + 12;
            var top = clientY - rect.top - 8;
            var tipW = tooltip.offsetWidth || 140;
            var tipH = tooltip.offsetHeight || 80;
            if (left + tipW > rect.width - 4) left = clientX - rect.left - tipW - 12;
            if (top + tipH > rect.height - 4) top = rect.height - tipH - 4;
            if (top < 4) top = 4;
            tooltip.style.left = left + 'px';
            tooltip.style.top = top + 'px';
        }

        function hideTooltip() {
            tooltip.hidden = true;
            hoverIndex = null;
        }

        function indexAtClientX(clientX) {
            if (!samples.length) return null;
            var rect = canvas.getBoundingClientRect();
            var x = clientX - rect.left;
            var plotW = layout.width - layout.padL - layout.padR;
            if (x < layout.padL - 6 || x > layout.padL + plotW + 6) return null;
            var ratio = (x - layout.padL) / Math.max(1, plotW);
            var idx = Math.round(ratio * (samples.length - 1));
            return Math.max(0, Math.min(samples.length - 1, idx));
        }

        function onPointerMove(ev) {
            var idx = indexAtClientX(ev.clientX);
            if (idx === null) {
                if (hoverIndex !== null) {
                    hoverIndex = null;
                    tooltip.hidden = true;
                }
                return;
            }
            hoverIndex = idx;
            showTooltip(idx, ev.clientX, ev.clientY);
        }

        function resizeCanvas() {
            var width = wrap.clientWidth - 16;
            var height = Math.max(220, Math.min(320, width * 0.38));
            var dpr = window.devicePixelRatio || 1;
            canvas.width = Math.floor(width * dpr);
            canvas.height = Math.floor(height * dpr);
            canvas.style.width = width + 'px';
            canvas.style.height = height + 'px';
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            if (samples.length) draw();
        }

        function frame() {
            tweenSmooth();
            if (samples.length) draw();
            rafId = window.requestAnimationFrame(frame);
        }

        canvas.addEventListener('mousemove', onPointerMove);
        canvas.addEventListener('mouseleave', hideTooltip);
        window.addEventListener('resize', resizeCanvas);
        resizeCanvas();
        rafId = window.requestAnimationFrame(frame);

        return {
            push: push,
            destroy: function () {
                window.cancelAnimationFrame(rafId);
            }
        };
    }

    function startTegraStream(charts) {
        var gotSample = false;
        var appId = mc.appId;
        if (!appId || !window.appWs) {
            setMonitorStatus('error', 'Stream unavailable');
            return;
        }

        window.appWs(appId, '/tegrastats/stream', function (d) {
            if (!d || String(d.type).toLowerCase() !== 'snapshot' || !d.data) {
                return;
            }
            gotSample = true;
            setMonitorStatus('live', 'Live');
            charts.push(d.data);
        }, { reconnectDelay: 2000 });

        setTimeout(function () {
            if (!gotSample) {
                setMonitorStatus('idle', 'Waiting for tegrastats…');
            }
        }, 5000);
    }

    wireCopyButton('ndt-copy-js', 'js');
    wireCopyButton('ndt-copy-root', 'root');
    document.getElementById('ndt-save').addEventListener('click', saveKeys);

    buildMetricCards();
    fillSshCommands();
    loadKeys();
    startTegraStream(TegraCharts());
})();
