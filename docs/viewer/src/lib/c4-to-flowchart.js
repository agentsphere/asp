/**
 * Transpile Mermaid C4 diagram syntax into flowchart syntax.
 *
 * Why: Mermaid's C4 diagrams use a simple grid layout with no edge routing,
 * causing arrows to cross over entities. Flowcharts use dagre layout which
 * routes edges around nodes properly.
 *
 * Supports: C4Context, C4Container, C4Component, C4Deployment
 * Preserves: node types (Person, System, System_Ext, SystemDb, Container, etc.),
 *            relationships, labels, descriptions, boundaries
 */

/** Detect if a Mermaid definition is a C4 diagram. */
export function isC4(definition) {
  const first = definition.trim().split('\n')[0].trim();
  return /^C4(Context|Container|Component|Deployment)\b/.test(first);
}

/** Transpile C4 → flowchart. Returns original if not C4. */
export function transpileC4(definition) {
  if (!isC4(definition)) return definition;

  const lines = definition.split('\n');
  const firstLine = lines[0].trim();

  const nodes = [];
  const rels = [];
  const boundaries = [];  // stack for nested Deployment_Node
  const boundaryLabels = new Map();  // id → label for subgraph rendering
  let title = '';

  for (const raw of lines) {
    const line = raw.trim();
    if (!line || line.startsWith('%%')) continue;

    // Title
    const titleMatch = line.match(/^\s*title\s+(.+)$/);
    if (titleMatch) {
      title = titleMatch[1];
      continue;
    }

    // Skip the diagram type declaration
    if (/^C4(Context|Container|Component|Deployment)\b/.test(line)) continue;

    // UpdateLayoutConfig — skip (not needed for flowchart)
    if (line.startsWith('UpdateLayoutConfig') || line.startsWith('UpdateRelStyle')) continue;

    // Boundary open: Deployment_Node(id, "label", "tech") {
    // or: Enterprise_Boundary(id, "label") {
    // or: System_Boundary(id, "label") {
    // or: Container_Boundary(id, "label") {
    const boundaryMatch = line.match(
      /^(Deployment_Node|Enterprise_Boundary|System_Boundary|Container_Boundary|Boundary)\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?\)\s*\{?\s*$/
    );
    if (boundaryMatch) {
      const bId = boundaryMatch[2];
      const bLabel = boundaryMatch[3];
      boundaries.push({ id: bId, label: bLabel, tech: boundaryMatch[4] || '' });
      boundaryLabels.set(bId, bLabel);
      continue;
    }

    // Boundary close
    if (line === '}') {
      boundaries.pop();
      continue;
    }

    // Person(id, "label", "desc")
    const personMatch = line.match(/^Person\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?\)/);
    if (personMatch) {
      nodes.push({ id: personMatch[1], label: personMatch[2], desc: personMatch[3] || '', type: 'person', boundary: currentBoundary() });
      continue;
    }

    // System_Ext(id, "label", "desc")
    const sysExtMatch = line.match(/^System_Ext\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?\)/);
    if (sysExtMatch) {
      nodes.push({ id: sysExtMatch[1], label: sysExtMatch[2], desc: sysExtMatch[3] || '', type: 'external', boundary: currentBoundary() });
      continue;
    }

    // SystemDb(id, "label", "desc")
    const sysDbMatch = line.match(/^SystemDb\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?\)/);
    if (sysDbMatch) {
      nodes.push({ id: sysDbMatch[1], label: sysDbMatch[2], desc: sysDbMatch[3] || '', type: 'database', boundary: currentBoundary() });
      continue;
    }

    // System(id, "label", "desc")
    const sysMatch = line.match(/^System\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?\)/);
    if (sysMatch) {
      nodes.push({ id: sysMatch[1], label: sysMatch[2], desc: sysMatch[3] || '', type: 'system', boundary: currentBoundary() });
      continue;
    }

    // ContainerDb(id, "label", "desc") or ContainerDb(id, "label", "tech", "desc")
    const cdbMatch = line.match(/^ContainerDb\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?(?:,\s*"([^"]*)")?\)/);
    if (cdbMatch) {
      nodes.push({ id: cdbMatch[1], label: cdbMatch[2], tech: cdbMatch[3] || '', desc: cdbMatch[4] || cdbMatch[3] || '', type: 'database', boundary: currentBoundary() });
      continue;
    }

    // Container(id, "label", "tech", "desc")
    const containerMatch = line.match(/^Container\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?(?:,\s*"([^"]*)")?\)/);
    if (containerMatch) {
      nodes.push({ id: containerMatch[1], label: containerMatch[2], tech: containerMatch[3] || '', desc: containerMatch[4] || '', type: 'container', boundary: currentBoundary() });
      continue;
    }

    // Component(id, "label", "tech", "desc")
    const compMatch = line.match(/^Component\((\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?(?:,\s*"([^"]*)")?\)/);
    if (compMatch) {
      nodes.push({ id: compMatch[1], label: compMatch[2], tech: compMatch[3] || '', desc: compMatch[4] || '', type: 'component', boundary: currentBoundary() });
      continue;
    }

    // Rel(from, to, "label", "protocol")
    const relMatch = line.match(/^Rel\((\w+),\s*(\w+),\s*"([^"]*)"(?:,\s*"([^"]*)")?\)/);
    if (relMatch) {
      rels.push({ from: relMatch[1], to: relMatch[2], label: relMatch[3], protocol: relMatch[4] || '' });
      continue;
    }
  }

  function currentBoundary() {
    return boundaries.length > 0 ? boundaries[boundaries.length - 1].id : null;
  }

  // Generate flowchart
  return generateFlowchart(title, nodes, rels, firstLine, boundaryLabels);
}

