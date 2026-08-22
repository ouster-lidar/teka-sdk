(function () {
    'use strict';

    var appApi = mcAppApi('hello-ouster-world');
    var ws = null;
    var controlling = false;
    var hoverStream = false;
    var sourceW = 0;
    var sourceH = 0;
    /** Separate fields so status polls don't append into meta-source. */
    var metaSourceLabel = '';
    var metaSourceRes = '';

    function refreshMetaSource() {
        var meta = document.getElementById('meta-source');
        if (!meta) return;
        if (metaSourceLabel && metaSourceRes) {
            meta.textContent = metaSourceLabel + ' · ' + metaSourceRes;
        } else {
            meta.textContent = metaSourceLabel || metaSourceRes || '—';
        }
    }
    var activePointers = Object.create(null);
    var lastMoveSent = 0;
    var pendingMove = null;
    var moveTimer = null;
    var MOVE_MIN_MS = 16;
    var paintBusy = false;
    var pendingFrame = null;
    var displayWsError = '';
    var displayReconnectTimer = null;

    var canvas = document.getElementById('stream-canvas');
    var ctx = canvas.getContext('2d');
    var wrap = document.getElementById('stream-wrap');
    var streamIdleHint = document.getElementById('stream-idle-hint');
    var peerSourceHint = document.getElementById('peer-source-hint');
    var peerSourceTitle = document.getElementById('peer-source-title');
    var peerSourceBody = document.getElementById('peer-source-body');
    var peerSourceActive = false;
    var lastSourceCfg = null;
    var hdmiToggle = document.getElementById('hdmi-toggle');
    var hdmiLabel = document.getElementById('hdmi-label');
    var hdmiToggleWrap = document.getElementById('hdmi-toggle-wrap');
    var streamToggle = document.getElementById('stream-toggle');
    var streamLabel = document.getElementById('stream-label');
    var controlStatus = document.getElementById('control-status');
    var consoleCard = document.querySelector('.how-console-card');
    var reconnectBtn = document.getElementById('btn-reconnect');
    // Equal visual width: "Controlling" is 11 chars; pad Watching with NBSP.
    var LABEL_WATCHING = 'Watching\u00A0\u00A0\u00A0';
    var LABEL_CONTROLLING = 'Controlling';

    function showError(msg) {
        window.showInlineError('display-error', msg || '');
    }

    function viewerLimitBanner(raw) {
        var m = /too many display viewers \(max (\d+)\)/i.exec(raw || '');
        if (!m) return '';
        return 'Display stream is limited to ' + m[1] +
            ' connections. Close other hello-ouster-world tabs.';
    }

    function api(path) {
        return appApi.api(path);
    }

    function wsUrl() {
        var path = appApi.api('/display/stream');
        var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
        return proto + '//' + location.host + path;
    }

    function sendJson(obj) {
        if (!ws || ws.readyState !== WebSocket.OPEN) return;
        ws.send(JSON.stringify(obj));
    }

    function hasActivePointers() {
        for (var k in activePointers) {
            if (activePointers[k]) return true;
        }
        return false;
    }

    function setHdmiLabel(_on) {
        if (!hdmiLabel) return;
        // Unplugged cable: N/A (not the software HDMI OFF blanking state).
        if (hdmiToggle && hdmiToggle.disabled) {
            hdmiLabel.textContent = 'HDMI N/A';
        } else {
            hdmiLabel.textContent = 'Show on HDMI';
        }
        updateIdleHint();
    }

    function setStreamLabel(_on) {
        if (streamLabel) streamLabel.textContent = 'Show in Browser';
        updateIdleHint();
    }

    function updateIdleHint() {
        if (peerSourceHint) {
            peerSourceHint.hidden = !peerSourceActive;
        }
        if (streamIdleHint) {
            var browserOff = streamToggle && !streamToggle.checked;
            var hdmiOn = hdmiToggle && hdmiToggle.checked && !hdmiToggle.disabled;
            // Peer-source waiting overlay takes precedence over the browser-off hint.
            streamIdleHint.hidden = peerSourceActive
                || !(browserOff && hdmiOn);
        }
    }

    function isAppSource(src) {
        return src === 'app';
    }

    function setPeerSourceUi(cfg) {
        lastSourceCfg = cfg || lastSourceCfg;
        var src = (cfg && cfg.source) || (lastSourceCfg && lastSourceCfg.source) || '';
        var rt = (cfg && cfg.runtime) || {};
        var bridge = rt.bridge || '';
        // Treat missing/empty runtime as waiting when a peer socket is selected.
        var waiting = isAppSource(src) && bridge !== 'connected';
        if (waiting !== peerSourceActive) {
            peerSourceActive = waiting;
            if (waiting) clearStreamBlack();
        } else {
            peerSourceActive = waiting;
        }
        if (peerSourceHint) {
            peerSourceHint.hidden = !peerSourceActive;
        }
        if (peerSourceTitle) {
            peerSourceTitle.textContent = bridge === 'disconnected'
                ? 'Peer source disconnected'
                : 'Waiting for peer FrameSets';
        }
        if (peerSourceBody) {
            if (rt.message && String(rt.message).trim()) {
                peerSourceBody.textContent = String(rt.message);
            } else if (bridge === 'disconnected') {
                peerSourceBody.textContent =
                    'Restart the peer producer — visualization resumes automatically.';
            } else {
                peerSourceBody.textContent =
                    'Start Ouster Dynamo (proprietary, installed separately) '
                    + 'so it publishes /run/ouster-dynamo/frameset.sock. '
                    + 'Visualization starts automatically when FrameSets arrive.';
            }
        }
        updateIdleHint();
    }

    function setHdmiPresent(present) {
        var ok = present !== false;
        if (hdmiToggle) {
            hdmiToggle.disabled = !ok;
        }
        if (hdmiToggleWrap) {
            hdmiToggleWrap.classList.toggle('how-toggle-disabled', !ok);
            hdmiToggleWrap.title = ok
                ? 'On-device HDMI panel'
                : 'HDMI cable unplugged — connect a display to enable';
        }
        // Only force the checkbox off when the cable is truly gone. Software
        // blanking (HDMI toggle OFF) must leave the control usable.
        if (!ok) {
            hdmiToggle.checked = false;
            setHdmiLabel(false);
        }
        updateIdleHint();
    }

    function clearStreamBlack() {
        pendingFrame = null;
        paintBusy = false;
        if (!ctx || !canvas) return;
        ctx.save();
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.fillStyle = '#000';
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        ctx.restore();
    }

    function setControlUi(on) {
        controlling = !!on;
        wrap.classList.toggle('how-controlling', controlling);
        if (controlStatus) {
            controlStatus.textContent = controlling ? LABEL_CONTROLLING : LABEL_WATCHING;
            controlStatus.classList.toggle('how-control-active', controlling);
        }
        if (!controlling) {
            activePointers = Object.create(null);
            pendingMove = null;
            if (moveTimer) {
                clearTimeout(moveTimer);
                moveTimer = null;
            }
        } else {
            try {
                wrap.focus({ preventScroll: true });
            } catch (_) {
                wrap.focus();
            }
        }
    }

    function requestAcquire() {
        if (!ws || ws.readyState !== WebSocket.OPEN) return;
        sendJson({ type: 'control', action: 'acquire' });
    }

    function requestRelease() {
        if (!ws || ws.readyState !== WebSocket.OPEN) {
            setControlUi(false);
            return;
        }
        sendJson({ type: 'control', action: 'release' });
        setControlUi(false);
    }

    function updateMeta(meta) {
        if (!meta) return;
        if (meta.width && meta.height) {
            sourceW = meta.width;
            sourceH = meta.height;
            metaSourceRes = meta.width + '×' + meta.height;
            refreshMetaSource();
        }
        if (meta.stream_width && meta.stream_height) {
            document.getElementById('meta-stream').textContent =
                meta.stream_width + '×' + meta.stream_height;
        }
        if (typeof meta.hdmi_present === 'boolean') {
            setHdmiPresent(meta.hdmi_present);
        }
        if (typeof meta.hdmi === 'boolean' && meta.hdmi_present !== false) {
            hdmiToggle.checked = meta.hdmi;
            setHdmiLabel(meta.hdmi);
        }
        if (typeof meta.stream === 'boolean') {
            streamToggle.checked = meta.stream;
            setStreamLabel(meta.stream);
            if (!meta.stream) clearStreamBlack();
        }
    }

    function paintFrame(blob) {
        // Stream toggle off: keep panel black (HDMI is independent).
        if (streamToggle && !streamToggle.checked) {
            pendingFrame = null;
            return;
        }
        // Peer waiting / disconnected: keep the instruction overlay clear.
        if (peerSourceActive) {
            pendingFrame = null;
            return;
        }
        if (paintBusy) {
            pendingFrame = blob;
            return;
        }
        paintBusy = true;
        var done = function () {
            paintBusy = false;
            if (pendingFrame) {
                var next = pendingFrame;
                pendingFrame = null;
                paintFrame(next);
            }
        };
        if (typeof createImageBitmap === 'function') {
            createImageBitmap(blob).then(function (bmp) {
                if (canvas.width !== bmp.width || canvas.height !== bmp.height) {
                    canvas.width = bmp.width;
                    canvas.height = bmp.height;
                }
                ctx.drawImage(bmp, 0, 0);
                bmp.close();
                done();
            }).catch(done);
            return;
        }
        // Fallback: decode via Image without swapping the visible surface size to 0.
        var url = URL.createObjectURL(blob);
        var im = new Image();
        im.onload = function () {
            if (canvas.width !== im.naturalWidth || canvas.height !== im.naturalHeight) {
                canvas.width = im.naturalWidth;
                canvas.height = im.naturalHeight;
            }
            ctx.drawImage(im, 0, 0);
            URL.revokeObjectURL(url);
            done();
        };
        im.onerror = function () {
            URL.revokeObjectURL(url);
            done();
        };
        im.src = url;
    }

    function mapPointer(ev, clamp) {
        if (!sourceW || !sourceH || !canvas.width) {
            return null;
        }
        var rect = canvas.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) return null;
        var nx = (ev.clientX - rect.left) / rect.width;
        var ny = (ev.clientY - rect.top) / rect.height;
        if (!clamp && (nx < 0 || ny < 0 || nx > 1 || ny > 1)) return null;
        if (clamp) {
            nx = Math.min(1, Math.max(0, nx));
            ny = Math.min(1, Math.max(0, ny));
        }
        return {
            x: nx * sourceW,
            y: ny * sourceH
        };
    }

    function buttonName(btn) {
        if (btn === 1) return 'middle';
        if (btn === 2) return 'right';
        return 'left';
    }

    function flushMove(force) {
        if (!pendingMove) return;
        var now = Date.now();
        if (!force && now - lastMoveSent < MOVE_MIN_MS) {
            if (!moveTimer) {
                moveTimer = setTimeout(function () {
                    moveTimer = null;
                    flushMove(true);
                }, MOVE_MIN_MS - (now - lastMoveSent));
            }
            return;
        }
        sendJson({ type: 'pointer_move', x: pendingMove.x, y: pendingMove.y });
        pendingMove = null;
        lastMoveSent = now;
        if (moveTimer) {
            clearTimeout(moveTimer);
            moveTimer = null;
        }
    }

    function queueMove(p) {
        pendingMove = p;
        flushMove(false);
    }

    function connect() {
        if (displayReconnectTimer) {
            clearTimeout(displayReconnectTimer);
            displayReconnectTimer = null;
        }
        var stayingOnLimit = !!viewerLimitBanner(displayWsError);
        if (!stayingOnLimit) {
            showError('');
            displayWsError = '';
        }
        if (ws) {
            try {
                ws.onclose = null;
                ws.close();
            } catch (_) {}
            ws = null;
        }
        ws = new WebSocket(wsUrl());
        ws.binaryType = 'blob';

        ws.onopen = function () {
            showError('');
            displayWsError = '';
            if (hoverStream) requestAcquire();
        };

        ws.onmessage = function (ev) {
            if (typeof ev.data === 'string') {
                var msg;
                try {
                    msg = JSON.parse(ev.data);
                } catch (_) {
                    return;
                }
                if (msg.type === 'meta') {
                    updateMeta(msg);
                } else if (msg.type === 'control') {
                    if (!msg.ok) {
                        setControlUi(false);
                        showError(msg.message || 'control rejected');
                    } else {
                        setControlUi(msg.message === 'controlling');
                        if (msg.ok) showError('');
                    }
                } else if (msg.type === 'error') {
                    displayWsError = msg.message || 'display error';
                    showError(viewerLimitBanner(displayWsError) || displayWsError);
                }
                return;
            }
            paintFrame(ev.data);
        };

        ws.onerror = function () {
            showError('Display WebSocket error');
        };

        ws.onclose = function () {
            setControlUi(false);
            var limit = viewerLimitBanner(displayWsError);
            if (limit) {
                showError(limit);
                displayReconnectTimer = setTimeout(connect, 8000);
                return;
            }
            displayWsError = '';
            showError('Display disconnected — reconnecting…');
            displayReconnectTimer = setTimeout(connect, 2000);
        };
    }

    var framesetSourceHint = document.getElementById('frameset-source-hint');
    var cfgOsfSelect = document.getElementById('cfg-osf-select');
    var cfgOsfUploadInput = document.getElementById('cfg-osf-upload-input');
    var osfLibraryLink = document.getElementById('osf-sample-library');
    var btnOsfDownload = document.getElementById('btn-osf-download');
    var btnOsfDelete = document.getElementById('btn-osf-delete');
    var osfFetchProgress = document.getElementById('osf-fetch-progress');
    var osfFetchBarWrap = document.getElementById('osf-fetch-bar-wrap');
    var osfFetchBar = document.getElementById('osf-fetch-bar');
    var osfFetchStatus = document.getElementById('osf-fetch-status');
    var cfgAppSocket = document.getElementById('cfg-app-socket');
    var cfgSensorHost = document.getElementById('cfg-sensor-host');
    var viaDynamoToggle = document.getElementById('via-dynamo-toggle');
    var viaDynamoLabel = document.getElementById('via-dynamo-label');
    var viaDynamoHint = document.getElementById('via-dynamo-hint');
    var btnApplySource = document.getElementById('btn-apply-source');
    var btnStopSource = document.getElementById('btn-stop-source');
    var recordEnabledToggle = document.getElementById('record-enabled-toggle');
    var recordFilename = document.getElementById('record-filename');
    var viaDynamoWrap = document.getElementById('via-dynamo-wrap');
    var lastRecord = { active: false };
    var lastRuntime = {};
    var osfWrapWasVisible = false;
    var recordFilenameUserEdited = false;
    var lastSuggestedRecordName = '';
    var sourceBusy = false;
    var sourceBusyTimer = null;
    var SOURCE_BUSY_MS = 500;

    function setSourceBusy(busy) {
        sourceBusy = busy;
        if (btnApplySource) btnApplySource.disabled = busy;
        if (btnStopSource) btnStopSource.disabled = busy;
    }
    var sourceDraftDirty = false;
    var lastPersistedSource = null;
    var osfBusy = false;
    var osfFetchPollTimer = null;
    var pendingApplyOpts = null;
    var lastOsfCatalog = null;
    var osfXferKey = '';
    var osfXferSamples = [];
    var osfXferStarted = 0;
    var DYNAMO_APP_SOCKET = '/run/ouster-dynamo/frameset.sock';
    var SEED_PREFIX = 'seed:';
    var lidarKind = 'osf';

    function viaDynamo() {
        return !!(viaDynamoToggle && viaDynamoToggle.checked);
    }

    function vizSource() {
        if (viaDynamo()) return 'app';
        return lidarKind === 'sensor' ? 'sensor' : 'local_file';
    }

    function syncVizSourceSelect() {
        if (cfgAppSocket && viaDynamo() && !cfgAppSocket.value.trim()) {
            cfgAppSocket.value = DYNAMO_APP_SOCKET;
        }
    }

    function canonicalizeSource(src) {
        return src || 'local_file';
    }

    function sourceLabel(src, sock) {
        if (isAppSource(src)) {
            var lidar = lidarKind === 'sensor' ? 'sensor' : 'OSF';
            if (sock === DYNAMO_APP_SOCKET || !sock) return 'ouster-dynamo · ' + lidar;
            return 'app FrameSet socket · ' + lidar;
        }
        if (src === 'sensor') return 'connected sensor';
        return 'local file';
    }

    function selectedOsfPath() {
        if (!cfgOsfSelect) return '';
        var v = (cfgOsfSelect.value || '').trim();
        if (!v || v.indexOf(SEED_PREFIX) === 0) return '';
        return v;
    }

    function selectedSeedName() {
        if (!cfgOsfSelect) return '';
        var v = (cfgOsfSelect.value || '').trim();
        if (v.indexOf(SEED_PREFIX) === 0) return v.slice(SEED_PREFIX.length);
        return '';
    }

    function selectedOsfName() {
        var path = selectedOsfPath();
        if (!path) return '';
        var i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
        return i >= 0 ? path.slice(i + 1) : path;
    }

    function importRunning(catalog) {
        var imp = catalog && catalog.import;
        return !!(imp && imp.state === 'running');
    }

    function updateOsfActionBtns(catalog) {
        catalog = catalog || lastOsfCatalog;
        var running = importRunning(catalog);
        if (btnOsfDownload) {
            btnOsfDownload.disabled = osfBusy || running || !selectedOsfName();
        }
        if (btnOsfDelete) btnOsfDelete.disabled = osfBusy || running || !selectedOsfName();
    }

    function formatBytes(n) {
        var v = Number(n) || 0;
        if (v >= 1073741824) return (v / 1073741824).toFixed(2) + ' GB';
        if (v >= 1048576) return (v / 1048576).toFixed(1) + ' MB';
        if (v >= 1024) return (v / 1024).toFixed(1) + ' KB';
        return v + ' B';
    }

    function formatDuration(sec) {
        if (!isFinite(sec) || sec < 0) return '0s';
        var s = Math.round(sec);
        if (s < 60) return s + 's';
        var m = Math.floor(s / 60);
        var r = s % 60;
        if (m < 60) return r ? m + 'm ' + r + 's' : m + 'm';
        var h = Math.floor(m / 60);
        m = m % 60;
        return m ? h + 'h ' + m + 'm' : h + 'h';
    }

    function formatEtaSeconds(sec) {
        if (!isFinite(sec) || sec < 0) return '';
        var s = Math.round(sec);
        if (s < 8) return 'a few seconds left';
        return formatDuration(s) + ' left';
    }

    function resetOsfXfer(key) {
        osfXferKey = key || '';
        osfXferSamples = [];
        osfXferStarted = key ? Date.now() : 0;
    }

    function noteOsfXfer(key, loaded) {
        if (key !== osfXferKey) resetOsfXfer(key);
        osfXferSamples.push({ t: Date.now(), n: Number(loaded) || 0 });
        var cutoff = Date.now() - 12000;
        while (osfXferSamples.length > 2 && osfXferSamples[0].t < cutoff) {
            osfXferSamples.shift();
        }
    }

    function osfEtaText(loaded, total) {
        loaded = Number(loaded) || 0;
        total = Number(total) || 0;
        if (!total) return '';
        if (loaded >= total) return 'finishing…';
        if (osfXferSamples.length < 2) return '';
        var a = osfXferSamples[0];
        var b = osfXferSamples[osfXferSamples.length - 1];
        var dt = (b.t - a.t) / 1000;
        var dn = b.n - a.n;
        if (dt >= 2 && dn <= 0) return 'stalled';
        if (dt < 0.8 || dn <= 0) return '';
        return formatEtaSeconds((total - loaded) / (dn / dt));
    }

    function osfElapsedText() {
        if (!osfXferStarted) return '';
        return formatDuration((Date.now() - osfXferStarted) / 1000) + ' elapsed';
    }

    function setFetchCaption(msg, erred) {
        if (!osfFetchStatus) return;
        osfFetchStatus.textContent = msg || '';
        osfFetchStatus.className = 'how-osf-fetch-caption' + (erred ? ' how-osf-fetch-caption--err' : '');
    }

    function showOsfTransfer(opts) {
        opts = opts || {};
        var active = !!opts.active;
        var erred = !!opts.erred;
        var msg = opts.msg || '';
        if (osfFetchProgress) osfFetchProgress.hidden = !active && !erred && !msg;
        if (osfFetchBarWrap) osfFetchBarWrap.hidden = !active;
        if (active) {
            var loaded = Number(opts.loaded) || 0;
            var total = Number(opts.total) || 0;
            var p = total ? Math.min(100, Math.round((loaded / total) * 100)) : 0;
            if (osfFetchBar) osfFetchBar.style.width = p + '%';
            var parts = [];
            if (total) {
                parts.push(p + '% · ' + formatBytes(loaded) + ' / ' + formatBytes(total));
            } else {
                parts.push(formatBytes(loaded));
            }
            var elapsed = osfElapsedText();
            if (elapsed) parts.push(elapsed);
            var eta = osfEtaText(loaded, total);
            if (eta) parts.push(eta);
            setFetchCaption(parts.join(' · '), false);
            return;
        }
        if (osfFetchBar) osfFetchBar.style.width = '0%';
        setFetchCaption(msg, erred);
        if (!msg && !erred && osfFetchProgress) osfFetchProgress.hidden = true;
    }

    function updateOsfFetchUi(imp) {
        if (!imp) imp = { state: 'idle' };
        var running = imp.state === 'running';
        var erred = imp.state === 'error';
        if (running) {
            noteOsfXfer('dl:' + (imp.name || ''), imp.downloaded);
            showOsfTransfer({
                active: true,
                loaded: imp.downloaded,
                total: imp.total,
            });
        } else if (erred) {
            resetOsfXfer();
            showOsfTransfer({ erred: true, msg: imp.error || 'Fetch failed' });
        } else {
            resetOsfXfer();
            showOsfTransfer({});
        }
        updateOsfActionBtns();
    }

    function renderOsfSelect(catalog, preferredPath) {
        if (!cfgOsfSelect || !catalog) return;
        lastOsfCatalog = catalog;
        if (osfLibraryLink && catalog.sample_library_url) {
            osfLibraryLink.href = catalog.sample_library_url;
        }
        var files = catalog.files || [];
        var seeds = catalog.seeds || [];
        var onDevice = {};
        files.forEach(function (f) { onDevice[f.name] = true; });
        var prev = preferredPath || selectedOsfPath() || (selectedSeedName() ? SEED_PREFIX + selectedSeedName() : '');
        cfgOsfSelect.innerHTML = '';
        if (!files.length && !seeds.length) {
            var empty = document.createElement('option');
            empty.value = '';
            empty.textContent = '— none on device —';
            cfgOsfSelect.appendChild(empty);
        }
        files.forEach(function (f) {
            var opt = document.createElement('option');
            opt.value = f.path;
            opt.textContent = f.name + ' (' + formatBytes(f.size) + ')';
            cfgOsfSelect.appendChild(opt);
        });
        seeds.forEach(function (s) {
            if (onDevice[s.name]) return;
            var opt = document.createElement('option');
            opt.value = SEED_PREFIX + s.name;
            opt.textContent = s.name + ' — not on device · fetches on Apply';
            cfgOsfSelect.appendChild(opt);
        });
        if (prev) {
            for (var i = 0; i < cfgOsfSelect.options.length; i++) {
                if (cfgOsfSelect.options[i].value === prev) {
                    cfgOsfSelect.selectedIndex = i;
                    break;
                }
            }
        }
        if (!cfgOsfSelect.value && cfgOsfSelect.options.length) {
            cfgOsfSelect.selectedIndex = 0;
        }
        updateOsfFetchUi(catalog.import);
    }

    function loadOsfLibrary(preferredPath) {
        var prefer = preferredPath
            || (lastPersistedSource && lastPersistedSource.osf_path)
            || '';
        return fetchAppJson(api('/osfs')).then(function (catalog) {
            if (!catalog) return null;
            renderOsfSelect(catalog, prefer);
            if (importRunning(catalog) && !osfFetchPollTimer) startOsfFetchPoll();
            return catalog;
        }).catch(function () { return null; });
    }

    function clearOsfFetchPoll() {
        if (osfFetchPollTimer) {
            clearInterval(osfFetchPollTimer);
            osfFetchPollTimer = null;
        }
    }

    function startOsfFetchPoll() {
        clearOsfFetchPoll();
        osfFetchPollTimer = setInterval(function () {
            fetchAppJson(api('/osfs')).then(function (catalog) {
                if (!catalog) return;
                lastOsfCatalog = catalog;
                updateOsfFetchUi(catalog.import);
                if (importRunning(catalog)) return;
                clearOsfFetchPoll();
                osfBusy = false;
                renderOsfSelect(catalog);
                if (catalog.import && catalog.import.state === 'error') {
                    pendingApplyOpts = null;
                    setSourceBusy(false);
                    return;
                }
                var name = (catalog.import && catalog.import.name) || '';
                var path = '';
                (catalog.files || []).forEach(function (f) {
                    if (f.name === name) path = f.path;
                });
                if (path) {
                    renderOsfSelect(catalog, path);
                    lidarKind = 'osf';
                    updateSourceFieldsUi();
                    if (pendingApplyOpts) {
                        var ao = pendingApplyOpts;
                        pendingApplyOpts = null;
                        deliverSourceConfig(ao);
                    }
                } else if (pendingApplyOpts) {
                    pendingApplyOpts = null;
                    setSourceBusy(false);
                }
            }).catch(function () {});
        }, 1000);
    }

    function setOsfStatus(msg, kind) {
        resetOsfXfer();
        showOsfTransfer({ msg: msg || '', erred: kind === 'ro' });
    }

    function uploadOsfFile(file) {
        if (!file || osfBusy) return Promise.reject(new Error('No file selected'));
        var name = file.name || 'upload.osf';
        if (!/\.osf$/i.test(name)) {
            return Promise.reject(new Error('File must have .osf extension'));
        }
        osfBusy = true;
        updateOsfActionBtns();
        resetOsfXfer('ul:' + name);
        showOsfTransfer({ active: true, loaded: 0, total: file.size || 0 });
        return new Promise(function (resolve, reject) {
            var xhr = new XMLHttpRequest();
            xhr.open('POST', api('/osfs/' + encodeURIComponent(name)), true);
            xhr.withCredentials = true;
            xhr.setRequestHeader('Content-Type', 'application/octet-stream');
            xhr.upload.onprogress = function (ev) {
                var loaded = ev.loaded;
                var total = ev.lengthComputable ? ev.total : (file.size || 0);
                noteOsfXfer('ul:' + name, loaded);
                showOsfTransfer({
                    active: true,
                    loaded: loaded,
                    total: total,
                });
            };
            xhr.onload = function () {
                var body = null;
                try { body = JSON.parse(xhr.responseText); } catch (_) {}
                if (xhr.status >= 200 && xhr.status < 300 && body && body.ok) {
                    resolve(body);
                    return;
                }
                var msg = (body && body.error && body.error.message)
                    || ('Upload failed (' + xhr.status + ')');
                reject(new Error(msg));
            };
            xhr.onerror = function () { reject(new Error('Upload failed')); };
            xhr.send(file);
        }).then(function (body) {
            resetOsfXfer();
            showOsfTransfer({});
            return loadOsfLibrary(body.path).then(function () { return body; });
        }).finally(function () {
            osfBusy = false;
            updateOsfActionBtns();
            if (cfgOsfUploadInput) cfgOsfUploadInput.value = '';
        });
    }

    function startOsfFetch() {
        if (osfBusy || importRunning(lastOsfCatalog)) return;
        var name = selectedSeedName();
        if (!name) {
            setOsfStatus('Select a Studio example that is not on the device', 'state');
            pendingApplyOpts = null;
            setSourceBusy(false);
            return;
        }
        osfBusy = true;
        updateOsfActionBtns();
        resetOsfXfer('dl:' + name);
        showOsfTransfer({ active: true, loaded: 0, total: 0 });
        postJson(api('/osfs/imports'), { name: name })
            .then(function (resp) {
                if (!resp) throw new Error('Fetch failed');
                if (resp.error) {
                    throw new Error(resp.error.message || 'Fetch failed');
                }
                if (resp.already_present) {
                    osfBusy = false;
                    resetOsfXfer();
                    return loadOsfLibrary(resp.path).then(function (catalog) {
                        if (pendingApplyOpts) {
                            var ao = pendingApplyOpts;
                            pendingApplyOpts = null;
                            deliverSourceConfig(ao);
                        }
                        return catalog;
                    });
                }
                startOsfFetchPoll();
            })
            .catch(function (e) {
                osfBusy = false;
                pendingApplyOpts = null;
                setSourceBusy(false);
                updateOsfActionBtns();
                setOsfStatus(e.message || String(e), 'ro');
            });
    }

    function downloadOsfToBrowser() {
        var name = selectedOsfName();
        if (!name || osfBusy || importRunning(lastOsfCatalog)) return;
        var a = document.createElement('a');
        a.href = api('/osfs/' + encodeURIComponent(name));
        a.download = name;
        a.rel = 'noopener';
        document.body.appendChild(a);
        a.click();
        a.remove();
    }

    function deleteSelectedOsf() {
        var name = selectedOsfName();
        if (!name || osfBusy || importRunning(lastOsfCatalog)) return;
        osfBusy = true;
        updateOsfActionBtns();
        window.fetchAppJsonPost(api('/osfs/' + encodeURIComponent(name)), {}, 'DELETE')
            .then(function (resp) {
                if (!resp || resp.error) {
                    throw new Error((resp && resp.error && resp.error.message) || 'Delete failed');
                }
                return loadOsfLibrary();
            })
            .catch(function (e) {
                setOsfStatus(e.message || String(e), 'ro');
            })
            .finally(function () {
                osfBusy = false;
                updateOsfActionBtns();
            });
    }

    function recordDraftEnabled() {
        return !!(recordEnabledToggle && recordEnabledToggle.checked);
    }

    function recordingUiLocked() {
        return recordDraftEnabled() || !!(lastRecord && lastRecord.active);
    }

    function syncRecordControlsUi() {
        if (recordFilename) {
            recordFilename.disabled = !recordDraftEnabled() || !!(lastRecord && lastRecord.active);
        }
        if (viaDynamoWrap) {
            viaDynamoWrap.classList.toggle('how-toggle-disabled', recordDraftEnabled());
            viaDynamoWrap.title = recordDraftEnabled()
                ? 'Recording requires direct sensor input (Dynamo off)'
                : '';
        }
    }

    function defaultRecordFilename() {
        var d = new Date();
        var y = d.getFullYear();
        var m = ('0' + (d.getMonth() + 1)).slice(-2);
        var day = ('0' + d.getDate()).slice(-2);
        var h = ('0' + d.getHours()).slice(-2);
        var mi = ('0' + d.getMinutes()).slice(-2);
        var s = ('0' + d.getSeconds()).slice(-2);
        return 'hello-ouster-world-' + y + m + day + '-' + h + mi + s + '.osf';
    }

    function ensureOsfFilename(name) {
        var s = String(name || '').trim();
        if (!s) return '';
        if (/\.osf$/i.test(s)) return s;
        return s + '.osf';
    }

    function suggestRecordFilenameIfNeeded() {
        if (!recordFilename || !recordDraftEnabled()) return;
        var suggested = defaultRecordFilename();
        lastSuggestedRecordName = suggested;
        var cur = recordFilename.value.trim();
        if (!recordFilenameUserEdited || !cur) {
            recordFilename.value = suggested;
        }
    }

    function normalizeRecordFilenameInput() {
        if (!recordFilename) return '';
        var v = ensureOsfFilename(recordFilename.value);
        if (v) recordFilename.value = v;
        return v;
    }

    function sourceEnabled(cfg) {
        if (!cfg) return true;
        if (typeof cfg.source_enabled === 'boolean') return cfg.source_enabled;
        return true;
    }

    function syncLidarSegments() {
        document.querySelectorAll('.how-source-mode-seg[data-lidar]').forEach(function (btn) {
            var kind = btn.getAttribute('data-lidar');
            var on = kind === lidarKind;
            btn.classList.toggle('how-source-mode-seg-active', on);
            btn.classList.toggle('btn-secondary', !on);
        });
    }

    function updateSourceFieldsUi() {
        syncVizSourceSelect();
        var osfWrap = document.getElementById('cfg-osf-wrap');
        var osfActions = document.getElementById('how-osf-actions');
        var sensorWrap = document.getElementById('cfg-sensor-wrap');
        var showOsf = lidarKind === 'osf';
        var showSensor = lidarKind === 'sensor';
        if (osfWrap) osfWrap.style.display = showOsf ? '' : 'none';
        if (osfActions) osfActions.style.display = showOsf ? '' : 'none';
        if (sensorWrap) sensorWrap.style.display = showSensor ? '' : 'none';
        if (viaDynamoLabel) {
            viaDynamoLabel.textContent = viaDynamo() ? 'On' : 'Off';
        }
        if (viaDynamoToggle) {
            viaDynamoToggle.disabled = recordingUiLocked();
        }
        if (viaDynamoHint) {
            if (recordingUiLocked()) {
                viaDynamoHint.textContent = 'Dynamo off while recording (direct sensor lidar)';
            } else {
                viaDynamoHint.textContent = viaDynamo()
                    ? 'Dynamo ingest — detections on the app FrameSet stream'
                    : 'Direct lidar/OSF — no Dynamo detections overlay';
            }
        }
        if (showOsf && !osfWrapWasVisible) {
            loadOsfLibrary();
        }
        osfWrapWasVisible = showOsf;
        syncRecordControlsUi();
        syncLidarSegments();
    }

    function applyPersistedSource(cfg) {
        if (!cfg) return;
        lastPersistedSource = cfg;
        if (sourceDraftDirty || sourceBusy) return;
        var src = canonicalizeSource(cfg.source);
        var via = typeof cfg.via_dynamo === 'boolean' ? cfg.via_dynamo : isAppSource(src);
        if (viaDynamoToggle) viaDynamoToggle.checked = via;
        if (typeof cfg.producer_kind === 'string' && cfg.producer_kind) {
            lidarKind = cfg.producer_kind === 'sensor' ? 'sensor' : 'osf';
        } else {
            lidarKind = src === 'sensor' ? 'sensor' : 'osf';
        }
        if (cfgOsfSelect && typeof cfg.osf_path === 'string' && cfg.osf_path) {
            var osfPath = cfg.osf_path;
            var matched = false;
            for (var i = 0; i < cfgOsfSelect.options.length; i++) {
                if (cfgOsfSelect.options[i].value === osfPath) {
                    cfgOsfSelect.selectedIndex = i;
                    matched = true;
                    break;
                }
            }
            if (!matched && lastOsfCatalog) {
                renderOsfSelect(lastOsfCatalog, osfPath);
            }
        }
        var sock = cfg.app_socket;
        if (cfgAppSocket && typeof sock === 'string') {
            cfgAppSocket.value = sock;
        }
        if (cfgSensorHost && typeof cfg.sensor_hostname === 'string') {
            cfgSensorHost.value = cfg.sensor_hostname;
        }
        if (recordEnabledToggle && typeof cfg.record_enabled === 'boolean') {
            recordEnabledToggle.checked = cfg.record_enabled;
        }
        if (recordFilename && typeof cfg.record_filename === 'string' && cfg.record_filename) {
            recordFilename.value = ensureOsfFilename(cfg.record_filename);
            recordFilenameUserEdited = true;
            lastSuggestedRecordName = recordFilename.value;
        } else if (recordEnabledToggle && recordEnabledToggle.checked) {
            recordFilenameUserEdited = false;
            suggestRecordFilenameIfNeeded();
        }
        var srcForLabel = vizSource();
        var sockForLabel = cfgAppSocket ? cfgAppSocket.value.trim() : '';
        metaSourceLabel = sourceLabel(srcForLabel, sockForLabel);
        refreshMetaSource();
        updateSourceFieldsUi();
    }

    function applySourceRuntime(rt, extra) {
        extra = extra || {};
        rt = rt || {};
        lastRuntime = rt;
        var srcForLabel = vizSource();
        var sockForLabel = cfgAppSocket ? cfgAppSocket.value.trim() : '';
        var sockAvailable = extra.app_socket_available;
        if (typeof sockAvailable !== 'boolean' && lastPersistedSource) {
            sockAvailable = lastPersistedSource.app_socket_available;
        }
        if (framesetSourceHint) {
            if (!sourceEnabled(lastPersistedSource)) {
                framesetSourceHint.textContent = 'Source stopped — Apply to resume';
                framesetSourceHint.hidden = false;
                framesetSourceHint.className = 'app-meta-chip app-meta-chip--state';
            } else if (!isAppSource(srcForLabel) && srcForLabel !== 'sensor') {
                framesetSourceHint.hidden = true;
                framesetSourceHint.textContent = '';
            } else {
                var hint = sourceLabel(srcForLabel, sockForLabel);
                var chip = 'app-meta-chip app-meta-chip--type';
                if (srcForLabel === 'sensor') {
                    if (lastRecord && lastRecord.active) {
                        hint += ' · recording';
                        chip = 'app-meta-chip app-meta-chip--usage';
                    } else if (recordDraftEnabled()) {
                        hint += ' · record pending Apply';
                    }
                }
                if (isAppSource(srcForLabel)) {
                    var bridge = rt.bridge || '';
                    if (bridge === 'connected') {
                        hint += ' · connected';
                    } else if (bridge === 'disconnected') {
                        hint += ' · disconnected (waiting)';
                    } else if (bridge === 'waiting') {
                        hint += ' · waiting for app';
                    } else {
                        hint += sockAvailable
                            ? ' · socket ready'
                            : ' · socket missing (start producer)';
                    }
                }
                framesetSourceHint.textContent = hint;
                framesetSourceHint.hidden = false;
                if (isAppSource(srcForLabel)) {
                    var st = rt.bridge || '';
                    if (st === 'connected') chip = 'app-meta-chip app-meta-chip--usage';
                    else if (st === 'disconnected' || st === 'waiting') chip = 'app-meta-chip app-meta-chip--state';
                    else if (!sockAvailable) chip = 'app-meta-chip app-meta-chip--ro';
                }
                framesetSourceHint.className = chip;
            }
        }
        setPeerSourceUi({
            source: srcForLabel,
            app_socket_available: sockAvailable,
            runtime: rt,
        });
    }

    function applyDisplayStatus(d) {
        if (!d) return;
        updateMeta(d);
        if (typeof d.hdmi_present === 'boolean') setHdmiPresent(d.hdmi_present);
        if (typeof d.hdmi === 'boolean' && d.hdmi_present !== false) {
            hdmiToggle.checked = d.hdmi;
            setHdmiLabel(d.hdmi);
        }
        if (typeof d.stream === 'boolean') {
            streamToggle.checked = d.stream;
            setStreamLabel(d.stream);
            if (!d.stream) clearStreamBlack();
        }
        if (Object.prototype.hasOwnProperty.call(d, 'error')) {
            if (d.error) showError(d.error);
            else showError('');
        }
    }

    function applyRecordStatus(rec) {
        lastRecord = rec || { active: false };
        if (lastRecord.error) {
            showError(lastRecord.error);
        }
        updateSourceFieldsUi();
        applySourceRuntime(lastRuntime);
        if (!lastRecord.active && lidarKind === 'osf') {
            loadOsfLibrary();
        }
    }

    function applyUiState(msg) {
        if (!msg) return;
        var t = String(msg.type || '').toLowerCase();
        if (t === 'heartbeat') return;
        if (t !== 'snapshot' && t !== 'delta') return;
        var data = t === 'snapshot' ? msg.data : msg.patch;
        if (!data) return;
        if (data.source) applyPersistedSource(data.source);
        if (data.runtime) applySourceRuntime(data.runtime);
        if (data.display) applyDisplayStatus(data.display);
        if (data.record) applyRecordStatus(data.record);
    }

    function bootStatusStream() {
        if (window.appWs) {
            window.appWs(appApi.appId, '/status/stream', function (d) {
                applyUiState(d);
            }, { reconnectDelay: 2000 });
        }
    }

    function deliverSourceConfig(opts) {
        opts = opts || {};
        var stoppingAll = !!opts.stoppingAll;
        var wantRecord = stoppingAll ? false : recordDraftEnabled();
        if (wantRecord) {
            lidarKind = 'sensor';
            if (viaDynamoToggle) viaDynamoToggle.checked = false;
            syncVizSourceSelect();
            normalizeRecordFilenameInput();
        }
        var src = vizSource();
        var sock = DYNAMO_APP_SOCKET;
        var osfPath = selectedOsfPath();
        if (!wantRecord && !stoppingAll && lidarKind === 'osf' && !osfPath) {
            var seed = selectedSeedName();
            if (seed) {
                pendingApplyOpts = opts;
                setSourceBusy(true);
                if (importRunning(lastOsfCatalog)) {
                    if (!osfFetchPollTimer) startOsfFetchPoll();
                    return;
                }
                startOsfFetch();
                return;
            }
            showError('Select an OSF on the device (Upload, or pick a catalog example to fetch on Apply)');
            return;
        }
        pendingApplyOpts = null;
        var host = cfgSensorHost ? cfgSensorHost.value.trim() : '';
        if (!stoppingAll && (wantRecord || lidarKind === 'sensor') && !host) {
            showError('Enter sensor hostname or IP');
            return;
        }
        var body = {
            source: src,
            osf_path: osfPath,
            sensor_hostname: cfgSensorHost ? cfgSensorHost.value : '',
            app_socket: sock,
            producer_kind: wantRecord ? 'sensor' : lidarKind,
            via_dynamo: wantRecord || stoppingAll ? false : viaDynamo(),
            record_enabled: wantRecord,
            source_enabled: !stoppingAll,
            record_filename: wantRecord ? normalizeRecordFilenameInput() : (
                recordFilename ? ensureOsfFilename(recordFilename.value) : ''
            ),
        };
        setSourceBusy(true);
        showError('');
        postJson(api('/source-config'), body)
            .then(function (resp) {
                if (!resp || resp.error) {
                    throw new Error(
                        (resp && resp.error && resp.error.message) || 'Failed to save source'
                    );
                }
                sourceDraftDirty = false;
                if (resp.config) {
                    applyPersistedSource(resp.config);
                    if (resp.config.runtime) {
                        applySourceRuntime(resp.config.runtime);
                    } else if (isAppSource(resp.config.source)) {
                        applySourceRuntime({
                            bridge: 'waiting',
                            message:
                                'Waiting for app FrameSets on '
                                + (resp.config.app_socket || sock)
                                + '. Visualization starts automatically when connected.',
                        });
                    }
                }
                if (resp.record) applyRecordStatus(resp.record);
                showError('');
                if (resp.dynamo && resp.dynamo.ok === false) {
                    showError(
                        'Source saved; Dynamo not updated: '
                        + (resp.dynamo.error || resp.dynamo.status || 'unreachable')
                    );
                }
                if (sourceBusyTimer) clearTimeout(sourceBusyTimer);
                sourceBusyTimer = setTimeout(function () {
                    sourceBusyTimer = null;
                    setSourceBusy(false);
                    showError('');
                }, SOURCE_BUSY_MS);
            })
            .catch(function (e) {
                showError(e.message || String(e));
                setSourceBusy(false);
            });
    }

    function postSourceConfig(opts) {
        if (sourceBusy) return;
        deliverSourceConfig(opts);
    }

    function hydrateStatus() {
        return fetchAppJson(api('/status')).then(function (st) {
            if (st && st.source) applyPersistedSource(st.source);
            if (st && st.display) applyDisplayStatus(st.display);
            if (st && st.record) applyRecordStatus(st.record);
            return st;
        });
    }

    function fetchAppJson(url) {
        if (window.fetchAppJson) return window.fetchAppJson(url);
        return fetch(url, { credentials: 'include' }).then(function (r) {
            return r.ok ? r.json() : null;
        });
    }

    function postJson(url, body) {
        return fetch(url, {
            method: 'POST',
            credentials: 'include',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body)
        }).then(function (r) {
            return r.json().then(function (data) {
                if (!r.ok) {
                    var msg = (data && data.error && data.error.message) || r.statusText || 'request failed';
                    return { error: { message: msg } };
                }
                return data;
            }).catch(function () {
                if (!r.ok) return { error: { message: r.statusText || 'request failed' } };
                return null;
            });
        });
    }

    hdmiToggle.addEventListener('change', function () {
        if (hdmiToggle.disabled) return;
        var enabled = hdmiToggle.checked;
        setHdmiLabel(enabled);
        postJson(api('/display/hdmi'), { enabled: enabled }).then(function (st) {
            updateMeta(st);
            if (st && st.error) showError(st.error);
            else showError('');
        }).catch(function () {
            showError('HDMI toggle failed');
        });
    });

    streamToggle.addEventListener('change', function () {
        var enabled = streamToggle.checked;
        setStreamLabel(enabled);
        if (!enabled) clearStreamBlack();
        postJson(api('/display/stream-enabled'), { enabled: enabled }).then(function (st) {
            updateMeta(st);
            if (st && st.error) showError(st.error);
            else showError('');
            if (!enabled) clearStreamBlack();
        }).catch(function () {
            showError('Stream toggle failed');
        });
    });

    wrap.addEventListener('pointerenter', function () {
        hoverStream = true;
        try {
            wrap.focus({ preventScroll: true });
        } catch (_) {
            wrap.focus();
        }
        requestAcquire();
    });

    wrap.addEventListener('pointerleave', function (ev) {
        // Frame paints / capture must not drop the lease mid-drag.
        if (hasActivePointers()) return;
        if (ev.relatedTarget && wrap.contains(ev.relatedTarget)) return;
        hoverStream = false;
        requestRelease();
    });

    wrap.addEventListener('pointerdown', function (ev) {
        if (!controlling && hoverStream) {
            // Race: enter acquire reply not yet applied — still send input after forcing acquire.
            requestAcquire();
            controlling = true;
            setControlUi(true);
        }
        if (!controlling) return;
        if (ev.button !== 0 && ev.button !== 1 && ev.button !== 2) return;
        ev.preventDefault();
        try {
            wrap.focus({ preventScroll: true });
        } catch (_) {
            wrap.focus();
        }
        try {
            wrap.setPointerCapture(ev.pointerId);
        } catch (_) {}
        activePointers[ev.pointerId] = true;
        var p = mapPointer(ev, true);
        if (!p) return;
        flushMove(true);
        sendJson({
            type: 'pointer_button',
            button: buttonName(ev.button),
            state: 'down',
            x: p.x,
            y: p.y
        });
    });

    wrap.addEventListener('pointermove', function (ev) {
        if (!controlling) return;
        var dragging = !!activePointers[ev.pointerId];
        var p = mapPointer(ev, dragging);
        if (!p) return;
        queueMove(p);
    });

    function endPointer(ev, sendUp) {
        var wasActive = !!activePointers[ev.pointerId];
        delete activePointers[ev.pointerId];
        if (!wasActive) return;
        if (controlling && sendUp) {
            var p = mapPointer(ev, true) || { x: 0, y: 0 };
            flushMove(true);
            sendJson({
                type: 'pointer_button',
                button: buttonName(ev.button),
                state: 'up',
                x: p.x,
                y: p.y
            });
        }
        try {
            wrap.releasePointerCapture(ev.pointerId);
        } catch (_) {}
        // If the pointer ended outside the stream, release the lease.
        if (!hoverStream && !hasActivePointers()) {
            requestRelease();
        }
    }

    wrap.addEventListener('pointerup', function (ev) {
        endPointer(ev, true);
    });

    wrap.addEventListener('pointercancel', function (ev) {
        endPointer(ev, true);
    });

    wrap.addEventListener('lostpointercapture', function (ev) {
        if (activePointers[ev.pointerId]) {
            endPointer(ev, true);
        }
    });

    wrap.addEventListener('wheel', function (ev) {
        if (!controlling) return;
        ev.preventDefault();
        sendJson({ type: 'pointer_axis', delta_x: ev.deltaX, delta_y: ev.deltaY });
    }, { passive: false });

    wrap.addEventListener('contextmenu', function (ev) {
        if (controlling || hoverStream) ev.preventDefault();
    });

    function isTypingTarget(el) {
        if (!el || el === document.body) return false;
        var tag = el.tagName;
        if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA' || tag === 'BUTTON') {
            return true;
        }
        if (el.isContentEditable) return true;
        if (el.closest && (el.closest('.how-terminal') || el.closest('.xterm'))) {
            return true;
        }
        return false;
    }

    function onKey(ev, state) {
        if (!controlling) return;
        if (isTypingTarget(ev.target)) return;
        ev.preventDefault();
        sendJson({
            type: 'key',
            key: ev.key,
            code: ev.code,
            state: state,
            shift: !!ev.shiftKey,
            ctrl: !!ev.ctrlKey,
            alt: !!ev.altKey,
            meta: !!ev.metaKey
        });
    }

    window.addEventListener('keydown', function (ev) { onKey(ev, 'down'); });
    window.addEventListener('keyup', function (ev) { onKey(ev, 'up'); });

    var term = null;
    var fitAddon = null;
    var ptyWs = null;
    var ptyReconnectTimer = null;
    var ptyReconnectAttempt = 0;
    var ptyWantConnected = true;

    function showConsoleError(msg) {
        window.showInlineError('console-error', msg || '');
    }

    function ptyUrl() {
        var path = appApi.api('/pty');
        var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
        return proto + '//' + location.host + path;
    }

    function sendPty(obj) {
        if (!ptyWs || ptyWs.readyState !== WebSocket.OPEN) return;
        ptyWs.send(JSON.stringify(obj));
    }

    function fitAndResize() {
        if (!fitAddon || !term) return;
        try {
            fitAddon.fit();
        } catch (_) {}
        sendPty({ type: 'resize', cols: term.cols, rows: term.rows });
    }

    function setReconnectUi(connected) {
        if (!reconnectBtn) return;
        if (connected) {
            reconnectBtn.textContent = 'Connected';
            reconnectBtn.disabled = true;
            reconnectBtn.setAttribute('aria-disabled', 'true');
        } else {
            reconnectBtn.textContent = 'Reconnect';
            reconnectBtn.disabled = false;
            reconnectBtn.removeAttribute('aria-disabled');
        }
    }

    function setCwdDisplay(path) {
        var el = document.getElementById('meta-cwd');
        if (!el || !path) return;
        el.innerHTML = '';
        var code = document.createElement('code');
        code.textContent = path;
        el.appendChild(code);
    }

    function parseOsc7Path(data) {
        // OSC 7: file://hostname/path  or file:///path
        if (!data || data.indexOf('file://') !== 0) return null;
        var rest = data.slice('file://'.length);
        // Drop hostname if present (file://host/path)
        if (rest.charAt(0) !== '/') {
            var slash = rest.indexOf('/');
            if (slash < 0) return null;
            rest = rest.slice(slash);
        }
        try {
            return decodeURIComponent(rest);
        } catch (_) {
            return rest;
        }
    }

    function clearPtyReconnect() {
        if (ptyReconnectTimer) {
            clearTimeout(ptyReconnectTimer);
            ptyReconnectTimer = null;
        }
    }

    function schedulePtyReconnect(reason) {
        if (!ptyWantConnected) return;
        clearPtyReconnect();
        var attempt = ptyReconnectAttempt;
        ptyReconnectAttempt = attempt + 1;
        // 0.5s, 1s, 2s, … capped at 5s — covers app restart / page load races.
        var delay = Math.min(5000, 500 * Math.pow(2, Math.min(attempt, 3)));
        var msg = reason
            ? (reason + ' — reconnecting…')
            : 'Console disconnected — reconnecting…';
        showConsoleError(msg);
        setReconnectUi(false);
        if (term && attempt === 0) {
            term.writeln('\r\n\x1b[90mDisconnected — reconnecting…\x1b[0m');
        }
        ptyReconnectTimer = setTimeout(function () {
            ptyReconnectTimer = null;
            connectPty();
        }, delay);
    }

    function closePty() {
        clearPtyReconnect();
        if (!ptyWs) return;
        try {
            ptyWs.onopen = null;
            ptyWs.onmessage = null;
            ptyWs.onerror = null;
            ptyWs.onclose = null;
            ptyWs.close();
        } catch (_) {}
        ptyWs = null;
        setReconnectUi(false);
    }

    function connectPty() {
        if (typeof Terminal === 'undefined' || typeof FitAddon === 'undefined') {
            showConsoleError('Terminal library failed to load');
            return;
        }
        ptyWantConnected = true;
        showConsoleError('');
        // Close any prior socket without cancelling the reconnect we may schedule later.
        if (ptyWs) {
            try {
                ptyWs.onopen = null;
                ptyWs.onmessage = null;
                ptyWs.onerror = null;
                ptyWs.onclose = null;
                ptyWs.close();
            } catch (_) {}
            ptyWs = null;
        }
        clearPtyReconnect();
        setReconnectUi(false);

        if (!term) {
            term = new Terminal({
                cursorBlink: true,
                fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
                fontSize: 13,
                theme: {
                    background: '#0b1220',
                    foreground: '#e2e8f0',
                    cursor: '#93c5fd'
                }
            });
            fitAddon = new FitAddon.FitAddon();
            term.loadAddon(fitAddon);
            term.open(document.getElementById('terminal'));
            term.onData(function (data) {
                sendPty({ type: 'input', data: data });
            });
            // Track cwd from shell PROMPT_COMMAND OSC 7.
            try {
                if (term.parser && typeof term.parser.registerOscHandler === 'function') {
                    term.parser.registerOscHandler(7, function (data) {
                        var path = parseOsc7Path(data);
                        if (path) setCwdDisplay(path);
                        return true;
                    });
                }
            } catch (_) {}
            window.addEventListener('resize', fitAndResize);
            if (window.ResizeObserver && consoleCard) {
                var ro = new ResizeObserver(function () {
                    fitAndResize();
                });
                ro.observe(consoleCard);
            }
        }

        if (ptyReconnectAttempt === 0) {
            term.writeln('\r\n\x1b[90mConnecting to portable console…\x1b[0m');
        }
        ptyWs = new WebSocket(ptyUrl());

        ptyWs.onopen = function () {
            ptyReconnectAttempt = 0;
            clearPtyReconnect();
            showConsoleError('');
            setReconnectUi(true);
            term.writeln('\x1b[90mConnected.\x1b[0m\r\n');
            fitAndResize();
        };

        ptyWs.onmessage = function (ev) {
            var msg;
            try {
                msg = JSON.parse(ev.data);
            } catch (_) {
                return;
            }
            if (msg.type === 'output' && typeof msg.data === 'string') {
                term.write(String(msg.data).replace(/\r\n/g, '\n').replace(/\n/g, '\r\n'));
            } else if (msg.type === 'error' && msg.message) {
                showConsoleError(String(msg.message));
                term.writeln('\r\n\x1b[31m' + String(msg.message) + '\x1b[0m');
                // Session rejected (e.g. too many PTYs) — keep retrying.
                schedulePtyReconnect(String(msg.message));
            }
        };

        ptyWs.onerror = function () {
            // onclose usually follows; avoid double-scheduling.
            setReconnectUi(false);
        };

        ptyWs.onclose = function () {
            ptyWs = null;
            if (!ptyWantConnected) {
                setReconnectUi(false);
                return;
            }
            schedulePtyReconnect('Console disconnected');
        };
    }

    function loadScript(src) {
        return new Promise(function (resolve, reject) {
            var s = document.createElement('script');
            s.src = src;
            s.onload = function () { resolve(); };
            s.onerror = function () { reject(new Error('failed to load ' + src)); };
            document.head.appendChild(s);
        });
    }

    function loadCss(href) {
        var link = document.createElement('link');
        link.rel = 'stylesheet';
        link.href = href;
        document.head.appendChild(link);
    }

    function staticUrl(name) {
        return '/app/' + encodeURIComponent(appApi.appId) + '/static/' + name;
    }

    function bootConsole() {
        setReconnectUi(false);
        if (reconnectBtn) {
            reconnectBtn.addEventListener('click', function () {
                if (reconnectBtn.disabled) return;
                ptyReconnectAttempt = 0;
                clearPtyReconnect();
                connectPty();
            });
        }
        loadCss(staticUrl('xterm.css'));
        Promise.resolve()
            .then(function () { return loadScript(staticUrl('xterm.js')); })
            .then(function () { return loadScript(staticUrl('xterm-addon-fit.js')); })
            .then(connectPty)
            .catch(function (e) {
                showConsoleError(e.message || String(e));
                schedulePtyReconnect(e.message || 'Terminal library failed');
            });
    }

    if (controlStatus) controlStatus.textContent = LABEL_WATCHING;
    setHdmiLabel(!!hdmiToggle.checked);
    setStreamLabel(!!streamToggle.checked);
    updateIdleHint();
    document.querySelectorAll('.how-source-mode-seg[data-lidar]').forEach(function (btn) {
        btn.addEventListener('click', function () {
            var kind = btn.getAttribute('data-lidar');
            if (!kind) return;
            sourceDraftDirty = true;
            lidarKind = kind === 'sensor' ? 'sensor' : 'osf';
            if (kind !== 'sensor' && recordEnabledToggle && recordEnabledToggle.checked) {
                recordEnabledToggle.checked = false;
            }
            updateSourceFieldsUi();
        });
    });
    if (viaDynamoToggle) {
        viaDynamoToggle.addEventListener('change', function () {
            sourceDraftDirty = true;
            updateSourceFieldsUi();
        });
    }
    updateSourceFieldsUi();
    if (cfgSensorHost) {
        cfgSensorHost.addEventListener('input', function () {
            sourceDraftDirty = true;
        });
        cfgSensorHost.addEventListener('change', function () {
            sourceDraftDirty = true;
        });
    }
    if (btnApplySource) {
        btnApplySource.addEventListener('click', postSourceConfig);
    }
    if (btnStopSource) {
        btnStopSource.addEventListener('click', function () {
            if (recordEnabledToggle) recordEnabledToggle.checked = false;
            if (viaDynamoToggle) viaDynamoToggle.checked = false;
            recordFilenameUserEdited = false;
            sourceDraftDirty = true;
            updateSourceFieldsUi();
            postSourceConfig({ stoppingAll: true });
        });
    }
    if (recordEnabledToggle) {
        recordEnabledToggle.addEventListener('change', function () {
            sourceDraftDirty = true;
            if (recordDraftEnabled()) {
                lidarKind = 'sensor';
                if (viaDynamoToggle) viaDynamoToggle.checked = false;
                suggestRecordFilenameIfNeeded();
            }
            updateSourceFieldsUi();
        });
    }
    if (recordFilename) {
        recordFilename.addEventListener('input', function () {
            sourceDraftDirty = true;
            var v = recordFilename.value.trim();
            recordFilenameUserEdited = !!(v && v !== lastSuggestedRecordName);
        });
        recordFilename.addEventListener('blur', function () {
            if (!recordFilename.value.trim()) return;
            recordFilename.value = ensureOsfFilename(recordFilename.value);
            var v = recordFilename.value.trim();
            recordFilenameUserEdited = !!(v && v !== lastSuggestedRecordName);
        });
    }
    if (cfgOsfUploadInput) {
        cfgOsfUploadInput.addEventListener('change', function () {
            var file = cfgOsfUploadInput.files && cfgOsfUploadInput.files[0];
            if (!file) return;
            uploadOsfFile(file).catch(function (e) {
                setOsfStatus(e.message || String(e), 'ro');
            });
        });
    }
    if (cfgOsfSelect) {
        cfgOsfSelect.addEventListener('focus', function () {
            loadOsfLibrary();
        });
        cfgOsfSelect.addEventListener('change', function () {
            sourceDraftDirty = true;
            updateOsfActionBtns();
        });
    }
    if (btnOsfDownload) {
        btnOsfDownload.addEventListener('click', downloadOsfToBrowser);
    }
    if (btnOsfDelete) {
        btnOsfDelete.addEventListener('click', deleteSelectedOsf);
    }
    hydrateStatus().finally(connect);
    bootStatusStream();
    loadOsfLibrary();
    bootConsole();
})();
