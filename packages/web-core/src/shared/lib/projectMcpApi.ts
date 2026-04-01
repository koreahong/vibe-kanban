// QRAFT-CUSTOM: API client for project-level MCP config (.mcp.json)
// Separate from user-level MCP config (machineClient.loadMcpServers / ~/.claude.json).

import { makeLocalApiRequest } from '@/shared/lib/localApiTransport';

export interface ProjectMcpResponse {
  servers: Record<string, unknown>;
  config_path: string;
}

export async function fetchProjectMcpServers(
  workspaceId: string
): Promise<ProjectMcpResponse> {
  const res = await makeLocalApiRequest(
    `/api/project-mcp-config?workspace_id=${workspaceId}`
  );
  if (!res.ok) {
    throw new Error(`Failed to fetch project MCP: ${res.status}`);
  }
  const json = await res.json();
  return json as ProjectMcpResponse;
}

export async function updateProjectMcpServers(
  workspaceId: string,
  servers: Record<string, unknown>
): Promise<ProjectMcpResponse> {
  const res = await makeLocalApiRequest(
    `/api/project-mcp-config?workspace_id=${workspaceId}`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ servers }),
    }
  );
  if (!res.ok) {
    throw new Error(`Failed to update project MCP: ${res.status}`);
  }
  const json = await res.json();
  return json as ProjectMcpResponse;
}
