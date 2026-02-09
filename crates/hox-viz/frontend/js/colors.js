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
