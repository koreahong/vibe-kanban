import { useState, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@vibe/ui/components/KeyboardDialog';
import { Button } from '@vibe/ui/components/Button';
import { Input } from '@vibe/ui/components/Input';
import { Badge } from '@vibe/ui/components/Badge';
import { create, useModal } from '@ebay/nice-modal-react';
import { Search, Download, Loader2 } from 'lucide-react';
import { defineModal } from '@/shared/lib/modals';
import {
  jiraApi,
  type JiraSearchResult,
  type JiraSearchResponse,
} from '@/shared/lib/api';

export interface JiraImportDialogProps {
  projectId: string;
}

export interface JiraImportDialogResult {
  action: 'imported' | 'canceled';
  importedKeys?: string[];
}

const priorityColor: Record<string, string> = {
  Highest: 'text-red-500',
  High: 'text-orange-500',
  Medium: 'text-yellow-500',
  Low: 'text-blue-500',
  Lowest: 'text-gray-400',
};

const JiraImportDialogImpl = create<JiraImportDialogProps>((props) => {
  const modal = useModal();
  const { projectId } = props;

  const [query, setQuery] = useState('');
  const [results, setResults] = useState<JiraSearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState<Set<string>>(new Set());
  const [imported, setImported] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const handleSearch = useCallback(async () => {
    if (!query.trim()) return;
    setLoading(true);
    setError(null);
    try {
      const response: JiraSearchResponse = await jiraApi.search(query, undefined, 20);
      setResults(response.issues);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Search failed');
    } finally {
      setLoading(false);
    }
  }, [query]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        handleSearch();
      }
    },
    [handleSearch]
  );

  const handleImport = useCallback(
    async (issue: JiraSearchResult) => {
      setImporting((prev) => new Set(prev).add(issue.key));
      try {
        await jiraApi.importIssue(issue.key, projectId);
        setImported((prev) => new Set(prev).add(issue.key));
      } catch (e) {
        setError(
          `Failed to import ${issue.key}: ${e instanceof Error ? e.message : 'Unknown error'}`
        );
      } finally {
        setImporting((prev) => {
          const next = new Set(prev);
          next.delete(issue.key);
          return next;
        });
      }
    },
    [projectId]
  );

  const handleClose = () => {
    const result: JiraImportDialogResult = imported.size > 0
      ? { action: 'imported', importedKeys: Array.from(imported) }
      : { action: 'canceled' };
    modal.resolve(result);
  };

  return (
    <Dialog open={modal.visible} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-[600px] max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>Import from Jira</DialogTitle>
          <DialogDescription>
            Search for Jira issues and import them into this project.
          </DialogDescription>
        </DialogHeader>

        <div className="flex gap-2">
          <Input
            placeholder="Search by key (MPD-172) or text..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            autoFocus
          />
          <Button
            variant="outline"
            size="icon"
            onClick={handleSearch}
            disabled={loading || !query.trim()}
          >
            {loading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Search className="h-4 w-4" />
            )}
          </Button>
        </div>

        {error && (
          <div className="text-sm text-destructive px-1">{error}</div>
        )}

        <div className="flex-1 overflow-y-auto min-h-0 space-y-1 mt-2">
          {results.length === 0 && !loading && query && (
            <div className="text-sm text-muted-foreground text-center py-4">
              No results found
            </div>
          )}
          {results.map((issue) => (
            <div
              key={issue.key}
              className="flex items-center gap-3 p-2 rounded-md hover:bg-muted/50 group"
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-mono text-muted-foreground">
                    {issue.key}
                  </span>
                  <Badge variant="outline" className="text-[10px]">
                    {issue.issuetype}
                  </Badge>
                  <span
                    className={`text-[10px] ${priorityColor[issue.priority] ?? 'text-muted-foreground'}`}
                  >
                    {issue.priority}
                  </span>
                </div>
                <div className="text-sm truncate">{issue.summary}</div>
                <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
                  <span>{issue.status}</span>
                  {issue.assignee && (
                    <>
                      <span>-</span>
                      <span>{issue.assignee}</span>
                    </>
                  )}
                </div>
              </div>
              <div className="shrink-0">
                {imported.has(issue.key) ? (
                  <Badge variant="secondary" className="text-[10px]">
                    Imported
                  </Badge>
                ) : (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleImport(issue)}
                    disabled={importing.has(issue.key)}
                    className="opacity-0 group-hover:opacity-100 transition-opacity"
                  >
                    {importing.has(issue.key) ? (
                      <Loader2 className="h-3 w-3 animate-spin mr-1" />
                    ) : (
                      <Download className="h-3 w-3 mr-1" />
                    )}
                    Import
                  </Button>
                )}
              </div>
            </div>
          ))}
        </div>

        {imported.size > 0 && (
          <div className="text-xs text-muted-foreground pt-2 border-t">
            {imported.size} issue{imported.size > 1 ? 's' : ''} imported
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
});

export const JiraImportDialog = defineModal<
  JiraImportDialogProps,
  JiraImportDialogResult
>(JiraImportDialogImpl);