/** Generate a Mermaid flowchart from parsed C4 data. */
function generateFlowchart(title, nodes, rels, c4Type, boundaryLabels) {
  // Choose direction based on diagram type
  const direction = c4Type.includes('Deployment') ? 'TD' : 'TD';

  let dsl = `flowchart ${direction}\n`;

  // Group nodes by boundary
  const byBoundary = new Map();
  for (const n of nodes) {
    const key = n.boundary || '__root__';
    if (!byBoundary.has(key)) byBoundary.set(key, []);
    byBoundary.get(key).push(n);
  }

  // Render nodes (grouped into subgraphs if they have a boundary)
  const boundaryNodes = nodes.filter(n => n.boundary);
  const boundaryIds = new Set(boundaryNodes.map(n => n.boundary));

  // Find boundary metadata from nodes that were parsed as boundaries
  // (we track them during parsing but they're consumed — use boundary IDs only)

  for (const [boundaryId, members] of byBoundary) {
    if (boundaryId === '__root__') {
      for (const n of members) {
        dsl += `  ${renderNode(n)}\n`;
      }
    } else {
      const bLabel = boundaryLabels?.get(boundaryId) || boundaryId;
      dsl += `  subgraph ${boundaryId}["${bLabel}"]\n`;
      for (const n of members) {
        dsl += `    ${renderNode(n)}\n`;
      }
      dsl += '  end\n';
    }
  }

  // Render relationships
  for (const r of rels) {
    const label = r.protocol ? `${r.label}\\n${r.protocol}` : r.label;
    dsl += `  ${r.from} -->|"${label}"| ${r.to}\n`;
  }

  // Apply C4-style classes
  dsl += '\n';
  for (const n of nodes) {
    dsl += `  class ${n.id} ${n.type}\n`;
  }

  dsl += `
  classDef person fill:#08427b,stroke:#073b6f,color:#fff,stroke-width:2px
  classDef system fill:#1168bd,stroke:#0b4884,color:#fff,stroke-width:2px
  classDef external fill:#999,stroke:#6b6b6b,color:#fff
  classDef database fill:#438dd5,stroke:#2e6295,color:#fff
  classDef container fill:#438dd5,stroke:#2e6295,color:#fff
  classDef component fill:#85bbf0,stroke:#5a9bd5,color:#000`;

  return dsl;
}

/** Render a single node with appropriate shape. */
function renderNode(n) {
  const label = buildLabel(n);
  switch (n.type) {
    case 'person':   return `${n.id}(["${label}"])`;
    case 'database': return `${n.id}[("${label}")]`;
    case 'external': return `${n.id}[/"${label}"\\]`;
    case 'system':   return `${n.id}["${label}"]`;
    case 'container': return `${n.id}["${label}"]`;
    case 'component': return `${n.id}["${label}"]`;
    default:          return `${n.id}["${label}"]`;
  }
}

/** Build a multi-line HTML label for a node. */
function buildLabel(n) {
  let label = n.label;
  if (n.tech) label += `<br/><i>${n.tech}</i>`;
  if (n.desc && n.desc !== n.tech) label += `<br/><small>${n.desc}</small>`;
  return label;
}
