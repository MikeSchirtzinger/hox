// hox-viz HUD overlay module
// Manages all 2D overlay elements: top bar, phase progress, metrics, detail panel, oplog feed

import { colorForStatus, colorForOpType, UI } from './colors.js';

// ── Module State ──

let container = null;
let uptimeInterval = null;
let sessionStartTime = null;
let emptyStateEl = null;

// DOM element references (populated by initHud)
const els = {
    topBar: null,
    sessionId: null,
    sessionBookmark: null,
    uptime: null,
    connectionDot: null,
    connectionLabel: null,
    phaseProgress: null,
    metricsPanel: null,
    metricTools: null,
    metricSuccess: null,
    metricActive: null,
    metricDone: null,
    metricDuration: null,
    detailPanel: null,
    detailContent: null,
    detailClose: null,
    oplogFeed: null,
    oplogEntries: null,
};

const MAX_OPLOG_ENTRIES = 100;

// ── Format Helpers ──

function formatDuration(ms) {
    if (ms == null || ms < 0) return '0s';
    const totalSeconds = Math.floor(ms / 1000);
    if (totalSeconds < 60) return `${totalSeconds}s`;
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    if (minutes < 60) return `${minutes}m ${seconds}s`;
    const hours = Math.floor(minutes / 60);
    const remainMin = minutes % 60;
    return `${hours}h ${remainMin}m`;
}

function formatNumber(n) {
    if (n == null) return '0';
    return n.toLocaleString('en-US');
}

function formatPercent(n) {
    if (n == null) return '0.0%';
    return `${n.toFixed(1)}%`;
}

// ── DOM Construction ──

function createElement(tag, attrs, children) {
    const el = document.createElement(tag);
    if (attrs) {
        for (const [key, val] of Object.entries(attrs)) {
            if (key === 'className') el.className = val;
            else if (key === 'textContent') el.textContent = val;
            else if (key === 'innerHTML') el.innerHTML = val;
            else el.setAttribute(key, val);
        }
    }
    if (children) {
        for (const child of Array.isArray(children) ? children : [children]) {
            if (typeof child === 'string') el.appendChild(document.createTextNode(child));
            else if (child) el.appendChild(child);
        }
    }
    return el;
}

// ── initHud ──

