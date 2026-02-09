import { initGraph, updateGraph, applyDelta as graphApplyDelta } from './graph.js';
import { initHud, updateAll, applyDelta as hudApplyDelta, addOplogEntry, setConnectionStatus } from './hud.js';
import { UI } from './colors.js';

let currentState = null;
let eventSource = null;
let reconnectDelay = 1000;

function connect() {
    setConnectionStatus('connecting');
    eventSource = new EventSource('/api/events');

    eventSource.addEventListener('state', (e) => {
        currentState = JSON.parse(e.data);
        updateGraph(currentState);
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
    for (const node of (delta.changed_nodes || [])) {
        const idx = state.nodes.findIndex(n => n.id === node.id);
        if (idx >= 0) state.nodes[idx] = node;
        else state.nodes.push(node);
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
    connect();
}

// Module scripts are deferred, so DOM is ready by the time this runs
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
} else {
    init();
}

export function getState() { return currentState; }
