# Plan: UI Preview Screenshots v2 — Unified Dev + Pipeline

## Context

The current screenshot pipeline builds a separate Docker image (`Dockerfile.screenshots`) with Playwright + the full FastAPI app, boots uvicorn, waits for health (up to 60s), then screenshots pages. This is:

- **Slow** — ~50s per pipeline run, plus ~100s to build the image
- **Fragile** — app boot timeouts on resource-constrained pods
- **Pipeline-only** — agents can't generate screenshots during dev sessions
- **8+ hours of debugging** image creation/pull/auth issues and still broken in pipeline

**Key insight**: Screenshots should work the same way whether an agent runs them during a dev session or a pipeline step runs them in CI. One tool, one image, one command.

**Approach**: Add Playwright to the project's dev image (`Dockerfile.dev`), pre-render Jinja2 templates to static HTML (no app boot), screenshot with Playwright. Delete `Dockerfile.screenshots` entirely — no separate build step needed.

### How it works in both contexts

| Context | Image | Command | Output |
|---|---|---|---|
| **Pipeline step** | `$REGISTRY/$PROJECT/dev:$COMMIT_SHA` | `python screenshots/render.py && python screenshots/capture.py` | `/output/` → artifacts collected |
| **Agent dev session** | Same dev image (already running) | Same command from workspace | `/output/` → uploaded via platform API |

Same image. Same script. Same output.

### Current pipeline (5 steps for screenshots)
```
build-dev → build-screenshots → ui-previews (pull image, boot app, wait, screenshot)
```

### New pipeline (2 steps)
```
build-dev → ui-previews (render templates, screenshot static HTML)
```

`build-screenshots` step and `Dockerfile.screenshots` are eliminated entirely.

---

## Design Principles

- **Unified execution** — Same script works in agent dev sessions and pipeline steps. No pipeline-specific tooling.
- **Dev image is the runtime** — Playwright browsers live in `Dockerfile.dev`. Agents already have the dev image; pipeline steps use it directly.
- **Static render, not live app** — Jinja2 renders templates to HTML files with seed data. No DB, no uvicorn, no OTEL. ~0.5s.
- **Same artifact contract** — Output to `/output/components/` and `/output/flows/` with `config.json` matching `UiPreviewConfig`. Zero platform code changes.

---

## PR 1: Add Playwright to Dev Image + Rewrite Screenshot Scripts

Single PR — changes are in demo project template files only. No Rust/platform changes.

- [ ] Types & errors defined (N/A — no Rust changes)
- [ ] Migration applied (N/A)
- [ ] Tests written (manual pipeline + agent verification)
- [ ] Implementation complete
- [ ] Integration/E2E tests passing
- [ ] Quality gate passed

### 1. Dockerfile.dev — Add Playwright

Current:
```dockerfile
ARG PLATFORM_RUNNER_IMAGE=platform-runner:v1
FROM ${PLATFORM_RUNNER_IMAGE}
USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip python3-venv postgresql-client \
  && rm -rf /var/lib/apt/lists/*
USER agent
```

New:
```dockerfile
ARG PLATFORM_RUNNER_IMAGE=platform-runner:v1
FROM ${PLATFORM_RUNNER_IMAGE}
USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip python3-venv postgresql-client \
  && rm -rf /var/lib/apt/lists/*

# Playwright for UI screenshots (used by both agents and pipeline)
RUN pip install --no-cache-dir --break-system-packages \
    jinja2 playwright==1.52.0 \
  && playwright install chromium --with-deps

USER agent
```

**Size impact**: Chromium + deps adds ~300MB. But we're deleting the entire `Dockerfile.screenshots` image (~900MB), and removing the `build-screenshots` pipeline step (which builds+pushes a ~900MB image). Net savings.

### 2. screenshots/render.py — Static Template Renderer

New file. Pure Jinja2, imports seed data from `app/db.py`, renders each page variant to `/tmp/rendered/`. Runs in <0.5s.