export function initHud(containerEl) {
    container = containerEl;

    // Top Bar
    const topBar = createElement('div', { id: 'top-bar' });

    const titleSpan = createElement('span', { className: 'title', textContent: 'HOX VIZ' });

    const sessionInfo = createElement('div', { className: 'session-info' });
    const sessionIdSpan = createElement('span');
    sessionIdSpan.innerHTML = '<span class="label">Session:</span> <span class="value" id="session-id">--</span>';
    const bookmarkSpan = createElement('span');
    bookmarkSpan.innerHTML = '<span class="label">Branch:</span> <span class="value" id="session-bookmark">--</span>';
    sessionInfo.append(sessionIdSpan, bookmarkSpan);

    const rightSection = createElement('div', { className: 'connection-status' });
    const uptimeSpan = createElement('span', { id: 'uptime', className: 'text-dim', textContent: '0s' });
    const connDot = createElement('span', { id: 'connection-dot', className: 'connection-dot' });
    const connLabel = createElement('span', { id: 'connection-label', className: 'connection-label', textContent: '--' });
    rightSection.append(uptimeSpan, connDot, connLabel);

    topBar.append(titleSpan, sessionInfo, rightSection);
    container.appendChild(topBar);

    // Phase Progress
    const phaseProgress = createElement('div', { id: 'phase-progress' });
    container.appendChild(phaseProgress);

    // Metrics Panel
    const metricsPanel = createElement('div', { id: 'metrics-panel', className: 'glass-panel' });
    const metricsTitle = createElement('div', { className: 'panel-title', textContent: 'Metrics' });
    const metricsContent = createElement('div', { className: 'metrics-content' });

    metricsContent.innerHTML = `
        <div class="metric-row">
            <span class="metric-label">Tool Calls</span>
            <span class="metric-value highlight-cyan" id="metric-tools">0</span>
        </div>
        <div class="metric-row">
            <span class="metric-label">Success</span>
            <span class="metric-value highlight-green" id="metric-success">100.0%</span>
        </div>
        <div class="metric-row">
            <span class="metric-label">Active</span>
            <span class="metric-value highlight-cyan" id="metric-active">0</span>
        </div>
        <div class="metric-row">
            <span class="metric-label">Done</span>
            <span class="metric-value highlight-magenta" id="metric-done">0</span>
        </div>
        <div class="metric-row">
            <span class="metric-label">Duration</span>
            <span class="metric-value" id="metric-duration">0s</span>
        </div>
    `;
    metricsPanel.append(metricsTitle, metricsContent);
    container.appendChild(metricsPanel);

    // Detail Panel (starts hidden off-screen)
    const detailPanel = createElement('div', { id: 'detail-panel', className: 'glass-panel' });
    const detailTitle = createElement('div', { className: 'panel-title', textContent: 'Details' });
    const closeBtn = createElement('button', { className: 'close-btn', innerHTML: '&times;' });
    const detailContent = createElement('div', { id: 'detail-content' });
    detailPanel.append(closeBtn, detailTitle, detailContent);
    container.appendChild(detailPanel);

    closeBtn.addEventListener('click', () => {
        detailPanel.classList.remove('visible');
        if (metricsPanel) metricsPanel.style.display = '';
    });

    // Oplog Feed
    const oplogFeed = createElement('div', { id: 'oplog-feed', className: 'glass-panel' });
    const oplogTitle = createElement('div', { className: 'panel-title', textContent: 'Oplog' });
    const oplogEntries = createElement('div', { id: 'oplog-entries' });
    oplogFeed.append(oplogTitle, oplogEntries);
    container.appendChild(oplogFeed);

    // Empty State
    emptyStateEl = createElement('div', { className: 'empty-state', textContent: 'No active session' });
    emptyStateEl.style.cssText = `
        position: fixed; top: 50%; left: 50%;
        transform: translate(-50%, -50%);
        text-align: center; color: #444444;
        font-size: 14px; letter-spacing: 2px;
        animation: glowPulse 3s ease-in-out infinite;
        z-index: 15; pointer-events: none;
    `;
    container.appendChild(emptyStateEl);

    // Cache element references
    els.topBar = topBar;
    els.sessionId = document.getElementById('session-id');
    els.sessionBookmark = document.getElementById('session-bookmark');
    els.uptime = document.getElementById('uptime');
    els.connectionDot = document.getElementById('connection-dot');
    els.connectionLabel = document.getElementById('connection-label');
    els.phaseProgress = phaseProgress;
    els.metricsPanel = metricsPanel;
    els.metricTools = document.getElementById('metric-tools');
    els.metricSuccess = document.getElementById('metric-success');
    els.metricActive = document.getElementById('metric-active');
    els.metricDone = document.getElementById('metric-done');
    els.metricDuration = document.getElementById('metric-duration');
    els.detailPanel = detailPanel;
    els.detailContent = detailContent;
    els.detailClose = closeBtn;
    els.oplogFeed = oplogFeed;
    els.oplogEntries = oplogEntries;

    // Keyboard shortcuts
    document.addEventListener('keydown', handleKeydown);

    // Node click listener (dispatched by graph.js)
    window.addEventListener('node-click', (e) => {
        showDetailPanel(e.detail);
    });
}

// ── Keyboard Shortcuts ──

function handleKeydown(e) {
    // Ignore when typing in inputs
    if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;

    switch (e.key.toUpperCase()) {
        case 'R':
            window.dispatchEvent(new CustomEvent('reset-camera'));
            break;
        case 'F':
            if (!document.fullscreenElement) {
                document.documentElement.requestFullscreen().catch(() => {});
            } else {
                document.exitFullscreen().catch(() => {});
            }
            break;
        case 'G':
            window.dispatchEvent(new CustomEvent('toggle-grid'));
            break;
        case 'L':
            window.dispatchEvent(new CustomEvent('toggle-labels'));
            break;
    }
}

// ── updateAll (full state refresh) ──

export function updateAll(state) {
    if (!state) return;

    // Hide empty state when we have data
    const hasData = (state.nodes && state.nodes.length > 0) ||
                    (state.phases && state.phases.length > 0);
    if (emptyStateEl) {
        emptyStateEl.style.display = hasData ? 'none' : 'block';
    }

    // Session info
    if (state.session) {
        if (els.sessionId) els.sessionId.textContent = state.session.id || '--';
        if (els.sessionBookmark) els.sessionBookmark.textContent = state.session.bookmark || '--';

        // Start uptime counter
        if (state.session.started_at) {
            sessionStartTime = new Date(state.session.started_at).getTime();
            startUptimeCounter();
        }
    }

    // Metrics
    if (state.metrics) {
        updateMetrics(state.metrics);
    }

    // Phases
    if (state.phases) {
        renderPhases(state.phases);
    }

    // Oplog (full replacement)
    if (state.oplog) {
        if (els.oplogEntries) els.oplogEntries.innerHTML = '';
        // Show newest first - reverse the array for display
        const entries = state.oplog.slice(-MAX_OPLOG_ENTRIES);
        for (let i = entries.length - 1; i >= 0; i--) {
            appendOplogDOM(entries[i], false);
        }
    }
}

