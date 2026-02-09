// hox-viz custom Three.js node geometry factories
// THREE loaded via UMD script tag (global)
import { colorForStatus } from './colors.js';

/**
 * Create a text sprite label for a node.
 * @param {string} text - Label text
 * @param {string} color - CSS color string
 * @returns {THREE.Sprite}
 */
function createTextSprite(text, color) {
    const canvas = document.createElement('canvas');
    canvas.width = 256;
    canvas.height = 64;
    const ctx = canvas.getContext('2d');

    ctx.clearRect(0, 0, 256, 64);
    ctx.font = '24px "JetBrains Mono", monospace';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';

    // Shadow for readability
    ctx.shadowColor = '#000000';
    ctx.shadowBlur = 4;
    ctx.shadowOffsetX = 1;
    ctx.shadowOffsetY = 1;

    ctx.fillStyle = color || '#e0e0e0';
    ctx.fillText(text, 128, 32, 240);

    const texture = new THREE.CanvasTexture(canvas);
    texture.needsUpdate = true;

    const material = new THREE.SpriteMaterial({
        map: texture,
        transparent: true,
        depthTest: false,
    });

    const sprite = new THREE.Sprite(material);
    sprite.scale.set(15, 3.75, 1);
    sprite.position.y = -8;

    return sprite;
}

/**
 * Create an emissive MeshStandardMaterial.
 * @param {string} color - hex color
 * @param {number} emissiveIntensity
 * @param {object} [overrides] - additional material properties
 * @returns {THREE.MeshStandardMaterial}
 */
function makeEmissiveMaterial(color, emissiveIntensity, overrides = {}) {
    return new THREE.MeshStandardMaterial({
        color: new THREE.Color(color),
        emissive: new THREE.Color(color),
        emissiveIntensity,
        metalness: 0.3,
        roughness: 0.7,
        ...overrides,
    });
}

/**
 * Build an Agent node (sphere + glow ring + progress arc + label).
 * @param {object} node
 * @returns {THREE.Group}
 */
function createAgentNode(node) {
    const group = new THREE.Group();
    const color = node.color || colorForStatus(node.status);
    const glowIntensity = node.glow_intensity ?? 0.8;

    // Core sphere
    const sphereGeo = new THREE.SphereGeometry(5, 16, 16);
    const sphereMat = makeEmissiveMaterial(color, glowIntensity);
    const sphere = new THREE.Mesh(sphereGeo, sphereMat);
    group.add(sphere);

    // Outer glow ring
    const ringGeo = new THREE.TorusGeometry(7, 0.3, 8, 32);
    const ringMat = makeEmissiveMaterial(color, glowIntensity * 0.6, {
        transparent: true,
        opacity: 0.4,
    });
    const ring = new THREE.Mesh(ringGeo, ringMat);
    ring.rotation.x = Math.PI / 2;
    group.add(ring);

    // Progress arc (partial torus)
    const progress = node.progress ?? 0;
    if (progress > 0) {
        const arcAngle = progress * Math.PI * 2;
        const arcGeo = new THREE.TorusGeometry(7, 0.5, 4, 32, arcAngle);
        const arcMat = makeEmissiveMaterial(color, 1.0, {
            transparent: true,
            opacity: 0.9,
        });
        const arc = new THREE.Mesh(arcGeo, arcMat);
        arc.rotation.x = Math.PI / 2;
        // Start from top (-Z in torus space after rotation)
        arc.rotation.z = -Math.PI / 2;
        group.add(arc);
    }

    // Pulsing ring animation for running agents
    if (node.status === 'running') {
        ring.userData.pulse = true;
        ring.userData.pulsePhase = Math.random() * Math.PI * 2;
    }

    // Label
    group.add(createTextSprite(node.label || node.id, color));

    return group;
}

/**
 * Build a Phase node (hexagonal cylinder + wireframe + label).
 * @param {object} node
 * @returns {THREE.Group}
 */
function createPhaseNode(node) {
    const group = new THREE.Group();
    const color = node.color || colorForStatus(node.status);
    const glowIntensity = node.glow_intensity ?? 0.5;

    // Hexagonal cylinder (6 radial segments = hexagon)
    const hexGeo = new THREE.CylinderGeometry(8, 8, 2, 6);

    // Solid fill
    const fillMat = makeEmissiveMaterial(color, glowIntensity, {
        transparent: true,
        opacity: 0.4,
    });
    const fill = new THREE.Mesh(hexGeo, fillMat);
    group.add(fill);

    // Wireframe edge
    const wireMat = new THREE.MeshBasicMaterial({
        wireframe: true,
        color: new THREE.Color(color),
        transparent: true,
        opacity: 0.8,
    });
    const wire = new THREE.Mesh(hexGeo, wireMat);
    group.add(wire);

    // Label
    group.add(createTextSprite(node.label || node.id, color));

    return group;
}

/**
 * Build a Task node (rotating octahedron + label).
 * @param {object} node
 * @returns {THREE.Group}
 */
function createTaskNode(node) {
    const group = new THREE.Group();
    const color = node.color || colorForStatus(node.status);
    const glowIntensity = node.glow_intensity ?? 0.6;

    // Octahedron
    const octGeo = new THREE.OctahedronGeometry(4);
    const octMat = makeEmissiveMaterial(color, glowIntensity);
    const oct = new THREE.Mesh(octGeo, octMat);
    oct.userData.rotate = true;
    group.add(oct);

    // Label
    group.add(createTextSprite(node.label || node.id, color));

    return group;
}

/**
 * Main factory: create a Three.js Object3D for a graph node based on its type.
 * @param {object} node - graph node data
 * @returns {THREE.Object3D}
 */
export function createNodeObject(node) {
    switch (node.node_type) {
        case 'agent':
            return createAgentNode(node);
        case 'phase':
            return createPhaseNode(node);
        case 'task':
            return createTaskNode(node);
        default:
            return createTaskNode(node);
    }
}
