// hox-viz 3D force graph with bloom post-processing
// THREE, ForceGraph3D loaded via UMD script tags (globals)
// Post-processing: THREE.EffectComposer, THREE.RenderPass, THREE.UnrealBloomPass
import { createNodeObject } from './nodes.js';
import { colorForAgentLane, DAG_LINK_COLORS } from './colors.js';

const ForceGraph3D = window.ForceGraph3D;

let graph = null;
let gridHelper = null;
let labelsVisible = true;
let nodeThreeObjectFn = node => createNodeObject(node);

// View mode: 'orchestration' | 'dag'
export let viewMode = 'orchestration';

// Callback invoked by V key handler — main.js sets this so it can also update HUD
let onViewModeChange = null;

/**
 * Register a callback for view mode changes.
 * @param {function} cb
 */
export function setViewModeChangeCallback(cb) {
    onViewModeChange = cb;
}

// Default camera position for overview
const DEFAULT_CAMERA = { x: 0, y: 80, z: 150 };

/**
 * Initialize the 3d-force-graph in the given container with bloom post-processing.
 * @param {HTMLElement} container
 */
export function initGraph(container) {
    graph = ForceGraph3D()(container)
        .graphData({ nodes: [], links: [] })
        .nodeThreeObject(node => createNodeObject(node))
        .nodeThreeObjectExtend(false)
        .nodeId('id')
        .linkDirectionalParticles(link => {
            if (link.link_type === 'merge_edge') return 5;
            if (link.link_type === 'agent_branch') return 3;
            if (link.link_type === 'file_touch') return 0;
            return link.particles || 0;
        })
        .linkDirectionalParticleSpeed(link => {
            if (link.link_type === 'merge_edge') return 0.015;
            return link.particle_speed || 0.005;
        })
        .linkDirectionalParticleWidth(2)
        .linkColor(link => {
            if (link.link_type === 'dag_parent') {
                // Use agent lane color derived from source node id
                const sourceId = typeof link.source === 'object' ? link.source.id : link.source;
                return colorForAgentLane(sourceId || '');
            }
            if (link.link_type === 'agent_branch') return DAG_LINK_COLORS.agent_branch;
            if (link.link_type === 'file_touch') return DAG_LINK_COLORS.file_touch;
            if (link.link_type === 'merge_edge') return DAG_LINK_COLORS.merge_edge;
            return link.color || '#666666';
        })
        .linkWidth(link => {
            if (link.link_type === 'dag_parent') return 2;
            if (link.link_type === 'agent_branch') return 2;
            if (link.link_type === 'file_touch') return 0.5;
            if (link.link_type === 'merge_edge') return 3;
            return link.width || 1;
        })
        .linkOpacity(0.6)
        .backgroundColor('#0a0a0f')
        .onNodeClick(node => {
            window.dispatchEvent(new CustomEvent('node-click', { detail: node }));
        });

    // Bloom post-processing
    const renderer = graph.renderer();
    const scene = graph.scene();
    const camera = graph.camera();

    // Post-processing - use THREE globals from UMD
    const EffectComposer = THREE.EffectComposer;
    const RenderPass = THREE.RenderPass;
    const UnrealBloomPass = THREE.UnrealBloomPass;

    // Only add bloom if post-processing classes are available
    const hasPostProcessing = EffectComposer && RenderPass && UnrealBloomPass;

    let bloomPass = null;
    if (hasPostProcessing) {
        bloomPass = new UnrealBloomPass(
            new THREE.Vector2(window.innerWidth, window.innerHeight),
            1.5,   // strength
            0.4,   // radius
            0.85   // threshold
        );
    }

    if (hasPostProcessing && bloomPass) {
        setupManualComposer(renderer, scene, camera, bloomPass);
    }

    // Grid helper
    gridHelper = new THREE.GridHelper(200, 40, 0x00ffff, 0x111122);
    gridHelper.material.opacity = 0.15;
    gridHelper.material.transparent = true;
    scene.add(gridHelper);

    // Lighting
    const ambient = new THREE.AmbientLight(0x222222);
    scene.add(ambient);

    const cyanLight = new THREE.PointLight(0x00ffff, 0.5);
    cyanLight.position.set(100, 100, 100);
    scene.add(cyanLight);

    const magentaLight = new THREE.PointLight(0xff00ff, 0.3);
    magentaLight.position.set(-100, -100, -100);
    scene.add(magentaLight);

    // Initial camera position
    graph.cameraPosition(DEFAULT_CAMERA);

    // Node animations: rotate task octahedrons, pulse running agent rings
    graph.onEngineTick(() => {
        scene.traverse(obj => {
            // Rotate task octahedrons
            if (obj.userData.rotate) {
                obj.rotation.y += 0.01;
                obj.rotation.x += 0.005;
            }
            // Pulse running agent rings
            if (obj.userData.pulse) {
                const phase = obj.userData.pulsePhase || 0;
                const t = performance.now() * 0.002 + phase;
                const scale = 1 + 0.08 * Math.sin(t);
                obj.scale.set(scale, scale, scale);
            }
        });
    });

    // Keyboard shortcuts
    window.addEventListener('keydown', handleKeydown);

    // Handle window resize for bloom pass
    if (bloomPass) {
        window.addEventListener('resize', () => {
            bloomPass.resolution.set(window.innerWidth, window.innerHeight);
        });
    }

    return graph;
}

