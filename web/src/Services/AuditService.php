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

    public static function setLogPath(string $path): void
    {
        self::$logPath = $path;
    }

    public static function getLogPath(): string
    {
        return self::$logPath;
    }

    /**
     * Registra una acción administrativa con marca de tiempo, IP, usuario, acción, objetivo, estado y detalles.
     */
    public static function log(string $action, string $target, string $status, array $details = []): void
    {
        $timestamp = date('Y-m-d H:i:s');
        $ip = self::resolveClientIp();
        $user = self::resolveCurrentUser();
        $statusUpper = strtoupper($status);
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
    }

    private static function resolveClientIp(): string
    {
        return (new Request())->getClientIp();
    }

    private static function resolveCurrentUser(): string
    {
        if (isset($_SESSION) && !empty($_SESSION['nas_user']['username'])) {
            return (string) $_SESSION['nas_user']['username'];
        }
        return 'sistema';
    }
}
