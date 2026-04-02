// QRAFT-CUSTOM: Session-level MCP toggle button
// Uses direct API calls (makeLocalApiRequest) instead of useSettingsMachineClient
// to avoid SettingsHostProvider dependency (SessionChatBox is outside that provider).

import { useCallback, useEffect, useRef, useState } from 'react';
import { PlugIcon, ToggleLeft, ToggleRight } from '@phosphor-icons/react';
import type { BaseCodingAgent } from 'shared/types';
import { cn } from '@/shared/lib/utils';
import { makeLocalApiRequest } from '@/shared/lib/localApiTransport';
import { fetchProjectMcpServers, updateProjectMcpServers } from '@/shared/lib/projectMcpApi';

interface ServerEntry {
  key: string;
  enabled: boolean;
  source: 'user' | 'project';
}

interface McpSessionToggleProps {
  workspaceId: string;
  executor: BaseCodingAgent | null | undefined;
}

// Load user-level MCP servers via /api/mcp-config
// Response: { success, data: { mcp_config: { servers: {...} }, config_path } }
async function loadUserMcpServers(
  executor: string
): Promise<Record<string, unknown>> {
  const res = await makeLocalApiRequest(
    `/api/mcp-config?executor=${encodeURIComponent(executor)}`
  );
  if (!res.ok) throw new Error(`Failed to load MCP config: ${res.status}`);
  const json = await res.json();
  return (json?.data?.mcp_config?.servers as Record<string, unknown>) ?? {};
}

// Save user-level MCP servers via /api/mcp-config POST
async function saveUserMcpServers(
  executor: string,
  servers: Record<string, unknown>
) {
  const res = await makeLocalApiRequest(
    `/api/mcp-config?executor=${encodeURIComponent(executor)}`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ servers }),
    }
  );
  if (!res.ok) throw new Error(`Failed to save MCP config: ${res.status}`);
}

export function McpSessionToggle({
  workspaceId,
  executor,
}: McpSessionToggleProps) {
  const [open, setOpen] = useState(false);
  const [servers, setServers] = useState<ServerEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [userServers, setUserServers] = useState<Record<string, unknown>>({});
  const [projectServers, setProjectServers] = useState<
    Record<string, unknown>
  >({});
  const popoverRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (
        !popoverRef.current?.contains(e.target as Node) &&
        !buttonRef.current?.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const load = useCallback(async () => {
    if (!executor) return;
    setLoading(true);
    try {
      // User-level MCP from ~/.claude.json
      const uServers = await loadUserMcpServers(executor);
      setUserServers(uServers);
      const userEntries: ServerEntry[] = Object.entries(uServers).map(
        ([key, val]) => ({
          key,
          enabled: (val as Record<string, unknown>)?.enabled !== false,
          source: 'user' as const,
        })
      );

      // Project-level MCP from .mcp.json
      const projectResult = await fetchProjectMcpServers(workspaceId);
      setProjectServers(projectResult.servers);
      const projectEntries: ServerEntry[] = Object.entries(
        projectResult.servers
      ).map(([key, val]) => ({
        key,
        enabled: (val as Record<string, unknown>)?.enabled !== false,
        source: 'project' as const,
      }));

      setServers([...userEntries, ...projectEntries]);
    } catch (err) {
      console.error('[McpSessionToggle] load error:', err);
    } finally {
      setLoading(false);
    }
  }, [executor, workspaceId]);

  useEffect(() => {
    if (open) load();
  }, [open, load]);

  const toggle = useCallback(
    async (entry: ServerEntry) => {
      const newEnabled = !entry.enabled;

      // Optimistic update
      setServers((prev) =>
        prev.map((s) =>
          s.key === entry.key && s.source === entry.source
            ? { ...s, enabled: newEnabled }
            : s
        )
      );

      try {
        if (entry.source === 'user') {
          const updated = { ...userServers };
          if (updated[entry.key] && typeof updated[entry.key] === 'object') {
            (updated[entry.key] as Record<string, unknown>).enabled =
              newEnabled;
          }
          setUserServers(updated);
          if (executor) {
            await saveUserMcpServers(executor, updated);
          }
        } else {
          const updated = {
            ...projectServers,
            [entry.key]: {
              ...(projectServers[entry.key] as Record<string, unknown>),
              enabled: newEnabled,
            },
          };
          setProjectServers(updated);
          await updateProjectMcpServers(workspaceId, updated);
        }
      } catch (err) {
        console.error('[McpSessionToggle] toggle error:', err);
        setServers((prev) =>
          prev.map((s) =>
            s.key === entry.key && s.source === entry.source
              ? { ...s, enabled: !newEnabled }
              : s
          )
        );
      }
    },
    [userServers, projectServers, executor, workspaceId]
  );

  const enabledCount = servers.filter((s) => s.enabled).length;

  return (
    <div className="relative">
      <button
        ref={buttonRef}
        type="button"
        title="MCP Servers"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'flex items-center justify-center text-low hover:text-normal transition-colors',
          open && 'text-normal'
        )}
      >
        <PlugIcon
          weight={enabledCount > 0 ? 'fill' : 'regular'}
          className="size-4"
        />
        {enabledCount > 0 && (
          <span className="ml-0.5 text-xs tabular-nums leading-none">
            {enabledCount}
          </span>
        )}
      </button>

      {open && (
        <div
          ref={popoverRef}
          className="absolute bottom-full mb-2 left-0 z-50 w-64 rounded-md border border-border bg-background shadow-md"
        >
          <div className="px-3 py-2 text-xs font-semibold text-low border-b border-border">
            MCP Servers
          </div>

          {loading ? (
            <div className="px-3 py-3 text-xs text-low">Loading…</div>
          ) : servers.length === 0 ? (
            <div className="px-3 py-3 text-xs text-low">
              No MCP servers configured
            </div>
          ) : (
            <div className="max-h-72 overflow-y-auto divide-y divide-border/40">
              {servers.map((entry) => (
                <button
                  key={`${entry.source}:${entry.key}`}
                  type="button"
                  onClick={() => toggle(entry)}
                  className={cn(
                    'flex w-full items-center justify-between px-3 py-2 text-left hover:bg-secondary/50 transition-colors',
                    !entry.enabled && 'opacity-50'
                  )}
                >
                  <span className="flex flex-col min-w-0">
                    <span className="text-sm font-medium text-normal truncate">
                      {entry.key}
                    </span>
                    <span className="text-xs text-low">{entry.source}</span>
                  </span>
                  {entry.enabled ? (
                    <ToggleRight
                      className="size-5 text-success shrink-0"
                      weight="fill"
                    />
                  ) : (
                    <ToggleLeft
                      className="size-5 text-low shrink-0"
                      weight="fill"
                    />
                  )}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
