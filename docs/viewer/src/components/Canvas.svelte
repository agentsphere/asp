<script>
  import Breadcrumb from './Breadcrumb.svelte';
  import LevelSwitcher from './LevelSwitcher.svelte';
  import MermaidRenderer from './MermaidRenderer.svelte';
  import NodeTooltip from './NodeTooltip.svelte';
  import FlowPanel from './FlowPanel.svelte';
  import {
    currentLevel, c3ParentLabel, deploymentId,
    goC3, flowPanelOpen,
  } from '../stores/navigation.js';
  import {
    c1Diagram, c2Diagram, getC3ForParent,
    deploymentDiagrams, hasC3,
  } from '../data/diagrams.js';

  /** Resolve the current diagram Mermaid content based on navigation state. */
  let diagram = $derived.by(() => {
    switch ($currentLevel) {
      case 'c1': return c1Diagram?.mermaid;
      case 'c2': return c2Diagram?.mermaid;
      case 'c3': {
        const c3 = getC3ForParent($c3ParentLabel);
        return c3?.mermaid;
      }
      case 'deployment': {
        const dep = deploymentDiagrams.find(d => d.id === $deploymentId);
        return dep?.mermaid || deploymentDiagrams[0]?.mermaid;
      }
      default: return null;
    }
  });

  /** Current diagram label for display. */
  let diagramLabel = $derived.by(() => {
    switch ($currentLevel) {
      case 'c1': return c1Diagram?.label;
      case 'c2': return c2Diagram?.label;
      case 'c3': return getC3ForParent($c3ParentLabel)?.label;
      case 'deployment': {
        const dep = deploymentDiagrams.find(d => d.id === $deploymentId);
        return dep?.label || 'Deployment';
      }
      default: return '';
    }
  });

  // Tooltip state
  let tooltipNodeId = $state(null);
  let tooltipX = $state(0);
  let tooltipY = $state(0);

  /**
   * Extract a label from a Mermaid SVG node for C2 drill-down.
   * In C4Container diagrams, the node label is the container name.
   * In flowchart-generated C2, the node has an id we can parse.
   */
  function handleNodeClick(nodeId, svgElement) {
    // Only drill down from C2 level
    if ($currentLevel !== 'c2') return;

    // Try to extract the node label from the SVG element
    const label = extractNodeLabel(svgElement);
    if (label && hasC3(label)) {
      goC3(label);
    }
  }

  function handleNodeDoubleClick(nodeId, svgElement) {
    // Future: open flow panel for this node
  }

  function handleNodeHover(nodeId, entering) {
    tooltipNodeId = entering ? nodeId : null;
  }

  function handleMouseMove(e) {
    if (tooltipNodeId) {
      tooltipX = e.clientX;
      tooltipY = e.clientY;
    }
  }

  /** Extract display label from a Mermaid SVG <g> element. */
  function extractNodeLabel(gElement) {
    if (!gElement) return null;
    // Try foreignObject (HTML labels)
    const fo = gElement.querySelector('foreignObject');
    if (fo) {
      const span = fo.querySelector('.nodeLabel, span');
      if (span) {
        const clone = span.cloneNode(true);
        for (const el of clone.querySelectorAll('i, br')) el.remove();
        return clone.textContent.trim();
      }
    }
    // Try text elements
    const texts = gElement.querySelectorAll('text');
    if (texts.length > 0) return texts[0].textContent.trim();
    return null;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="canvas-layout" onmousemove={handleMouseMove}>
  <div class="canvas-main">
    <Breadcrumb />

    <div class="diagram-area">
      {#if diagram}
        {#key `${$currentLevel}-${$c3ParentLabel}-${$deploymentId}`}
          <div class="diagram-wrapper">
            <MermaidRenderer
              definition={diagram}
              onNodeClick={handleNodeClick}
              onNodeDoubleClick={handleNodeDoubleClick}
              onNodeHover={handleNodeHover}
            />
          </div>
        {/key}
      {:else}
        <div class="no-diagram">
          <p>No diagram available for this view.</p>
          <p class="hint">Select a diagram from the sidebar.</p>
        </div>
      {/if}
    </div>

    <LevelSwitcher />
  </div>

  <FlowPanel />
</div>

<style>
  .canvas-layout {
    display: flex;
    height: 100%;
    overflow: hidden;
  }

  .canvas-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .diagram-area {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    position: relative;
    background: radial-gradient(circle at 50% 50%, var(--bg-canvas) 0%, var(--bg-canvas-edge) 100%);
  }

  .diagram-wrapper {
    width: 100%;
    height: 100%;
    animation: zoomFadeIn 0.4s ease-out;
  }

  @keyframes zoomFadeIn {
    from { opacity: 0; transform: scale(0.94); }
    to   { opacity: 1; transform: scale(1); }
  }

  .no-diagram {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-heading);
    text-align: center;
    gap: 0.3rem;
  }

  .no-diagram p { margin: 0; }
  .no-diagram .hint { font-size: 0.8rem; color: var(--text-muted); }
</style>
