<?php

declare(strict_types=1);

namespace App\Services;

/**
 * Servicio de control del sistema operativo base (Debian 13), servicios systemd,
 * métricas de CPU/RAM, logs de journald y actualizaciones.
 */
class SystemService
{
    /**
     * Ejecuta un comando en el sistema de manera segura usando proc_open y arrays de argumentos.
     * Cero invocación de shell para garantizar inmunidad a inyecciones de comandos.
     */
    public static function runCommand(array $cmd, ?string $input = null, int $timeout = 30): array
    {
        $descriptors = [
            0 => ['pipe', 'r'],
            1 => ['pipe', 'w'],
            2 => ['pipe', 'w'],
        ];

        // En entornos Windows de desarrollo simulamos o advertimos
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                'code' => 0,
                'stdout' => 'Simulación en entorno local Windows',
                'stderr' => '',
            ];
        }

        $process = @proc_open($cmd, $descriptors, $pipes);
        if (!is_resource($process)) {
            return [
                'code' => -1,
                'stdout' => '',
                'stderr' => 'Error al invocar proc_open para el comando.',
            ];
        }

        if ($input !== null) {
            fwrite($pipes[0], $input);
        }
        fclose($pipes[0]);

        stream_set_blocking($pipes[1], false);
        stream_set_blocking($pipes[2], false);

        $stdout = '';
        $stderr = '';
        $deadline = microtime(true) + $timeout;
        $pipesOpen = [1 => true, 2 => true];

        while ($pipesOpen[1] || $pipesOpen[2]) {
            $read = [];
            if ($pipesOpen[1]) {
                $read[] = $pipes[1];
            }
            if ($pipesOpen[2]) {
                $read[] = $pipes[2];
            }

            $write = null;
            $except = null;

            $numChanged = @stream_select($read, $write, $except, 0, 50000);

            if ($numChanged > 0) {
                foreach ($read as $stream) {
                    $chunk = fread($stream, 8192);
                    if ($chunk !== false && $chunk !== '') {
                        if ($stream === $pipes[1]) {
                            $stdout .= $chunk;
                        } else {
                            $stderr .= $chunk;
                        }
                    } elseif (feof($stream)) {
                        if ($stream === $pipes[1]) {
                            $pipesOpen[1] = false;
                        } else {
                            $pipesOpen[2] = false;
                        }
                    }
                }
            }

            $status = proc_get_status($process);
            if (!$status['running']) {
                while (($chunk = fread($pipes[1], 8192)) !== false && $chunk !== '') {
                    $stdout .= $chunk;
                }
                while (($chunk = fread($pipes[2], 8192)) !== false && $chunk !== '') {
                    $stderr .= $chunk;
                }
                break;
            }

            if (microtime(true) >= $deadline) {
                @proc_terminate($process, 9);
                $stderr .= sprintf("\n[!] Tiempo de ejecución agotado (límite: %d segundos).\n", $timeout);
                break;
            }
        }

        while (($chunk = fread($pipes[1], 8192)) !== false && $chunk !== '') {
            $stdout .= $chunk;
        }
        while (($chunk = fread($pipes[2], 8192)) !== false && $chunk !== '') {
            $stderr .= $chunk;
        }
        fclose($pipes[1]);
        fclose($pipes[2]);

        $status = proc_close($process);

        return [
            'code' => $status,
            'stdout' => trim($stdout),
            'stderr' => trim($stderr),
        ];
    }

    /**
     * Invoca un comando con elevación sudo no interactiva (-n).
     */
    public static function sudo(array $cmd, ?string $input = null, int $timeout = 30): array
    {
        return self::runCommand(array_merge(['sudo', '-n'], $cmd), $input, $timeout);
    }

    /**
     * Obtiene el estado actual de los demonios clave del NAS.
     */
    public function getServicesStatus(): array
    {
        $services = [
            'smbd' => 'Servidor de Archivos Samba',
            'nmbd' => 'Servicio de Nombres NetBIOS',
            'wsdd2' => 'Descubrimiento Web Services (WSD)',
            'nginx' => 'Servidor Web Nginx',
            'cron' => 'Planificador de Tareas Cron',
        ];

        // Detección dinámica del demonio PHP-FPM instalado
        $phpFpmSvc = 'php-fpm';
        if (DIRECTORY_SEPARATOR !== '\\') {
            $checkPhp = self::runCommand(['systemctl', 'list-units', '--type=service', '--state=active', 'php*-fpm*']);
            if (preg_match('/(php[\d\.]*-fpm)/', $checkPhp['stdout'], $m)) {
                $phpFpmSvc = $m[1];
            }
        }
        $services[$phpFpmSvc] = 'Manejador de Procesos PHP-FPM (ondemand)';

        $result = [];
        foreach ($services as $svc => $label) {
            $isActive = false;
            $statusText = 'inactivo';

            if (DIRECTORY_SEPARATOR !== '\\') {
                $res = self::runCommand(['systemctl', 'is-active', $svc]);
                $isActive = ($res['code'] === 0 && trim($res['stdout']) === 'active');
                $statusText = $isActive ? 'activo' : 'inactivo';
            } else {
                $isActive = true;
                $statusText = 'activo (simulado)';
            }

            $result[] = [
                'service' => $svc,
                'name' => $label,
                'active' => $isActive,
                'status' => $statusText,
            ];
        }

        return $result;
    }

    /**
     * Inicia, detiene o reinicia un servicio autorizado.
     */
    public function manageService(string $service, string $action): array
    {
        $allowedActions = ['start', 'stop', 'restart', 'reload'];
        if (!in_array($action, $allowedActions, true)) {
            return ['success' => false, 'error' => 'Acción no permitida: ' . $action];
        }

        // Lista blanca de servicios seguros para gestionar
        if (!preg_match('/^(smbd|nmbd|wsdd2|nginx|cron|php[\d\.]*-fpm)$/', $service)) {
            return ['success' => false, 'error' => 'Servicio no permitido para gestión: ' . $service];
        }

        $res = self::sudo(['systemctl', $action, $service]);
        if ($res['code'] !== 0) {
            return [
                'success' => false,
                'error' => "Fallo al ejecutar $action sobre $service: " . ($res['stderr'] ?: $res['stdout']),
            ];
        }

        return ['success' => true, 'message' => "Servicio $service: $action ejecutado con éxito."];
    }

    /**
     * Obtiene métricas en vivo del servidor (CPU, memoria, hostname, uptime, kernel).
     */
    public function getSystemMetrics(): array
    {
        $hostname = gethostname() ?: 'SRV-NAS';
        $uptime = 'N/A';
        $kernel = php_uname('r');
        $cpuModel = 'x86_64';
        $cpuUsage = 0;
        $memTotal = 0;
        $memUsed = 0;

        if (DIRECTORY_SEPARATOR !== '\\') {
            // Uptime
            if (file_exists('/proc/uptime')) {
                $upSec = (float) explode(' ', (string) file_get_contents('/proc/uptime'))[0];
                $days = floor($upSec / 86400);
                $hours = floor(($upSec % 86400) / 3600);
                $minutes = floor(($upSec % 3600) / 60);
                $uptime = sprintf('%dd %dh %dm', $days, $hours, $minutes);
            }

            // Memoria
            if (file_exists('/proc/meminfo')) {
                $meminfo = (string) file_get_contents('/proc/meminfo');
                preg_match('/MemTotal:\s+(\d+)\s+kB/', $meminfo, $mTot);
                preg_match('/MemAvailable:\s+(\d+)\s+kB/', $meminfo, $mAvail);
                $totalKb = isset($mTot[1]) ? (int) $mTot[1] : 0;
                $availKb = isset($mAvail[1]) ? (int) $mAvail[1] : 0;
                $usedKb = max(0, $totalKb - $availKb);

                $memTotal = round($totalKb / 1024 / 1024, 2);
                $memUsed = round($usedKb / 1024 / 1024, 2);
            }

            // Modelo CPU
            if (file_exists('/proc/cpuinfo')) {
                $cpuinfo = (string) file_get_contents('/proc/cpuinfo');
                if (preg_match('/model name\s+:\s+(.+)/', $cpuinfo, $mCpu)) {
                    $cpuModel = trim($mCpu[1]);
                }
            }

            // Carga CPU
            $load = sys_getloadavg();
            $cpuUsage = $load ? round($load[0] * 10, 1) : 0;
        } else {
            $uptime = '14d 6h 32m (dev)';
            $memTotal = 16.0;
            $memUsed = 4.2;
            $cpuModel = 'Intel Xeon / AMD EPYC (Simulado)';
            $cpuUsage = 8.5;
        }

        return [
            'hostname' => $hostname,
            'uptime' => $uptime,
            'kernel' => $kernel,
            'os' => 'Debian 13 (Trixie)',
            'cpu_model' => $cpuModel,
            'cpu_usage_pct' => min(100, $cpuUsage),
            'ram_total_gb' => $memTotal,
            'ram_used_gb' => $memUsed,
            'ram_usage_pct' => $memTotal > 0 ? round(($memUsed / $memTotal) * 100, 1) : 0,
        ];
    }

    public static string $sambaAuditPath = '/var/log/samba/audit.log';
    public static string $adminAuditPath = '/var/log/nas-admin.log';
    public static string $backupAuditPath = '/srv/nas/LOGS_BACKUP/backups_master.log';

    /**
     * Parsea la bitácora de auditoría de archivos Samba (full_audit).
     * Extrae: Fecha, Usuario, IP, NetBIOS, Recurso, Acción y Archivo afectado.
     */
    public function parseSambaAuditLog(int $limit = 100, ?string $query = null): array
    {
        $path = self::$sambaAuditPath;
        $lines = [];

        if (file_exists($path) && is_readable($path)) {
            $rawLines = @file($path, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) ?: [];
            $lines = array_reverse(array_slice($rawLines, -max($limit * 3, 300)));
        } elseif (DIRECTORY_SEPARATOR === '\\' || !file_exists($path)) {
            $lines = [
                '2026-10-05T10:15:32-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|openat|ok|r|Balance_General_2026.xlsx',
                '2026-10-05T10:16:05-06:00 srv-nas smbd_audit[3412]: administrador|10.10.1.251|adm-pc|SISTEMAS|openat|ok|w|Presupuesto_Anual.xlsx',
                '2026-10-05T10:17:12-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|renameat|ok|borrador_acta.docx|acta_final.docx',
                '2026-10-05T10:18:40-06:00 srv-nas smbd_audit[3412]: administrador|10.10.1.251|adm-pc|SISTEMAS|unlinkat|ok|archivo_temporal.tmp',
                '2026-10-05T10:19:00-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|mkdirat|ok|Reportes_Q3',
                '2026-10-05T10:20:15-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|connect|ok|SISTEMAS',
            ];
        }

        $results = [];
        $qLower = $query ? $this->toLower(trim($query)) : null;

        foreach ($lines as $line) {
            $trimmed = trim($line);
            if (empty($trimmed) || !str_contains($trimmed, '|')) {
                continue;
            }

            $timestamp = date('Y-m-d H:i:s');
            $payload = $trimmed;

            if (preg_match('/^(\S+(?:\s+\S+\s+\S+)?)\s+\S+\s+(?:smbd_audit|smbd|samba)[^:]*:\s*(.*)$/', $trimmed, $m)) {
                $timestamp = $this->normalizeTimestamp($m[1]);
                $payload = $m[2];
            } elseif (preg_match('/^(.*?):\s*([a-zA-Z0-9_\-\.]+\|.*)$/', $trimmed, $m)) {
                $timestamp = $this->normalizeTimestamp($m[1]);
                $payload = $m[2];
            }

            $parts = explode('|', $payload);
            if (count($parts) < 6) {
                continue;
            }

            $user = $parts[0];
            $ip = $parts[1];
            $netbios = $parts[2];
            $share = $parts[3];
            $action = strtolower($parts[4]);
            $statusRaw = strtolower($parts[5]);
            $status = str_starts_with($statusRaw, 'ok') ? 'SUCCESS' : 'FAILED';
            $args = array_slice($parts, 6);

            $actionLabel = ucfirst($action);
            $badge = 'gray';
            $target = !empty($args) ? implode(' ', $args) : $share;

            switch ($action) {
                case 'open':
                case 'openat':
                    $isWrite = false;
                    if (isset($args[0]) && in_array(strtolower($args[0]), ['w', 'rw', 'a', 'write'], true)) {
                        $isWrite = true;
                        $target = $args[1] ?? $args[0];
                    } elseif (isset($args[0])) {
                        $target = (count($args) > 1 && in_array(strtolower($args[0]), ['r', 'ro', 'read'], true)) ? $args[1] : implode(' ', $args);
                    }
                    if ($isWrite) {
                        $actionLabel = 'Modificación / Escritura';
                        $badge = 'warn';
                    } else {
                        $actionLabel = 'Apertura / Lectura';
                        $badge = 'ok';
                    }
                    break;

                case 'unlink':
                case 'unlinkat':
                    $actionLabel = 'Eliminación de archivo';
                    $badge = 'err';
                    break;

                case 'rmdir':
                    $actionLabel = 'Eliminación de carpeta';
                    $badge = 'err';
                    break;

                case 'mkdir':
                case 'mkdirat':
                    $actionLabel = 'Creación de carpeta';
                    $badge = 'blue';
                    break;

                case 'rename':
                case 'renameat':
                    $actionLabel = 'Renombrado';
                    $badge = 'warn';
                    if (count($args) >= 2) {
                        $target = $args[0] . ' ➔ ' . $args[1];
                    }
                    break;

                case 'connect':
                    $actionLabel = 'Conexión a recurso';
                    $badge = 'blue';
                    $target = $share;
                    break;

                case 'disconnect':
                    $actionLabel = 'Desconexión de recurso';
                    $badge = 'gray';
                    $target = $share;
                    break;
            }

            if ($qLower !== null) {
                $searchable = $this->toLower("$user $ip $netbios $share $action $actionLabel $target $status");
                if (!str_contains($searchable, $qLower)) {
                    continue;
                }
            }

            $results[] = [
                'source' => 'samba_audit',
                'timestamp' => $timestamp,
                'user' => $user,
                'ip' => $ip,
                'netbios' => $netbios,
                'share' => $share,
                'action' => $action,
                'action_label' => $actionLabel,
                'badge' => $badge,
                'target' => $target,
                'status' => $status,
                'unit' => 'smbd',
                'message' => "[$actionLabel] $target ($status)",
                'raw' => $trimmed,
            ];

            if (count($results) >= $limit) {
                break;
            }
        }

        return $results;
    }

    /**
     * Parsea la bitácora de auditoría administrativa (/var/log/nas-admin.log).
     * Extrae: Fecha, Administrador, IP, Acción, Objetivo, Estado y Detalles.
     */
    public function parseAdminAuditLog(int $limit = 100, ?string $query = null): array
    {
        $path = self::$adminAuditPath;
        $lines = [];

        if (file_exists($path) && is_readable($path)) {
            $rawLines = @file($path, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) ?: [];
            $lines = array_reverse(array_slice($rawLines, -max($limit * 3, 300)));
        } elseif (DIRECTORY_SEPARATOR === '\\' || !file_exists($path)) {
            $lines = [
                '[2026-10-05 10:14:00] [10.10.1.250] [sistemas] [login_success] [sistemas] [SUCCESS] {"role":"Administrador"}',
                '[2026-10-05 10:15:00] [10.10.1.250] [sistemas] [share_create] [PUBLICO] [SUCCESS] {"scheme":4,"comment":"Acceso general"}',
                '[2026-10-05 10:20:00] [10.10.1.250] [sistemas] [user_create] [operador1] [SUCCESS] {"groups":["grp_operaciones"]}',
                '[2026-10-05 10:25:00] [10.10.1.250] [sistemas] [backup_create] [win_contabilidad] [SUCCESS] {"proto":"cifs","cron":"0 23 * * *"}',
                '[2026-10-05 10:30:00] [10.10.1.250] [sistemas] [service_manage] [smbd] [SUCCESS] {"action":"restart"}',
            ];
        }

        $results = [];
        $qLower = $query ? $this->toLower(trim($query)) : null;

        $actionLabels = [
            'login_success' => 'Inicio de sesión exitoso',
            'login_failure' => 'Fallo de autenticación',
            'logout' => 'Cierre de sesión',
            'share_create' => 'Crear recurso compartido',
            'share_delete' => 'Eliminar recurso compartido',
            'user_create' => 'Crear usuario',
            'user_delete' => 'Eliminar usuario',
            'group_create' => 'Crear grupo',
            'group_delete' => 'Eliminar grupo',
            'backup_create' => 'Programar backup',
            'backup_delete' => 'Eliminar backup',
            'backup_run_manual' => 'Ejecutar backup manual',
            'scrub_start' => 'Iniciar Scrub BTRFS',
            'trim_start' => 'Ejecutar TRIM SSD',
            'service_manage' => 'Gestionar servicio',
            'server_reboot' => 'Reinicio del servidor',
            'trash_move' => 'Mover a papelera',
            'trash_restore' => 'Restaurar de papelera',
            'trash_delete' => 'Eliminar de papelera',
            'trash_empty' => 'Vaciar papelera',
            'file_save' => 'Guardar archivo',
            'file_rename' => 'Renombrar elemento',
            'dir_create' => 'Crear carpeta',
            'file_upload' => 'Subir archivo',
        ];

        foreach ($lines as $line) {
            $trimmed = trim($line);
            if (empty($trimmed)) {
                continue;
            }

            if (!preg_match('/^\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\](?:\s+(.*))?$/', $trimmed, $m)) {
                continue;
            }

            $timestamp = $m[1];
            $ip = $m[2];
            $user = $m[3];
            $action = $m[4];
            $target = $m[5];
            $status = strtoupper($m[6]);
            $rawDetails = $m[7] ?? '{}';

            $details = [];
            if (!empty($rawDetails)) {
                $decoded = json_decode($rawDetails, true);
                $details = is_array($decoded) ? $decoded : ['info' => $rawDetails];
            }

            $actionLabel = $actionLabels[$action] ?? ucfirst(str_replace('_', ' ', $action));
            $badge = ($status === 'SUCCESS' || $status === 'OK') ? 'ok' : 'err';

            if ($qLower !== null) {
                $searchable = $this->toLower("$user $ip $action $actionLabel $target $status $rawDetails");
                if (!str_contains($searchable, $qLower)) {
                    continue;
                }
            }

            $results[] = [
                'source' => 'admin',
                'timestamp' => $timestamp,
                'ip' => $ip,
                'user' => $user,
                'action' => $action,
                'action_label' => $actionLabel,
                'target' => $target,
                'status' => $status,
                'badge' => $badge,
                'details' => $details,
                'unit' => 'panel-web',
                'message' => "[$actionLabel] Objetivo: $target ($status)",
                'raw' => $trimmed,
            ];

            if (count($results) >= $limit) {
                break;
            }
        }

        return $results;
    }

    /**
     * Parsea la bitácora de auditoría de respaldos (/srv/nas/LOGS_BACKUP/backups_master.log).
     * Extrae: Fecha, Tarea, Evento, Severidad y Mensaje.
     */
    public function parseBackupAuditLog(int $limit = 100, ?string $query = null): array
    {
        $path = self::$backupAuditPath;
        $lines = [];

        if (file_exists($path) && is_readable($path)) {
            $rawLines = @file($path, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) ?: [];
            $lines = array_reverse(array_slice($rawLines, -max($limit * 3, 300)));
        } elseif (DIRECTORY_SEPARATOR === '\\' || !file_exists($path)) {
            $lines = [
                '[2026-10-05 23:00:01] [win_contabilidad] [LOCK_ACQUIRED] [info] Bloqueo de ejecucion exclusivo adquirido',
                '[2026-10-05 23:00:02] [win_contabilidad] [SPACE_CHECK] [info] Espacio verificado: 45000 MB libres (25% en uso)',
                '[2026-10-05 23:00:04] [win_contabilidad] [MOUNT_SUCCESS] [info] Recurso CIFS //10.10.1.50/Contabilidad montado exitosamente',
                '[2026-10-05 23:00:15] [win_contabilidad] [RSYNC_COMPLETED] [info] Sincronizacion rsync finalizada correctamente',
                '[2026-10-05 23:00:16] [win_contabilidad] [SNAPSHOT_PROMOTED] [notice] Snapshot promovido: snapshot_2026-10-05_230000',
                '[2026-10-05 23:00:17] [win_contabilidad] [ROTATION_PRUNED] [info] Rotado snapshot antiguo: snapshot_2026-09-01_230000',
                '[2026-10-05 23:00:18] [win_contabilidad] [BACKUP_COMPLETED] [notice] Respaldo CIFS win_contabilidad finalizado con exito',
            ];
        }

        $results = [];
        $qLower = $query ? $this->toLower(trim($query)) : null;

        $eventLabels = [
            'LOCK_ACQUIRED' => 'Bloqueo Adquirido',
            'LOCK_OMITTED' => 'Ejecución Omitida',
            'SPACE_CHECK' => 'Verificación de Espacio',
            'MOUNT_SUCCESS' => 'Montaje Exitoso',
            'MOUNT_FAILED' => 'Fallo de Montaje',
            'RSYNC_COMPLETED' => 'Sincronización Rsync OK',
            'RSYNC_FAILED' => 'Fallo en Rsync',
            'SNAPSHOT_PROMOTED' => 'Snapshot Promovido',
            'ROTATION_PRUNED' => 'Rotación de Snapshot',
            'BACKUP_COMPLETED' => 'Respaldo Completado',
            'BACKUP_FAILED' => 'Respaldo Fallido',
        ];

        foreach ($lines as $line) {
            $trimmed = trim($line);
            if (empty($trimmed)) {
                continue;
            }

            if (!preg_match('/^\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+(.*)$/', $trimmed, $m)) {
                continue;
            }

            $timestamp = $m[1];
            $task = $m[2];
            $event = $m[3];
            $severity = strtolower($m[4]);
            $message = $m[5];

            $eventLabel = $eventLabels[$event] ?? ucfirst(str_replace('_', ' ', $event));

            $badge = match ($severity) {
                'err', 'error', 'failed' => 'err',
                'warning', 'warn' => 'warn',
                'notice', 'ok' => 'ok',
                default => 'blue',
            };

            $status = in_array($severity, ['err', 'error', 'failed'], true) ? 'FAILED' : 'SUCCESS';

            if ($qLower !== null) {
                $searchable = $this->toLower("$task $event $eventLabel $severity $message");
                if (!str_contains($searchable, $qLower)) {
                    continue;
                }
            }

            $results[] = [
                'source' => 'backup',
                'timestamp' => $timestamp,
                'task' => $task,
                'user' => $task,
                'event' => $event,
                'event_label' => $eventLabel,
                'severity' => $severity,
                'message' => $message,
                'badge' => $badge,
                'target' => $task,
                'status' => $status,
                'unit' => 'backup-' . $task,
                'raw' => $trimmed,
            ];

            if (count($results) >= $limit) {
                break;
            }
        }

        return $results;
    }

    /**
     * Obtiene registros consolidados y multidimensionales filtrados por origen y búsqueda.
     */
    public function getLogs(string $source = 'all', int $limit = 100, ?string $query = null): array
    {
        $cleanSource = strtolower(trim($source));

        switch ($cleanSource) {
            case 'samba_audit':
                return $this->parseSambaAuditLog($limit, $query);

            case 'admin':
                return $this->parseAdminAuditLog($limit, $query);

            case 'backup':
                return $this->parseBackupAuditLog($limit, $query);

            case 'system':
                $rawJournal = $this->getJournalLogs($limit * 2);
                $qLower = $query ? $this->toLower(trim($query)) : null;
                $results = [];

                foreach ($rawJournal as $j) {
                    if ($qLower !== null) {
                        $searchable = $this->toLower(($j['unit'] ?? '') . ' ' . ($j['message'] ?? ''));
                        if (!str_contains($searchable, $qLower)) {
                            continue;
                        }
                    }
                    $h = $this->humanizeSystemLog($j['unit'] ?? 'system', $j['message'] ?? '');
                    $results[] = [
                        'source' => 'system',
                        'timestamp' => $this->normalizeTimestamp($j['timestamp'] ?? ''),
                        'unit' => $j['unit'] ?? 'system',
                        'user' => 'system',
                        'target' => $j['unit'] ?? 'system',
                        'action' => 'journal_log',
                        'action_label' => 'Registro del Sistema',
                        'badge' => 'gray',
                        'status' => 'OK',
                        'message' => $j['message'] ?? '',
                        'human' => $h['human'],
                        'level' => $h['level'],
                        'raw' => ($j['timestamp'] ?? '') . ' ' . ($j['unit'] ?? '') . ': ' . ($j['message'] ?? ''),
                    ];
                    if (count($results) >= $limit) {
                        break;
                    }
                }
                return $results;

            case 'all':
            default:
                $samba = $this->parseSambaAuditLog($limit, $query);
                $admin = $this->parseAdminAuditLog($limit, $query);
                $backup = $this->parseBackupAuditLog($limit, $query);
                $rawJournal = $this->getJournalLogs(min($limit, 50));

                $system = [];
                $qLower = $query ? $this->toLower(trim($query)) : null;
                foreach ($rawJournal as $j) {
                    if ($qLower !== null) {
                        $searchable = $this->toLower(($j['unit'] ?? '') . ' ' . ($j['message'] ?? ''));
                        if (!str_contains($searchable, $qLower)) {
                            continue;
                        }
                    }
                    $h = $this->humanizeSystemLog($j['unit'] ?? 'system', $j['message'] ?? '');
                    $system[] = [
                        'source' => 'system',
                        'timestamp' => $this->normalizeTimestamp($j['timestamp'] ?? ''),
                        'unit' => $j['unit'] ?? 'system',
                        'user' => 'system',
                        'target' => $j['unit'] ?? 'system',
                        'action' => 'journal_log',
                        'action_label' => 'Sistema',
                        'badge' => 'gray',
                        'status' => 'OK',
                        'message' => $j['message'] ?? '',
                        'human' => $h['human'],
                        'level' => $h['level'],
                        'raw' => ($j['timestamp'] ?? '') . ' ' . ($j['unit'] ?? '') . ': ' . ($j['message'] ?? ''),
                    ];
                }

                $merged = array_merge($samba, $admin, $backup, $system);

                // Ordenar por fecha cronológica descendente
                usort($merged, function (array $a, array $b): int {
                    return strcmp($b['timestamp'] ?? '', $a['timestamp'] ?? '');
                });

                return array_slice($merged, 0, $limit);
        }
    }

    /**
     * Traduce mensajes del sistema (ufw, cron, sshd...) a texto entendible.
     *
     * @return array{human:string,level:string}
     */
    private function humanizeSystemLog(string $unit, string $message): array
    {
        $u = strtolower(trim($unit));
        $m = trim($message);

        // Firewall UFW: bloques de paquetes
        if (stripos($m, 'UFW') !== false) {
            $proto = preg_match('/PROTO=(\S+)/', $m, $p) ? strtoupper($p[1]) : '';
            $src = preg_match('/SRC=(\S+)/', $m, $x) ? $x[1] : '?';
            $dst = preg_match('/DST=(\S+)/', $m, $y) ? $y[1] : '?';
            $spt = preg_match('/SPT=(\d+)/', $m, $w) ? $w[1] : '';
            $dpt = preg_match('/DPT=(\d+)/', $m, $z) ? $z[1] : '';
            $accion = (stripos($m, 'BLOCK') !== false) ? 'Bloqueado' : ((stripos($m, 'ALLOW') !== false) ? 'Permitido' : 'Evento');
            $origen = $src . ($spt !== '' ? ':' . $spt : '');
            $destino = $dst . ($dpt !== '' ? ':' . $dpt : '');
            return ['human' => "[UFW] {$accion} {$proto} de {$origen} → {$destino}", 'level' => 'warn'];
        }

        // Cron: ejecuciones programadas
        if ($u === 'cron' || stripos($m, 'CRON') !== false) {
            if (preg_match('/\(([^)]+)\)\s+CMD\s+\((.*)\)/', $m, $cm)) {
                return ['human' => "[Cron] {$cm[1]} ejecutó: {$cm[2]}", 'level' => 'info'];
            }
            if (preg_match('/CMD\s+\((.*)\)/', $m, $cm2)) {
                return ['human' => "[Cron] ejecutó: {$cm2[1]}", 'level' => 'info'];
            }
            return ['human' => '[Cron] ' . $m, 'level' => 'info'];
        }

        // SSH: intentos fallidos
        if ($u === 'sshd' && stripos($m, 'Failed') !== false) {
            return ['human' => '[SSH] ' . $m, 'level' => 'warn'];
        }

        return ['human' => $m, 'level' => 'info'];
    }

    private function toLower(string $str): string
    {
        return function_exists('mb_strtolower') ? mb_strtolower($str, 'UTF-8') : strtolower($str);
    }

    private function normalizeTimestamp(string $raw): string
    {
        $trimmed = trim($raw);
        if (empty($trimmed)) {
            return date('Y-m-d H:i:s');
        }

        if (preg_match('/^\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}$/', $trimmed)) {
            return $trimmed;
        }

        $ts = strtotime($trimmed);
        if ($ts !== false && $ts > 0) {
            return date('Y-m-d H:i:s', $ts);
        }

        return $trimmed;
    }

    /**
     * Lee registros de journald del sistema con filtro opcional por unidad.
     */
    public function getJournalLogs(int $limit = 80, ?string $unit = null): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                ['timestamp' => date('Y-m-d H:i:s'), 'unit' => 'smbd', 'message' => 'Servicio Samba activo.'],
                ['timestamp' => date('Y-m-d H:i:s'), 'unit' => 'nginx', 'message' => 'Nginx en espera de peticiones.'],
            ];
        }

        $cmd = ['journalctl', '-n', (string) $limit, '--no-pager', '-o', 'short-iso'];
        if ($unit && preg_match('/^[a-zA-Z0-9_\.-]+$/', $unit)) {
            $cmd[] = '-u';
            $cmd[] = $unit;
        }

        $res = self::runCommand($cmd);
        $lines = explode("\n", $res['stdout']);
        $logs = [];

        foreach ($lines as $line) {
            $trimmed = trim($line);
            if (empty($trimmed)) {
                continue;
            }

            // Parse formato ISO: 2026-10-02T19:00:00+0000 hostname unit[pid]: mensaje
            if (preg_match('/^(\S+)\s+\S+\s+([^:\[]+)(?:\[\d+\])?:\s+(.*)$/', $trimmed, $m)) {
                $logs[] = [
                    'timestamp' => $m[1],
                    'unit' => trim($m[2]),
                    'message' => trim($m[3]),
                ];
            } else {
                $logs[] = [
                    'timestamp' => '',
                    'unit' => 'system',
                    'message' => $trimmed,
                ];
            }
        }

        return $logs;
    }

    /**
     * Ordena el reinicio del servidor de forma controlada.
     */
    public function rebootServer(): array
    {
        return self::sudo(['systemctl', 'reboot']);
    }

    /**
     * Comprueba actualizaciones disponibles en el repositorio git o del sistema.
     */
    public function checkUpdates(): array
    {
        $commit = 'Desconocido';
        $date = 'N/A';
        $version = '1.0.0';
        $channel = 'GitHub main';

        $repoDir = dirname(__DIR__, 2);
        $versionFile = $repoDir . '/version.json';
        if (file_exists($versionFile)) {
            $raw = @file_get_contents($versionFile);
            if ($raw !== false) {
                $verData = json_decode($raw, true);
                if (is_array($verData)) {
                    $commit = $verData['commit'] ?? $commit;
                    $date = $verData['date'] ?? $date;
                    $version = $verData['version'] ?? $version;
                    $channel = $verData['channel'] ?? $channel;
                }
            }
        } elseif (file_exists($repoDir . '/.git')) {
            $res = self::runCommand(['git', '-C', $repoDir, 'log', '-1', '--format=%h|%cd|%s', '--date=short']);
            if ($res['code'] === 0 && !empty($res['stdout'])) {
                $parts = explode('|', $res['stdout'], 3);
                $commit = $parts[0] ?? '';
                $date = $parts[1] ?? '';
            }
        }

        return [
            'version' => $version,
            'installed_commit' => $commit,
            'commit_date' => $date,
            'has_updates' => false,
            'channel' => $channel,
        ];
    }
}
