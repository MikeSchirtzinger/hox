// Agent lane colors — distinct cyberpunk palette cycling per agent lane
export const AGENT_LANE_COLORS = [
    '#00ffff',  // Cyan
    '#ff00ff',  // Magenta
    '#00ff88',  // Green
    '#ff8800',  // Orange
    '#8888ff',  // Periwinkle
    '#ffff00',  // Yellow
    '#ff0088',  // Hot pink
    '#00ccff',  // Sky blue
    '#ff4400',  // Red-orange
    '#88ff00',  // Lime
];

/**
 * Get a deterministic lane color for an agent by index or id.
 * @param {number|string} agentIndexOrId
 * @returns {string} hex color
 */
export function colorForAgentLane(agentIndexOrId) {
    let idx;
    if (typeof agentIndexOrId === 'number') {
        idx = agentIndexOrId;
    } else {
        // Hash string to index
        let h = 0;
        for (let i = 0; i < agentIndexOrId.length; i++) {
            h = (h * 31 + agentIndexOrId.charCodeAt(i)) >>> 0;
        }
        idx = h;
    }
    return AGENT_LANE_COLORS[idx % AGENT_LANE_COLORS.length];
}

// DAG-specific link colors
export const DAG_LINK_COLORS = {
    dag_parent:   null,          // per-agent lane color (computed at render time)
    agent_branch: '#00ffff',     // Cyan
    file_touch:   '#224444',     // Dark teal
    merge_edge:   '#ff00ff',     // Magenta
};

// Status colors (cyberpunk palette)
export const STATUS_COLORS = {
    running:   '#00ffff',  // Cyan
    completed: '#ff00ff',  // Magenta
    blocked:   '#ffff00',  // Yellow
    failed:    '#ff0044',  // Red
    pending:   '#444444',  // Dim gray
    active:    '#00ffff',  // Cyan (for phases)
};

// Link type colors
export const LINK_COLORS = {
    working_on: '#00ffff',
    dependency: '#666666',
    message:    '#ff00ff',
};

// Op type colors (for oplog feed borders)
export const OP_COLORS = {
    new:       '#00ffff',
    describe:  '#00cc99',
    squash:    '#ff00ff',
    bookmark:  '#ffff00',
    commit:    '#00ffff',
    rebase:    '#ff8800',
    workspace: '#8888ff',
    other:     '#666666',
};

// UI accent colors
export const UI = {
    bg:         '#0a0a0f',
    panel:      'rgba(10, 10, 20, 0.85)',
    border:     'rgba(0, 255, 255, 0.3)',
    text:       '#e0e0e0',
    textDim:    '#888888',
    textBright: '#ffffff',
    cyan:       '#00ffff',
    magenta:    '#ff00ff',
    yellow:     '#ffff00',
    red:        '#ff0044',
    green:      '#00ff88',
};

// Glow intensities by status
export const GLOW = {
    running:   0.8,
    completed: 0.5,
    failed:    1.0,
    blocked:   0.6,
    pending:   0.1,
};

// Helper: get color for a status string
export function colorForStatus(status) {
    return STATUS_COLORS[status] || STATUS_COLORS.pending;
}

// Helper: get color for an op type
export function colorForOpType(opType) {
    return OP_COLORS[opType] || OP_COLORS.other;
}
