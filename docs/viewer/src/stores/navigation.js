import { writable, derived } from 'svelte/store';
import { hasC3 } from '../data/diagrams.js';

/**
 * Navigation state for the diagram viewer.
 *
 * Levels:
 *   'doc'        — Documentation page
 *   'c1'         — System Context
 *   'c2'         — Containers (modules)
 *   'c3'         — Components inside a module (identified by parentLabel)
 *   'deployment' — Deployment topology (identified by diagramId)
 *   'flow'       — Runtime flow diagrams
 *   'concept'    — Concept diagrams (state machines, ER, module comm)
 */

/** Current view level. */
export const currentLevel = writable('c1');

/** For C3: the container label we zoomed into (e.g., "Agent Orchestrator"). */
export const c3ParentLabel = writable(null);

/** For deployment: which deployment diagram. */
export const deploymentId = writable(null);

/** For flow: which flow diagram ID is active. */
export const activeFlowId = writable(null);

/** For doc: which doc page is active. */
export const activeDocId = writable(null);

/** For concept: which concept diagram is active. */
export const activeConceptId = writable(null);

/** Flow panel (slide-out) state. */
export const flowPanelOpen = writable(false);
export const flowPanelNodeLabel = writable(null);

/** Breadcrumb derived from current state. */
export const breadcrumb = derived(
  [currentLevel, c3ParentLabel],
  ([$level, $parent]) => {
    const crumbs = [{ label: 'System Context', level: 'c1' }];
    if ($level === 'c2' || $level === 'c3') {
      crumbs.push({ label: 'Containers', level: 'c2' });
    }
    if ($level === 'c3' && $parent) {
      crumbs.push({ label: $parent, level: 'c3' });
    }
    if ($level === 'deployment') {
      crumbs.push({ label: 'Deployment', level: 'deployment' });
    }
    return crumbs;
  }
);

/** Navigate to C1. */
export function goC1() {
  currentLevel.set('c1');
  c3ParentLabel.set(null);
  activeFlowId.set(null);
  activeDocId.set(null);
  activeConceptId.set(null);
  flowPanelOpen.set(false);
}

/** Navigate to C2. */
export function goC2() {
  currentLevel.set('c2');
  c3ParentLabel.set(null);
  activeFlowId.set(null);
  activeDocId.set(null);
  activeConceptId.set(null);
  flowPanelOpen.set(false);
}

/** Zoom into a C2 container to see its C3 components. */
export function goC3(parentLabel) {
  if (!hasC3(parentLabel)) return false;
  currentLevel.set('c3');
  c3ParentLabel.set(parentLabel);
  activeFlowId.set(null);
  activeDocId.set(null);
  activeConceptId.set(null);
  flowPanelOpen.set(false);
  return true;
}

/** Navigate to a deployment diagram. */
export function goDeployment(id) {
  currentLevel.set('deployment');
  deploymentId.set(id);
  activeFlowId.set(null);
  activeDocId.set(null);
  activeConceptId.set(null);
  flowPanelOpen.set(false);
}

/** Switch to flow view. */
export function showFlow(flowId) {
  currentLevel.set('flow');
  activeFlowId.set(flowId);
  activeDocId.set(null);
  activeConceptId.set(null);
  flowPanelOpen.set(false);
}

/** Switch to doc view. */
export function showDoc(docId) {
  currentLevel.set('doc');
  activeDocId.set(docId);
  activeFlowId.set(null);
  activeConceptId.set(null);
  flowPanelOpen.set(false);
}

/** Switch to concept view. */
export function showConcept(conceptId) {
  currentLevel.set('concept');
  activeConceptId.set(conceptId);
  activeFlowId.set(null);
  activeDocId.set(null);
  flowPanelOpen.set(false);
}

/** Zoom out one level. */
export function zoomOut() {
  let lvl;
  currentLevel.subscribe(v => lvl = v)();
  if (lvl === 'c3') goC2();
  else if (lvl === 'c2') goC1();
  else if (lvl === 'deployment') goC2();
  else if (lvl === 'flow') goC2();
  else if (lvl === 'concept') goC2();
}

/** Back to architecture from flow/doc view. */
export function backToArchitecture() {
  goC2();
}

/** Close flow panel. */
export function closeFlowPanel() {
  flowPanelOpen.set(false);
  flowPanelNodeLabel.set(null);
}
