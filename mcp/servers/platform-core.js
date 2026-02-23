/**
 * Platform Core MCP Server
 *
 * Provides project info and general platform queries.
 * Always loaded for every agent role.
 */

import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { apiGet, apiPost, PROJECT_ID } from "../lib/client.js";

const SESSION_ID = process.env.SESSION_ID || "";

const server = new Server(
  { name: "platform-core", version: "0.1.0" },
  { capabilities: { tools: {} } },
);

const TOOLS = [
  {
    name: "get_project",
    description: "Get project details (name, owner, visibility, default branch, description)",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: `Project UUID. Defaults to ${PROJECT_ID || "current project"}.`,
        },
      },
    },
  },
  {
    name: "list_projects",
    description: "List projects the agent has access to",
    inputSchema: {
      type: "object",
      properties: {
        limit: { type: "integer", description: "Max results (default 50, max 100)" },
        offset: { type: "integer", description: "Pagination offset" },
        search: { type: "string", description: "Search by name" },
      },
    },
  },
  {
    name: "spawn_agent",
    description:
      "Spawn a child agent session to work on a sub-task. " +
      "The child inherits your project context and runs in its own pod. " +
      "Requires agent:spawn permission.",
    inputSchema: {
      type: "object",
      properties: {
        prompt: {
          type: "string",
          description: "The task description / prompt for the child agent",
        },
        allowed_child_roles: {
          type: "array",
          items: { type: "string" },
          description: "Roles the child is allowed to spawn (e.g. ['dev', 'ops']). Optional.",
        },
      },
      required: ["prompt"],
    },
  },
  {
    name: "list_children",
    description: "List child agent sessions spawned from the current session",
    inputSchema: {
      type: "object",
      properties: {},
    },
  },
];

server.setRequestHandler({ method: "tools/list" }, async () => ({ tools: TOOLS }));

server.setRequestHandler({ method: "tools/call" }, async (request) => {
  const { name, arguments: args = {} } = request.params;

  switch (name) {
    case "get_project": {
      const pid = args.project_id || PROJECT_ID;
      const data = await apiGet(`/api/projects/${pid}`);
      return { content: [{ type: "text", text: JSON.stringify(data, null, 2) }] };
    }
    case "list_projects": {
      const data = await apiGet("/api/projects", {
        query: { limit: args.limit, offset: args.offset, search: args.search },
      });
      return { content: [{ type: "text", text: JSON.stringify(data, null, 2) }] };
    }
    case "spawn_agent": {
      if (!SESSION_ID) throw new Error("SESSION_ID not set — cannot spawn child agents");
      if (!PROJECT_ID) throw new Error("PROJECT_ID not set — cannot spawn child agents");
      const payload = { prompt: args.prompt };
      if (args.allowed_child_roles) payload.allowed_child_roles = args.allowed_child_roles;
      const data = await apiPost(
        `/api/projects/${PROJECT_ID}/sessions/${SESSION_ID}/spawn`,
        { body: payload },
      );
      return { content: [{ type: "text", text: JSON.stringify(data, null, 2) }] };
    }
    case "list_children": {
      if (!SESSION_ID) throw new Error("SESSION_ID not set");
      if (!PROJECT_ID) throw new Error("PROJECT_ID not set");
      const data = await apiGet(
        `/api/projects/${PROJECT_ID}/sessions/${SESSION_ID}/children`,
      );
      return { content: [{ type: "text", text: JSON.stringify(data, null, 2) }] };
    }
    default:
      throw new Error(`Unknown tool: ${name}`);
  }
});

const transport = new StdioServerTransport();
await server.connect(transport);