// ── applyDelta (incremental update) ──

export function applyDelta(delta) {
    if (!delta) return;

    if (delta.metrics) {
        updateMetrics(delta.metrics);
    }

    if (delta.changed_phases && delta.changed_phases.length > 0) {
        // Re-render all phases since order matters
        // Merge changed phases into existing state tracked by the phase bars
        const existing = getCurrentPhases();
        for (const changed of delta.changed_phases) {
            const idx = existing.findIndex(p => p.number === changed.number);
            if (idx >= 0) existing[idx] = changed;
            else existing.push(changed);
        }
        existing.sort((a, b) => a.number - b.number);
        renderPhases(existing);
    }

    if (delta.new_oplog) {
        for (const entry of delta.new_oplog) {
            addOplogEntry(entry);
        }
    }

    // Update empty state
    const hasPhases = els.phaseProgress && els.phaseProgress.children.length > 0;
    if (emptyStateEl) {
        emptyStateEl.style.display = hasPhases ? 'none' : 'block';
    }
}

// ── Metrics ──

function updateMetrics(metrics) {
    if (els.metricTools) els.metricTools.textContent = formatNumber(metrics.total_tool_calls);
    if (els.metricSuccess) {
        const rate = metrics.success_rate;
        els.metricSuccess.textContent = formatPercent(rate);
        // Color-code success rate
        els.metricSuccess.className = 'metric-value';
        if (rate >= 95) els.metricSuccess.classList.add('highlight-green');
        else if (rate >= 80) els.metricSuccess.classList.add('highlight-yellow');
        else els.metricSuccess.classList.add('highlight-red');
    }
    if (els.metricActive) els.metricActive.textContent = formatNumber(metrics.active_agents);
    if (els.metricDone) els.metricDone.textContent = formatNumber(metrics.completed_agents);
    if (els.metricDuration) els.metricDuration.textContent = formatDuration(metrics.total_time_ms);
}

// ── Phase Progress Bars ──

function renderPhases(phases) {
    if (!els.phaseProgress) return;
    els.phaseProgress.innerHTML = '';

    const sorted = [...phases].sort((a, b) => a.number - b.number);
    for (const phase of sorted) {
        const bar = createElement('div', { className: `phase-bar ${phase.status || 'pending'}` });

        const fill = createElement('div', { className: 'fill' });
        if (phase.status === 'completed') fill.classList.add('completed');
        const pct = Math.max(0, Math.min(1, phase.progress || 0)) * 100;
        fill.style.width = `${pct}%`;

        const label = createElement('div', {
            className: 'label',
            textContent: `${phase.name || 'Phase ' + phase.number} ${Math.round(pct)}%`
        });

        bar.append(fill, label);
        // Store phase data for delta merging
        bar.dataset.phaseNumber = phase.number;
        bar.dataset.phaseData = JSON.stringify(phase);
        els.phaseProgress.appendChild(bar);
    }
}

function getCurrentPhases() {
    if (!els.phaseProgress) return [];
    const phases = [];
    for (const bar of els.phaseProgress.children) {
        if (bar.dataset.phaseData) {
            try { phases.push(JSON.parse(bar.dataset.phaseData)); } catch {}
        }
    }
    return phases;
}

// ── Oplog Feed ──

export function addOplogEntry(entry) {
    if (!entry || !els.oplogEntries) return;
    appendOplogDOM(entry, true);

    // Trim excess entries
    while (els.oplogEntries.children.length > MAX_OPLOG_ENTRIES) {
        els.oplogEntries.removeChild(els.oplogEntries.lastChild);
    }
}

function appendOplogDOM(entry, prepend) {
    if (!els.oplogEntries) return;

    const opType = entry.op_type || 'other';
    const row = createElement('div', { className: `oplog-entry op-${opType}` });

    const time = createElement('span', { className: 'op-time', textContent: entry.timestamp || '' });
    const desc = createElement('span', { className: 'op-desc', textContent: entry.description || '' });

    row.append(time, desc);

    if (entry.agent_id) {
        const agent = createElement('span', { className: 'op-agent', textContent: entry.agent_id });
        row.appendChild(agent);
    }

    if (prepend) {
        els.oplogEntries.insertBefore(row, els.oplogEntries.firstChild);
    } else {
        els.oplogEntries.appendChild(row);
    }
}

