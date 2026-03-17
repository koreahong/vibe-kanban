import { useState, useEffect, useCallback } from 'react';
import { Button } from '@vibe/ui/components/Button';
import { Input } from '@vibe/ui/components/Input';
import { jiraApi, type JiraConfig } from '@/shared/lib/api';

export function JiraSettingsSectionContent() {
  const [config, setConfig] = useState<JiraConfig>({
    jira_base_url: '',
    jira_email: '',
    jira_api_token: '',
    jira_project_key: 'MPD',
    organization_id: '',
    user_mappings: [],
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    jiraApi
      .getConfig()
      .then((cfg) => {
        if (cfg) setConfig(cfg);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setMessage(null);
    try {
      await jiraApi.updateConfig(config);
      setMessage('Configuration saved successfully.');
    } catch (e) {
      setMessage(
        `Failed to save: ${e instanceof Error ? e.message : 'Unknown error'}`
      );
    } finally {
      setSaving(false);
    }
  }, [config]);

  const handleChange = useCallback(
    (field: keyof JiraConfig, value: string) => {
      setConfig((prev) => ({ ...prev, [field]: value }));
    },
    []
  );

  if (loading) {
    return <div className="text-sm text-muted-foreground">Loading...</div>;
  }

  return (
    <div className="space-y-4 pb-6">
      <p className="text-sm text-muted-foreground">
        Configure Jira integration to import issues and push updates between
        Jira and your kanban board.
      </p>

      <div className="space-y-3">
        <div className="space-y-1">
          <label className="text-xs font-medium text-muted-foreground">
            Jira Base URL
          </label>
          <Input
            placeholder="https://yourcompany.atlassian.net"
            value={config.jira_base_url}
            onChange={(e) => handleChange('jira_base_url', e.target.value)}
          />
        </div>

        <div className="space-y-1">
          <label className="text-xs font-medium text-muted-foreground">
            Email
          </label>
          <Input
            placeholder="you@company.com"
            value={config.jira_email}
            onChange={(e) => handleChange('jira_email', e.target.value)}
          />
        </div>

        <div className="space-y-1">
          <label className="text-xs font-medium text-muted-foreground">
            API Token
          </label>
          <Input
            type="password"
            placeholder="Your Jira API token"
            value={config.jira_api_token}
            onChange={(e) => handleChange('jira_api_token', e.target.value)}
          />
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="space-y-1">
            <label className="text-xs font-medium text-muted-foreground">
              Project Key
            </label>
            <Input
              placeholder="MPD"
              value={config.jira_project_key}
              onChange={(e) =>
                handleChange('jira_project_key', e.target.value)
              }
            />
          </div>

          <div className="space-y-1">
            <label className="text-xs font-medium text-muted-foreground">
              Organization ID
            </label>
            <Input
              placeholder="UUID"
              value={config.organization_id}
              onChange={(e) => handleChange('organization_id', e.target.value)}
            />
          </div>
        </div>
      </div>

      {message && (
        <p
          className={`text-sm ${message.startsWith('Failed') ? 'text-destructive' : 'text-green-600'}`}
        >
          {message}
        </p>
      )}

      <Button onClick={handleSave} disabled={saving}>
        {saving ? 'Saving...' : 'Save Configuration'}
      </Button>
    </div>
  );
}
