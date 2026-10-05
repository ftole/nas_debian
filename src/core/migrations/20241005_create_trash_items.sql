CREATE TABLE IF NOT EXISTS trash_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    original_path TEXT NOT NULL,
    trash_name TEXT NOT NULL,
    filename TEXT NOT NULL,
    is_dir INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    deleted_by TEXT NOT NULL,
    deleted_at TEXT NOT NULL
);