```python
"""Pre-render Jinja2 templates to static HTML with seed data.

Works in both pipeline pods and agent dev sessions — no DB,
no server framework, no OTEL. Just templates + mock data.
"""

import os
import shutil
import sys
from pathlib import Path

# Allow importing app modules from workspace
sys.path.insert(0, os.getcwd())

from jinja2 import Environment, FileSystemLoader
from app.db import SEED_PRODUCTS

OUT = Path(os.getenv("RENDER_DIR", "/tmp/rendered"))
STATIC_SRC = Path("app/static")


class MockRequest:
    """Minimal request stub for Jinja2 templates that reference request.*"""

    class url:
        path = "/"

    def url_for(self, name, **kw):
        routes = {
            "catalog": "/index.html",
            "product_detail": f"/product-{kw.get('product_id', 1)}.html",
            "view_cart": "/cart-empty.html",
            "orders_page": "/orders.html",
        }
        return routes.get(name, "/index.html")


def main():
    products = [{**p, "id": i + 1, "stock": 100} for i, p in enumerate(SEED_PRODUCTS)]
    request = MockRequest()

    pages = {
        "index.html": ("catalog.html", {"products": products, "request": request}),
        "product-1.html": ("product.html", {"product": products[0], "request": request}),
        "cart-empty.html": ("cart.html", {"items": [], "total": 0, "request": request}),
        "cart-items.html": (
            "cart.html",
            {
                "items": [
                    {"product_id": 1, "name": "Starter Kit", "price_cents": 2900, "quantity": 2},
                    {"product_id": 2, "name": "Pro Bundle", "price_cents": 9900, "quantity": 1},
                ],
                "total": 15700,
                "request": request,
            },
        ),
        "orders.html": ("orders.html", {"orders": [], "request": request}),
    }

    OUT.mkdir(parents=True, exist_ok=True)

    # Copy static assets (CSS, images)
    static_dest = OUT / "static"
    if static_dest.exists():
        shutil.rmtree(static_dest)
    if STATIC_SRC.exists():
        shutil.copytree(STATIC_SRC, static_dest)

    # Render each template
    env = Environment(loader=FileSystemLoader("app/templates"), autoescape=True)
    for filename, (template_name, context) in pages.items():
        tmpl = env.get_template(template_name)
        html = tmpl.render(**context, session_id="preview-session")
        (OUT / filename).write_text(html)

    print(f"Rendered {len(pages)} pages to {OUT}")


if __name__ == "__main__":
    main()
```

### 3. screenshots/capture.py — Rewrite

Serves static HTML via `http.server`, screenshots with Playwright. No FastAPI, no health check wait.

