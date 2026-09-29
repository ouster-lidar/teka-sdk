(function () {
    'use strict';

    var mc = jetstreamAppApi('nvidia-devtools');
    var SSH_PORT = 2223;
    var WINDOW_SEC = 300;
    var RAW_WINDOW_SEC = 30;
    var CHART_HEIGHT_MIN = 240;
    var CHART_HEIGHT_MAX = 960;
    var CHART_HEIGHT_DEFAULT = 560;
    var CHART_HEIGHT_KEY = 'ndt-chart-height';
    var RAW_HEIGHT_MIN = 72;
    var RAW_HEIGHT_MAX = 2400;
    var RAW_HEIGHT_DEFAULT = 72;
    var RAW_HEIGHT_KEY = 'ndt-raw-height';

    function watts(mw) {
        return ((mw || 0) / 1000).toFixed(1);
    }

    function coreSummary(s) {
        if (!s.cpu_cores || !s.cpu_cores.length) return 'avg active cores';
        var on = [];
        var off = 0;
        s.cpu_cores.forEach(function (c) {
            if (c === 'off') {
                off += 1;
                return;
            }
            on.push(String(Math.round(parseFloat(c))));
        });
        var text = on.join('/');
        if (off) text += ' · ' + off + ' off';
        return text;
    }

    var METRICS = [
        {
            id: 'ram_pct',
            label: 'RAM',
            color: '#f59e0b',
            value: function (s) { return s.ram_pct; },
            text: function (s) { return s.ram_pct.toFixed(1) + '%'; },
            sub: function (s) {
                var ram = s.ram_used_mb + '/' + s.ram_total_mb + ' MB';
                return s.lfb ? ram + ' · ' + s.lfb : ram;
            },
            min: 0,
            max: 100
        },
        {
            id: 'cpu_pct',
            label: 'CPU',
            color: '#38bdf8',
            value: function (s) { return s.cpu_pct; },
            text: function (s) { return s.cpu_pct.toFixed(1) + '%'; },
            sub: coreSummary,
            min: 0,
            max: 100
        },
        {
            id: 'gpu_pct',
            label: 'GPU',
            color: '#4ade80',
            value: function (s) { return s.gpu_pct; },
            text: function (s) { return s.gpu_pct.toFixed(1) + '%'; },
            sub: function (s) { return s.gpu_mhz ? s.gpu_mhz + ' MHz' : 'GR3D_FREQ'; },
            min: 0,
            max: 100
        },
        {
            id: 'emc_pct',
            label: 'EMC',
            color: '#22d3ee',
            value: function (s) { return s.emc_pct || 0; },
            text: function (s) { return (s.emc_pct || 0).toFixed(1) + '%'; },
            sub: function (s) { return s.emc_mhz ? s.emc_mhz + ' MHz mem' : 'memory'; },
            min: 0,
            max: 100
        },
        {
            id: 'gpu_temp_c',
            label: 'GPU temp',
            color: '#f87171',
            value: function (s) { return s.gpu_temp_c; },
            text: function (s) { return s.gpu_temp_c.toFixed(1) + '°C'; },
            sub: function (s) {
                return s.tj_temp_c ? 'tj ' + s.tj_temp_c.toFixed(1) + '°C' : 'gpu@ sensor';
            },
            min: 30,
            max: 90
        },
        {
            id: 'power_mw',
            label: 'Power',
            color: '#a78bfa',
            value: function (s) { return s.power_mw / 1000; },
            text: function (s) { return (s.power_mw / 1000).toFixed(2) + ' W'; },
            sub: function (s) {
                return 'CV ' + watts(s.vdd_cpu_gpu_cv_mw) +
                    ' · SOC ' + watts(s.vdd_soc_mw);
            },
            min: 0,
            max: 30
        }
    ];

    var ENGINE_ORDER = ['NVENC0', 'NVDEC0', 'NVJPG0', 'NVJPG1', 'VIC', 'OFA', 'NVDLA0', 'NVDLA1', 'PVA0'];
    var ENGINE_KEYS = [
        'NVENC0', 'NVENC1', 'NVDEC0', 'NVDEC1',
        'NVJPG0', 'NVJPG1', 'VIC', 'OFA',
        'NVDLA0', 'NVDLA1', 'PVA0_FREQ', 'PVA1_FREQ'
    ];

    function afterKey(line, key) {
        var idx = line.indexOf(key);
        if (idx < 0) return null;
        return line.slice(idx + key.length);
    }

    function parseRam(line) {
        var rest = afterKey(line, 'RAM ');
        if (!rest) return null;
        var mb = rest.indexOf('MB');
        if (mb < 0) return null;
        var pair = rest.slice(0, mb).split('/');
        var used = parseInt(pair[0], 10);
        var total = parseInt(pair[1], 10);
        if (!isFinite(used) || !isFinite(total)) return null;
        return { used: used, total: total };
    }

    function parseLfb(line) {
        var rest = afterKey(line, '(lfb ');
        if (!rest) return '';
        var end = rest.indexOf(')');
        return end < 0 ? '' : rest.slice(0, end);
    }

    function parseCpuCores(line) {
        var rest = afterKey(line, 'CPU [');
        if (!rest) return null;
        var end = rest.indexOf(']');
        if (end < 0) return null;
        return rest.slice(0, end).split(',').map(function (p) {
            return p.trim();
        }).filter(Boolean);
    }

    function cpuAvg(cores) {
        var sum = 0;
        var count = 0;
        cores.forEach(function (part) {
            if (part === 'off') return;
            var pct = parseFloat(part);
            if (!isFinite(pct)) return;
            sum += pct;
            count += 1;
        });
        return count ? sum / count : 0;
    }

    function parseMhzSuffix(after) {
        var t = String(after || '').replace(/^\s+/, '').replace(/^@/, '').replace(/^\[/, '');
        var m = t.match(/^\d+/);
        return m ? parseInt(m[0], 10) : 0;
    }

    function parseFreqToken(line, key) {
        var rest = afterKey(line, key);
        if (!rest) return null;
        var pctEnd = rest.indexOf('%');
        if (pctEnd < 0) return null;
        var pct = parseFloat(rest.slice(0, pctEnd));
        if (!isFinite(pct)) return null;
        return { pct: pct, mhz: parseMhzSuffix(rest.slice(pctEnd + 1)) };
    }

    function parseTemp(line, key) {
        var rest = afterKey(line, key);
        if (!rest) return null;
        var end = rest.indexOf('C');
        if (end < 0) return null;
        var n = parseFloat(rest.slice(0, end));
        return isFinite(n) ? n : null;
    }

    function parseRailMw(line, key) {
        var rest = afterKey(line, key);
        if (!rest) return null;
        var end = rest.indexOf('m');
        if (end < 0) return null;
        var n = parseInt(rest.slice(0, end), 10);
        return isFinite(n) ? n : null;
    }

    function parseEngines(line) {
        var out = [];
        ENGINE_KEYS.forEach(function (key) {
            var rest = afterKey(line, key + ' ');
            if (rest === null) return;
            var token = rest.trim().split(/\s+/)[0] || 'off';
            out.push({ id: key.replace(/_FREQ$/, ''), on: token !== 'off' });
        });
        return out;
    }

    function parseTegraLine(raw, tsMs) {
        if (!raw) return null;
        var ram = parseRam(raw);
        var cpuCores = parseCpuCores(raw);
        var gpu = parseFreqToken(raw, 'GR3D_FREQ ');
        var gpuTemp = parseTemp(raw, 'gpu@');
        var power = parseRailMw(raw, 'VDD_IN ');
        if (!ram || !cpuCores || !gpu || gpuTemp === null || power === null) return null;
        var emc = parseFreqToken(raw, 'EMC_FREQ ') || { pct: 0, mhz: 0 };
        var tj = parseTemp(raw, 'tj@');
        return {
            ts_ms: Number(tsMs) || Date.now(),
            raw: raw,
            ram_used_mb: ram.used,
            ram_total_mb: ram.total,
            ram_pct: ram.total > 0 ? (ram.used / ram.total) * 100 : 0,
            lfb: parseLfb(raw),
            cpu_pct: cpuAvg(cpuCores),
            cpu_cores: cpuCores,
            gpu_pct: gpu.pct,
            gpu_mhz: gpu.mhz,
            gpu_temp_c: gpuTemp,
            tj_temp_c: tj || 0,
            emc_pct: emc.pct,
            emc_mhz: emc.mhz,
            power_mw: power,
            vdd_cpu_gpu_cv_mw: parseRailMw(raw, 'VDD_CPU_GPU_CV ') || 0,
            vdd_soc_mw: parseRailMw(raw, 'VDD_SOC ') || 0,
            engines: parseEngines(raw)
        };
    }

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

    function flagMarkup(id, on) {
        return '<div class="ndt-flag" id="ndt-flag-' + id + '">' +
            '<span class="ndt-flag-label">' + id + '</span>' +
            '<span class="ndt-flag-value ndt-flag-value--' + (on ? 'on' : 'off') + '">' +
            (on ? 'on' : 'off') + '</span></div>';
    }

    function buildFlagPills() {
        document.getElementById('ndt-flags').innerHTML = '';
    }

    function updateFlags(sample) {
        var engines = sample.engines || [];
        var byId = {};
        engines.forEach(function (e) {
            byId[e.id] = !!e.on;
        });
        var ids = ENGINE_ORDER.filter(function (id) { return Object.prototype.hasOwnProperty.call(byId, id); });
        engines.forEach(function (e) {
            if (ids.indexOf(e.id) === -1) ids.push(e.id);
        });
        document.getElementById('ndt-flags').innerHTML = ids.map(function (id) {
            return flagMarkup(id, byId[id]);
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
        var rawLog = [];
        var rawEl = document.getElementById('ndt-tegra-raw');
        var layout = { padL: 42, padR: 10, padT: 8, padB: 18, width: 0, height: 0, rowH: 0, rowGap: 10 };
        var hoverIndex = null;
        var chartHeight = loadChartHeight();
        var handle = document.getElementById('ndt-chart-resize');

        function windowEndMs() {
            return samples.length ? samples[samples.length - 1].ts_ms : Date.now();
        }

        function trimSamples(endMs) {
            var cutoff = endMs - WINDOW_SEC * 1000;
            var dropped = 0;
            while (samples.length && samples[0].ts_ms < cutoff) {
                samples.shift();
                dropped += 1;
            }
            if (hoverIndex !== null && dropped) {
                hoverIndex -= dropped;
                if (hoverIndex < 0) hoverIndex = null;
            }
        }

        function push(sample) {
            samples.push(sample);
            trimSamples(sample.ts_ms || Date.now());
            updateCards(sample);
            updateFlags(sample);
            appendRawLog(sample);
            if (hoverIndex !== null && hoverIndex >= samples.length) {
                hoverIndex = samples.length - 1;
            }
            draw();
        }

        function appendRawLog(sample) {
            if (!sample.raw) return;
            rawLog.push({ ts_ms: sample.ts_ms, raw: sample.raw });
            var cutoff = (sample.ts_ms || Date.now()) - RAW_WINDOW_SEC * 1000;
            while (rawLog.length && rawLog[0].ts_ms < cutoff) {
                rawLog.shift();
            }
            rawEl.textContent = rawLog.map(function (e) { return e.raw; }).join('\n');
            rawEl.scrollTop = rawEl.scrollHeight;
        }

        function updateCards(sample) {
            METRICS.forEach(function (m) {
                var card = document.getElementById('ndt-metric-' + m.id);
                if (!card) return;
                card.querySelector('.ndt-metric-value').textContent = m.text(sample);
                card.querySelector('.ndt-metric-sub').textContent = m.sub(sample);
            });
        }

        function plotXAt(tsMs, plotW, nowMs) {
            var u = (tsMs - (nowMs - WINDOW_SEC * 1000)) / (WINDOW_SEC * 1000);
            return layout.padL + u * plotW;
        }

        function plotY(value, y0, rowH, minV, maxV) {
            var norm = (value - minV) / Math.max(0.001, maxV - minV);
            norm = Math.max(0, Math.min(1, norm));
            return y0 + rowH - norm * (rowH - 4) - 2;
        }

        function strokeSeries(m, y0, rowH, plotW, nowMs) {
            var n = samples.length;
            if (n < 2) return;

            ctx.beginPath();
            ctx.moveTo(plotXAt(samples[0].ts_ms, plotW, nowMs), plotY(m.value(samples[0]), y0, rowH, m.min, m.max));
            for (var i = 1; i < n; i++) {
                ctx.lineTo(
                    plotXAt(samples[i].ts_ms, plotW, nowMs),
                    plotY(m.value(samples[i]), y0, rowH, m.min, m.max)
                );
            }
            ctx.strokeStyle = m.color;
            ctx.lineWidth = 1.75;
            ctx.lineJoin = 'round';
            ctx.lineCap = 'round';
            ctx.stroke();
        }

        function draw() {
            var width = canvas.clientWidth;
            var height = canvas.clientHeight;
            if (!width || !height) return;

            var nowMs = windowEndMs();
            trimSamples(nowMs);

            layout.width = width;
            layout.height = height;
            var plotW = width - layout.padL - layout.padR;
            var rowH = (height - layout.padT - layout.padB - layout.rowGap * (METRICS.length - 1)) / METRICS.length;
            layout.rowH = rowH;

            ctx.clearRect(0, 0, width, height);

            METRICS.forEach(function (m, i) {
                var y0 = layout.padT + i * (rowH + layout.rowGap);

                ctx.fillStyle = '#64748b';
                ctx.font = '11px ui-monospace, monospace';
                ctx.textAlign = 'right';
                ctx.textBaseline = 'middle';
                ctx.fillText(m.label, layout.padL - 8, y0 + rowH * 0.5);

                ctx.strokeStyle = '#1e293b';
                ctx.lineWidth = 1;
                ctx.strokeRect(layout.padL, y0, plotW, rowH);

                if (samples.length < 2) return;

                ctx.save();
                ctx.beginPath();
                ctx.rect(layout.padL, y0, plotW, rowH);
                ctx.clip();
                strokeSeries(m, y0, rowH, plotW, nowMs);
                ctx.restore();

                if (hoverIndex !== null && hoverIndex < samples.length) {
                    var hx = plotXAt(samples[hoverIndex].ts_ms, plotW, nowMs);
                    var hy = plotY(m.value(samples[hoverIndex]), y0, rowH, m.min, m.max);
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
                var cx = plotXAt(samples[hoverIndex].ts_ms, plotW, nowMs);
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
            ctx.fillText('−300s', layout.padL, height - layout.padB + 2);
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
            if (hoverIndex === null) return;
            hoverIndex = null;
            draw();
        }

        function indexAtClientX(clientX) {
            if (!samples.length) return null;
            var rect = canvas.getBoundingClientRect();
            var x = clientX - rect.left;
            var plotW = layout.width - layout.padL - layout.padR;
            var nowMs = windowEndMs();
            var best = null;
            var bestD = Infinity;
            for (var i = 0; i < samples.length; i++) {
                var px = plotXAt(samples[i].ts_ms, plotW, nowMs);
                if (px < layout.padL - 6 || px > layout.padL + plotW + 6) continue;
                var d = Math.abs(px - x);
                if (d < bestD) {
                    bestD = d;
                    best = i;
                }
            }
            if (best === null || bestD > 14) return null;
            return best;
        }

        function onPointerMove(ev) {
            var idx = indexAtClientX(ev.clientX);
            if (idx === null) {
                hideTooltip();
                return;
            }
            var moved = hoverIndex !== idx;
            hoverIndex = idx;
            showTooltip(idx, ev.clientX, ev.clientY);
            if (moved) draw();
        }

        function clampChartHeight(h) {
            return Math.max(CHART_HEIGHT_MIN, Math.min(CHART_HEIGHT_MAX, Math.round(h)));
        }

        function loadChartHeight() {
            try {
                var stored = parseInt(window.localStorage.getItem(CHART_HEIGHT_KEY), 10);
                if (stored) return clampChartHeight(stored);
            } catch (e) { /* ignore */ }
            return CHART_HEIGHT_DEFAULT;
        }

        function saveChartHeight() {
            try {
                window.localStorage.setItem(CHART_HEIGHT_KEY, String(chartHeight));
            } catch (e) { /* ignore */ }
        }

        function resizeCanvas() {
            var width = wrap.clientWidth - 16;
            var height = chartHeight;
            var dpr = window.devicePixelRatio || 1;
            canvas.width = Math.floor(width * dpr);
            canvas.height = Math.floor(height * dpr);
            canvas.style.width = width + 'px';
            canvas.style.height = height + 'px';
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            if (samples.length) draw();
        }

        function startHeightDrag(ev) {
            ev.preventDefault();
            var startY = ev.clientY;
            var startH = chartHeight;
            wrap.classList.add('is-resizing');

            function onMove(e) {
                chartHeight = clampChartHeight(startH + (e.clientY - startY));
                resizeCanvas();
            }

            function onUp() {
                wrap.classList.remove('is-resizing');
                document.removeEventListener('pointermove', onMove);
                document.removeEventListener('pointerup', onUp);
                saveChartHeight();
            }

            document.addEventListener('pointermove', onMove);
            document.addEventListener('pointerup', onUp);
        }

        canvas.addEventListener('mousemove', onPointerMove);
        canvas.addEventListener('mouseleave', hideTooltip);
        if (handle) {
            handle.addEventListener('pointerdown', startHeightDrag);
            handle.addEventListener('keydown', function (ev) {
                var step = ev.shiftKey ? 48 : 16;
                if (ev.key === 'ArrowUp') {
                    ev.preventDefault();
                    chartHeight = clampChartHeight(chartHeight - step);
                    resizeCanvas();
                    saveChartHeight();
                } else if (ev.key === 'ArrowDown') {
                    ev.preventDefault();
                    chartHeight = clampChartHeight(chartHeight + step);
                    resizeCanvas();
                    saveChartHeight();
                }
            });
        }
        window.addEventListener('resize', resizeCanvas);
        resizeCanvas();

        return {
            push: push,
            destroy: function () { }
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
            var payload = d.data;
            var raw = typeof payload === 'string' ? payload : payload.raw;
            var sample = parseTegraLine(raw, payload && payload.ts_ms);
            if (!sample) return;
            gotSample = true;
            setMonitorStatus('live', 'Live');
            charts.push(sample);
        }, { reconnectDelay: 2000 });

        setTimeout(function () {
            if (!gotSample) {
                setMonitorStatus('idle', 'Waiting for tegrastats…');
            }
        }, 5000);
    }

    function clampRawHeight(h) {
        return Math.max(RAW_HEIGHT_MIN, Math.min(RAW_HEIGHT_MAX, Math.round(h)));
    }

    function wireRawResize() {
        var wrap = document.getElementById('ndt-tegra-raw-wrap');
        var pre = document.getElementById('ndt-tegra-raw');
        var handle = document.getElementById('ndt-tegra-raw-resize');
        if (!wrap || !pre || !handle) return;

        var height = RAW_HEIGHT_DEFAULT;
        try {
            var stored = parseInt(window.localStorage.getItem(RAW_HEIGHT_KEY), 10);
            if (stored) height = clampRawHeight(stored);
        } catch (e) { /* ignore */ }

        function apply() {
            pre.style.height = height + 'px';
        }

        function save() {
            try {
                window.localStorage.setItem(RAW_HEIGHT_KEY, String(height));
            } catch (e) { /* ignore */ }
        }

        handle.addEventListener('pointerdown', function (ev) {
            ev.preventDefault();
            var startY = ev.clientY;
            var startH = height;
            wrap.classList.add('is-resizing');

            function onMove(e) {
                height = clampRawHeight(startH + (e.clientY - startY));
                apply();
            }

            function onUp() {
                wrap.classList.remove('is-resizing');
                document.removeEventListener('pointermove', onMove);
                document.removeEventListener('pointerup', onUp);
                save();
            }

            document.addEventListener('pointermove', onMove);
            document.addEventListener('pointerup', onUp);
        });

        handle.addEventListener('keydown', function (ev) {
            var step = ev.shiftKey ? 48 : 16;
            if (ev.key === 'ArrowUp') {
                ev.preventDefault();
                height = clampRawHeight(height - step);
                apply();
                save();
            } else if (ev.key === 'ArrowDown') {
                ev.preventDefault();
                height = clampRawHeight(height + step);
                apply();
                save();
            }
        });

        apply();
    }

    wireCopyButton('ndt-copy-js', 'js');
    wireCopyButton('ndt-copy-root', 'root');
    document.getElementById('ndt-save').addEventListener('click', saveKeys);

    buildMetricCards();
    buildFlagPills();
    fillSshCommands();
    loadKeys();
    wireRawResize();
    startTegraStream(TegraCharts());
})();
