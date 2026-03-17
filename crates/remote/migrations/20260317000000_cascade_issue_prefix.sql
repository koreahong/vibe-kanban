-- When an organization's issue_prefix changes, automatically update
-- all issues' simple_id under that organization.
CREATE OR REPLACE FUNCTION cascade_issue_prefix_update()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.issue_prefix IS DISTINCT FROM NEW.issue_prefix THEN
        UPDATE issues
        SET simple_id = NEW.issue_prefix || '-' || issues.issue_number
        FROM projects p
        WHERE issues.project_id = p.id AND p.organization_id = NEW.id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_organizations_issue_prefix_cascade
    AFTER UPDATE OF issue_prefix ON organizations
    FOR EACH ROW
    EXECUTE FUNCTION cascade_issue_prefix_update();