/**
 * Fallback manual composer when native postProcessingComposer is unavailable.
 */
function setupManualComposer(renderer, scene, camera, bloomPass) {
    const composer = new THREE.EffectComposer(renderer);
    composer.addPass(new THREE.RenderPass(scene, camera));
    composer.addPass(bloomPass);

    const animate = () => {
        requestAnimationFrame(animate);
        composer.render();
    };
    animate();
}

/**
 * Handle keyboard shortcuts.
 * @param {KeyboardEvent} e
 */
function handleKeydown(e) {
    if (!graph) return;

    // Don't capture if user is typing in an input
    if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;

    switch (e.key.toUpperCase()) {
        case 'R':
            // Reset camera
            graph.cameraPosition(DEFAULT_CAMERA, { x: 0, y: 0, z: 0 }, 1000);
            break;
        case 'F':
            // Toggle fullscreen
            if (!document.fullscreenElement) {
                document.documentElement.requestFullscreen();
            } else {
                document.exitFullscreen();
            }
            break;
        case 'G':
            // Toggle grid
            if (gridHelper) gridHelper.visible = !gridHelper.visible;
            break;
        case 'L':
            // Toggle labels
            labelsVisible = !labelsVisible;
            graph.scene().traverse(obj => {
                if (obj instanceof THREE.Sprite) {
                    obj.visible = labelsVisible;
                }
            });
            break;
        case 'V':
            // Toggle view mode: orchestration <-> dag
            viewMode = viewMode === 'dag' ? 'orchestration' : 'dag';
            if (onViewModeChange) onViewModeChange(viewMode);
            break;
    }
}

// --- Helpers for incremental graph updates ---

/** Extract the ID from a link endpoint (may be a string or a node object). */
function linkEndpointId(ref) {
    return typeof ref === 'object' && ref !== null ? ref.id : ref;
}

/** Check if the set of links changed structurally (different topology). */
function linksChanged(oldLinks, newLinks) {
    if (oldLinks.length !== newLinks.length) return true;
    const key = l => `${linkEndpointId(l.source)}\0${linkEndpointId(l.target)}`;
    const oldSet = new Set(oldLinks.map(key));
    for (const l of newLinks) {
        if (!oldSet.has(key(l))) return true;
    }
    return false;
}

/** Check if any visual properties of a node changed. */
function nodeVisualChanged(existing, updated) {
    return existing.status !== updated.status
        || existing.color !== updated.color
        || existing.label !== updated.label
        || existing.progress !== updated.progress
        || existing.glow_intensity !== updated.glow_intensity
        || existing.node_type !== updated.node_type;
}

/** Save simulation position fields from a node, including fixed position pins. */
function savePos(node) {
    return { x: node.x, y: node.y, z: node.z,
             vx: node.vx, vy: node.vy, vz: node.vz,
             fx: node.fx, fy: node.fy, fz: node.fz };
}

/**
 * Refresh Three.js node objects without restarting the force simulation.
 * Re-setting the nodeThreeObject accessor triggers object recreation on the
 * next render tick while leaving the d3-force simulation untouched.
 */
function refreshNodeVisuals() {
    nodeThreeObjectFn = node => createNodeObject(node); // new ref so library detects change
    graph.nodeThreeObject(nodeThreeObjectFn);
}

/**
 * Full state update -- merges new state, preserving existing node positions.
 * Only calls graphData() (which reheats the simulation) when the graph
 * topology actually changed (nodes added/removed, links changed).
 * For property-only changes, mutates in-place and refreshes visuals.
 * @param {object} state - { nodes: [...], links: [...] }
 */
