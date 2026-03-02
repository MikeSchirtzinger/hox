import {
    initGraph, updateGraph, applyDelta as graphApplyDelta,
    configureDagForces, configureOrchForces, dagCamera,
    setViewModeChangeCallback,
} from './graph.js';
import { initHud, updateAll, applyDelta as hudApplyDelta, addOplogEntry, setConnectionStatus, setViewModeBadge } from './hud.js';
import { UI } from './colors.js';

let currentState = null;
let eventSource = null;
let reconnectDelay = 1000;

// View mode: 'orchestration' | 'dag'
export let viewMode = 'orchestration';

// Node types visible in each mode
const VIEW_MODE_NODE_TYPES = {
    orchestration: new Set(['agent', 'phase', 'task']),
    dag: new Set(['change', 'file', 'merge', 'root', 'agent']),
};

/**
 * Filter state nodes/links to those relevant for the current view mode.
 */
function filterStateForMode(state, mode) {
    const allowed = VIEW_MODE_NODE_TYPES[mode] || VIEW_MODE_NODE_TYPES.orchestration;
    const filteredNodes = (state.nodes || []).filter(n => allowed.has(n.node_type));
    const nodeIds = new Set(filteredNodes.map(n => n.id));
    const filteredLinks = (state.links || []).filter(l => {
        const src = typeof l.source === 'object' ? l.source.id : l.source;
        const tgt = typeof l.target === 'object' ? l.target.id : l.target;
        return nodeIds.has(src) && nodeIds.has(tgt);
    });
    return { ...state, nodes: filteredNodes, links: filteredLinks };
}

function onViewModeChanged(newMode) {
    viewMode = newMode;
    setViewModeBadge(viewMode);
    if (currentState) {
        updateGraph(filterStateForMode(currentState, viewMode));
        if (viewMode === 'dag') {
            configureDagForces();
            dagCamera();
        } else {
            configureOrchForces();
        }
    }
}

function connect() {
    setConnectionStatus('connecting');
    eventSource = new EventSource('/api/events');

    eventSource.addEventListener('state', (e) => {
        currentState = JSON.parse(e.data);
        updateGraph(filterStateForMode(currentState, viewMode));
        updateAll(currentState);
        setConnectionStatus('connected');
        reconnectDelay = 1000;
    });

    eventSource.addEventListener('update', (e) => {
        const delta = JSON.parse(e.data);
        if (currentState) {
            applyDeltaToState(currentState, delta);
        }
        graphApplyDelta(delta);
        hudApplyDelta(delta);
    });

    eventSource.addEventListener('oplog', (e) => {
        const entry = JSON.parse(e.data);
        addOplogEntry(entry);
    });

    eventSource.onerror = () => {
        setConnectionStatus('disconnected');
        eventSource.close();
        setTimeout(() => {
            reconnectDelay = Math.min(reconnectDelay * 2, 10000);
            connect();
        }, reconnectDelay);
    };
}

function applyDeltaToState(state, delta) {
    // Remove nodes that no longer exist
    if (delta.removed_node_ids && delta.removed_node_ids.length > 0) {
        const removeSet = new Set(delta.removed_node_ids);
        state.nodes = state.nodes.filter(n => !removeSet.has(n.id));
        // Also remove links referencing removed nodes
        state.links = (state.links || []).filter(l => {
            const src = typeof l.source === 'object' ? l.source.id : l.source;
            const tgt = typeof l.target === 'object' ? l.target.id : l.target;
            return !removeSet.has(src) && !removeSet.has(tgt);
        });
    }
    // Update or add changed nodes
    for (const node of (delta.changed_nodes || [])) {
        const idx = state.nodes.findIndex(n => n.id === node.id);
        if (idx >= 0) state.nodes[idx] = node;
        else state.nodes.push(node);
    }
    // Update or add changed links
    for (const link of (delta.changed_links || [])) {
        const idx = state.links.findIndex(l => {
            const src = typeof l.source === 'object' ? l.source.id : l.source;
            const tgt = typeof l.target === 'object' ? l.target.id : l.target;
            return src === link.source && tgt === link.target;
        });
        if (idx >= 0) state.links[idx] = link;
        else state.links.push(link);
    }
    if (delta.metrics) state.metrics = delta.metrics;
    for (const entry of (delta.new_oplog || [])) {
        state.oplog.push(entry);
    }
    for (const phase of (delta.changed_phases || [])) {
        const idx = state.phases.findIndex(p => p.number === phase.number);
        if (idx >= 0) state.phases[idx] = phase;
        else state.phases.push(phase);
    }
}

function init() {
    initGraph(document.getElementById('graph-container'));
    initHud(document.getElementById('hud-overlay'));
    setViewModeChangeCallback(onViewModeChanged);
    setViewModeBadge(viewMode);
    connect();
}

// Module scripts are deferred, so DOM is ready by the time this runs
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
} else {
    init();
}

export function getState() { return currentState; }
