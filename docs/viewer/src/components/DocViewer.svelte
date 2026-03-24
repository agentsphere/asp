<script>
  import { marked } from 'marked';
  import MermaidRenderer from './MermaidRenderer.svelte';
  import { activeDocId, backToArchitecture, goC1, goC2, goC3, goDeployment, showFlow, showConcept } from '../stores/navigation.js';
  import { getDoc } from '../data/docs.js';
  import { getNavForFile } from '../data/diagrams.js';

  let doc = $derived($activeDocId ? getDoc($activeDocId) : null);

  /**
   * Split markdown into segments: regular markdown and mermaid code blocks.
   * Extracts source .mmd filename from preceding <!-- mermaid:diagrams/xxx.mmd --> comments.
   * Returns [{type: 'md', content}, {type: 'mermaid', content, source?}, ...]
   */
  function splitContent(md) {
    const segments = [];
    // Match optional preceding <!-- mermaid:diagrams/xxx.mmd --> comment + the code block
    const regex = /(?:^<!-- mermaid:diagrams\/([^\s]+) -->\n)?^```mermaid\s*\n([\s\S]*?)^```/gm;
    let lastIndex = 0;
    let match;

    while ((match = regex.exec(md)) !== null) {
      if (match.index > lastIndex) {
        segments.push({ type: 'md', content: md.slice(lastIndex, match.index) });
      }
      segments.push({
        type: 'mermaid',
        content: match[2].trim(),
        source: match[1] || null,  // e.g. "runtime-auth.mmd"
      });
      lastIndex = match.index + match[0].length;
    }

    if (lastIndex < md.length) {
      segments.push({ type: 'md', content: md.slice(lastIndex) });
    }

    return segments;
  }

  /** Render markdown to HTML, stripping the leading H1 (shown in header). */
  function renderMarkdown(md, isFirst) {
    let text = md;
    if (isFirst) {
      text = text.replace(/^#\s+.+\n*/, '');
    }
    return marked.parse(text, { gfm: true, breaks: false });
  }

  /** Navigate to the diagram viewer for a given .mmd source file. */
  function navigateToDiagram(source) {
    const nav = getNavForFile(source);
    if (!nav) return;
    switch (nav.level) {
      case 'c1': goC1(); break;
      case 'c2': goC2(); break;
      case 'c3': goC3(nav.parent); break;
      case 'deployment': goDeployment(nav.id); break;
      case 'flow': showFlow(nav.id); break;
      case 'concept': showConcept(nav.id); break;
    }
  }

  let segments = $derived(doc?.markdown ? splitContent(doc.markdown) : []);
</script>

{#if doc}
  <div class="doc-viewer">
    <div class="doc-header">
      <button class="back-btn" onclick={backToArchitecture}>
        &larr; Diagrams
      </button>
      <div class="doc-info">
        <span class="doc-section">Section {doc.section}</span>
        <h2 class="doc-title">{doc.title}</h2>
      </div>
    </div>

    <div class="doc-content">
      {#each segments as seg, i}
        {#if seg.type === 'mermaid'}
          {@const nav = seg.source ? getNavForFile(seg.source) : null}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="mermaid-block"
            class:clickable={!!nav}
            onclick={() => nav && navigateToDiagram(seg.source)}
          >
            <MermaidRenderer definition={seg.content} inline={true} />
            {#if nav}
              <div class="diagram-link-hint">
                <span class="hint-icon">&#8599;</span>
                View in diagram viewer
              </div>
            {/if}
          </div>
        {:else}
          {@html renderMarkdown(seg.content, i === 0)}
        {/if}
      {/each}
    </div>
  </div>
{/if}

<style>
  .doc-viewer { display: flex; flex-direction: column; height: 100%; }

  .doc-header {
    display: flex; align-items: center; gap: 1rem;
    padding: 0.5rem 1rem; border-bottom: 1px solid var(--border); background: var(--bg-header); flex-shrink: 0;
  }

  .back-btn {
    background: var(--bg-btn); border: 1px solid var(--border-light); color: var(--text-secondary);
    padding: 0.3rem 0.7rem; border-radius: 4px; cursor: pointer; font-size: 0.8rem; white-space: nowrap; transition: all 0.15s;
  }
  .back-btn:hover { background: var(--bg-btn-hover); color: var(--text-primary); }

  .doc-info { display: flex; align-items: baseline; gap: 0.5rem; flex-wrap: wrap; }
  .doc-section { font-size: 0.65rem; color: var(--accent); text-transform: uppercase; letter-spacing: 0.05em; background: var(--accent-bg); padding: 0.1rem 0.4rem; border-radius: 3px; }
  .doc-title { font-size: 1rem; font-weight: 600; color: var(--text-primary); margin: 0; }

  .doc-content { flex: 1; overflow-y: auto; padding: 1.5rem 2.5rem 3rem; max-width: 900px; width: 100%; margin: 0 auto; }

  /* Markdown styling */
  .doc-content :global(h1) { font-size: 1.6rem; font-weight: 700; color: var(--text-primary); margin: 2rem 0 0.8rem; padding-bottom: 0.3rem; border-bottom: 1px solid var(--border); }
  .doc-content :global(h2) { font-size: 1.3rem; font-weight: 600; color: var(--text-primary); margin: 1.8rem 0 0.6rem; padding-bottom: 0.2rem; border-bottom: 1px solid var(--border-heading); }
  .doc-content :global(h3) { font-size: 1.1rem; font-weight: 600; color: var(--text-subtle); margin: 1.4rem 0 0.5rem; }
  .doc-content :global(h4) { font-size: 0.95rem; font-weight: 600; color: var(--text-secondary); margin: 1.2rem 0 0.4rem; }
  .doc-content :global(p) { color: var(--text-body); line-height: 1.7; margin: 0.6rem 0; }
  .doc-content :global(ul), .doc-content :global(ol) { color: var(--text-body); padding-left: 1.5rem; margin: 0.5rem 0; }
  .doc-content :global(li) { margin: 0.25rem 0; line-height: 1.6; }

  .doc-content :global(code) { background: var(--code-bg); color: var(--code-color); padding: 0.15rem 0.35rem; border-radius: 3px; font-size: 0.85em; font-family: 'JetBrains Mono', 'Fira Code', monospace; }
  .doc-content :global(pre) { background: var(--code-bg); border: 1px solid var(--border); border-radius: 6px; padding: 1rem; overflow-x: auto; margin: 0.8rem 0; }
  .doc-content :global(pre code) { background: none; padding: 0; color: var(--text-primary); font-size: 0.82rem; line-height: 1.5; }

  .doc-content :global(table) { width: 100%; border-collapse: collapse; margin: 0.8rem 0; font-size: 0.85rem; }
  .doc-content :global(th) { background: var(--code-bg); color: var(--accent); padding: 0.5rem 0.75rem; text-align: left; font-weight: 600; border: 1px solid var(--border); }
  .doc-content :global(td) { padding: 0.4rem 0.75rem; border: 1px solid var(--border); color: var(--text-body); }
  .doc-content :global(tr:nth-child(even)) { background: var(--bg-even-row); }

  .doc-content :global(blockquote) { border-left: 3px solid var(--primary); padding: 0.5rem 1rem; margin: 0.8rem 0; background: var(--blockquote-bg); color: var(--text-secondary); }
  .doc-content :global(a) { color: var(--primary); text-decoration: none; }
  .doc-content :global(a:hover) { color: var(--accent); text-decoration: underline; }
  .doc-content :global(hr) { border: none; border-top: 1px solid var(--border); margin: 1.5rem 0; }
  .doc-content :global(strong) { color: var(--text-primary); font-weight: 600; }
  .doc-content :global(em) { color: var(--text-subtle); }

  /* Mermaid diagram blocks */
  .mermaid-block {
    background: var(--bg-card); border: 1px solid var(--border); border-radius: 8px;
    padding: 1rem; margin: 1rem 0; overflow-x: auto; text-align: center;
    position: relative; transition: border-color 0.2s;
  }
  .mermaid-block.clickable { cursor: pointer; }
  .mermaid-block.clickable:hover { border-color: var(--primary); }
  .mermaid-block :global(.mermaid-container) { height: auto; overflow: visible; }

  .diagram-link-hint {
    position: absolute; top: 0.4rem; right: 0.5rem; font-size: 0.7rem; color: var(--text-muted);
    display: flex; align-items: center; gap: 0.25rem; opacity: 0; transition: opacity 0.2s, color 0.2s; pointer-events: none;
  }
  .mermaid-block.clickable:hover .diagram-link-hint { opacity: 1; color: var(--primary); }
  .hint-icon { font-size: 0.85rem; }
</style>
