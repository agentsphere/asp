<script>
  import { onMount, tick, untrack } from 'svelte';
  import { renderMermaid, initMermaidTheme } from '../lib/mermaid-queue.js';
  import { setupZoom } from '../lib/zoom.js';
  import { theme } from '../stores/theme.js';

  let {
    definition = '',
    inline = false,
    onNodeClick = null,
    onNodeDoubleClick = null,
    onReady = null,
  } = $props();

  let container;
  let zoomControls = null;

  async function render(def) {
    if (!container || !def) return;

    try {
      const { svg } = await renderMermaid(def);
      if (!container) return;
      container.innerHTML = svg;

      const svgEl = container.querySelector('svg');

      if (inline) {
        if (svgEl) {
          svgEl.style.maxWidth = '100%';
          svgEl.style.height = 'auto';
        }
      } else {
        if (onNodeClick && svgEl) {
          const nodeGroups = svgEl.querySelectorAll('.node');
          for (const g of nodeGroups) {
            g.style.cursor = 'pointer';
            g.addEventListener('click', (e) => {
              e.stopPropagation();
              onNodeClick(g.id, g);
            });
            if (onNodeDoubleClick) {
              g.addEventListener('dblclick', (e) => {
                e.stopPropagation();
                onNodeDoubleClick(g.id, g);
              });
            }
          }
        }

        if (zoomControls) {
          zoomControls.reset(0);
        } else {
          zoomControls = setupZoom(container);
        }

        await tick();
        requestAnimationFrame(() => zoomControls?.fitToView(500));
      }

      onReady?.({ zoomControls });
    } catch (err) {
      console.error('Mermaid render error:', err);
      if (container) {
        container.innerHTML = `<div class="render-error">Diagram render error: ${err.message}</div>`;
      }
    }
  }

  // Re-render when definition changes
  $effect(() => {
    const def = definition;
    if (def && container) {
      untrack(() => render(def));
    }
  });

  // Re-render when theme changes
  $effect(() => {
    const t = $theme;
    initMermaidTheme(t);
    const def = definition;
    if (def && container) {
      untrack(() => render(def));
    }
  });

  function bindContainer(node) {
    container = node;
    if (definition) {
      render(definition);
    }
    return {
      destroy() { container = null; }
    };
  }
</script>

<div class="mermaid-container" use:bindContainer></div>

<style>
  .mermaid-container {
    width: 100%;
    height: 100%;
    overflow: hidden;
    position: relative;
  }

  .mermaid-container :global(svg) {
    display: block;
  }

  .mermaid-container :global(.render-error) {
    color: var(--error);
    padding: 2rem;
    text-align: center;
    font-family: monospace;
  }

  .mermaid-container :global(.node) {
    transition: opacity 0.2s ease, filter 0.2s ease;
  }

  .mermaid-container :global(.node:hover) {
    filter: brightness(1.25);
    cursor: pointer;
  }

</style>
