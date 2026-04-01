// QRAFT-CUSTOM: Session-level MCP toggle button
// Shows user-level (~/.claude.json) and project-level (.mcp.json) MCP servers
// as a popover in the session chatbox toolbar. Writes enabled/disabled back to
// the appropriate config files; Claude Code CLI handles the rest (loading, session reset).

import { useCallback, useEffect, useRef, useState } from 'react';
import { PlugIcon, ToggleLeft, ToggleRight } from '@phosphor-icons/react';
import type { BaseCodingAgent } from 'shared/types';
import { cn } from '@/shared/lib/utils';
import { fetchProjectMcpServers, updateProjectMcpServers } from '@/shared/lib/projectMcpApi';
import { useSettingsMachineClient } from '@/shared/dialogs/settings/settings/SettingsHostContext';
import { McpConfigStrategyGeneral } from '@/shared/lib/mcpStrategies';
import { useUserSystem } from '@/shared/hooks/useUserSystem';

interface ServerEntry {
  key: string;
  enabled: boolean;
  source: 'user' | 'project';
}

interface McpSessionToggleProps {
  workspaceId: string;
  executor: BaseCodingAgent | null | undefined;
}

export function McpSessionToggle({ workspaceId, executor }: McpSessionToggleProps) {
  const [open, setOpen] = useState(false);
  const [servers, setServers] = useState<ServerEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [userRawJson, setUserRawJson] = useState<string>('{}');
  const [projectServers, setProjectServers] = useState<Record<string, unknown>>({});
  const popoverRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const machineClient = useSettingsMachineClient();
  const { profiles } = useUserSystem();

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
    if (!machineClient || !executor) return;
    setLoading(true);
    try {
      // User-level MCP from ~/.claude.json
      const profileKey = profiles ? Object.keys(profiles).find((k) => k === executor) : null;
      const execKey = (profileKey ?? executor) as import('shared/types').BaseCodingAgent;
      const userResult = await machineClient.loadMcpServers({ executor: execKey });
      const fullConfig = McpConfigStrategyGeneral.createFullConfig(userResult.mcp_config);
      const rawJson = JSON.stringify(fullConfig, null, 2);
      setUserRawJson(rawJson);

      const userEntries: ServerEntry[] = Object.entries(
        (userResult.mcp_config as { servers?: Record<string, unknown> }).servers ?? {}
      ).map(([key, val]) => ({
        key,
        enabled: (val as Record<string, unknown>)?.enabled !== false,
        source: 'user',
      }));

      // Project-level MCP from .mcp.json
      const projectResult = await fetchProjectMcpServers(workspaceId);
      setProjectServers(projectResult.servers);
      const projectEntries: ServerEntry[] = Object.entries(projectResult.servers).map(
        ([key, val]) => ({
          key,
          enabled: (val as Record<string, unknown>)?.enabled !== false,
          source: 'project',
        })
      );

      setServers([...userEntries, ...projectEntries]);
    } catch (err) {
      console.error('[McpSessionToggle] load error:', err);
    } finally {
      setLoading(false);
    }
  }, [machineClient, executor, profiles, workspaceId]);

  useEffect(() => {
    if (open) load();
  }, [open, load]);

  const toggle = useCallback(
    async (entry: ServerEntry) => {
      const newEnabled = !entry.enabled;

      // Optimistic update
      setServers((prev) =>
        prev.map((s) => (s.key === entry.key && s.source === entry.source ? { ...s, enabled: newEnabled } : s))
      );

      try {
        if (entry.source === 'user') {
          // Write enabled flag into user-level config JSON
          const parsed = JSON.parse(userRawJson);
          const root = parsed.mcpServers ?? parsed;
          if (root[entry.key] && typeof root[entry.key] === 'object') {
            root[entry.key].enabled = newEnabled;
          }
          const updated = JSON.stringify(parsed, null, 2);
          setUserRawJson(updated);

          if (machineClient && executor) {
            const profileKey = profiles ? Object.keys(profiles).find((k) => k === executor) : null;
            const execKey = (profileKey ?? executor) as import('shared/types').BaseCodingAgent;
            const userResult = await machineClient.loadMcpServers({ executor: execKey });
            const fullCfg = McpConfigStrategyGeneral.createFullConfig(userResult.mcp_config);
            const rootCfg = (fullCfg.mcpServers ?? fullCfg) as Record<string, unknown>;
            if (rootCfg[entry.key] && typeof rootCfg[entry.key] === 'object') {
              (rootCfg[entry.key] as Record<string, unknown>).enabled = newEnabled;
            }
            const apiServers = McpConfigStrategyGeneral.extractServersForApi(
              userResult.mcp_config,
              fullCfg
            );
            await machineClient.saveMcpServers({ executor: execKey }, { servers: apiServers });
          }
        } else {
          // Write enabled flag into project-level config
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
        // Revert optimistic update
        setServers((prev) =>
          prev.map((s) => (s.key === entry.key && s.source === entry.source ? { ...s, enabled: !newEnabled } : s))
        );
      }
    },
    [userRawJson, projectServers, machineClient, executor, profiles, workspaceId]
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
        <PlugIcon weight={enabledCount > 0 ? 'fill' : 'regular'} className="size-4" />
        {enabledCount > 0 && (
          <span className="ml-0.5 text-xs tabular-nums leading-none">{enabledCount}</span>
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
            <div className="px-3 py-3 text-xs text-low">No MCP servers configured</div>
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
                    <span className="text-sm font-medium text-normal truncate">{entry.key}</span>
                    <span className="text-xs text-low">{entry.source}</span>
                  </span>
                  {entry.enabled ? (
                    <ToggleRight className="size-5 text-success shrink-0" weight="fill" />
                  ) : (
                    <ToggleLeft className="size-5 text-low shrink-0" weight="fill" />
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
