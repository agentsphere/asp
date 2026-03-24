<script>
  let { nodeId = null, label = '', x = 0, y = 0 } = $props();
</script>

{#if label}
  <div class="tooltip" style="left: {x + 12}px; top: {y + 12}px;">
    <div class="tooltip-label">{label}</div>
    <div class="tooltip-hint">Click to drill down</div>
  </div>
{/if}

<style>
  .tooltip {
    position: fixed;
    z-index: 100;
    background: #1e1e3a;
    border: 1px solid #3d3d5c;
    border-radius: 6px;
    padding: 0.5rem 0.7rem;
    max-width: 250px;
    pointer-events: none;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
  }

  .tooltip-label {
    font-weight: 600;
    color: #ccd6f6;
    font-size: 0.85rem;
  }

  .tooltip-hint {
    font-size: 0.7rem;
    color: #4a4a6a;
    margin-top: 0.2rem;
  }
</style>
