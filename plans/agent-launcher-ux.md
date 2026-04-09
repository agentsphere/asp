# Agent Launcher UX — Design Plan

## Problem

The current chat bubble says "chat with me." But the agent isn't a chatbot — it's a **doer**. The icon and interaction should communicate: "I'll handle this for you."

## Icon: The Spark

A **4-point starburst/spark** — not a chat bubble, not a robot. It says:
- "Something will happen when you click me"
- Multifaceted (each point = a capability)
- Feels like igniting an action, not starting a conversation

Small, subtle pulse animation when idle. Glows brighter on hover. When an agent session is running, the spark **spins slowly** — user learns to associate the spin with "agent is working."

## Placement: Context-Aware FAB

Bottom-right floating button, adapts to context:

| Location | What it shows |
|----------|--------------|
| Dashboard (no project expanded) | Global: Create App, Import from Git |
| Project expanded (any tab) | Project flows: New Feature, Audit, Debug, Docs, Tests, Design, Ops |
| Platform admin pages | Platform: Admin tasks (future) |
| Already has running session | Pulse + progress text peek, click opens session |

## Expansion: Action Palette

Click the spark → slides up a compact palette:

```
+------------------------------------------+
|  What should I work on?                  |
|  [____________________________] [Start]  |
|                                          |
|  FOR platform-demo                       |
|  +------+ +------+ +------+ +------+    |
|  | Feat | | Audit| | Tests| | Debug|    |
|  +------+ +------+ +------+ +------+    |
|  +------+ +------+ +------+ +------+    |
|  | Docs | |Design| |  Ops | |  UX  |    |
|  +------+ +------+ +------+ +------+    |
|                                          |
|  GENERAL                                 |
|  +------+ +------+                       |
|  |Create| |Import|                       |
|  +------+ +------+                       |
+------------------------------------------+
```

Each tile: icon + label. Hover shows a one-liner description. Click either:
- **Immediate start** (e.g., Audit → starts full audit agent session)
- **Brief prompt** (e.g., Feature → "Describe the feature" input, then starts)

## Flow Details

| Flow | Trigger | What happens |
|------|---------|-------------|
| **Feature** | Free text prompt | Creates branch, implements, runs tests |
| **Audit** | Choose: Full / Security / Tests / API / Perf | Runs audit skill, produces findings |
| **Tests** | "Improve coverage" | Analyzes gaps, writes tests, verifies |
| **Debug** | Paste error or describe bug | Investigates, proposes fix |
| **Docs** | "Improve docs" or specific file | Updates/generates docs |
| **Design** | Describe what to design | Architecture/system design output |
| **UX** | Describe flow to improve | UI/UX improvements |
| **Ops** | Choose: Deploy / Scale / Config | Ops-related changes |
| **Create** | Describe app | Creates new project from template |
| **Import** | Git URL | Clones, sets up pipeline |

## Running Session Indicator

When an agent is working:
- Spark icon **spins** + shows a tiny progress badge
- Clicking opens the **session panel** (existing chat panel, reframed as "session progress")
- Multiple sessions: badge shows count, click shows list

## Design Principles

1. **Not a chatbot** — the spark is an action launcher, not a conversation starter
2. **Context-first** — project flows appear when you're in a project, no clutter otherwise
3. **Progressive disclosure** — icon → palette → prompt → session
4. **Learnable** — spin = working, pulse = ready, the icon becomes muscle memory

## Implementation

### Components

1. **`AgentLauncher`** — replaces ManagerChat bubble, renders the floating spark + palette
2. **`AgentPalette`** — expandable action grid with context-aware flow tiles
3. **Context hook** — reads current route to determine which flows to show
4. **Flow configs** — each flow has: id, icon, label, description, promptRequired, skill/endpoint

### Files to create/modify

- `ui/src/components/AgentLauncher.tsx` — new
- `ui/src/components/AgentPalette.tsx` — new
- `ui/src/index.tsx` — replace `<ManagerChat />` with `<AgentLauncher />`
- `ui/src/style.css` — spark animation, palette styles
