-- Add 'lowest' value to the issue_priority enum type
ALTER TYPE issue_priority ADD VALUE IF NOT EXISTS 'lowest' AFTER 'low';
