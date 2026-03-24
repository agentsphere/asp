/**
 * Load all diagrams from docs/arc42/diagrams/ using the manifest.
 *
 * Vite's import.meta.glob imports .mmd files as raw text at build time,
 * and the manifest (diagrams.json) declares the role of each file.
 */

// Import all .mmd files as raw strings (resolved at build time)
const mmdFiles = import.meta.glob(
  '../../../arc42/diagrams/*.mmd',
  { eager: true, query: '?raw', import: 'default' },
);

// Import the manifest
import manifest from '../../../arc42/diagrams/diagrams.json';

/** Resolve a manifest filename to its raw Mermaid content. */
function resolve(filename) {
  // The glob keys look like "../../../arc42/diagrams/context.mmd"
  const key = `../../../arc42/diagrams/${filename}`;
  const content = mmdFiles[key];
  if (!content) {
    console.warn(`Diagram file not found: ${filename}`);
    return null;
  }
  return content;
}

// ── Architecture diagrams ───────────────────────────────────────────

export const c1Diagram = {
  ...manifest.architecture.c1,
  mermaid: resolve(manifest.architecture.c1.file),
};

export const c2Diagram = {
  ...manifest.architecture.c2,
  mermaid: resolve(manifest.architecture.c2.file),
};

export const c3Diagrams = manifest.architecture.c3.map(entry => ({
  ...entry,
  id: entry.file.replace('.mmd', ''),
  mermaid: resolve(entry.file),
}));

export const deploymentDiagrams = manifest.architecture.deployment.map(entry => ({
  ...entry,
  id: entry.file.replace('.mmd', ''),
  mermaid: resolve(entry.file),
}));

/** Get C3 diagram for a given parent label (matched against containers.mmd names). */
export function getC3ForParent(parentLabel) {
  return c3Diagrams.find(d =>
    d.parent.toLowerCase() === parentLabel.toLowerCase()
  ) || null;
}

// ── Flow diagrams ───────────────────────────────────────────────────

export const flowDiagrams = manifest.flows.map(entry => ({
  ...entry,
  id: entry.file.replace('.mmd', ''),
  mermaid: resolve(entry.file),
}));

/** Get flows grouped by category. */
export function getFlowsByCategory() {
  const groups = {};
  for (const flow of flowDiagrams) {
    (groups[flow.category] ??= []).push(flow);
  }
  return groups;
}

// ── Concept diagrams ─────────────────────────────────────────────────

export const conceptDiagrams = manifest.concepts.map(entry => ({
  ...entry,
  id: entry.file.replace('.mmd', ''),
  mermaid: resolve(entry.file),
}));

/** Get concepts grouped by category. */
export function getConceptsByCategory() {
  const groups = {};
  for (const c of conceptDiagrams) {
    (groups[c.category] ??= []).push(c);
  }
  return groups;
}

// ── All diagram IDs for sidebar listing ─────────────────────────────

/** C3 parent labels that can be zoomed into. */
export const c3Parents = c3Diagrams.map(d => d.parent);

/** Check if a C2 container label has a C3 drill-down. */
export function hasC3(containerLabel) {
  return c3Parents.some(p => p.toLowerCase() === containerLabel.toLowerCase());
}

// ── File → navigation mapping ─────────────────────────────────────

/** Build a map from .mmd filename to a navigation action descriptor. */
function buildFileMap() {
  const map = {};

  // Architecture
  map[manifest.architecture.c1.file] = { level: 'c1' };
  map[manifest.architecture.c2.file] = { level: 'c2' };
  for (const c3 of manifest.architecture.c3) {
    map[c3.file] = { level: 'c3', parent: c3.parent };
  }
  for (const dep of manifest.architecture.deployment) {
    map[dep.file] = { level: 'deployment', id: dep.file.replace('.mmd', '') };
  }

  // Flows
  for (const flow of manifest.flows) {
    map[flow.file] = { level: 'flow', id: flow.file.replace('.mmd', '') };
  }

  // Concepts
  for (const concept of manifest.concepts) {
    map[concept.file] = { level: 'concept', id: concept.file.replace('.mmd', '') };
  }

  return map;
}

const fileNavMap = buildFileMap();

/** Get navigation target for a .mmd filename, or null if not in the manifest. */
export function getNavForFile(filename) {
  return fileNavMap[filename] || null;
}
