<script>
  import MermaidRenderer from './MermaidRenderer.svelte';
  import { activeFlowId, backToArchitecture } from '../stores/navigation.js';
  import { flowDiagrams } from '../data/diagrams.js';

  let flow = $derived($activeFlowId ? flowDiagrams.find(f => f.id === $activeFlowId) : null);
</script>

{#if flow}
  <div class="flow-viewer">
    <div class="flow-header">
      <button class="back-btn" onclick={backToArchitecture}>
        &larr; Architecture
      </button>
      <div class="flow-info">
        <span class="flow-category">{flow.category}</span>
        <h2 class="flow-name">{flow.name}</h2>
      </div>
    </div>

    <div class="flow-desc">{flow.description}</div>

    <div class="flow-canvas">
      {#if flow.mermaid}
        <MermaidRenderer definition={flow.mermaid} />
      {:else}
        <div class="missing">Diagram file not found: {flow.file}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .flow-viewer { display: flex; flex-direction: column; height: 100%; }

  .flow-header {
    display: flex; align-items: center; gap: 1rem;
    padding: 0.5rem 1rem; border-bottom: 1px solid var(--border); background: var(--bg-header);
  }

  .back-btn {
    background: var(--bg-btn); border: 1px solid var(--border-light); color: var(--text-secondary);
    padding: 0.3rem 0.7rem; border-radius: 4px; cursor: pointer; font-size: 0.8rem; white-space: nowrap; transition: all 0.15s;
  }
  .back-btn:hover { background: var(--bg-btn-hover); color: var(--text-primary); }

  .flow-info { display: flex; align-items: baseline; gap: 0.5rem; flex-wrap: wrap; }
  .flow-category { font-size: 0.65rem; color: var(--accent); text-transform: uppercase; letter-spacing: 0.05em; background: var(--accent-bg); padding: 0.1rem 0.4rem; border-radius: 3px; }
  .flow-name { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin: 0; }
  .flow-desc { padding: 0.4rem 1rem; font-size: 0.8rem; color: var(--text-secondary); background: var(--bg-sidebar); border-bottom: 1px solid var(--border); }
  .flow-canvas { flex: 1; min-height: 0; overflow: hidden; background: radial-gradient(circle at 50% 50%, var(--bg-canvas) 0%, var(--bg-canvas-edge) 100%); }
  .missing { color: var(--error); padding: 2rem; text-align: center; }
</style>
