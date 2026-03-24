import { zoom, zoomIdentity } from 'd3-zoom';
import { select } from 'd3-selection';

const ZOOM_ROOT_ID = 'zoom-root';

/**
 * Ensure all SVG content is wrapped in a single <g id="zoom-root">.
 * Mermaid sequence/state diagrams render multiple top-level <g> elements;
 * d3-zoom needs a single root to transform everything together.
 */
function ensureZoomRoot(svg) {
  let root = svg.getElementById(ZOOM_ROOT_ID);
  if (root) return root;

  root = document.createElementNS('http://www.w3.org/2000/svg', 'g');
  root.id = ZOOM_ROOT_ID;

  const children = Array.from(svg.childNodes);
  for (const child of children) {
    const tag = child.nodeName?.toLowerCase();
    if (tag === 'defs' || tag === 'style' || tag === 'desc' || tag === 'title') continue;
    root.appendChild(child);
  }

  svg.appendChild(root);
  return root;
}

/**
 * Read the diagram's natural bounds from the SVG before any DOM mutations.
 * Prefers viewBox (coordinate-system truth), falls back to width/height attrs.
 */
function readNaturalBounds(svg) {
  // 1. viewBox string — most reliable, set by mermaid on almost all diagrams
  const vbStr = svg.getAttribute('viewBox');
  if (vbStr) {
    const parts = vbStr.split(/[\s,]+/).map(Number);
    if (parts.length === 4 && parts[2] > 0 && parts[3] > 0) {
      return { x: parts[0], y: parts[1], width: parts[2], height: parts[3] };
    }
  }

  // 2. Explicit width/height attributes (pixel values mermaid sets)
  const w = parseFloat(svg.getAttribute('width'));
  const h = parseFloat(svg.getAttribute('height'));
  if (w > 0 && h > 0) {
    return { x: 0, y: 0, width: w, height: h };
  }

  return null;
}

/**
 * Set up d3-zoom on a container that holds a Mermaid SVG.
 */
export function setupZoom(container) {
  const z = zoom()
    .scaleExtent([0.1, 8])
    .on('zoom', (event) => {
      const root = container.querySelector(`#${ZOOM_ROOT_ID}`);
      if (root) root.setAttribute('transform', event.transform.toString());
    });

  const sel = select(container);
  sel.call(z);
  sel.on('dblclick.zoom', null);

  return {
    /**
     * Wrap SVG content and fit to viewport.
     * Targets ~90% of the container area, centered.
     */
    fitToView(duration = 500) {
      requestAnimationFrame(() => {
        const svg = container.querySelector('svg');
        if (!svg) return;

        // Read natural bounds BEFORE mutating the SVG
        const bounds = readNaturalBounds(svg);

        const root = ensureZoomRoot(svg);
        root.setAttribute('transform', '');

        // Now make SVG fill the container for d3-zoom mouse capture
        svg.removeAttribute('width');
        svg.removeAttribute('height');
        svg.removeAttribute('viewBox');
        svg.style.width = '100%';
        svg.style.height = '100%';
        svg.style.overflow = 'visible';

        const cw = container.clientWidth;
        const ch = container.clientHeight;

        if (cw === 0 || ch === 0) {
          requestAnimationFrame(() => this.fitToView(duration));
          return;
        }

        // Fall back to getBBox if we didn't get bounds from attributes
        let bx, by, bw, bh;
        if (bounds) {
          bx = bounds.x; by = bounds.y; bw = bounds.width; bh = bounds.height;
        } else {
          try {
            const bb = root.getBBox();
            bx = bb.x; by = bb.y; bw = bb.width; bh = bb.height;
          } catch {
            sel.call(z.transform, zoomIdentity);
            return;
          }
        }

        if (bw <= 0 || bh <= 0) {
          sel.call(z.transform, zoomIdentity);
          return;
        }

        // Target 90% of viewport, centered
        const scale = Math.min((cw * 0.9) / bw, (ch * 0.9) / bh);
        const clampedScale = Math.min(Math.max(scale, 0.05), 3);

        const tx = (cw - bw * clampedScale) / 2 - bx * clampedScale;
        const ty = (ch - bh * clampedScale) / 2 - by * clampedScale;

        if (duration > 0) {
          sel.transition()
            .duration(duration)
            .call(z.transform, zoomIdentity.translate(tx, ty).scale(clampedScale));
        } else {
          sel.call(z.transform, zoomIdentity.translate(tx, ty).scale(clampedScale));
        }
      });
    },

    /** Reset to identity transform. */
    reset(duration = 0) {
      if (duration > 0) {
        sel.transition().duration(duration).call(z.transform, zoomIdentity);
      } else {
        sel.call(z.transform, zoomIdentity);
      }
    },
  };
}
