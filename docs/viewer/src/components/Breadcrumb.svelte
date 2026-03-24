<script>
  import { breadcrumb, currentLevel, goC1, goC2 } from '../stores/navigation.js';

  function handleClick(crumb) {
    if (crumb.level === 'c1') goC1();
    else if (crumb.level === 'c2') goC2();
  }

  const levelLabels = {
    c1: 'C1',
    c2: 'C2',
    c3: 'C3',
    deployment: 'Deploy',
    flow: 'Flow',
  };
</script>

<nav class="breadcrumb">
  {#each $breadcrumb as crumb, i}
    {#if i > 0}
      <span class="sep">&rsaquo;</span>
    {/if}
    {#if i === $breadcrumb.length - 1}
      <span class="current">{crumb.label}</span>
    {:else}
      <button class="crumb" onclick={() => handleClick(crumb)}>
        {crumb.label}
      </button>
    {/if}
  {/each}
  <span class="level-badge">{levelLabels[$currentLevel] || $currentLevel}</span>
</nav>

<style>
  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem 1rem;
    font-size: 0.85rem;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border);
    background: var(--bg-header);
  }

  .crumb {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0.15rem 0.3rem;
    border-radius: 3px;
    font-size: inherit;
  }

  .crumb:hover {
    background: var(--bg-btn);
    color: var(--text-primary);
  }

  .current { color: var(--text-primary); font-weight: 500; }
  .sep { color: var(--text-muted); }

  .level-badge {
    margin-left: auto;
    background: var(--primary);
    color: #fff;
    padding: 0.15rem 0.5rem;
    border-radius: 10px;
    font-size: 0.75rem;
    font-weight: 600;
  }
</style>
