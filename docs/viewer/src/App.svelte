<script>
  import Sidebar from './components/Sidebar.svelte';
  import Canvas from './components/Canvas.svelte';
  import FlowViewer from './components/FlowViewer.svelte';
  import ConceptViewer from './components/ConceptViewer.svelte';
  import DocViewer from './components/DocViewer.svelte';
  import {
    currentLevel, zoomOut, backToArchitecture,
    flowPanelOpen, closeFlowPanel,
  } from './stores/navigation.js';
  import './stores/theme.js'; // init theme on load

  function handleKeydown(e) {
    if (e.key === 'Escape') {
      if ($flowPanelOpen) {
        closeFlowPanel();
      } else if ($currentLevel === 'flow' || $currentLevel === 'concept') {
        backToArchitecture();
      } else if ($currentLevel !== 'doc') {
        zoomOut();
      }
      e.preventDefault();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="app">
  <Sidebar />
  <main class="main-content">
    {#if $currentLevel === 'doc'}
      <DocViewer />
    {:else if $currentLevel === 'flow'}
      <FlowViewer />
    {:else if $currentLevel === 'concept'}
      <ConceptViewer />
    {:else}
      <Canvas />
    {/if}
  </main>
</div>

<style>
  .app {
    display: flex;
    height: 100vh;
    overflow: hidden;
    background: var(--bg-app);
    color: var(--text-primary);
  }

  .main-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>
