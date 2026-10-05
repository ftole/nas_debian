<?php

declare(strict_types=1);

namespace App\Services;

use PDO;
use Throwable;

/**
 * Servicio de Base de Datos Nativa SQLite con PDO.
 * Proporciona almacenamiento estructurado para auditoría, tareas, configuraciones
 * e historial de comandos sin servicios pesados en memoria (0 MB RAM en reposo).
 */
class DatabaseService
{
    private static ?PDO $pdo = null;
    private static ?string $customDbPath = null;

    public static function setDbPath(?string $path): void
    {
        self::$customDbPath = $path;
        self::$pdo = null;
    }

    public static function getDbPath(): string
    {
        if (self::$customDbPath !== null) {
            return self::$customDbPath;
        }

        $prodDir = '/var/lib/nas';
        if (is_dir($prodDir) && is_writable($prodDir)) {
            return $prodDir . '/nas.sqlite';
        }

        $localDir = dirname(__DIR__, 2) . '/data';
        if (!is_dir($localDir)) {
            @mkdir($localDir, 0770, true);
        }
        if (is_dir($localDir) && is_writable($localDir)) {
            return $localDir . '/nas.sqlite';
        }

        return sys_get_temp_dir() . '/nas_debian.sqlite';
    }

    public static function getConnection(): PDO
    {
        if (self::$pdo !== null) {
            return self::$pdo;
        }

        $path = self::getDbPath();
        $dir = dirname($path);
        if (!is_dir($dir)) {
            @mkdir($dir, 0770, true);
        }

        $dsn = 'sqlite:' . $path;
        $options = [
            PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,
            PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
            PDO::ATTR_EMULATE_PREPARES => false,
        ];

        try {
            $pdo = new PDO($dsn, null, null, $options);
            $pdo->exec('PRAGMA journal_mode = WAL;');
            $pdo->exec('PRAGMA synchronous = NORMAL;');
            $pdo->exec('PRAGMA foreign_keys = ON;');
            $pdo->exec('PRAGMA busy_timeout = 5000;');

            self::$pdo = $pdo;
            self::initializeSchema($pdo);
            return self::$pdo;
        } catch (Throwable $e) {
            error_log('Error conectando a SQLite: ' . $e->getMessage());
            throw $e;
        }
    }

    private static function initializeSchema(PDO $pdo): void
    {
        $schema = <<<'SQL'
CREATE TABLE IF NOT EXISTS audit_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    ip TEXT NOT NULL,
    username TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    status TEXT NOT NULL,
    details TEXT DEFAULT '{}',
    source TEXT DEFAULT 'admin'
);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_logs(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_logs(action);
CREATE INDEX IF NOT EXISTS idx_audit_source ON audit_logs(source);

CREATE TABLE IF NOT EXISTS backup_tasks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    proto TEXT NOT NULL,
    source TEXT NOT NULL,
    cron_expr TEXT NOT NULL,
    retention INTEGER DEFAULT 14,
    enabled INTEGER DEFAULT 1,
    last_status TEXT DEFAULT 'PENDING',
    last_run_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS backup_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    snapshot_name TEXT NOT NULL,
    started_at DATETIME NOT NULL,
    finished_at DATETIME,
    status TEXT NOT NULL,
    bytes_transferred INTEGER DEFAULT 0,
    files_count INTEGER DEFAULT 0,
    duration_sec INTEGER DEFAULT 0,
    log_output TEXT DEFAULT '',
    FOREIGN KEY (task_id) REFERENCES backup_tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_hist_task ON backup_history(task_id, finished_at DESC);

CREATE TABLE IF NOT EXISTS system_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS domain_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    domain TEXT,
    realm TEXT,
    dc_host TEXT,
    joined_at DATETIME,
    status TEXT DEFAULT 'standalone'
);

CREATE TABLE IF NOT EXISTS terminal_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    command TEXT NOT NULL,
    executed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    username TEXT NOT NULL,
    cwd TEXT NOT NULL,
    exit_code INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_term_time ON terminal_history(executed_at DESC);
SQL;

        $pdo->exec($schema);
    }

    public static function query(string $sql, array $params = []): array
    {
        $stmt = self::getConnection()->prepare($sql);
        $stmt->execute($params);
        return $stmt->fetchAll();
    }

    public static function execute(string $sql, array $params = []): bool
    {
        $stmt = self::getConnection()->prepare($sql);
        return $stmt->execute($params);
    }

    public static function insert(string $table, array $data): int
    {
        $pdo = self::getConnection();
        $fields = array_keys($data);
        $placeholders = array_map(fn($f) => ':' . $f, $fields);

        $sql = sprintf(
            'INSERT INTO %s (%s) VALUES (%s)',
            $table,
            implode(', ', $fields),
            implode(', ', $placeholders)
        );

        $stmt = $pdo->prepare($sql);
        $stmt->execute($data);
        return (int) $pdo->lastInsertId();
    }
}
