// QRAFT-CUSTOM: MCP server toggle panel
// Allows enabling/disabling individual MCP servers without editing JSON directly.
// Reads/writes the same mcpServers JSON string as the textarea editor.
import { useMemo } from 'react';
import { ToggleLeft, ToggleRight } from '@phosphor-icons/react';
import { cn } from '@/shared/lib/utils';

interface McpTogglePanelProps {
  mcpServers: string;
  onChange: (json: string) => void;
}

interface ServerEntry {
  key: string;
  type: string;
  enabled: boolean;
}

function parseServers(json: string): ServerEntry[] {
  try {
    const parsed = JSON.parse(json);
    if (typeof parsed !== 'object' || parsed === null) return [];
    const root = parsed.mcpServers ?? parsed;
    return Object.entries(root as Record<string, unknown>).map(([key, val]) => {
      const server = (val ?? {}) as Record<string, unknown>;
      return {
        key,
        type: (server.type as string) ?? 'stdio',
        enabled: server.enabled !== false, // default true if missing
      };
    });
  } catch {
    return [];
  }
}

function setServerEnabled(json: string, key: string, enabled: boolean): string {
  try {
    const parsed = JSON.parse(json);
    const hasMcpServers = 'mcpServers' in parsed;
    const root = hasMcpServers ? parsed.mcpServers : parsed;
    if (root[key] && typeof root[key] === 'object') {
      root[key].enabled = enabled;
    }
    return JSON.stringify(parsed, null, 2);
  } catch {
    return json;
  }
}

export function McpTogglePanel({ mcpServers, onChange }: McpTogglePanelProps) {
  const servers = useMemo(() => parseServers(mcpServers), [mcpServers]);

  if (servers.length === 0) return null;

  return (
    <div className="space-y-1">
      <label className="text-sm font-medium text-normal">Active servers</label>
      <div className="divide-y divide-border/40 rounded-sm border border-border/50">
        {servers.map(({ key, type, enabled }) => (
          <button
            key={key}
            type="button"
            className={cn(
              'flex w-full items-center justify-between px-3 py-2 text-left transition-colors',
              'hover:bg-secondary/50',
              !enabled && 'opacity-50'
            )}
            onClick={() => onChange(setServerEnabled(mcpServers, key, !enabled))}
          >
            <span className="flex flex-col">
              <span className="text-sm font-medium text-normal">{key}</span>
              <span className="text-xs text-low">{type}</span>
            </span>
            {enabled ? (
              <ToggleRight className="size-5 text-success shrink-0" weight="fill" />
            ) : (
              <ToggleLeft className="size-5 text-low shrink-0" weight="fill" />
            )}
          </button>
        ))}
      </div>
    </div>
  );
}
