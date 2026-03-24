/**
 * Load all arc42 documentation markdown files using Vite's import.meta.glob.
 * Pairs raw markdown content with the docs.json manifest.
 */

const mdFiles = import.meta.glob(
  '../../../arc42/*.md',
  { eager: true, query: '?raw', import: 'default' },
);

import manifest from '../../../arc42/docs.json';

function resolve(filename) {
  const key = `../../../arc42/${filename}`;
  const content = mdFiles[key];
  if (!content) {
    console.warn(`Doc file not found: ${filename}`);
    return null;
  }
  return content;
}

export const docs = manifest.docs.map(entry => ({
  ...entry,
  markdown: resolve(entry.file),
}));

/** Get a doc by id. */
export function getDoc(id) {
  return docs.find(d => d.id === id) || null;
}
