"""Capture UI preview screenshots for platform artifact collection.

Starts the FastAPI app locally (no DB required — uses seed data),
then uses Playwright to screenshot each page and component state.
Outputs to /output/components/ and /output/flows/ with config.json files
that define the group hierarchy for the platform UI preview viewer.
"""

import asyncio
import json
import os
import sys
from pathlib import Path

COMP_DIR = Path(os.getenv("OUTPUT_DIR", "/output")) / "components"
FLOW_DIR = Path(os.getenv("OUTPUT_DIR", "/output")) / "flows"
APP_PORT = int(os.getenv("APP_PORT", "8099"))
APP_URL = f"http://localhost:{APP_PORT}"


async def start_app():
    """Start the FastAPI app in the background with seed data only."""
    proc = await asyncio.create_subprocess_exec(
        sys.executable, "-m", "uvicorn", "app.main:app",
        "--host", "127.0.0.1", "--port", str(APP_PORT),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env={**os.environ, "OTEL_SDK_DISABLED": "true"},
    )
    import httpx
    for _ in range(30):
        try:
            async with httpx.AsyncClient() as client:
                r = await client.get(f"{APP_URL}/healthz", timeout=2)
                if r.status_code == 200:
                    return proc
        except Exception:
            pass
        await asyncio.sleep(0.5)
    raise RuntimeError("App did not become ready within 15 seconds")


async def capture_components(browser):
    """Screenshot individual pages and component states."""
    page = await browser.new_page(viewport={"width": 1280, "height": 720})

    async def goto(url):
        await page.goto(url)
        await page.wait_for_load_state("domcontentloaded")
        await page.wait_for_timeout(1000)

    # --- Full pages ---
    await goto(f"{APP_URL}/")
    await page.screenshot(path=str(COMP_DIR / "catalog.png"), full_page=True)

    await goto(f"{APP_URL}/product/1")
    await page.screenshot(path=str(COMP_DIR / "product-detail.png"), full_page=True)

    await goto(f"{APP_URL}/cart")
    await page.screenshot(path=str(COMP_DIR / "cart-empty.png"), full_page=True)

    await goto(f"{APP_URL}/orders")
    await page.screenshot(path=str(COMP_DIR / "orders-empty.png"), full_page=True)

    # --- Isolated components ---
    await goto(f"{APP_URL}/")

    nav = page.locator("nav")
    if await nav.count() > 0:
        await nav.screenshot(path=str(COMP_DIR / "nav-bar.png"))

    card = page.locator(".grid > div").first
    if await card.count() > 0:
        await card.screenshot(path=str(COMP_DIR / "product-card.png"))

    # --- Stateful: cart with items ---
    await goto(f"{APP_URL}/product/1")
    await page.locator("button:has-text('Add to Cart')").click()
    await page.wait_for_timeout(500)
    await goto(f"{APP_URL}/product/2")
    await page.locator("button:has-text('Add to Cart')").click()
    await page.wait_for_timeout(500)

    await goto(f"{APP_URL}/cart")
    await page.screenshot(path=str(COMP_DIR / "cart-with-items.png"), full_page=True)

    await page.close()


async def capture_flows(browser):
    """Screenshot a multi-step purchase flow."""
    page = await browser.new_page(viewport={"width": 1280, "height": 720})

    # Step 1: Browse catalog
    await page.goto(f"{APP_URL}/")
    await page.wait_for_load_state("domcontentloaded")
    await page.wait_for_timeout(1000)
    await page.screenshot(path=str(FLOW_DIR / "01-browse-catalog.png"), full_page=True)

    # Step 2: View product (navigate directly — CDN scripts may delay link rendering)
    await page.goto(f"{APP_URL}/product/1")
    await page.wait_for_load_state("domcontentloaded")
    await page.wait_for_timeout(1000)
    await page.screenshot(path=str(FLOW_DIR / "02-view-product.png"), full_page=True)

    # Step 3: Add to cart
    await page.locator("button:has-text('Add to Cart')").click()
    await page.wait_for_timeout(1000)
    await page.screenshot(path=str(FLOW_DIR / "03-add-to-cart.png"), full_page=True)

    # Step 4: View cart
    await page.goto(f"{APP_URL}/cart")
    await page.wait_for_load_state("domcontentloaded")
    await page.wait_for_timeout(1000)
    await page.screenshot(path=str(FLOW_DIR / "04-view-cart.png"), full_page=True)

    # Step 5: Checkout
    await page.locator("button:has-text('Checkout')").click()
    await page.wait_for_load_state("domcontentloaded")
    await page.wait_for_timeout(1000)
    await page.screenshot(path=str(FLOW_DIR / "05-order-confirmed.png"), full_page=True)

    await page.close()


def write_configs():
    """Write config.json files defining group hierarchy for the preview viewer."""
    comp_config = {
        "groups": {
            "pages": {
                "label": "Pages",
                "items": {
                    "catalog.png": {
                        "label": "Product Catalog",
                        "meta": {"route": "/", "type": "page"},
                    },
                    "product-detail.png": {
                        "label": "Product Detail",
                        "meta": {"route": "/product/1", "type": "page"},
                    },
                    "orders-empty.png": {
                        "label": "Order History",
                        "meta": {"route": "/orders", "type": "page"},
                    },
                },
            },
            "components": {
                "label": "Components",
                "items": {
                    "product-card.png": {
                        "label": "Product Card",
                        "meta": {"component": "card"},
                    },
                    "nav-bar.png": {
                        "label": "Navigation Bar",
                        "meta": {"component": "nav"},
                    },
                },
            },
            "states": {
                "label": "States",
                "items": {
                    "cart-empty.png": {
                        "label": "Empty Cart",
                        "meta": {"route": "/cart", "state": "empty"},
                    },
                    "cart-with-items.png": {
                        "label": "Cart with Items",
                        "meta": {"route": "/cart", "state": "filled"},
                    },
                },
            },
        }
    }

    flow_config = {
        "groups": {
            "purchase": {
                "label": "Purchase Flow",
                "items": {
                    "01-browse-catalog.png": {
                        "label": "1. Browse Catalog",
                        "meta": {"step": "1"},
                    },
                    "02-view-product.png": {
                        "label": "2. View Product",
                        "meta": {"step": "2"},
                    },
                    "03-add-to-cart.png": {
                        "label": "3. Add to Cart",
                        "meta": {"step": "3"},
                    },
                    "04-view-cart.png": {
                        "label": "4. View Cart",
                        "meta": {"step": "4"},
                    },
                    "05-order-confirmed.png": {
                        "label": "5. Order Confirmed",
                        "meta": {"step": "5"},
                    },
                },
            },
        }
    }

    (COMP_DIR / "config.json").write_text(json.dumps(comp_config, indent=2))
    (FLOW_DIR / "config.json").write_text(json.dumps(flow_config, indent=2))


async def main():
    from playwright.async_api import async_playwright

    COMP_DIR.mkdir(parents=True, exist_ok=True)
    FLOW_DIR.mkdir(parents=True, exist_ok=True)

    proc = await start_app()
    try:
        async with async_playwright() as p:
            browser = await p.chromium.launch()
            await capture_components(browser)
            await capture_flows(browser)
            await browser.close()

        write_configs()

        comp_count = len(list(COMP_DIR.glob("*.png")))
        flow_count = len(list(FLOW_DIR.glob("*.png")))
        print(f"OK components={comp_count} flows={flow_count}")
    finally:
        proc.terminate()
        await proc.wait()


if __name__ == "__main__":
    asyncio.run(main())
