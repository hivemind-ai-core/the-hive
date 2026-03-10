-- Track who last updated a topic (creator or last commenter).
ALTER TABLE topics ADD COLUMN last_updated_by TEXT;
