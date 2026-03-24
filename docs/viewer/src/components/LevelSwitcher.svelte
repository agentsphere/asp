<script>
  import { currentLevel, goC1, goC2 } from '../stores/navigation.js';

  const levels = [
    { id: 'c1', label: 'C1 Context' },
    { id: 'c2', label: 'C2 Containers' },
    { id: 'c3', label: 'C3 Components' },
    { id: 'deployment', label: 'Deployment' },
  ];

  function handleClick(id) {
    if (id === 'c1') goC1();
    else if (id === 'c2') goC2();
  }

  const hints = {
    c1: 'Click a node to zoom in',
    c2: 'Click a module name to see its components',
    c3: 'ESC to zoom out to containers',
    deployment: 'Deployment topology view',
    flow: 'ESC to go back to architecture',
  };
</script>

<div class="level-switcher">
  {#each levels as lvl}
    <button
      class="level-btn"
      class:active={$currentLevel === lvl.id}
      onclick={() => handleClick(lvl.id)}
    >
      {lvl.label}
    </button>
  {/each}

  <div class="hint">{hints[$currentLevel] || ''}</div>
</div>

<style>
  .level-switcher {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 1rem;
    border-top: 1px solid var(--border);
    background: var(--bg-header);
  }

  .level-btn {
    background: var(--bg-btn);
    border: 1px solid var(--border-light);
    color: var(--text-secondary);
    padding: 0.3rem 0.8rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.78rem;
    transition: all 0.15s ease;
  }

  .level-btn:hover {
    background: var(--bg-btn-hover);
    color: var(--text-primary);
  }

  .level-btn.active {
    background: var(--primary);
    border-color: var(--primary);
    color: #fff;
    font-weight: 600;
  }

  .hint { margin-left: auto; font-size: 0.7rem; color: var(--text-muted); }
</style>
