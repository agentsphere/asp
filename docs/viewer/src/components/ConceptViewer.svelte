<script>
  import MermaidRenderer from './MermaidRenderer.svelte';
  import { activeConceptId, backToArchitecture } from '../stores/navigation.js';
  import { conceptDiagrams } from '../data/diagrams.js';

  let concept = $derived($activeConceptId ? conceptDiagrams.find(c => c.id === $activeConceptId) : null);
</script>

{#if concept}
  <div class="concept-viewer">
    <div class="concept-header">
      <button class="back-btn" onclick={backToArchitecture}>
        &larr; Architecture
      </button>
      <div class="concept-info">
        <span class="concept-category">{concept.category}</span>
        <h2 class="concept-name">{concept.name}</h2>
      </div>
    </div>

    <div class="concept-desc">{concept.description}</div>

    <div class="concept-canvas">
      {#if concept.mermaid}
        <MermaidRenderer definition={concept.mermaid} />
      {:else}
        <div class="missing">Diagram file not found: {concept.file}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .concept-viewer { display: flex; flex-direction: column; height: 100%; }

  .concept-header {
    display: flex; align-items: center; gap: 1rem;
    padding: 0.5rem 1rem; border-bottom: 1px solid var(--border); background: var(--bg-header);
  }

  .back-btn {
    background: var(--bg-btn); border: 1px solid var(--border-light); color: var(--text-secondary);
    padding: 0.3rem 0.7rem; border-radius: 4px; cursor: pointer; font-size: 0.8rem; white-space: nowrap; transition: all 0.15s;
  }
  .back-btn:hover { background: var(--bg-btn-hover); color: var(--text-primary); }

  .concept-info { display: flex; align-items: baseline; gap: 0.5rem; flex-wrap: wrap; }
  .concept-category { font-size: 0.65rem; color: var(--accent); text-transform: uppercase; letter-spacing: 0.05em; background: var(--accent-bg); padding: 0.1rem 0.4rem; border-radius: 3px; }
  .concept-name { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin: 0; }
  .concept-desc { padding: 0.4rem 1rem; font-size: 0.8rem; color: var(--text-secondary); background: var(--bg-sidebar); border-bottom: 1px solid var(--border); }
  .concept-canvas { flex: 1; min-height: 0; overflow: hidden; background: radial-gradient(circle at 50% 50%, var(--bg-canvas) 0%, var(--bg-canvas-edge) 100%); }
  .missing { color: var(--error); padding: 2rem; text-align: center; }
</style>
