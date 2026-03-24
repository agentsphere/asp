<script>
  import {
    currentLevel, c3ParentLabel, activeFlowId, deploymentId, activeDocId, activeConceptId,
    goC1, goC2, goC3, goDeployment, showFlow, showDoc, showConcept,
  } from '../stores/navigation.js';
  import { searchQuery, searchResults } from '../stores/search.js';
  import {
    c3Diagrams, deploymentDiagrams, flowDiagrams, conceptDiagrams,
    getFlowsByCategory, getConceptsByCategory, hasC3,
  } from '../data/diagrams.js';
  import { docs } from '../data/docs.js';
  import { theme, toggleTheme } from '../stores/theme.js';

  let flowCategories = getFlowsByCategory();
  let conceptCategories = getConceptsByCategory();
  let expandedCategories = $state(new Set([...Object.keys(flowCategories), ...Object.keys(conceptCategories)]));

  function toggleCategory(cat) {
    if (expandedCategories.has(cat)) {
      expandedCategories.delete(cat);
    } else {
      expandedCategories.add(cat);
    }
    expandedCategories = new Set(expandedCategories);
  }

  function handleSearch(e) {
    $searchQuery = e.target.value;
  }
</script>

<aside class="sidebar">
  <!-- Search + Theme toggle -->
  <div class="search-box">
    <div class="search-row">
      <input
        type="text"
        placeholder="Search docs & diagrams..."
        value={$searchQuery}
        oninput={handleSearch}
      />
      <button class="theme-toggle" onclick={toggleTheme} title="Toggle light/dark mode">
        {$theme === 'dark' ? '\u2600' : '\u263E'}
      </button>
    </div>
  </div>

  {#if $searchQuery.length >= 2}
    <!-- Search Results -->
    <div class="section">
      <div class="section-title">Results</div>
      {#each $searchResults.docs as d}
        <button
          class="nav-item"
          onclick={() => showDoc(d.id)}
        >
          <span class="doc-tag">Doc</span>
          {d.title}
        </button>
      {/each}
      {#each $searchResults.diagrams as d}
        <button
          class="nav-item"
          onclick={() => d.type === 'c3' ? goC3(d.parent) : goDeployment(d.id)}
        >
          <span class="level-tag">{d.type === 'c3' ? 'C3' : 'Deploy'}</span>
          {d.label}
        </button>
      {/each}
      {#each $searchResults.flows as flow}
        <button class="nav-item flow-item" onclick={() => showFlow(flow.id)}>
          <span class="flow-tag">{flow.category}</span>
          {flow.name}
        </button>
      {/each}
      {#each $searchResults.concepts as concept}
        <button class="nav-item" onclick={() => showConcept(concept.id)}>
          <span class="flow-tag">{concept.category}</span>
          {concept.name}
        </button>
      {/each}
      {#if $searchResults.docs.length === 0 && $searchResults.diagrams.length === 0 && $searchResults.flows.length === 0 && $searchResults.concepts.length === 0}
        <div class="empty">No matches</div>
      {/if}
    </div>
  {:else}
    <!-- Documentation (first) -->
    <div class="section">
      <div class="section-title">Documentation</div>
      {#each docs as d}
        <button
          class="nav-item"
          class:active={$currentLevel === 'doc' && $activeDocId === d.id}
          onclick={() => showDoc(d.id)}
        >
          <span class="doc-number">{d.section}.</span>
          <span class="doc-label">{d.title}</span>
        </button>
      {/each}
    </div>

    <!-- Divider -->
    <div class="divider"></div>

    <!-- Architecture -->
    <div class="section">
      <div class="section-title">Architecture</div>

      <button
        class="nav-item"
        class:active={$currentLevel === 'c1'}
        onclick={goC1}
      >
        C1 System Context
      </button>

      <button
        class="nav-item"
        class:active={$currentLevel === 'c2'}
        onclick={goC2}
      >
        C2 Containers
      </button>

      <!-- C3 drill-downs -->
      {#each c3Diagrams as c3}
        <button
          class="nav-item sub"
          class:active={$currentLevel === 'c3' && $c3ParentLabel === c3.parent}
          onclick={() => goC3(c3.parent)}
        >
          <span class="mod-label">C3 {c3.parent}</span>
          <span class="chevron">&#9656;</span>
        </button>
      {/each}
    </div>

    <!-- Deployment -->
    <div class="section">
      <div class="section-title">Deployment</div>
      {#each deploymentDiagrams as dep}
        <button
          class="nav-item"
          class:active={$currentLevel === 'deployment' && $deploymentId === dep.id}
          onclick={() => goDeployment(dep.id)}
        >
          {dep.label}
        </button>
      {/each}
    </div>

    <!-- Divider -->
    <div class="divider"></div>

    <!-- Flows -->
    <div class="section">
      <div class="section-title">Flows</div>

      {#each Object.entries(flowCategories) as [category, categoryFlows]}
        <button class="nav-item cat-header" onclick={() => toggleCategory(category)}>
          <span class="cat-label">{category}</span>
          <span class="badge">{categoryFlows.length}</span>
        </button>
        {#if expandedCategories.has(category)}
          <div class="sub-items">
            {#each categoryFlows as flow}
              <button
                class="nav-item sub flow-item"
                class:active={$currentLevel === 'flow' && $activeFlowId === flow.id}
                onclick={() => showFlow(flow.id)}
              >
                {flow.name}
              </button>
            {/each}
          </div>
        {/if}
      {/each}
    </div>

    <!-- Divider -->
    <div class="divider"></div>

    <!-- Concepts -->
    <div class="section">
      <div class="section-title">Concepts</div>

      {#each Object.entries(conceptCategories) as [category, categoryConcepts]}
        <button class="nav-item cat-header" onclick={() => toggleCategory(category)}>
          <span class="cat-label">{category}</span>
          <span class="badge">{categoryConcepts.length}</span>
        </button>
        {#if expandedCategories.has(category)}
          <div class="sub-items">
            {#each categoryConcepts as concept}
              <button
                class="nav-item sub"
                class:active={$currentLevel === 'concept' && $activeConceptId === concept.id}
                onclick={() => showConcept(concept.id)}
              >
                {concept.name}
              </button>
            {/each}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</aside>

<style>
  .sidebar {
    width: 260px;
    min-width: 260px;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    font-size: 0.82rem;
  }

  .search-box {
    padding: 0.6rem;
    border-bottom: 1px solid var(--border);
  }

  .search-row {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }

  .search-box input {
    flex: 1;
    min-width: 0;
    background: var(--bg-input);
    border: 1px solid var(--border);
    color: var(--text-primary);
    padding: 0.4rem 0.6rem;
    border-radius: 4px;
    font-size: 0.8rem;
    outline: none;
    box-sizing: border-box;
  }

  .search-box input::placeholder { color: var(--text-muted); }
  .search-box input:focus { border-color: var(--primary); }

  .theme-toggle {
    background: var(--bg-btn);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    width: 2rem;
    height: 2rem;
    border-radius: 4px;
    cursor: pointer;
    font-size: 1rem;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    transition: all 0.15s;
  }

  .theme-toggle:hover {
    background: var(--bg-btn-hover);
    color: var(--text-primary);
  }

  .section { padding: 0.4rem 0; }

  .section-title {
    padding: 0.3rem 0.8rem;
    font-size: 0.7rem;
    color: var(--text-heading);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 600;
  }

  .divider {
    border-top: 1px solid var(--border);
    margin: 0.3rem 0.6rem;
  }

  .nav-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 0.35rem 0.8rem;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
    font-size: inherit;
    gap: 0.3rem;
    transition: all 0.1s;
  }

  .nav-item:hover {
    background: var(--bg-active);
    color: var(--text-primary);
  }

  .nav-item.active {
    background: var(--bg-active);
    color: var(--accent);
    border-left: 2px solid var(--accent);
  }

  .nav-item.sub { padding-left: 1.4rem; font-size: 0.78rem; }
  .sub-items { display: flex; flex-direction: column; }
  .mod-label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chevron { color: var(--text-muted); font-size: 0.7rem; }

  .badge {
    background: var(--bg-btn);
    color: var(--accent);
    padding: 0 0.35rem;
    border-radius: 8px;
    font-size: 0.65rem;
    font-weight: 600;
    min-width: 1.1rem;
    text-align: center;
  }

  .cat-header { font-weight: 500; color: var(--text-heading); text-transform: uppercase; font-size: 0.72rem; letter-spacing: 0.04em; }
  .cat-header:hover { color: var(--text-secondary); }
  .cat-label { flex: 1; text-align: left; }

  .level-tag { background: var(--primary); color: #fff; padding: 0.05rem 0.35rem; border-radius: 3px; font-size: 0.65rem; font-weight: 600; margin-right: 0.3rem; }
  .flow-tag { background: var(--bg-btn); color: var(--accent); padding: 0.05rem 0.35rem; border-radius: 3px; font-size: 0.65rem; margin-right: 0.3rem; }
  .doc-tag { background: var(--doc-tag-bg); color: var(--doc-tag-color); padding: 0.05rem 0.35rem; border-radius: 3px; font-size: 0.65rem; font-weight: 600; margin-right: 0.3rem; flex-shrink: 0; }
  .doc-number { color: var(--text-muted); font-size: 0.75rem; min-width: 1.4rem; flex-shrink: 0; }
  .doc-label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { padding: 1rem; color: var(--text-muted); text-align: center; font-size: 0.8rem; }
</style>