```python
"""Capture UI preview screenshots from pre-rendered static HTML.

Works in both pipeline pods and agent dev sessions.
Requires: screenshots/render.py to have run first.
"""

import asyncio
import json
import os
import subprocess
import sys
from pathlib import Path

COMP_DIR = Path(os.getenv("OUTPUT_DIR", "/output")) / "components"
FLOW_DIR = Path(os.getenv("OUTPUT_DIR", "/output")) / "flows"
RENDERED = Path(os.getenv("RENDER_DIR", "/tmp/rendered"))
PORT = int(os.getenv("CAPTURE_PORT", "8099"))
URL = f"http://localhost:{PORT}"


async def main():
    from playwright.async_api import async_playwright

    COMP_DIR.mkdir(parents=True, exist_ok=True)
    FLOW_DIR.mkdir(parents=True, exist_ok=True)

    # Minimal static file server — starts instantly
    server = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(PORT), "--directory", str(RENDERED)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    try:
        await asyncio.sleep(0.3)  # socket bind

        async with async_playwright() as p:
            browser = await p.chromium.launch()

            # --- Component screenshots ---
            page = await browser.new_page(viewport={"width": 1280, "height": 720})

            async def snap(html_file, out_path, full_page=True):
                await page.goto(f"{URL}/{html_file}")
                await page.wait_for_load_state("domcontentloaded")
                await page.wait_for_timeout(300)
                await page.screenshot(path=str(out_path), full_page=full_page)

            # Full pages
            await snap("index.html", COMP_DIR / "catalog.png")
            await snap("product-1.html", COMP_DIR / "product-detail.png")
            await snap("cart-empty.html", COMP_DIR / "cart-empty.png")
            await snap("orders.html", COMP_DIR / "orders-empty.png")
            await snap("cart-items.html", COMP_DIR / "cart-with-items.png")

            # Isolated component elements
            await page.goto(f"{URL}/index.html")
            await page.wait_for_load_state("domcontentloaded")
            await page.wait_for_timeout(300)

            nav = page.locator("nav")
            if await nav.count() > 0:
                await nav.screenshot(path=str(COMP_DIR / "nav-bar.png"))

            card = page.locator(".grid > div").first
            if await card.count() > 0:
                await card.screenshot(path=str(COMP_DIR / "product-card.png"))

            await page.close()

            # --- Flow screenshots ---
            page = await browser.new_page(viewport={"width": 1280, "height": 720})

            await snap("index.html", FLOW_DIR / "01-browse-catalog.png")
            await snap("product-1.html", FLOW_DIR / "02-view-product.png")
            await snap("cart-items.html", FLOW_DIR / "03-add-to-cart.png")
            await snap("cart-items.html", FLOW_DIR / "04-view-cart.png")
            await snap("orders.html", FLOW_DIR / "05-order-confirmed.png")

            await page.close()
            await browser.close()

        write_configs()

        comp_count = len(list(COMP_DIR.glob("*.png")))
        flow_count = len(list(FLOW_DIR.glob("*.png")))
        print(f"OK components={comp_count} flows={flow_count}")

    finally:
        server.terminate()
        server.wait()


def write_configs():
    """Write config.json files for the platform UI preview viewer."""
    comp_config = {
        "groups": {
            "pages": {
                "label": "Pages",
                "items": {
                    "catalog.png": {"label": "Product Catalog", "meta": {"route": "/", "type": "page"}},
                    "product-detail.png": {"label": "Product Detail", "meta": {"route": "/product/1", "type": "page"}},
                    "orders-empty.png": {"label": "Order History", "meta": {"route": "/orders", "type": "page"}},
                },
            },
            "components": {
                "label": "Components",
                "items": {
                    "product-card.png": {"label": "Product Card", "meta": {"component": "card"}},
                    "nav-bar.png": {"label": "Navigation Bar", "meta": {"component": "nav"}},
                },
            },
            "states": {
                "label": "States",
                "items": {
                    "cart-empty.png": {"label": "Empty Cart", "meta": {"route": "/cart", "state": "empty"}},
                    "cart-with-items.png": {"label": "Cart with Items", "meta": {"route": "/cart", "state": "filled"}},
                },
            },
        }
    }

    flow_config = {
        "groups": {
            "purchase": {
                "label": "Purchase Flow",
                "items": {
                    "01-browse-catalog.png": {"label": "1. Browse Catalog", "meta": {"step": "1"}},
                    "02-view-product.png": {"label": "2. View Product", "meta": {"step": "2"}},
                    "03-add-to-cart.png": {"label": "3. Add to Cart", "meta": {"step": "3"}},
                    "04-view-cart.png": {"label": "4. View Cart", "meta": {"step": "4"}},
                    "05-order-confirmed.png": {"label": "5. Order Confirmed", "meta": {"step": "5"}},
                },
            },
        }
    }

    (COMP_DIR / "config.json").write_text(json.dumps(comp_config, indent=2))
    (FLOW_DIR / "config.json").write_text(json.dumps(flow_config, indent=2))


if __name__ == "__main__":
    asyncio.run(main())
```

### 4. Pipeline YAML changes

**platform_v0.1.yaml** and **platform_v0.2.yaml** — remove `build-screenshots`, change `ui-previews`:

