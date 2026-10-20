<?php

declare(strict_types=1);

namespace App\Services;

use App\Core\Request;

/**
 * Servicio de Auditoría Administrativa y Trazabilidad de Acciones del Sistema.
 * Registra eventos atómicamente en /var/log/nas-admin.log y los emite a syslog (LOCAL6).
 * Formato estructurado: [TIMESTAMP] [IP] [USER] [ACTION] [TARGET] [STATUS] [DETAILS_JSON]
 */
class AuditService
{
    private static string $logPath = '/var/log/nas-admin.log';
    private static ?string $currentUserOverride = null;

    public static function setLogPath(string $path): void
    {
        self::$logPath = $path;
    }

    public static function getLogPath(): string
    {
        return self::$logPath;
    }

    public static function setCurrentUser(?string $user): void
    {
        self::$currentUserOverride = $user;
    }

    public static function getCurrentUser(): string
    {
        return self::resolveCurrentUser();
    }

    /**
     * Registra una acción administrativa con marca de tiempo, IP, usuario, acción, objetivo, estado y detalles.
     */
    public static function log(string $action, string $target, string $status, array $details = []): void
    {
        $timestamp = date('Y-m-d H:i:s');
        // Neutraliza CR/LF y otros caracteres de control para impedir la
        // inyección o falsificación de líneas en la bitácora de auditoría.
        $clean = static fn (mixed $value): string => trim((string) preg_replace('/[\x00-\x1F\x7F]+/', ' ', (string) $value));
        $rawIp = !empty($details['ip']) && filter_var((string) $details['ip'], FILTER_VALIDATE_IP)
            ? (string) $details['ip']
            : self::resolveClientIp();
        $ip = $clean($rawIp);
        $user = $clean(self::resolveCurrentUser());
        $action = $clean($action);
        $target = $clean($target);
        $statusUpper = strtoupper($clean($status));
        $detailsJson = !empty($details) ? json_encode($details, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE) : '{}';

        $line = sprintf(
            "[%s] [%s] [%s] [%s] [%s] [%s] %s\n",
            $timestamp,
            $ip,
            $user,
            $action,
            $target,
            $statusUpper,
            $detailsJson
        );

        // 1. Escritura a archivo de registro con bloqueo exclusivo
        $targetFile = self::$logPath;
        $dir = dirname($targetFile);
        if (is_dir($dir) && (is_writable($dir) || (file_exists($targetFile) && is_writable($targetFile)))) {
            @file_put_contents($targetFile, $line, FILE_APPEND | LOCK_EX);
        } elseif (DIRECTORY_SEPARATOR === '\\') {
            // Entorno local de desarrollo
            @file_put_contents($targetFile, $line, FILE_APPEND | LOCK_EX);
        }

        // 2. Emisión a syslog LOCAL6 (Debian 13)
        if (function_exists('openlog') && function_exists('syslog') && DIRECTORY_SEPARATOR !== '\\') {
            try {
                @openlog('nas_admin', LOG_PID, LOG_LOCAL6);
                $priority = ($statusUpper === 'SUCCESS' || $statusUpper === 'OK') ? LOG_NOTICE : LOG_WARNING;
                @syslog($priority, sprintf("[%s] [%s] [%s] -> %s: %s", $user, $action, $target, $statusUpper, $detailsJson));
                @closelog();
            } catch (\Throwable) {
                // Silencioso ante contingencias de syslog
            }
        }

        // 3. Persistencia estructurada en Base de Datos Nativa SQLite
        try {
            DatabaseService::insert('audit_logs', [
                'timestamp' => $timestamp,
                'ip' => $ip,
                'username' => $user,
                'action' => $action,
                'target' => $target,
                'status' => $statusUpper,
                'details' => $detailsJson,
                'source' => 'admin',
            ]);
        } catch (\Throwable) {
            // Tolerante ante contingencias de base de datos
        }
    }

    /**
     * Resuelve la dirección IP del cliente universalmente (HTTP Proxy, headers o entorno SSH/CLI).
     */
    private static function resolveClientIp(): string
    {
        // 1. Detección HTTP mediante abstracción Request
        if (isset($_SERVER['REMOTE_ADDR']) || isset($_SERVER['HTTP_X_FORWARDED_FOR']) || isset($_SERVER['HTTP_X_REAL_IP'])) {
            $clientIp = (new Request())->getClientIp();
            if ($clientIp !== '127.0.0.1' && filter_var($clientIp, FILTER_VALIDATE_IP)) {
                return $clientIp;
            }
        }

        // 2. Detección en sesiones SSH remotas (CLI / Paramiko / OpenSSH)
        $sshClient = getenv('SSH_CLIENT') ?: ($_SERVER['SSH_CLIENT'] ?? null);
        if (!empty($sshClient)) {
            $parts = explode(' ', trim((string) $sshClient));
            if (!empty($parts[0]) && filter_var($parts[0], FILTER_VALIDATE_IP)) {
                return $parts[0];
            }
        }
        $sshConn = getenv('SSH_CONNECTION') ?: ($_SERVER['SSH_CONNECTION'] ?? null);
        if (!empty($sshConn)) {
            $parts = explode(' ', trim((string) $sshConn));
            if (!empty($parts[0]) && filter_var($parts[0], FILTER_VALIDATE_IP)) {
                return $parts[0];
            }
        }

        // 3. Fallback a REMOTE_ADDR o localhost
        if (isset($_SERVER['REMOTE_ADDR']) && filter_var($_SERVER['REMOTE_ADDR'], FILTER_VALIDATE_IP)) {
            return (string) $_SERVER['REMOTE_ADDR'];
        }

        return '127.0.0.1';
    }

    private static function resolveCurrentUser(): string
    {
        if (self::$currentUserOverride !== null && self::$currentUserOverride !== '') {
            return self::$currentUserOverride;
        }
        if (isset($_SESSION) && !empty($_SESSION['nas_user']['username'])) {
            return (string) $_SESSION['nas_user']['username'];
        }
        $sudoUser = getenv('SUDO_USER') ?: ($_SERVER['SUDO_USER'] ?? null);
        if (!empty($sudoUser)) {
            return (string) $sudoUser;
        }
        $logname = getenv('LOGNAME') ?: ($_SERVER['USER'] ?? null);
        if (!empty($logname)) {
            return (string) $logname;
        }
        return 'sistema';
    }
}
