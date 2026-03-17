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
import { Search, Loader2, FolderDown } from 'lucide-react';
import { defineModal } from '@/shared/lib/modals';
import {
  jiraApi,
  type JiraSearchResult,
  type JiraSearchResponse,
} from '@/shared/lib/api';

export interface ImportEpicAsProjectDialogProps {
  organizationId: string;
}

export interface ImportEpicAsProjectDialogResult {
  action: 'imported' | 'canceled';
  projectName?: string;
  issuesImported?: number;
}

const ImportEpicAsProjectDialogImpl = create<ImportEpicAsProjectDialogProps>(
  (props) => {
    const modal = useModal();
    const { organizationId } = props;

    const [query, setQuery] = useState('');
    const [results, setResults] = useState<JiraSearchResult[]>([]);
    const [loading, setLoading] = useState(false);
    const [importing, setImporting] = useState<string | null>(null);
    const [importResult, setImportResult] = useState<{
      projectName: string;
      issuesImported: number;
    } | null>(null);
    const [error, setError] = useState<string | null>(null);

    const handleSearch = useCallback(async () => {
      setLoading(true);
      setError(null);
      try {
        const response: JiraSearchResponse = await jiraApi.search(
          query,
          'Epic',
          20
        );
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

    const handleImportEpic = useCallback(
      async (epic: JiraSearchResult) => {
        setImporting(epic.key);
        setError(null);
        try {
          const result = await jiraApi.importEpic(
            epic.key,
            organizationId,
            true
          );
          setImportResult({
            projectName: result.project_name,
            issuesImported: result.issues_imported,
          });
          if (result.errors.length > 0) {
            setError(
              `Imported with ${result.errors.length} error(s): ${result.errors[0]}`
            );
          }
        } catch (e) {
          setError(
            `Failed to import ${epic.key}: ${e instanceof Error ? e.message : 'Unknown error'}`
          );
        } finally {
          setImporting(null);
        }
      },
      [organizationId]
    );

    const handleClose = () => {
      const result: ImportEpicAsProjectDialogResult = importResult
        ? {
            action: 'imported',
            projectName: importResult.projectName,
            issuesImported: importResult.issuesImported,
          }
        : { action: 'canceled' };
      modal.resolve(result);
    };

    return (
      <Dialog open={modal.visible} onOpenChange={handleClose}>
        <DialogContent className="sm:max-w-[600px] max-h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle>Import Jira Epic as Project</DialogTitle>
            <DialogDescription>
              Search for a Jira Epic to import as a new project with all its
              child issues.
            </DialogDescription>
          </DialogHeader>

          {importResult ? (
            <div className="space-y-3 py-4">
              <div className="text-center space-y-2">
                <FolderDown className="h-8 w-8 mx-auto text-green-500" />
                <p className="text-sm font-medium">
                  Project &quot;{importResult.projectName}&quot; created
                </p>
                <p className="text-xs text-muted-foreground">
                  {importResult.issuesImported} issue
                  {importResult.issuesImported !== 1 ? 's' : ''} imported
                </p>
              </div>
              {error && (
                <div className="text-sm text-destructive text-center">
                  {error}
                </div>
              )}
              <div className="flex justify-center">
                <Button onClick={handleClose}>Done</Button>
              </div>
            </div>
          ) : (
            <>
              <div className="flex gap-2">
                <Input
                  placeholder="Search Epics by name or key..."
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={handleKeyDown}
                  autoFocus
                />
                <Button
                  variant="outline"
                  size="icon"
                  onClick={handleSearch}
                  disabled={loading}
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
                {results.length === 0 && !loading && (
                  <div className="text-sm text-muted-foreground text-center py-4">
                    {query
                      ? 'No Epics found'
                      : 'Search for Jira Epics to import'}
                  </div>
                )}
                {results.map((epic) => (
                  <div
                    key={epic.key}
                    className="flex items-center gap-3 p-3 rounded-md hover:bg-muted/50 group"
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-xs font-mono text-muted-foreground">
                          {epic.key}
                        </span>
                        <Badge variant="outline" className="text-[10px]">
                          Epic
                        </Badge>
                        <Badge
                          variant="secondary"
                          className="text-[10px]"
                        >
                          {epic.status}
                        </Badge>
                      </div>
                      <div className="text-sm font-medium mt-1">
                        {epic.summary}
                      </div>
                      {epic.assignee && (
                        <div className="text-[10px] text-muted-foreground mt-0.5">
                          {epic.assignee}
                        </div>
                      )}
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => handleImportEpic(epic)}
                      disabled={importing !== null}
                      className="opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                    >
                      {importing === epic.key ? (
                        <Loader2 className="h-3 w-3 animate-spin mr-1" />
                      ) : (
                        <FolderDown className="h-3 w-3 mr-1" />
                      )}
                      Import as Project
                    </Button>
                  </div>
                ))}
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    );
  }
);

export const ImportEpicAsProjectDialog = defineModal<
  ImportEpicAsProjectDialogProps,
  ImportEpicAsProjectDialogResult
>(ImportEpicAsProjectDialogImpl);