Before:
```yaml
    - name: build-screenshots
      type: imagebuild
      imageName: screenshots
      dockerfile: Dockerfile.screenshots

    - name: ui-previews
      depends_on: [build-screenshots]
      image: $REGISTRY/$PROJECT/screenshots:$COMMIT_SHA
      commands:
        - python screenshots/capture.py
      artifacts:
        - name: components
          path: /output/components/
          type: ui-comp
          config: /output/components/config.json
        - name: flows
          path: /output/flows/
          type: ui-flow
          config: /output/flows/config.json
```

After:
```yaml
    - name: ui-previews
      depends_on: [build-dev]
      image: $REGISTRY/$PROJECT/dev:$COMMIT_SHA
      commands:
        - python3 screenshots/render.py && python3 screenshots/capture.py
      artifacts:
        - name: components
          path: /output/components/
          type: ui-comp
          config: /output/components/config.json
        - name: flows
          path: /output/flows/
          type: ui-flow
          config: /output/flows/config.json
```

Changes:
- `depends_on: [build-dev]` instead of `[build-screenshots]`
- Uses dev image instead of screenshots image
- Command runs both render + capture
- Artifacts section unchanged

### 5. Delete Dockerfile.screenshots

Remove `src/onboarding/templates/Dockerfile.screenshots` — no longer needed.

### Code Changes Summary

| File | Change |
|---|---|
| `src/onboarding/templates/Dockerfile.dev` (template) | Add Playwright + jinja2 install |
| `src/onboarding/templates/screenshots/render.py` | **New** — static Jinja2 renderer |
| `src/onboarding/templates/screenshots/capture.py` | **Rewrite** — static HTML + Playwright (no FastAPI) |
| `src/onboarding/templates/Dockerfile.screenshots` | **Delete** |
| `src/onboarding/templates/platform_v0.1.yaml` | Remove `build-screenshots`, update `ui-previews` |
| `src/onboarding/templates/platform_v0.2.yaml` | Same |
| `src/onboarding/demo_project.rs` | Remove `Dockerfile.screenshots` from template file list (if listed) |

### Performance comparison

| Metric | Current | New |
|---|---|---|
| Pipeline steps for screenshots | 2 (build image + run) | 1 (run from dev image) |
| Image build time | ~100s (Dockerfile.screenshots) | 0s (already built by build-dev) |
| App boot wait | 10-60s (uvicorn + healthcheck) | 0s (static render) |
| Screenshot capture | ~10s | ~8s |
| **Total pipeline time** | **~160s** | **~10s** |
| Agent dev session | Not possible | Same ~10s command |

### What stays the same (zero platform changes)

- Artifact collection in `src/pipeline/executor.rs`
- `ArtifactDef` parsing in `src/pipeline/definition.rs`
- UI preview viewer in `ui/src/pages/ProjectDetail.tsx`
- `config.json` schema (`UiPreviewConfig`)
- API endpoints (`/api/projects/{id}/ui-previews`, compare)
- MinIO storage

### Caveats

1. **Dev image is ~300MB larger** due to Chromium. Trade-off: eliminates a ~900MB separate image and its build step. Net savings in total pipeline time and registry storage.

2. **No interactive flows** — flow screenshots are pre-rendered states, not click-through recordings. Acceptable for preview purposes.

3. **MockRequest compatibility** — `render.py` mocks `request.url_for()`. If templates add new request attributes, the mock needs updating. Simpler than maintaining a running app.

4. **Existing demo projects** need the updated `Dockerfile.dev` pushed to their repos. New projects get it from the template automatically.

### Verification

- [ ] `Dockerfile.dev` builds successfully with Playwright
- [ ] `render.py` produces 5 HTML files in `/tmp/rendered/`
- [ ] `capture.py` produces 7 component + 5 flow PNGs
- [ ] config.json files match `UiPreviewConfig` schema
- [ ] Pipeline `ui-previews` step completes in <15s
- [ ] Agent can run `python3 screenshots/render.py && python3 screenshots/capture.py` from workspace
- [ ] UI preview tab displays all screenshots correctly
- [ ] Side-by-side comparison works between branches
