/**
 * Serialized mermaid render queue.
 *
 * mermaid.render() uses shared internal state (parser, DOM) and cannot be
 * called concurrently — concurrent calls produce empty SVGs or ID collisions.
 * This module ensures only one render runs at a time.
 */
import mermaid from 'mermaid';
import { transpileC4 } from './c4-to-flowchart.js';

let globalCounter = 0;
let queue = Promise.resolve();

const DARK_THEME = {
  darkMode: true,
  background: '#1a1a2e',
  primaryColor: '#1168bd',
  primaryTextColor: '#e0e0e0',
  primaryBorderColor: '#0b4884',
  lineColor: '#8892b0',
  secondaryColor: '#438dd5',
  tertiaryColor: '#2d2d4e',
  fontFamily: '"Inter", system-ui, sans-serif',
  fontSize: '14px',
  noteBkgColor: '#2d2d4e',
  noteTextColor: '#ccd6f6',
  noteBorderColor: '#3d3d5c',
  actorBkg: '#1168bd',
  actorTextColor: '#fff',
  actorBorder: '#0b4884',
  signalColor: '#8892b0',
  signalTextColor: '#ccd6f6',
  labelBoxBkgColor: '#1e1e3a',
  labelBoxBorderColor: '#3d3d5c',
  labelTextColor: '#ccd6f6',
  loopTextColor: '#8892b0',
  activationBorderColor: '#64ffda',
  activationBkgColor: '#1e1e3a',
  sequenceNumberColor: '#64ffda',
  // ER diagram row fills (mermaid uses rowOdd/rowEven from themeVariables)
  rowOdd: '#1a1a34',
  rowEven: '#2a2a50',
};

const LIGHT_THEME = {
  darkMode: false,
  background: '#ffffff',
  primaryColor: '#1168bd',
  primaryTextColor: '#1a1a2e',
  primaryBorderColor: '#0b4884',
  lineColor: '#4a5568',
  secondaryColor: '#438dd5',
  tertiaryColor: '#e2e4ea',
  // ER diagram row fills
  rowOdd: '#ffffff',
  rowEven: '#e8eaf0',
  fontFamily: '"Inter", system-ui, sans-serif',
  fontSize: '14px',
  noteBkgColor: '#f3f4f6',
  noteTextColor: '#1a1a2e',
  noteBorderColor: '#d4d6de',
  actorBkg: '#1168bd',
  actorTextColor: '#fff',
  actorBorder: '#0b4884',
  signalColor: '#4a5568',
  signalTextColor: '#1a1a2e',
  labelBoxBkgColor: '#f0f1f5',
  labelBoxBorderColor: '#d4d6de',
  labelTextColor: '#1a1a2e',
  loopTextColor: '#6b7280',
  activationBorderColor: '#0d9488',
  activationBkgColor: '#f0f1f5',
  sequenceNumberColor: '#0d9488',
};

/** Initialize or re-initialize mermaid with the given theme mode. */
export function initMermaidTheme(mode) {
  const isDark = mode === 'dark';
  mermaid.initialize({
    startOnLoad: false,
    theme: isDark ? 'dark' : 'default',
    themeVariables: isDark ? DARK_THEME : LIGHT_THEME,
    flowchart: { htmlLabels: true, curve: 'basis', padding: 16 },
    sequence: { mirrorActors: false, messageMargin: 40, useMaxWidth: false },
    state: { useMaxWidth: false },
  });
}

// Default to dark
initMermaidTheme('dark');

/** Render a mermaid definition, serialized with all other renders. */
export function renderMermaid(definition) {
  const id = `mmd-${++globalCounter}`;
  const finalDef = transpileC4(definition);

  // Chain onto the queue so renders run one at a time
  const result = queue.then(() => mermaid.render(id, finalDef));

  // Update the queue — swallow errors so one failure doesn't block the chain
  queue = result.then(() => {}, () => {});

  return result;
}
