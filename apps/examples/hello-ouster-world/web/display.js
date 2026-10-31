(function () {
    'use strict';

    var appApi = tekaAppApi('hello-ouster-world');
    var howStack = document.querySelector('.how-stack');
    var guestAccessToggle = document.getElementById('guest-access-toggle');
    var guestView = false;
    var guestAccessEnabled = true;
    var guestEditable = false;
    var guestAccessBusy = false;
    var ws = null;
    var controlling = false;
    var hoverStream = false;
    var sourceW = 0;
    var sourceH = 0;
    var metaSourceLabel = '';

    function refreshMetaSource() {
        var meta = document.getElementById('meta-source');
        if (meta) meta.textContent = metaSourceLabel || '—';
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
    var keybindingsDialog = document.getElementById('keybindings-dialog');
    var btnScreenshot = document.getElementById('btn-screenshot');
    var streamIdleHint = document.getElementById('stream-idle-hint');
    var sourceStateOverlay = document.getElementById('source-state-overlay');
    var sourceStateTitle = document.getElementById('source-state-title');
    var sourceStateMessage = document.getElementById('source-state-message');
    var sourceWaiting = true;
    var sourceRunning = null;
    var sourceBlock = document.querySelector('.how-source-block');
    var btnSourceToggle = document.getElementById('btn-source-toggle');
    var hdmiToggle = document.getElementById('hdmi-toggle');
    var hdmiLabel = document.getElementById('hdmi-label');
    var hdmiToggleWrap = document.getElementById('hdmi-toggle-wrap');
    var streamToggle = document.getElementById('stream-toggle');
    var streamLabel = document.getElementById('stream-label');
    var consoleCard = document.querySelector('.how-console-card');
    var consoleToggleBtn = document.getElementById('btn-console-toggle');
    var streamAlert = document.getElementById('stream-alert');
    var streamAlertText = '';
    function showError(msg) {
        streamAlertText = msg || '';
        if (streamAlert) streamAlert.textContent = streamAlertText;
        updateIdleHint();
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
        if (guestView) return;
        if (!ws || ws.readyState !== WebSocket.OPEN) return;
        ws.send(JSON.stringify(obj));
    }

    function streamAllowed() {
        return guestEditable || guestAccessEnabled;
    }

    function applyGuestAccessUi(data) {
        if (!guestAccessToggle) return;
        var wrapLabel = guestAccessToggle.closest('.how-guest-access');
        guestAccessToggle.checked = !!(data && data.enabled);
        guestAccessToggle.disabled = !guestEditable || guestAccessBusy;
        if (wrapLabel) wrapLabel.hidden = !guestEditable;
    }

    function applyGuestMode() {
        if (!howStack) return;
        var guestOffSignedOut = !guestEditable && !guestAccessEnabled;
        var guestUi = (guestView && guestAccessEnabled) || guestOffSignedOut;
        howStack.classList.toggle('how-guest', guestUi);
        if (guestView && guestAccessEnabled && guestSourceChips) {
            guestSourceChips.replaceChildren(metaChip('View only', 'type'));
        }
        if (guestView && guestAccessEnabled && sourceBlock) {
            sourceBlock.querySelectorAll('input, select, button, textarea').forEach(function (el) {
                el.disabled = true;
            });
        }
        if (guestOffSignedOut) {
            showError('Guest view is off.');
            if (streamAlert) {
                streamAlert.innerHTML = 'Guest view is off. <a href="/login?next=/app/hello-ouster-world">Sign in</a> to view.';
            }
        }
    }

    function loadGuestAccess() {
        // On failure keep the full UI; the backend still enforces access.
        return fetchAppJson(api('/guest-access')).catch(function () { return null; }).then(function (data) {
            if (!data) return;
            guestView = !!data.guest;
            guestAccessEnabled = data.enabled !== false;
            guestEditable = !!data.editable;
            applyGuestAccessUi(data);
            applyGuestMode();
        });
    }

    function saveGuestAccess(enabled) {
        if (guestAccessBusy || !guestEditable) return;
        guestAccessBusy = true;
        guestAccessToggle.disabled = true;
        fetch(api('/guest-access'), {
            method: 'PUT',
            credentials: 'include',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ enabled: enabled })
        }).catch(function () {}).finally(function () {
            guestAccessBusy = false;
            loadGuestAccess();
        });
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
        btnScreenshot.disabled = sourceWaiting || !(streamToggle && streamToggle.checked);
        if (sourceStateOverlay) {
            sourceStateOverlay.hidden = !sourceWaiting && !streamAlertText;
            sourceStateOverlay.classList.toggle('how-alert-only', !sourceWaiting);
            if (sourceStateTitle) sourceStateTitle.hidden = !sourceWaiting;
            if (sourceStateMessage) sourceStateMessage.hidden = !sourceWaiting;
            if (streamAlert) streamAlert.hidden = !streamAlertText;
        }
        if (streamIdleHint) {
            var browserOff = streamToggle && !streamToggle.checked;
            var hdmiOn = hdmiToggle && hdmiToggle.checked && !hdmiToggle.disabled;
            streamIdleHint.hidden = sourceWaiting
                || !(browserOff && hdmiOn);
        }
    }

    function applySourceState(cfg) {
        cfg = cfg || {};
        var runtime = cfg.runtime || {};
        var discovery = cfg.discovery || {};
        var state = runtime.state || discovery.status || '';
        var receiving = !cfg.config_error && cfg.source_enabled !== false
            && (state === 'streaming' || state === 'playing_osf');
        var message = cfg.config_error || runtime.message || discovery.message || '';
        var title = 'Waiting for source';
        var chip = ['Waiting', 'state'];
        if (cfg.config_error) { title = 'Source configuration needs attention'; chip = ['Needs attention', 'ro']; }
        else if (cfg.source_enabled === false || state === 'stopped') { title = 'Source stopped'; chip = ['Stopped', 'type']; }
        else if (state === 'no_sensors') { title = 'No sensors detected'; chip = ['No sensors', 'state']; }
        else if (state === 'selection_required') { title = 'Select a sensor'; chip = ['Select sensor', 'state']; }
        else if (state === 'discovering') { title = 'Scanning for sensors'; chip = ['Scanning', 'state']; }
        else if (state === 'configuring') { title = 'Auto-configuring sensor'; chip = ['Configuring', 'state']; }
        else if (state === 'configuration_error' || state === 'error') { title = 'Source error'; chip = ['Error', 'ro']; }
        else if (receiving) { title = 'Receiving frames'; chip = [state === 'playing_osf' ? 'Playing' : 'Streaming', 'usage']; }
        sourceWaiting = !receiving;
        updateKeybindings(receiving ? runtime.controls : null, cfg.source);
        if (sourceWaiting) clearStreamBlack();
        // Collapse only on running/not-running transitions so a manual toggle sticks.
        if (receiving !== sourceRunning && !sourceDraftDirty && !sourceBusy) {
            sourceRunning = receiving;
            setSourceCollapsed(receiving);
        }
        if (sourceStateTitle) sourceStateTitle.textContent = title;
        if (sourceStateMessage) sourceStateMessage.textContent = message || 'Waiting for source status';
        if (sourceStatusChips) {
            var isSensor = canonicalizeSource(cfg.source) === 'sensor';
            var target = isSensor ? cfg.sensor_hostname : String(cfg.osf_path || '').split('/').pop();
            var kind = isSensor ? 'Sensor' : 'Local file';
            var chips = [metaChip(chip[0], chip[1], message), metaChip(target ? kind + ': ' + target : kind, 'type')];
            [sourceStatusChips, guestSourceChips].forEach(function (el) {
                if (el) el.replaceChildren.apply(el, chips.map(function (c) { return c.cloneNode(true); }));
            });
        }
        if (sourceStatusMessage) {
            sourceStatusMessage.textContent = receiving ? '' : message;
            sourceStatusMessage.title = message;
            sourceStatusMessage.hidden = receiving || !message;
        }
        applySensorDiscovery(discovery);
        updateIdleHint();
    }

    function metaChip(text, variant, title) {
        var el = document.createElement('span');
        el.className = 'app-meta-chip app-meta-chip--' + variant;
        el.textContent = text;
        if (title) el.title = title;
        return el;
    }

    function setSourceCollapsed(collapsed) {
        if (!sourceBlock || !btnSourceToggle) return;
        sourceBlock.classList.toggle('how-source-collapsed', collapsed);
        btnSourceToggle.setAttribute('aria-expanded', String(!collapsed));
        btnSourceToggle.textContent = collapsed ? 'Configure' : 'Hide';
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
        if (guestView) return;
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
        if (guestView && sourceWaiting) {
            sourceWaiting = false;
            if (guestSourceChips) {
                guestSourceChips.replaceChildren(metaChip('Live stream', 'usage'));
            }
            updateIdleHint();
        }
        // Stream toggle off: keep panel black (HDMI is independent).
        if (streamToggle && !streamToggle.checked) {
            pendingFrame = null;
            return;
        }
        // Peer waiting / disconnected: keep the instruction overlay clear.
        if (sourceWaiting) {
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
                    fitStream();
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
                fitStream();
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
        // object-fit: contain letterboxes the stream in fullscreen.
        var scale = Math.min(rect.width / canvas.width, rect.height / canvas.height);
        var w = canvas.width * scale;
        var h = canvas.height * scale;
        var nx = (ev.clientX - rect.left - (rect.width - w) / 2) / w;
        var ny = (ev.clientY - rect.top - (rect.height - h) / 2) / h;
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
        if (!streamAllowed()) return;
        if (displayReconnectTimer) {
            clearTimeout(displayReconnectTimer);
            displayReconnectTimer = null;
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

    var sourceStatusChips = document.getElementById('source-status-chips');
    var guestSourceChips = document.getElementById('guest-source-chips');
    var sourceStatusMessage = document.getElementById('source-status-message');
    var cfgOsfSelect = document.getElementById('cfg-osf-select');
    var cfgOsfUploadInput = document.getElementById('cfg-osf-upload-input');
    var osfLibraryLink = document.getElementById('osf-sample-library');
    var btnOsfDownload = document.getElementById('btn-osf-download');
    var btnOsfDelete = document.getElementById('btn-osf-delete');
    var osfFetchProgress = document.getElementById('osf-fetch-progress');
    var osfFetchBarWrap = document.getElementById('osf-fetch-bar-wrap');
    var osfFetchBar = document.getElementById('osf-fetch-bar');
    var osfFetchStatus = document.getElementById('osf-fetch-status');
    var cfgSensorHost = document.getElementById('cfg-sensor-host');
    var sensorPicker = document.getElementById('sensor-picker');
    var sensorCandidateSelect = document.getElementById('sensor-candidate-select');
    var btnApplySource = document.getElementById('btn-apply-source');
    var btnStopSource = document.getElementById('btn-stop-source');
    var recordEnabledToggle = document.getElementById('record-enabled-toggle');
    var recordFilename = document.getElementById('record-filename');
    var recordDuration = document.getElementById('record-duration');
    var recordControls = document.querySelector('.how-record-controls');
    var lastRecord = { active: false };
    var lastRuntime = {};
    var lastDiscovery = {};
    var sensorPickerSocket = null;
    var lastCandidateKey = '';
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
        updateSourceFieldsUi();
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
    var SEED_PREFIX = 'seed:';
    var lidarKind = 'sensor';

    function vizSource() {
        return lidarKind === 'sensor' ? 'sensor' : 'local_file';
    }

    function canonicalizeSource(src) {
        return src === 'sensor' ? 'sensor' : 'local_file';
    }

    function sourceLabel(src) {
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
        if (guestView) return Promise.resolve(null);
        var prefer = preferredPath
            || (lastPersistedSource && lastPersistedSource.osf_path)
            || '';
        return fetchAppJson(api('/osfs')).then(function (catalog) {
            if (!catalog) return null;
            renderOsfSelect(catalog, preferredPath || (sourceDraftDirty ? selectedOsfPath() : prefer));
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

    function syncRecordControlsUi() {
        if (recordDuration) {
            recordDuration.disabled = !recordDraftEnabled() || !!(lastRecord && lastRecord.active);
        }
        if (recordFilename) {
            recordFilename.disabled = !recordDraftEnabled() || !!(lastRecord && lastRecord.active);
        }
        if (recordControls) recordControls.classList.toggle('how-record-off', !recordDraftEnabled());
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

    function syncLidarSegments() {
        document.querySelectorAll('.how-source-mode-seg[data-lidar]').forEach(function (btn) {
            var kind = btn.getAttribute('data-lidar');
            var on = kind === lidarKind;
            btn.classList.toggle('how-source-mode-seg-active', on);
            btn.classList.toggle('btn-secondary', !on);
        });
    }

    function updateSourceFieldsUi() {
        var osfWrap = document.getElementById('cfg-osf-wrap');
        var osfActions = document.getElementById('how-osf-actions');
        var sensorWrap = document.getElementById('cfg-sensor-wrap');
        var showOsf = lidarKind === 'osf';
        var showSensor = lidarKind === 'sensor';
        if (osfWrap) osfWrap.style.display = showOsf ? '' : 'none';
        if (osfActions) osfActions.style.display = showOsf ? '' : 'none';
        if (sensorWrap) sensorWrap.style.display = showSensor ? '' : 'none';
        if (sensorCandidateSelect) sensorCandidateSelect.disabled = sourceBusy;
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
        lastRuntime = cfg.runtime || {};
        lastDiscovery = cfg.discovery || {};
        applySourceState(cfg);
        if (sourceDraftDirty || sourceBusy) return;
        var src = canonicalizeSource(cfg.source);
        lidarKind = src === 'sensor' ? 'sensor' : 'osf';
        if (cfgOsfSelect && typeof cfg.osf_path === 'string' && cfg.osf_path) {
            var matched = false;
            for (var i = 0; i < cfgOsfSelect.options.length; i++) {
                if (cfgOsfSelect.options[i].value === cfg.osf_path) {
                    cfgOsfSelect.selectedIndex = i;
                    matched = true;
                    break;
                }
            }
            if (!matched && lastOsfCatalog) renderOsfSelect(lastOsfCatalog, cfg.osf_path);
        }
        if (cfgSensorHost && typeof cfg.sensor_hostname === 'string') {
            cfgSensorHost.value = cfg.sensor_hostname;
        }
        if (recordEnabledToggle && typeof cfg.record_enabled === 'boolean') {
            recordEnabledToggle.checked = !!lastRecord.active;
        }
        if (recordDuration && typeof cfg.record_duration_seconds === 'number') {
            recordDuration.value = cfg.record_duration_seconds;
        }
        if (recordFilename && typeof cfg.record_filename === 'string' && cfg.record_filename) {
            recordFilename.value = ensureOsfFilename(cfg.record_filename);
            recordFilenameUserEdited = true;
            lastSuggestedRecordName = recordFilename.value;
        } else if (recordEnabledToggle && recordEnabledToggle.checked) {
            recordFilenameUserEdited = false;
            suggestRecordFilenameIfNeeded();
        }
        metaSourceLabel = sourceLabel(src);
        refreshMetaSource();
        updateSourceFieldsUi();
    }

    function applySourceRuntime(rt) {
        lastRuntime = rt || {};
        var cfg = Object.assign({}, lastPersistedSource || {});
        cfg.runtime = lastRuntime;
        cfg.discovery = lastDiscovery;
        applySourceState(cfg);
    }

    function applySensorDiscovery(discovery) {
        discovery = discovery || {};
        lastDiscovery = discovery;
        if (sensorCandidateSelect) {
            var selectedId = sensorCandidateSelect.value;
            var candidates = Array.isArray(discovery.candidates) ? discovery.candidates : [];
            var candidateKey = JSON.stringify(candidates.length ? candidates : discovery.status || '');
            if (candidateKey !== lastCandidateKey) {
                lastCandidateKey = candidateKey;
                sensorCandidateSelect.innerHTML = '';
                var placeholder = document.createElement('option');
                placeholder.value = '';
                placeholder.textContent = candidates.length
                    ? 'Choose a detected sensor…'
                    : discovery.status === 'no_sensors' ? 'No detected sensors'
                    : discovery.status === 'unavailable' ? 'Sensor discovery unavailable'
                    : 'Discovering sensors…';
                sensorCandidateSelect.appendChild(placeholder);
                candidates.slice().sort(function (a, b) {
                    return (a.targets_this_device ? 0 : 1) - (b.targets_this_device ? 0 : 1)
                        || (Number(a.serial) || Infinity) - (Number(b.serial) || Infinity)
                        || a.id.localeCompare(b.id);
                }).forEach(function (candidate) {
                    var option = document.createElement('option');
                    option.value = candidate.id;
                    var label = candidate.hostname || candidate.target || candidate.id;
                    var details = [candidate.model, candidate.firmware].filter(Boolean);
                    if (details.length) label += ' · ' + details.join(' · ');
                    option.textContent = (candidate.targets_this_device ? '*' : '\u00a0') + '\u00a0' + label;
                    if (candidate.targets_this_device) {
                        option.title = 'Preferred: already configured to stream lidar data to this device';
                    }
                    sensorCandidateSelect.appendChild(option);
                });
                if (candidates.some(function (candidate) { return candidate.id === selectedId; })) {
                    sensorCandidateSelect.value = selectedId;
                }
            }
        }
    }

    function sensorPickerUrl() {
        var path = api('/sensors/picker');
        var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
        return proto + '//' + location.host + path;
    }

    function openSensorPicker() {
        if (guestView) return;
        if (sensorPickerSocket && sensorPickerSocket.readyState < WebSocket.CLOSING) return;
        var socket = new WebSocket(sensorPickerUrl());
        sensorPickerSocket = socket;
        socket.onclose = function () {
            if (sensorPickerSocket === socket) sensorPickerSocket = null;
        };
    }

    function closeSensorPicker() {
        if (!sensorPickerSocket) return;
        try {
            sensorPickerSocket.close();
        } catch (_) {}
        sensorPickerSocket = null;
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
        if (!sourceDraftDirty && !sourceBusy && recordEnabledToggle) {
            recordEnabledToggle.checked = !!lastRecord.active;
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
            normalizeRecordFilenameInput();
        }
        var src = vizSource();
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
        if (!stoppingAll && wantRecord && !host) {
            showError('Select or enter a sensor before enabling recording');
            return;
        }
        var duration = recordDuration ? Number(recordDuration.value) : 120;
        if (!stoppingAll && (!Number.isInteger(duration) || duration < 1 || duration > 3600)) {
            showError('Record duration must be between 1 and 3600 seconds');
            return;
        }
        var body = {
            record_duration_seconds: stoppingAll ? ((lastPersistedSource && lastPersistedSource.record_duration_seconds) || 120) : duration,
            source: src,
            osf_path: osfPath,
            sensor_hostname: cfgSensorHost ? cfgSensorHost.value : '',
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
                }
                if (resp.record) applyRecordStatus(resp.record);
                showError('');
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
        return fetchAppJson(api('/display/status')).then(function (st) {
            if (st) applyDisplayStatus(st);
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
        if (guestView) return;
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
        if (guestView) return;
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
        if (guestView) return;
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
        if (guestView) return;
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
        if (keybindingsDialog.open) return;
        if (!isTypingTarget(ev.target) && wrap.contains(document.activeElement)
            && (ev.key === '?' || (ev.code === 'Slash' && ev.shiftKey))
            && !ev.ctrlKey && !ev.altKey && !ev.metaKey) {
            ev.preventDefault();
            if (state === 'down' && !ev.repeat) keybindingsDialog.showModal();
            return;
        }
        if (!controlling) return;
        if (isTypingTarget(ev.target)) return;
        if (ev.key === 'Escape') return;
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
    var ptyWantConnected = false;
    var terminalLibs = null;

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

    function setConsoleOpen(open) {
        if (consoleCard) consoleCard.classList.toggle('how-console-collapsed', !open);
        if (consoleToggleBtn) {
            consoleToggleBtn.textContent = open ? 'Close' : 'Open';
            consoleToggleBtn.setAttribute('aria-expanded', String(open));
        }
    }

    function clearPtyReconnect() {
        if (ptyReconnectTimer) {
            clearTimeout(ptyReconnectTimer);
            ptyReconnectTimer = null;
        }
    }

    function schedulePtyReconnect(reason) {
        if (guestView) return;
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
    }

    function connectPty() {
        if (guestView) return;
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
            window.addEventListener('resize', fitAndResize);
            if (window.ResizeObserver && consoleCard) {
                var ro = new ResizeObserver(function () {
                    fitAndResize();
                });
                ro.observe(consoleCard);
            }
        }

        if (ptyReconnectAttempt === 0) {
            term.writeln('\r\n\x1b[90mConnecting to app console…\x1b[0m');
        }
        ptyWs = new WebSocket(ptyUrl());

        ptyWs.onopen = function () {
            ptyReconnectAttempt = 0;
            clearPtyReconnect();
            showConsoleError('');
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

        ptyWs.onclose = function () {
            ptyWs = null;
            if (!ptyWantConnected) return;
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

    function loadTerminalLibs() {
        if (!terminalLibs) {
            loadCss(staticUrl('xterm.css'));
            terminalLibs = loadScript(staticUrl('xterm.js'))
                .then(function () { return loadScript(staticUrl('xterm-addon-fit.js')); })
                .catch(function (e) {
                    terminalLibs = null;
                    throw e;
                });
        }
        return terminalLibs;
    }

    function openConsole() {
        setConsoleOpen(true);
        ptyWantConnected = true;
        ptyReconnectAttempt = 0;
        loadTerminalLibs().then(function () {
            if (ptyWantConnected) connectPty();
        }).catch(function (e) {
            showConsoleError(e.message || String(e));
        });
    }

    function closeConsole() {
        ptyWantConnected = false;
        closePty();
        showConsoleError('');
        setConsoleOpen(false);
    }

    function bootConsole() {
        if (consoleToggleBtn) {
            consoleToggleBtn.addEventListener('click', function () {
                if (ptyWantConnected) closeConsole();
                else openConsole();
            });
        }
    }

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
                suggestRecordFilenameIfNeeded();
            }
            updateSourceFieldsUi();
        });
    }
    if (recordDuration) {
        recordDuration.addEventListener('input', function () { sourceDraftDirty = true; });
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
    if (sensorPicker) {
        sensorPicker.addEventListener('focusin', openSensorPicker);
        sensorPicker.addEventListener('focusout', function () {
            setTimeout(function () {
                if (!sourceBusy && !sensorPicker.contains(document.activeElement)) {
                    closeSensorPicker();
                }
            }, 0);
        });
    }
    if (sensorCandidateSelect) {
        sensorCandidateSelect.addEventListener('change', function () {
            var candidate = (lastDiscovery.candidates || []).find(function (item) {
                return item.id === sensorCandidateSelect.value;
            });
            if (candidate && cfgSensorHost) {
                cfgSensorHost.value = candidate.target;
                sourceDraftDirty = true;
            }
        });
    }
    if (btnOsfDownload) {
        btnOsfDownload.addEventListener('click', downloadOsfToBrowser);
    }
    // Fit the stream between its top edge and the bottom of the viewport.
    function fitStream() {
        var top = wrap.getBoundingClientRect().top + window.scrollY;
        var pad = parseFloat(getComputedStyle(wrap).paddingTop) * 2;
        // Tall source controls must leave a usable viewer after scrolling.
        var height = Math.max(1, window.innerHeight - Math.min(top, window.innerHeight / 2) - pad - 16);
        wrap.style.setProperty('--stream-max-h', height + 'px');
        wrap.style.setProperty('--stream-max-w', (height * canvas.width / canvas.height + pad) + 'px');
    }
    window.addEventListener('resize', fitStream);
    if (window.ResizeObserver) new ResizeObserver(fitStream).observe(document.body);
    fitStream();
    var btnKeybindings = document.getElementById('btn-keybindings');
    btnScreenshot.addEventListener('pointerdown', function (ev) { ev.stopPropagation(); });
    var screenshotStatus = document.getElementById('screenshot-status');
    var screenshotStatusTimer;
    function showScreenshotStatus(message) {
        clearTimeout(screenshotStatusTimer);
        screenshotStatus.textContent = message;
        screenshotStatus.hidden = false;
        screenshotStatusTimer = setTimeout(function () { screenshotStatus.hidden = true; }, 8000);
    }
    btnScreenshot.addEventListener('click', async function () {
        if (btnScreenshot.disabled) return;
        var png = new Promise(function (resolve, reject) {
            canvas.toBlob(function (blob) {
                if (blob) resolve(blob);
                else reject(new Error('Screenshot capture failed'));
            }, 'image/png');
        });
        var fallback = window.isSecureContext ? 'Clipboard unavailable' : 'Clipboard needs HTTPS';
        if (navigator.clipboard && window.ClipboardItem) {
            try {
                // Start the write during the click; encoding can finish asynchronously.
                await navigator.clipboard.write([new ClipboardItem({ 'image/png': png })]);
                showScreenshotStatus('Screenshot copied');
                return;
            } catch (err) {
                fallback = 'Clipboard blocked';
            }
        }
        try {
            var blob = await png;
            var url = URL.createObjectURL(blob);
            var link = document.createElement('a');
            link.href = url;
            link.download = 'ouster-' + new Date().toISOString().replace(/[:.]/g, '-') + '.png';
            document.body.appendChild(link);
            link.click();
            link.remove();
            setTimeout(function () { URL.revokeObjectURL(url); }, 1000);
            showScreenshotStatus(fallback + ' — PNG downloaded');
        } catch (err) {
            showScreenshotStatus('Screenshot capture failed');
        }
    });
    btnKeybindings.addEventListener('pointerdown', function (ev) { ev.stopPropagation(); });
    btnKeybindings.addEventListener('click', function () {
        keybindingsDialog.showModal();
    });
    function updateKeybindings(controls, source) {
        controls = controls || {};
        var replay = controls.replay && source === 'local_file';
        document.getElementById('keybindings-playback').hidden = !replay;
        document.getElementById('keybindings-advanced').hidden = !replay;
        document.getElementById('keybindings-objects').hidden = !controls.objects;
        document.getElementById('keybindings-zones').hidden = !controls.zones;
        var count = Math.min(9, controls.sensor_count || 0);
        var rows = document.getElementById('keybindings-sensor-rows');
        if (rows.childElementCount !== count) {
            rows.replaceChildren();
            for (var i = 1; i <= count; i++) {
                var row = document.createElement('tr');
                var keys = document.createElement('td');
                var key = document.createElement('kbd');
                key.textContent = 'Ctrl+' + i;
                keys.appendChild(key);
                var action = document.createElement('td');
                action.textContent = 'Show / hide sensor ' + i;
                row.append(keys, action);
                rows.appendChild(row);
            }
        }
        document.getElementById('keybindings-sensors').hidden = !count;
    }
    ['pointerdown', 'pointermove', 'wheel'].forEach(function (name) {
        keybindingsDialog.addEventListener(name, function (ev) { ev.stopPropagation(); });
    });
    document.getElementById('btn-close-keybindings').addEventListener('click', function () {
        keybindingsDialog.close();
    });
    if (btnSourceToggle) {
        btnSourceToggle.addEventListener('click', function () {
            setSourceCollapsed(!sourceBlock.classList.contains('how-source-collapsed'));
        });
    }
    var btnFullscreen = document.getElementById('btn-fullscreen');
    if (btnFullscreen) {
        btnFullscreen.hidden = !document.fullscreenEnabled;
        // Inside the stream: keep clicks from reaching the viz.
        btnFullscreen.addEventListener('pointerdown', function (ev) { ev.stopPropagation(); });
        btnFullscreen.addEventListener('click', function () {
            if (document.fullscreenElement) {
                document.exitFullscreen().catch(function () {});
                return;
            }
            wrap.requestFullscreen({ navigationUI: 'hide' }).then(function () {
                wrap.focus();
            }).catch(function () {});
        });
    }
    if (btnOsfDelete) {
        btnOsfDelete.addEventListener('click', deleteSelectedOsf);
    }
    if (guestAccessToggle) {
        guestAccessToggle.addEventListener('change', function () {
            saveGuestAccess(guestAccessToggle.checked);
        });
    }
    loadGuestAccess().then(function () {
        if (!streamAllowed()) return;
        hydrateStatus().finally(connect);
        if (!guestView) bootStatusStream();
        if (!guestView) {
            loadOsfLibrary();
            bootConsole();
        }
    });
})();
