// hox-viz 3D force graph with bloom post-processing
// THREE, ForceGraph3D loaded via UMD script tags (globals)
// Post-processing: THREE.EffectComposer, THREE.RenderPass, THREE.UnrealBloomPass
import { createNodeObject } from './nodes.js';

const ForceGraph3D = window.ForceGraph3D;

let graph = null;
let gridHelper = null;
let labelsVisible = true;

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
        .linkDirectionalParticles(link => link.particles || 0)
        .linkDirectionalParticleSpeed(link => link.particle_speed || 0.005)
        .linkDirectionalParticleWidth(2)
        .linkColor(link => link.color || '#666666')
        .linkWidth(link => link.width || 1)
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
    }
}

/**
 * Full state update -- replaces all graph data.
 * @param {object} state - { nodes: [...], links: [...] }
 */
export function updateGraph(state) {
    if (!graph) return;
    graph.graphData({
        nodes: state.nodes || [],
        links: state.links || [],
    });
}

/**
 * Incremental delta update -- modifies changed nodes in-place.
 * @param {object} delta - { changed_nodes: [...] }
 */
export function applyDelta(delta) {
    if (!graph) return;

    const data = graph.graphData();
    let changed = false;

    for (const updated of (delta.changed_nodes || [])) {
        const existing = data.nodes.find(n => n.id === updated.id);
        if (existing) {
            Object.assign(existing, updated);
            changed = true;
        }
    }

    if (changed) {
        // Re-trigger nodeThreeObject for updated nodes by refreshing graph data
        graph.graphData(data);
    }
}

/**
 * Returns the graph instance for external access.
 * @returns {object|null}
 */
export function getGraph() {
    return graph;
}