// ── Detail Panel ──

function showDetailPanel(node) {
    if (!node || !els.detailPanel || !els.detailContent) return;

    // Hide metrics panel when detail panel opens (same position)
    if (els.metricsPanel) els.metricsPanel.style.display = 'none';

    const statusColor = colorForStatus(node.status);
    let html = '';

    // Node name
    html += `<div style="font-size: 16px; font-weight: 600; color: ${statusColor}; margin-bottom: 8px; word-break: break-word;">${escapeHtml(node.label || node.id)}</div>`;

    // Status badge
    html += `<span class="status-badge ${node.status || 'pending'}">${escapeHtml(node.status || 'unknown')}</span>`;

    // Progress bar (if applicable)
    if (node.progress != null && node.progress > 0) {
        const pct = Math.round(node.progress * 100);
        html += `
            <div class="detail-section">
                <div class="section-label">Progress</div>
                <div class="progress-bar">
                    <div class="progress-fill" style="width: ${pct}%; background: linear-gradient(90deg, ${statusColor}88, ${statusColor});"></div>
                </div>
                <div style="font-size: 10px; color: #888888; margin-top: 2px;">${pct}%</div>
            </div>
        `;
    }

    // Type-specific details
    if (node.details) {
        const d = node.details;

        if (node.node_type === 'agent') {
            html += `
                <div class="detail-section">
                    <div class="section-label">Agent Stats</div>
                    <div class="metric-row"><span class="metric-label">Tool Calls</span><span class="metric-value">${formatNumber(d.tool_calls)}</span></div>
                    <div class="metric-row"><span class="metric-label">Success</span><span class="metric-value">${formatPercent(d.success_rate != null ? d.success_rate * 100 : null)}</span></div>
                    <div class="metric-row"><span class="metric-label">Duration</span><span class="metric-value">${formatDuration(d.duration_ms)}</span></div>
                </div>
            `;
            if (d.task) {
                html += `
                    <div class="detail-section">
                        <div class="section-label">Task</div>
                        <div style="font-size: 12px; color: #e0e0e0; padding: 6px 8px; background: rgba(255,255,255,0.03); border-left: 2px solid ${statusColor};">${escapeHtml(d.task)}</div>
                    </div>
                `;
            }
            if (d.change_id) {
                html += `<div style="font-size: 10px; color: #666666; margin-top: 6px; font-family: 'JetBrains Mono', monospace;">Change: ${escapeHtml(d.change_id)}</div>`;
            }
        } else if (node.node_type === 'phase') {
            html += `
                <div class="detail-section">
                    <div class="section-label">Phase Info</div>
                    <div class="metric-row"><span class="metric-label">Agents</span><span class="metric-value">${formatNumber(d.agent_count)}</span></div>
                </div>
            `;
            if (d.blocking_status) {
                html += `<div style="font-size: 11px; color: #ffff00; margin-top: 4px;">Blocked: ${escapeHtml(d.blocking_status)}</div>`;
            }
        } else if (node.node_type === 'task') {
            if (d.description || d.task) {
                html += `
                    <div class="detail-section">
                        <div class="section-label">Description</div>
                        <div style="font-size: 12px; color: #e0e0e0; line-height: 1.4;">${escapeHtml(d.description || d.task)}</div>
                    </div>
                `;
            }
        }
    }

    // Phase indicator
    if (node.phase != null) {
        html += `<div style="font-size: 10px; color: #666666; margin-top: 8px;">Phase ${node.phase}</div>`;
    }

    els.detailContent.innerHTML = html;
    els.detailPanel.classList.add('visible');
}

export function hideDetailPanel() {
    if (els.detailPanel) els.detailPanel.classList.remove('visible');
    if (els.metricsPanel) els.metricsPanel.style.display = '';
}

// ── Connection Status ──

export function setConnectionStatus(status) {
    if (els.connectionDot) {
        els.connectionDot.className = `connection-dot ${status}`;
    }
    if (els.connectionLabel) {
        els.connectionLabel.textContent = status;
    }
}

// ── Uptime Counter ──

function startUptimeCounter() {
    if (uptimeInterval) clearInterval(uptimeInterval);
    updateUptimeDisplay();
    uptimeInterval = setInterval(updateUptimeDisplay, 1000);
}

function updateUptimeDisplay() {
    if (!els.uptime || !sessionStartTime) return;
    const elapsed = Date.now() - sessionStartTime;
    els.uptime.textContent = formatDuration(elapsed);
}

// ── Utility ──

function escapeHtml(str) {
    if (!str) return '';
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}