export function updateGraph(state) {
    if (!graph) return;

    const currentData = graph.graphData();
    const newNodes = state.nodes || [];
    const newLinks = state.links || [];

    // First load — just set data normally and let the simulation run
    if (currentData.nodes.length === 0) {
        graph.graphData({ nodes: newNodes, links: newLinks });
        return;
    }

    // Build map of current nodes keyed by id
    const currentMap = new Map();
    for (const n of currentData.nodes) {
        currentMap.set(n.id, n);
    }

    // Detect structural changes (added/removed nodes or changed link topology)
    const newIds = new Set(newNodes.map(n => n.id));
    const hasNewNodes = newNodes.some(n => !currentMap.has(n.id));
    const hasRemovedNodes = currentData.nodes.some(n => !newIds.has(n.id));
    const structureChanged = hasNewNodes || hasRemovedNodes
        || linksChanged(currentData.links, newLinks);

    if (!structureChanged) {
        // Topology unchanged — update properties in-place, refresh only visuals
        let anyVisualChange = false;
        for (const newNode of newNodes) {
            const existing = currentMap.get(newNode.id);
            if (existing && nodeVisualChanged(existing, newNode)) {
                const pos = savePos(existing);
                // Backend-provided fx/fy/fz always take precedence over saved positions
                const mergedPos = {
                    x: pos.x, y: pos.y, z: pos.z,
                    vx: pos.vx, vy: pos.vy, vz: pos.vz,
                    fx: newNode.fx != null ? newNode.fx : pos.fx,
                    fy: newNode.fy != null ? newNode.fy : pos.fy,
                    fz: newNode.fz != null ? newNode.fz : pos.fz,
                };
                Object.assign(existing, newNode, mergedPos);
                anyVisualChange = true;
            }
        }
        if (anyVisualChange) {
            refreshNodeVisuals();
        }
        return;
    }

    // Structural change — merge with position preservation, then graphData()
    const mergedNodes = newNodes.map(n => {
        const existing = currentMap.get(n.id);
        if (existing) {
            const pos = savePos(existing);
            // Backend-provided fx/fy/fz always take precedence
            return {
                ...n,
                x: pos.x, y: pos.y, z: pos.z,
                vx: pos.vx, vy: pos.vy, vz: pos.vz,
                fx: n.fx != null ? n.fx : pos.fx,
                fy: n.fy != null ? n.fy : pos.fy,
                fz: n.fz != null ? n.fz : pos.fz,
            };
        }
        return n; // new node — simulation will position it
    });

    graph.graphData({ nodes: mergedNodes, links: newLinks });
}

/**
 * Incremental delta update -- modifies changed nodes in-place.
 * Avoids graphData() (and simulation reheat) unless a genuinely new node
 * appears that needs to be added to the simulation.
 * @param {object} delta - { changed_nodes: [...] }
 */
export function applyDelta(delta) {
    if (!graph) return;

    const data = graph.graphData();
    let visualChange = false;
    let structuralChange = false;

    // Remove nodes (and their connected links)
    if (delta.removed_node_ids && delta.removed_node_ids.length > 0) {
        const removeSet = new Set(delta.removed_node_ids);
        const before = data.nodes.length;
        data.nodes = data.nodes.filter(n => !removeSet.has(n.id));
        data.links = data.links.filter(l => {
            const src = typeof l.source === 'object' ? l.source.id : l.source;
            const tgt = typeof l.target === 'object' ? l.target.id : l.target;
            return !removeSet.has(src) && !removeSet.has(tgt);
        });
        if (data.nodes.length !== before) structuralChange = true;
    }

    // Update or add changed nodes
    for (const updated of (delta.changed_nodes || [])) {
        const existing = data.nodes.find(n => n.id === updated.id);
        if (existing) {
            const pos = savePos(existing);
            Object.assign(existing, updated, pos);
            visualChange = true;
        } else {
            data.nodes.push(updated);
            structuralChange = true;
        }
    }

    // Update or add changed links
    for (const link of (delta.changed_links || [])) {
        const idx = data.links.findIndex(l => {
            const src = typeof l.source === 'object' ? l.source.id : l.source;
            const tgt = typeof l.target === 'object' ? l.target.id : l.target;
            return src === link.source && tgt === link.target;
        });
        if (idx >= 0) {
            data.links[idx] = link;
        } else {
            data.links.push(link);
            structuralChange = true;
        }
    }

    if (structuralChange) {
        graph.graphData({ nodes: data.nodes, links: data.links });
    } else if (visualChange) {
        refreshNodeVisuals();
    }
}

/**
 * Configure forces for DAG layout mode.
 * Change/root/merge nodes have fixed positions (fx/fy/fz from backend).
 * File nodes float with gentle repulsion.
 */
export function configureDagForces() {
    if (!graph) return;
    graph.d3Force('center', null);
    const charge = graph.d3Force('charge');
    if (charge) {
        charge.strength(node => node.node_type === 'file' ? -20 : 0);
    }
    const link = graph.d3Force('link');
    if (link) {
        link.distance(l => l.link_type === 'file_touch' ? 15 : 40)
            .strength(l => l.link_type === 'file_touch' ? 0.3 : 0.1);
    }
}

/**
 * Restore default forces for orchestration layout mode.
 */
export function configureOrchForces() {
    if (!graph) return;
    // Restore center force
    const d3 = window.d3;
    if (d3 && d3.forceCenter) {
        graph.d3Force('center', d3.forceCenter());
    }
    const charge = graph.d3Force('charge');
    if (charge) {
        charge.strength(-120);
    }
    const link = graph.d3Force('link');
    if (link) {
        link.distance(30).strength(1);
    }
}

/**
 * Move camera to a DAG-appropriate overview angle based on fixed-position nodes.
 */
export function dagCamera() {
    if (!graph) return;
    const nodes = graph.graphData().nodes.filter(n => n.fx != null);
    if (nodes.length === 0) return;
    const maxX = Math.max(...nodes.map(n => n.fx));
    const maxY = Math.max(...nodes.map(n => n.fy));
    graph.cameraPosition(
        { x: maxX / 2, y: maxY / 2, z: Math.max(maxX, 100) * 0.8 },
        { x: maxX / 2, y: maxY / 2, z: 0 },
        1000
    );
}

/**
 * Returns the graph instance for external access.
 * @returns {object|null}
 */
export function getGraph() {
    return graph;
}
