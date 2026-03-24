import { writable, derived } from 'svelte/store';
import { c3Diagrams, flowDiagrams, deploymentDiagrams, conceptDiagrams } from '../data/diagrams.js';
import { docs } from '../data/docs.js';

export const searchQuery = writable('');

export const searchResults = derived(searchQuery, ($q) => {
  if (!$q || $q.length < 2) return { docs: [], diagrams: [], flows: [], concepts: [] };
  const lower = $q.toLowerCase();

  const matchedDocs = docs.filter(d =>
    d.title.toLowerCase().includes(lower) ||
    (d.markdown && d.markdown.toLowerCase().includes(lower))
  );

  const matchedDiagrams = [
    ...c3Diagrams.filter(d =>
      d.label.toLowerCase().includes(lower) ||
      d.parent.toLowerCase().includes(lower) ||
      d.description.toLowerCase().includes(lower)
    ).map(d => ({ ...d, type: 'c3' })),
    ...deploymentDiagrams.filter(d =>
      d.label.toLowerCase().includes(lower) ||
      d.description.toLowerCase().includes(lower)
    ).map(d => ({ ...d, type: 'deployment' })),
  ];

  const matchedFlows = flowDiagrams.filter(f =>
    f.name.toLowerCase().includes(lower) ||
    f.description.toLowerCase().includes(lower) ||
    f.category.toLowerCase().includes(lower)
  );

  const matchedConcepts = conceptDiagrams.filter(c =>
    c.name.toLowerCase().includes(lower) ||
    c.description.toLowerCase().includes(lower) ||
    c.category.toLowerCase().includes(lower)
  );

  return { docs: matchedDocs, diagrams: matchedDiagrams, flows: matchedFlows, concepts: matchedConcepts };
});
