-- QRAFT-CUSTOM: session soft-delete support
ALTER TABLE sessions ADD COLUMN deleted_at DATETIME DEFAULT NULL;
CREATE INDEX idx_sessions_deleted_at ON sessions(deleted_at);
