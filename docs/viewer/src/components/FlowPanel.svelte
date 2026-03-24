<script>
  import { flowPanelOpen, flowPanelNodeLabel, closeFlowPanel, showFlow } from '../stores/navigation.js';

  /** Placeholder — flow panel for node-to-flow cross-referencing.
   *  Currently not wired (needs involvedNodes metadata in diagrams.json).
   */
  function handleViewFlow(flowId) {
    closeFlowPanel();
    showFlow(flowId);
  }
</script>

{#if $flowPanelOpen && $flowPanelNodeLabel}
  <aside class="flow-panel">
    <div class="panel-header">
      <div>
        <div class="panel-title">Flows involving</div>
        <div class="panel-node-name">{$flowPanelNodeLabel}</div>
      </div>
      <button class="close-btn" onclick={closeFlowPanel}>&times;</button>
    </div>
    <div class="panel-body">
      <div class="no-flows">
        Flow cross-referencing requires <code>involvedNodes</code> in diagrams.json.
      </div>
    </div>
  </aside>
{/if}

<style>
  .flow-panel {
    width: 320px;
    background: #16162a;
    border-left: 1px solid #2d2d4e;
    display: flex;
    flex-direction: column;
    animation: slideIn 0.25s ease-out;
    overflow-y: auto;
  }

  @keyframes slideIn {
    from { transform: translateX(100%); opacity: 0; }
    to   { transform: translateX(0);    opacity: 1; }
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: 1rem;
    border-bottom: 1px solid #2d2d4e;
  }

  .panel-title {
    font-size: 0.75rem;
    color: #8892b0;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .panel-node-name {
    font-size: 1rem;
    font-weight: 600;
    color: #ccd6f6;
    margin-top: 0.2rem;
  }

  .close-btn {
    background: none;
    border: none;
    color: #8892b0;
    font-size: 1.4rem;
    cursor: pointer;
    padding: 0 0.3rem;
    line-height: 1;
  }

  .close-btn:hover { color: #ff6b6b; }

  .panel-body { padding: 0.75rem; }

  .no-flows {
    color: #6272a4;
    font-size: 0.85rem;
    text-align: center;
    padding: 2rem 0;
    line-height: 1.4;
  }

  .no-flows code {
    background: #2d2d4e;
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
    font-size: 0.8rem;
  }
</style>
