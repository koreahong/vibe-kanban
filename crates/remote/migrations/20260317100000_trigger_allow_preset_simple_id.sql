-- Replace the trigger function so that when simple_id and issue_number are
-- pre-set (e.g., from Jira sync), the auto-generation logic is skipped.
CREATE OR REPLACE FUNCTION set_issue_simple_id()
RETURNS TRIGGER AS $$
DECLARE
    v_issue_number INTEGER;
    v_issue_prefix VARCHAR(10);
BEGIN
    -- If simple_id is pre-set (e.g., Jira key "MPD-172") and issue_number > 0,
    -- skip auto-generation.
    IF NEW.simple_id IS NOT NULL AND NEW.simple_id != '' AND NEW.issue_number > 0 THEN
        RETURN NEW;
    END IF;

    -- Existing auto-generation logic
    UPDATE projects
    SET issue_counter = issue_counter + 1
    WHERE id = NEW.project_id
    RETURNING issue_counter INTO v_issue_number;

    SELECT o.issue_prefix INTO v_issue_prefix
    FROM projects p
    JOIN organizations o ON o.id = p.organization_id
    WHERE p.id = NEW.project_id;

    NEW.issue_number := v_issue_number;
    NEW.simple_id := v_issue_prefix || '-' || v_issue_number;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
