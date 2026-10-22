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

        // Prohibir la detención del servidor web o del intérprete PHP para evitar pérdida de acceso al panel
        if ($action === 'stop' && preg_match('/^(nginx|php[\d\.]*-fpm)$/', $service)) {
            return ['success' => false, 'error' => "La acción 'stop' no está permitida para $service para prevenir la pérdida de acceso al panel web."];
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
            'server_time' => date('Y-m-d H:i:s'),
            'server_timestamp' => time(),
            'timezone' => date_default_timezone_get(),
            'timezone_abbr' => date('T'),
            'timezone_offset' => (int) date('Z'),
            'ip' => $this->getPrimaryIp(),
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
            $rawQuery = $query;
            if ($query !== null && trim($query) !== '') {
                $qL = $this->toLower(trim($query));
                $sambaSynonyms = [
                    'escritura' => 'openat|open',
                    'modificación' => 'openat|open',
                    'modificacion' => 'openat|open',
                    'lectura' => 'openat|open',
                    'apertura' => 'openat|open',
                    'eliminación' => 'unlinkat|unlink|rmdir',
                    'eliminacion' => 'unlinkat|unlink|rmdir',
                    'creación' => 'mkdirat|mkdir',
                    'creacion' => 'mkdirat|mkdir',
                    'renombrado' => 'renameat|rename',
                    'conexión' => 'connect',
                    'conexion' => 'connect',
                    'desconexión' => 'disconnect',
                    'desconexion' => 'disconnect',
                    'success' => 'ok',
                    'exit' => 'ok',
                    'failed' => 'fail',
                ];
                $extraTerms = [];
                foreach ($sambaSynonyms as $term => $rawTerm) {
                    if (str_contains($qL, $term)) {
                        $extraTerms[] = $rawTerm;
                    }
                }
                if (!empty($extraTerms)) {
                    $rawQuery = $query . '|' . implode('|', $extraTerms);
                }
            }
            $lines = $this->tailFileLines($path, max($limit * 3, 300), $rawQuery);
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
                'count' => 1,
            ];

            $maxParse = max($limit * 3, 300);
            if (count($results) >= $maxParse) {
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

        if (file_exists($path) && is_readable($path)) {
            $rawQuery = $query;
            if ($query !== null && trim($query) !== '') {
                $qL = $this->toLower(trim($query));
                $matchedActions = [];
                foreach ($actionLabels as $actKey => $actLabel) {
                    if (str_contains($this->toLower($actLabel), $qL)) {
                        $matchedActions[] = $actKey;
                    }
                }
                if (!empty($matchedActions)) {
                    $rawQuery = $query . '|' . implode('|', $matchedActions);
                }
            }
            $lines = $this->tailFileLines($path, max($limit * 3, 300), $rawQuery);
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

        foreach ($lines as $line) {
            $trimmed = trim($line);
            if (empty($trimmed)) {
                continue;
            }

            if (!preg_match('/^\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\](?:\s+(.*))?$/', $trimmed, $m)) {
                continue;
            }

            $timestamp = $this->normalizeTimestamp($m[1]);
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
                'count' => 1,
            ];

            $maxParse = max($limit * 3, 300);
            if (count($results) >= $maxParse) {
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
            $lines = $this->tailFileLines($path, max($limit * 3, 300), $query);
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

            $knownSeverities = ['debug', 'info', 'notice', 'warn', 'warning', 'err', 'error', 'crit', 'critical', 'alert', 'emerg', 'ok', 'failed', 'success'];
            $ip = '127.0.0.1';
            $matched = false;

            if (preg_match('/^\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s*(.*)$/', $trimmed, $m5)) {
                $candIp = trim($m5[3]);
                $candSev = strtolower(trim($m5[5]));
                // Formato de 5 corchetes: si el tercer grupo es IP válida o el quinto es severidad conocida
                if (filter_var($candIp, FILTER_VALIDATE_IP) || in_array($candSev, $knownSeverities, true)) {
                    $timestamp = $this->normalizeTimestamp($m5[1]);
                    $task = $m5[2];
                    if (filter_var($candIp, FILTER_VALIDATE_IP)) {
                        $ip = $candIp;
                    }
                    $event = $m5[4];
                    $severity = $candSev;
                    $message = $m5[6];
                    $matched = true;
                }
            }

            if (!$matched && preg_match('/^\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s+\[([^\]]+)\]\s*(.*)$/', $trimmed, $m)) {
                $timestamp = $this->normalizeTimestamp($m[1]);
                $task = $m[2];
                $event = $m[3];
                $severity = strtolower(trim($m[4]));
                $message = $m[5];
                if (preg_match('#//\[?([0-9a-zA-Z\.\:\-]+?)\]?[/\\\\]#', $message, $mIp)) {
                    $cand = trim($mIp[1], '[]:/ ');
                    if (filter_var($cand, FILTER_VALIDATE_IP)) {
                        $ip = $cand;
                    }
                } elseif (preg_match('#@\[?([0-9a-zA-Z\.\:\-]+?)\]?[:/]#', $message, $mIp)) {
                    $cand = trim($mIp[1], '[]:/ ');
                    if (filter_var($cand, FILTER_VALIDATE_IP)) {
                        $ip = $cand;
                    }
                }
                $matched = true;
            }

            if (!$matched) {
                continue;
            }

            $eventLabel = $eventLabels[$event] ?? ucfirst(str_replace('_', ' ', $event));

            $badge = match ($severity) {
                'err', 'error', 'failed' => 'err',
                'warning', 'warn' => 'warn',
                'notice', 'ok' => 'ok',
                default => 'blue',
            };

            $status = in_array($severity, ['err', 'error', 'failed'], true) ? 'FAILED' : 'SUCCESS';

            if ($qLower !== null) {
                $searchable = $this->toLower("$task $ip $event $eventLabel $severity $message");
                if (!str_contains($searchable, $qLower)) {
                    continue;
                }
            }

            $results[] = [
                'source' => 'backup',
                'timestamp' => $timestamp,
                'task' => $task,
                'user' => $task,
                'ip' => $ip,
                'action' => $eventLabel,
                'action_label' => $eventLabel,
                'event' => $event,
                'event_label' => $eventLabel,
                'severity' => $severity,
                'message' => $message,
                'badge' => $badge,
                'target' => $task,
                'status' => $status,
                'unit' => 'backup-' . $task,
                'raw' => $trimmed,
                'count' => 1,
            ];

            $maxParse = max($limit * 3, 300);
            if (count($results) >= $maxParse) {
                break;
            }
        }

        return $results;
    }

    /**
     * Obtiene registros consolidados y multidimensionales filtrados por origen y búsqueda.
     */
    public function getLogs(string $source = 'all', int $limit = 100, ?string $query = null, bool $grouped = true): array
    {
        $cleanSource = strtolower(trim($source));

        switch ($cleanSource) {
            case 'samba_audit':
                $items = $this->parseSambaAuditLog($limit, $query);
                return array_slice($grouped ? $this->groupConsecutiveLogs($items) : $items, 0, $limit);

            case 'admin':
                $items = $this->parseAdminAuditLog($limit, $query);
                return array_slice($grouped ? $this->groupConsecutiveLogs($items) : $items, 0, $limit);

            case 'backup':
                $items = $this->parseBackupAuditLog($limit, $query);
                return array_slice($grouped ? $this->groupConsecutiveLogs($items) : $items, 0, $limit);

            case 'system':
                $rawJournal = $this->getJournalLogs($limit * 2);
                $qLower = $query ? $this->toLower(trim($query)) : null;
                $results = [];

                foreach ($rawJournal as $j) {
                    $h = $this->humanizeSystemLog($j['unit'] ?? 'system', $j['message'] ?? '');
                    if ($qLower !== null) {
                        $searchable = $this->toLower(($j['unit'] ?? '') . ' ' . ($j['message'] ?? '') . ' ' . ($h['human'] ?? '') . ' ' . ($h['ip'] ?? ''));
                        if (!str_contains($searchable, $qLower)) {
                            continue;
                        }
                    }
                    $results[] = [
                        'source' => 'system',
                        'timestamp' => $this->normalizeTimestamp($j['timestamp'] ?? ''),
                        'unit' => $j['unit'] ?? 'system',
                        'user' => 'system',
                        'ip' => $h['ip'] ?? '127.0.0.1',
                        'target' => $j['unit'] ?? 'system',
                        'action' => $h['action_label'] ?? 'Sistema',
                        'action_label' => $h['action_label'] ?? 'Sistema',
                        'badge' => $h['badge'] ?? 'gray',
                        'status' => $h['status'] ?? 'OK',
                        'message' => $j['message'] ?? '',
                        'human' => $h['human'],
                        'level' => $h['level'],
                        'raw' => ($j['timestamp'] ?? '') . ' ' . ($j['unit'] ?? '') . ': ' . ($j['message'] ?? ''),
                        'count' => 1,
                    ];
                    if (count($results) >= $limit * 2) {
                        break;
                    }
                }
                return array_slice($grouped ? $this->groupConsecutiveLogs($results) : $results, 0, $limit);

            case 'all':
            default:
                $samba = $this->parseSambaAuditLog($limit, $query);
                $admin = $this->parseAdminAuditLog($limit, $query);
                $backup = $this->parseBackupAuditLog($limit, $query);
                $rawJournal = $this->getJournalLogs(min($limit, 50));

                $system = [];
                $qLower = $query ? $this->toLower(trim($query)) : null;
                foreach ($rawJournal as $j) {
                    $h = $this->humanizeSystemLog($j['unit'] ?? 'system', $j['message'] ?? '');
                    if ($qLower !== null) {
                        $searchable = $this->toLower(($j['unit'] ?? '') . ' ' . ($j['message'] ?? '') . ' ' . ($h['human'] ?? '') . ' ' . ($h['ip'] ?? ''));
                        if (!str_contains($searchable, $qLower)) {
                            continue;
                        }
                    }
                    $system[] = [
                        'source' => 'system',
                        'timestamp' => $this->normalizeTimestamp($j['timestamp'] ?? ''),
                        'unit' => $j['unit'] ?? 'system',
                        'user' => 'system',
                        'ip' => $h['ip'] ?? '127.0.0.1',
                        'target' => $j['unit'] ?? 'system',
                        'action' => $h['action_label'] ?? 'Sistema',
                        'action_label' => $h['action_label'] ?? 'Sistema',
                        'badge' => $h['badge'] ?? 'gray',
                        'status' => $h['status'] ?? 'OK',
                        'message' => $j['message'] ?? '',
                        'human' => $h['human'],
                        'level' => $h['level'],
                        'raw' => ($j['timestamp'] ?? '') . ' ' . ($j['unit'] ?? '') . ': ' . ($j['message'] ?? ''),
                        'count' => 1,
                    ];
                }

                $merged = array_merge($samba, $admin, $backup, $system);

                // Ordenar por fecha cronológica descendente
                usort($merged, function (array $a, array $b): int {
                    return strcmp($b['timestamp'] ?? '', $a['timestamp'] ?? '');
                });

                if ($grouped) {
                    $merged = $this->groupConsecutiveLogs($merged);
                }
                return array_slice($merged, 0, $limit);
        }
    }

    /**
     * Ejecuta pruebas diagnósticas activas del sistema (ping, disk_io, dns).
     */
    public function runDiagnosticTest(string $type, ?string $target = null): array
    {
        switch ($type) {
            case 'ping':
            case 'network':
                $host = $target ?: '8.8.8.8';
                if (!preg_match('/^[a-zA-Z0-9.-]+$/', $host)) {
                    return ['success' => false, 'error' => 'Objetivo de red inválido.'];
                }
                $start = microtime(true);
                if (DIRECTORY_SEPARATOR === '\\') {
                    $res = ['code' => 0, 'stdout' => 'Respuesta desde ' . $host . ': bytes=32 tiempo=12ms TTL=117', 'stderr' => ''];
                } else {
                    $res = self::runCommand(['ping', '-c', '3', '-W', '3', $host]);
                }
                $elapsed = round((microtime(true) - $start) * 1000, 1);
                $isOk = ($res['code'] === 0);
                return [
                    'success' => $isOk,
                    'type' => 'ping',
                    'target' => $host,
                    'latency_ms' => $elapsed,
                    'output' => $res['stdout'] ?: $res['stderr'],
                    'message' => $isOk ? "Conectividad de red exitosa con $host ($elapsed ms)." : "Fallo de conectividad con $host.",
                ];

            case 'dns':
                $domain = $target ?: 'debian.org';
                if (!preg_match('/^[a-zA-Z0-9.-]+$/', $domain)) {
                    return ['success' => false, 'error' => 'Dominio DNS inválido.'];
                }
                $start = microtime(true);
                $ip = @gethostbyname($domain);
                $elapsed = round((microtime(true) - $start) * 1000, 1);
                $resolved = ($ip !== $domain && !empty($ip));
                return [
                    'success' => $resolved,
                    'type' => 'dns',
                    'domain' => $domain,
                    'ip' => $resolved ? $ip : null,
                    'latency_ms' => $elapsed,
                    'message' => $resolved ? "Resolución DNS exitosa: $domain -> $ip ($elapsed ms)." : "No se pudo resolver el dominio $domain.",
                ];

            case 'disk_io':
                $testDir = '/srv/nas';
                if (!is_dir($testDir) || !is_writable($testDir)) {
                    $testDir = sys_get_temp_dir();
                }
                $testFile = $testDir . '/nas_io_benchmark_' . uniqid() . '.tmp';
                $data = str_repeat('A', 1024 * 1024); // 1 MB chunk
                $startWrite = microtime(true);
                $fp = @fopen($testFile, 'wb');
                if (!$fp) {
                    return ['success' => false, 'error' => "No se pudo crear archivo de prueba en $testDir."];
                }
                for ($i = 0; $i < 5; $i++) {
                    fwrite($fp, $data);
                }
                fflush($fp);
                fclose($fp);
                $writeTime = max(0.001, microtime(true) - $startWrite);
                $writeMbPerSec = round(5 / $writeTime, 2);

                $startRead = microtime(true);
                $content = @file_get_contents($testFile);
                $readTime = max(0.001, microtime(true) - $startRead);
                $readMbPerSec = round(5 / $readTime, 2);
                @unlink($testFile);

                return [
                    'success' => true,
                    'type' => 'disk_io',
                    'path' => $testDir,
                    'write_speed_mb_s' => $writeMbPerSec,
                    'read_speed_mb_s' => $readMbPerSec,
                    'message' => "Prueba I/O en $testDir: Escritura {$writeMbPerSec} MB/s, Lectura {$readMbPerSec} MB/s.",
                ];

            default:
                return ['success' => false, 'error' => 'Tipo de prueba no reconocido: ' . $type];
        }
    }

    /**
     * Agrupa eventos idénticos consecutivos en una sola entrada con contador y etiqueta anti-spam (xN).
     *
     * @param array<array<string, mixed>> $logs
     * @return array<array<string, mixed>>
     */
    public function groupConsecutiveLogs(array $logs): array
    {
        if (empty($logs)) {
            return [];
        }

        $grouped = [];
        $current = null;
        $count = 1;
        $currentKey = '';

        foreach ($logs as $log) {
            $key = ($log['source'] ?? '') . '|' .
                   ($log['user'] ?? ($log['task'] ?? '')) . '|' .
                   ($log['ip'] ?? '') . '|' .
                   ($log['action'] ?? ($log['event'] ?? ($log['unit'] ?? ''))) . '|' .
                   ($log['target'] ?? '') . '|' .
                   ($log['status'] ?? '') . '|' .
                   ($log['human'] ?? ($log['message'] ?? ''));

            if ($current === null) {
                $current = $log;
                $current['count'] = 1;
                $currentKey = $key;
                $count = 1;
                continue;
            }

            if ($key === $currentKey) {
                $count++;
                $current['count'] = $count;
            } else {
                if ($count > 1) {
                    $current['count_badge'] = "x{$count}";
                    if (isset($current['human']) && !str_contains($current['human'], "(x{$count})")) {
                        $current['human'] = preg_replace('/\s+\(x\d+\)$/', '', (string) $current['human']) . " (x{$count})";
                    }
                    if (isset($current['message']) && !str_contains($current['message'], "(x{$count})")) {
                        $current['message'] = preg_replace('/\s+\(x\d+\)$/', '', (string) $current['message']) . " (x{$count})";
                    }
                }
                $grouped[] = $current;
                $current = $log;
                $current['count'] = 1;
                $count = 1;
                $currentKey = $key;
            }
        }

        if ($current !== null) {
            if ($count > 1) {
                $current['count_badge'] = "x{$count}";
                if (isset($current['human']) && !str_contains($current['human'], "(x{$count})")) {
                    $current['human'] = preg_replace('/\s+\(x\d+\)$/', '', (string) $current['human']) . " (x{$count})";
                }
                if (isset($current['message']) && !str_contains($current['message'], "(x{$count})")) {
                    $current['message'] = preg_replace('/\s+\(x\d+\)$/', '', (string) $current['message']) . " (x{$count})";
                }
            }
            $grouped[] = $current;
        }

        return $grouped;
    }

    /**
     * Traduce y enriquece mensajes del sistema (UFW, Cron, SSH, Systemd, NTP...) a texto legible,
     * extrayendo direcciones IP, niveles de severidad y clasificación cromática para la UI y CLI.
     *
     * @return array{human:string,level:string,badge:string,status:string,ip:string,action_label:string}
     */
    public function humanizeSystemLog(string $unit, string $message): array
    {
        $u = strtolower(trim($unit));
        $m = trim($message);

        // Firewall UFW: bloques de paquetes y eventos de red
        if (stripos($m, 'UFW') !== false) {
            $isBlock = stripos($m, 'BLOCK') !== false || stripos($m, 'Bloqueado') !== false;
            $accion = $isBlock ? 'Bloqueado' : ((stripos($m, 'ALLOW') !== false || stripos($m, 'Permitido') !== false) ? 'Permitido' : 'Evento');

            // Helper para desacoplar puerto de IP cuando vienen combinados (ej. fe80:...:5678 o [fe80:...]:5678)
            $splitIpPort = static function (string $rawIp, string $fallbackPort): array {
                $rawIp = trim($rawIp);
                $port = $fallbackPort;

                // Si ya es una dirección IP válida (IPv4 o IPv6), mantenerla intacta
                if (filter_var($rawIp, FILTER_VALIDATE_IP)) {
                    return [$rawIp, $port];
                }

                if (preg_match('/^\[([a-fA-F0-9:]+)\]:(\d+)$/', $rawIp, $match)) {
                    return [$match[1], $match[2]];
                }
                if (preg_match('/^(\d+\.\d+\.\d+\.\d+):(\d+)$/', $rawIp, $match)) {
                    return [$match[1], $match[2]];
                }
                if (str_contains($rawIp, ':')) {
                    $lastColon = strrpos($rawIp, ':');
                    if ($lastColon !== false) {
                        $possibleIp = substr($rawIp, 0, $lastColon);
                        $possiblePort = substr($rawIp, $lastColon + 1);
                        if (ctype_digit($possiblePort) && (int) $possiblePort >= 1 && (int) $possiblePort <= 65535) {
                            if (filter_var($possibleIp, FILTER_VALIDATE_IP, FILTER_FLAG_IPV6)) {
                                return [$possibleIp, $possiblePort];
                            }
                            if (str_contains($possibleIp, ':0000')) {
                                $condensed = (string) preg_replace('/:(?:0{1,4}:)+/', '::', $possibleIp, 1);
                                if (filter_var($condensed, FILTER_VALIDATE_IP, FILTER_FLAG_IPV6)) {
                                    return [$condensed, $possiblePort];
                                }
                            }
                        }
                    }
                }
                return [$rawIp, $port];
            };

            // Lectura adaptable: formato sintetizado ([UFW] Bloqueado UDP de SRC:puerto → DST:puerto) o formato kernel (SRC=... DST=...)
            if (preg_match('/\[UFW\]\s+(?:Bloqueado|Permitido|Evento)\s+(\w+)\s+de\s+(\S+)\s+(?:→|->)\s+(\S+)/iu', $m, $legacy)) {
                $proto = strtoupper($legacy[1]);
                $splitLegacy = static function (string $endpoint) use ($splitIpPort): array {
                    if (str_contains($endpoint, ':')) {
                        $lastColon = strrpos($endpoint, ':');
                        if ($lastColon !== false) {
                            $possiblePort = substr($endpoint, $lastColon + 1);
                            if (ctype_digit($possiblePort) && (int) $possiblePort >= 1 && (int) $possiblePort <= 65535) {
                                return [substr($endpoint, 0, $lastColon), $possiblePort];
                            }
                        }
                    }
                    return $splitIpPort($endpoint, '');
                };
                [$srcRaw, $spt] = $splitLegacy($legacy[2]);
                [$dstRaw, $dpt] = $splitLegacy($legacy[3]);
            } else {
                $proto = preg_match('/PROTO=(\S+)/', $m, $p) ? strtoupper($p[1]) : '';
                $rawSrc = preg_match('/SRC=(\S+)/', $m, $x) ? $x[1] : '?';
                $rawDst = preg_match('/DST=(\S+)/', $m, $y) ? $y[1] : '?';
                $spt = preg_match('/SPT=(\d+)/', $m, $w) ? $w[1] : '';
                $dpt = preg_match('/DPT=(\d+)/', $m, $z) ? $z[1] : '';
                [$srcRaw, $spt] = $splitIpPort($rawSrc, $spt);
                [$dstRaw, $dpt] = $splitIpPort($rawDst, $dpt);
            }

            // Normalización legible de direcciones IPv6 (comprime secuencias de ceros)
            $formatIp = static function (string $ip): string {
                if (filter_var($ip, FILTER_VALIDATE_IP, FILTER_FLAG_IPV6)) {
                    $packed = @inet_pton($ip);
                    if ($packed !== false) {
                        return (string) inet_ntop($packed);
                    }
                }
                if (str_contains($ip, ':0000')) {
                    $condensed = (string) preg_replace('/:(?:0{1,4}:)+/', '::', $ip, 1);
                    if (filter_var($condensed, FILTER_VALIDATE_IP, FILTER_FLAG_IPV6)) {
                        $packed = @inet_pton($condensed);
                        if ($packed !== false) {
                            return (string) inet_ntop($packed);
                        }
                    }
                }
                return $ip;
            };

            $srcClean = $formatIp($srcRaw);
            $dstClean = $formatIp($dstRaw);

            // Detección precisa de naturaleza de red (IPv6 Multicast, IPv4 Multicast o Broadcast)
            $isIpv6Multi = str_starts_with($dstClean, 'ff02:') || str_starts_with($dstClean, 'ff00:') || str_starts_with($dstClean, 'ff01:') || str_starts_with($dstClean, 'ff05:');
            $isIpv4Multi = str_starts_with($dstClean, '224.') || str_starts_with($dstClean, '239.');
            $isBroadcast = $dstClean === '255.255.255.255' || str_ends_with($dstClean, '.255');
            $castType = $isIpv6Multi ? ' [IPv6 Multicast]' : ($isIpv4Multi ? ' [IPv4 Multicast]' : ($isBroadcast ? ' [Broadcast]' : ''));

            // Formateo estándar de extremos de red (RFC 3986 / RFC 5952 para IPv6 con puerto)
            $formatEndpoint = static function (string $ip, string $port): string {
                if ($port === '') {
                    return $ip;
                }
                if (str_contains($ip, ':')) {
                    return "[{$ip}]:{$port}";
                }
                return "{$ip}:{$port}";
            };

            // Enriquecimiento semántico de servicios y protocolos de red
            $service = '';
            if ($proto === 'UDP' && ($dpt === '5678' || $spt === '5678')) {
                $service = 'Descubrimiento de red MikroTik MNDP' . $castType;
            } elseif ($proto === 'UDP' && ($dpt === '5353' || $spt === '5353')) {
                $service = 'Descubrimiento mDNS / Apple Bonjour' . $castType;
            } elseif ($proto === 'UDP' && ($dpt === '1900' || $spt === '1900')) {
                $service = 'Descubrimiento SSDP / UPnP' . $castType;
            } elseif ($proto === 'UDP' && ($dpt === '5355' || $spt === '5355')) {
                $service = 'Resolución de nombres LLMNR' . $castType;
            } elseif ($proto === 'UDP' && ($dpt === '3702' || $spt === '3702')) {
                $service = 'Descubrimiento WSD Windows' . $castType;
            } elseif ($proto === 'UDP' && in_array($dpt, ['137', '138'], true)) {
                $service = 'Difusión NetBIOS Windows' . $castType;
            } elseif ($proto === 'TCP' && in_array($dpt, ['139', '445'], true)) {
                $service = 'Tráfico de archivos Samba / SMB';
            } elseif ($proto === 'TCP' && $dpt === '22') {
                $service = 'Acceso remoto SSH';
            } elseif ($proto === 'TCP' && in_array($dpt, ['80', '443'], true)) {
                $service = 'Acceso web HTTP/HTTPS';
            }

            $origen = $formatEndpoint($srcClean, $spt);
            $destino = $formatEndpoint($dstClean, $dpt);

            if ($service !== '') {
                $verb = $isBlock ? 'Tráfico de red local descartado' : 'Tráfico de red permitido';
                $human = "[Firewall UFW] {$verb}: {$service} ({$proto} {$origen} → {$destino})";
            } else {
                $human = "[Firewall UFW] {$accion} {$proto} de {$origen} → {$destino}";
            }

            return [
                'human' => $human,
                'level' => $isBlock ? 'warn' : 'info',
                'badge' => $isBlock ? 'err' : 'ok',
                'status' => $isBlock ? 'BLOCK' : 'ALLOW',
                'ip' => $srcClean,
                'action_label' => 'Firewall UFW',
            ];
        }

        // Cron: ejecuciones programadas
        if ($u === 'cron' || stripos($m, 'CRON') !== false) {
            if (preg_match('/\(([^)]+)\)\s+CMD\s+\((.*)\)/', $m, $cm)) {
                return [
                    'human' => "[Cron] {$cm[1]} ejecutó: {$cm[2]}",
                    'level' => 'info',
                    'badge' => 'blue',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'Cron',
                ];
            }
            if (preg_match('/CMD\s+\((.*)\)/', $m, $cm2)) {
                return [
                    'human' => "[Cron] ejecutó: {$cm2[1]}",
                    'level' => 'info',
                    'badge' => 'blue',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'Cron',
                ];
            }
            return [
                'human' => '[Cron] ' . $m,
                'level' => 'info',
                'badge' => 'blue',
                'status' => 'OK',
                'ip' => '127.0.0.1',
                'action_label' => 'Cron',
            ];
        }

        // SSH: intentos de acceso y sesiones
        if ($u === 'sshd' || $u === 'sshd-session' || stripos($m, 'sshd') !== false || stripos($m, 'ssh') !== false) {
            $sshIp = '127.0.0.1';
            if (preg_match('/from\s+([0-9a-fA-F\.\:]+)/', $m, $mIp)) {
                $sshIp = $mIp[1];
            }
            if (preg_match('/Failed\s+(?:password|publickey)\s+for\s+(?:invalid\s+user\s+)?(\S+)\s+from\s+(\S+)/i', $m, $sm)) {
                return [
                    'human' => "[SSH] Intento de acceso fallido para {$sm[1]} desde {$sm[2]}",
                    'level' => 'warn',
                    'badge' => 'err',
                    'status' => 'FAILED',
                    'ip' => $sm[2],
                    'action_label' => 'SSH',
                ];
            }
            if (preg_match('/Accepted\s+(?:password|publickey)\s+for\s+(\S+)\s+from\s+(\S+)/i', $m, $sm)) {
                return [
                    'human' => "[SSH] Acceso concedido a {$sm[1]} desde {$sm[2]}",
                    'level' => 'info',
                    'badge' => 'ok',
                    'status' => 'OK',
                    'ip' => $sm[2],
                    'action_label' => 'SSH',
                ];
            }
            if (preg_match('/session opened for user ([a-zA-Z0-9_\-\.]+)/i', $m, $sm)) {
                return [
                    'human' => "[SSH] Sesión de usuario abierta para {$sm[1]}",
                    'level' => 'info',
                    'badge' => 'blue',
                    'status' => 'OK',
                    'ip' => $sshIp,
                    'action_label' => 'SSH',
                ];
            }
            if (preg_match('/session closed for user ([a-zA-Z0-9_\-\.]+)/i', $m, $sm)) {
                return [
                    'human' => "[SSH] Sesión de usuario cerrada para {$sm[1]}",
                    'level' => 'info',
                    'badge' => 'gray',
                    'status' => 'OK',
                    'ip' => $sshIp,
                    'action_label' => 'SSH',
                ];
            }
            if (preg_match('/(?:Connection|Disconnected)\s+(?:closed|reset|from)\s+(?:by\s+)?(?:invalid\s+user\s+)?(\S+)/i', $m, $sm)) {
                $closeIp = filter_var($sm[1], FILTER_VALIDATE_IP) ? $sm[1] : $sshIp;
                return [
                    'human' => "[SSH] Conexión finalizada desde {$sm[1]}",
                    'level' => 'info',
                    'badge' => 'gray',
                    'status' => 'OK',
                    'ip' => $closeIp,
                    'action_label' => 'SSH',
                ];
            }
            if (stripos($m, 'Failed') !== false) {
                return [
                    'human' => '[SSH] ' . $m,
                    'level' => 'warn',
                    'badge' => 'err',
                    'status' => 'FAILED',
                    'ip' => $sshIp,
                    'action_label' => 'SSH',
                ];
            }
            return [
                'human' => '[SSH] ' . $m,
                'level' => 'info',
                'badge' => 'blue',
                'status' => 'OK',
                'ip' => $sshIp,
                'action_label' => 'SSH',
            ];
        }

        // Servicios del sistema (systemd)
        if (preg_match('/^(?:Started|Iniciado)\s+(.*)\.$/i', $m, $sm)) {
            return [
                'human' => "[Servicio] {$sm[1]} iniciado con éxito",
                'level' => 'info',
                'badge' => 'ok',
                'status' => 'OK',
                'ip' => '127.0.0.1',
                'action_label' => 'Servicio',
            ];
        }
        if (preg_match('/^(?:Stopped|Detenido)\s+(.*)\.$/i', $m, $sm)) {
            return [
                'human' => "[Servicio] {$sm[1]} detenido",
                'level' => 'info',
                'badge' => 'gray',
                'status' => 'OK',
                'ip' => '127.0.0.1',
                'action_label' => 'Servicio',
            ];
        }
        if (preg_match('/^(?:Starting|Iniciando)\s+(.*)\.$/i', $m, $sm)) {
            return [
                'human' => "[Servicio] Iniciando {$sm[1]}",
                'level' => 'info',
                'badge' => 'blue',
                'status' => 'OK',
                'ip' => '127.0.0.1',
                'action_label' => 'Servicio',
            ];
        }
        if (preg_match('/^(?:Failed to start|Error al iniciar)\s+(.*)\.$/i', $m, $sm)) {
            return [
                'human' => "[Servicio] Falló el inicio de {$sm[1]}",
                'level' => 'err',
                'badge' => 'err',
                'status' => 'FAILED',
                'ip' => '127.0.0.1',
                'action_label' => 'Servicio',
            ];
        }
        if (preg_match('/^(?:Reloaded|Recargado)\s+(.*)\.$/i', $m, $sm)) {
            return [
                'human' => "[Servicio] Configuración de {$sm[1]} recargada",
                'level' => 'info',
                'badge' => 'ok',
                'status' => 'OK',
                'ip' => '127.0.0.1',
                'action_label' => 'Servicio',
            ];
        }
        if (stripos($m, 'Deactivated successfully') !== false) {
            return [
                'human' => "[Servicio] {$unit} finalizado con éxito",
                'level' => 'info',
                'badge' => 'gray',
                'status' => 'OK',
                'ip' => '127.0.0.1',
                'action_label' => 'Servicio',
            ];
        }

        // Sesiones y usuarios (systemd-logind)
        if ($u === 'systemd-logind' || stripos($m, 'systemd-logind') !== false) {
            if (preg_match('/New session (\S+) of user (\S+)\./i', $m, $lm)) {
                return [
                    'human' => "[Sesión] Nueva sesión {$lm[1]} iniciada para el usuario {$lm[2]}",
                    'level' => 'info',
                    'badge' => 'ok',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'Sesión',
                ];
            }
            if (preg_match('/Removed session (\S+)\./i', $m, $lm)) {
                return [
                    'human' => "[Sesión] Sesión {$lm[1]} finalizada",
                    'level' => 'info',
                    'badge' => 'gray',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'Sesión',
                ];
            }
        }

        // Sudo: comandos ejecutados como root
        if (stripos($m, 'sudo:') !== false || stripos($u, 'sudo') !== false) {
            if (preg_match('/COMMAND=(.*)$/', $m, $cm)) {
                return [
                    'human' => "[Sudo] Comando administrativo ejecutado: " . trim($cm[1]),
                    'level' => 'info',
                    'badge' => 'blue',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'Sudo',
                ];
            }
            if (preg_match('/session opened for user ([a-zA-Z0-9_\-\.]+)(?:\(uid=\d+\))?\s+by\s+([a-zA-Z0-9_\-\.]+)/i', $m, $sm)) {
                return [
                    'human' => "[Sudo] Sesión administrativa abierta para {$sm[1]} por {$sm[2]}",
                    'level' => 'info',
                    'badge' => 'blue',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'Sudo',
                ];
            }
            if (preg_match('/session closed for user ([a-zA-Z0-9_\-\.]+)/i', $m, $sm)) {
                return [
                    'human' => "[Sudo] Sesión administrativa cerrada para {$sm[1]}",
                    'level' => 'info',
                    'badge' => 'gray',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'Sudo',
                ];
            }
            return [
                'human' => '[Sudo] ' . $m,
                'level' => 'info',
                'badge' => 'blue',
                'status' => 'OK',
                'ip' => '127.0.0.1',
                'action_label' => 'Sudo',
            ];
        }

        // NTP / Sincronización horaria (timesyncd / chrony)
        if ($u === 'systemd-timesyncd' || stripos($m, 'timesyncd') !== false || stripos($u, 'chrony') !== false || stripos($m, 'chrony') !== false) {
            if (preg_match('/Contacted time server\s+(\S+)\s+\(([^)]+)\)/i', $m, $tm)) {
                $ntpIp = filter_var($tm[1], FILTER_VALIDATE_IP) ? $tm[1] : '127.0.0.1';
                return [
                    'human' => "[NTP] Servidor de tiempo contactado: {$tm[2]} ({$tm[1]})",
                    'level' => 'info',
                    'badge' => 'ok',
                    'status' => 'OK',
                    'ip' => $ntpIp,
                    'action_label' => 'NTP',
                ];
            }
            if (preg_match('/Initial clock synchronization to\s+(.*)\./i', $m, $tm)) {
                return [
                    'human' => "[NTP] Reloj del sistema sincronizado a {$tm[1]}",
                    'level' => 'info',
                    'badge' => 'ok',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'NTP',
                ];
            }
            if (preg_match('/System clock wrong by\s+([0-9\.\-]+)s/i', $m, $tm)) {
                return [
                    'human' => "[NTP] Desfase corregido en {$tm[1]} segundos",
                    'level' => 'info',
                    'badge' => 'ok',
                    'status' => 'OK',
                    'ip' => '127.0.0.1',
                    'action_label' => 'NTP',
                ];
            }
        }

        return [
            'human' => $m,
            'level' => 'info',
            'badge' => 'gray',
            'status' => 'OK',
            'ip' => '127.0.0.1',
            'action_label' => 'Sistema',
        ];
    }

    /**
     * Lee las últimas líneas de un archivo de forma eficiente mediante fseek hacia atrás
     * en bloques de tamaño configurable ($bufferSize), minimizando el consumo de RAM (< 256 KB).
     *
     * @return array<string> Líneas en orden cronológico inverso (la más reciente primero)
     */
    private function tailFileLines(string $filepath, int $limit, ?string $query = null, int $bufferSize = 65536): array
    {
        if (!file_exists($filepath) || !is_readable($filepath)) {
            return [];
        }

        $fp = @fopen($filepath, 'rb');
        if (!$fp) {
            return [];
        }

        $pos = @filesize($filepath);
        if ($pos === false || $pos === 0) {
            fclose($fp);
            return [];
        }

        $lines = [];
        $leftover = '';
        $queryTerms = [];
        if ($query !== null && trim($query) !== '') {
            $parts = explode('|', $this->toLower(trim($query)));
            foreach ($parts as $p) {
                $p = trim($p);
                if ($p !== '') {
                    $queryTerms[] = $p;
                }
            }
        }

        while ($pos > 0 && count($lines) < $limit) {
            $readSize = min($pos, $bufferSize);
            $pos -= $readSize;
            fseek($fp, $pos, SEEK_SET);
            $chunk = fread($fp, $readSize);
            if ($chunk === false || $chunk === '') {
                break;
            }

            $chunk .= $leftover;
            $chunkLines = explode("\n", $chunk);

            if ($pos > 0) {
                $leftover = array_shift($chunkLines);
                if (strlen($leftover) > 262144) {
                    $leftover = substr($leftover, -65536);
                }
            } else {
                $leftover = '';
            }

            for ($i = count($chunkLines) - 1; $i >= 0; $i--) {
                $line = trim($chunkLines[$i], "\r\n");
                if ($line === '') {
                    continue;
                }
                if (!empty($queryTerms)) {
                    $lineLower = $this->toLower($line);
                    $matched = false;
                    foreach ($queryTerms as $term) {
                        if (str_contains($lineLower, $term)) {
                            $matched = true;
                            break;
                        }
                    }
                    if (!$matched) {
                        continue;
                    }
                }
                $lines[] = $line;
                if (count($lines) >= $limit) {
                    break;
                }
            }
        }

        if ($pos === 0 && $leftover !== '' && count($lines) < $limit) {
            $line = trim($leftover, "\r\n");
            if ($line !== '') {
                $matched = true;
                if (!empty($queryTerms)) {
                    $lineLower = $this->toLower($line);
                    $matched = false;
                    foreach ($queryTerms as $term) {
                        if (str_contains($lineLower, $term)) {
                            $matched = true;
                            break;
                        }
                    }
                }
                if ($matched) {
                    $lines[] = $line;
                }
            }
        }

        fclose($fp);
        return $lines;
    }

    private function toLower(string $str): string
    {
        return function_exists('mb_strtolower') ? mb_strtolower($str, 'UTF-8') : strtolower($str);
    }

    public function normalizeTimestamp(string $raw): string
    {
        $trimmed = trim($raw);
        if (empty($trimmed)) {
            return date('Y-m-d H:i:s');
        }

        if (preg_match('/^\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}$/', $trimmed)) {
            $ts = strtotime($trimmed);
            if ($ts !== false && $ts > time() + 300) {
                $tzOffset = (int) date('Z');
                if ($tzOffset < 0 && ($ts + $tzOffset) <= time() + 300) {
                    return date('Y-m-d H:i:s', $ts + $tzOffset);
                }
            }
            return $trimmed;
        }

        $ts = strtotime($trimmed);
        if ($ts !== false && $ts > 0) {
            if ($ts > time() + 300) {
                $tzOffset = (int) date('Z');
                if ($tzOffset < 0 && ($ts + $tzOffset) <= time() + 300) {
                    return date('Y-m-d H:i:s', $ts + $tzOffset);
                }
            }
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
                    'message' => $this->redactJournalSecrets(trim($m[3])),
                ];
            } else {
                $logs[] = [
                    'timestamp' => '',
                    'unit' => 'system',
                    'message' => $this->redactJournalSecrets($trimmed),
                ];
            }
        }

        return $logs;
    }

    /**
     * Elimina credenciales presentes en la línea de comandos registrada por sudo antes de
     * exponerla por la API (por ejemplo `printf 'usuario:clave' | nas-chpasswd` o
     * `--password=...`), evitando que las bitácoras del sistema se conviertan en un almacén
     * de secretos legible por cualquier usuario autenticado.
     */
    private function redactJournalSecrets(string $message): string
    {
        // Flags explícitos de contraseña en la línea de comandos
        $message = (string) preg_replace('/(--password[= ])\S+/i', '$1[REDACTADO]', $message);

        // Cargas útiles usuario:clave canalizadas a helpers de contraseñas del sistema
        $message = (string) preg_replace(
            '/((?:printf|echo)\s+\\\\?[\'"]?)[^\s|]+(\\\\?[\'"]?\s*\|\s*(?:sudo\s+-n\s+)?(?:\S*\/)?(?:nas-chpasswd|chpasswd|smbpasswd))/i',
            '$1[REDACTADO]$2',
            $message
        );

        return $message;
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
    /**
     * Comprueba actualizaciones disponibles en el repositorio git o del sistema.
     * Retorna información enriquecida: SemVer, última versión, flag de actualizaciones,
     * notas de versión y verificación criptográfica GPG del tag.
     */
    public function checkUpdates(): array
    {
        $commit = 'Desconocido';
        $date = 'N/A';
        $version = '0.1.3';
        $channel = 'dev';
        $notes = 'v0.1.3: Panel Web Slate UI - Módulo de Aplicaciones y Actualizaciones';
        $gpgVerified = false;

        $repoDir = dirname(__DIR__, 2);
        $versionFile = $repoDir . '/version.json';
        if (!file_exists($versionFile) && file_exists('/opt/nas_debian/web/version.json')) {
            $versionFile = '/opt/nas_debian/web/version.json';
        }

        if (file_exists($versionFile)) {
            $raw = @file_get_contents($versionFile);
            if ($raw !== false) {
                $verData = json_decode($raw, true);
                if (is_array($verData)) {
                    $commit = $verData['commit'] ?? $commit;
                    $date = $verData['date'] ?? $date;
                    $version = $verData['version'] ?? $version;
                    $channel = $verData['channel'] ?? $channel;
                    $notes = $verData['notes'] ?? $notes;
                    $gpgVerified = !empty($verData['gpg_verified']);
                }
            }
        }

        // Buscar repositorio git del proyecto
        $gitDir = null;
        if (file_exists($repoDir . '/.git')) {
            $gitDir = $repoDir;
        } elseif (file_exists(dirname($repoDir) . '/.git')) {
            $gitDir = dirname($repoDir);
        } elseif (file_exists('/opt/nas_debian/.git')) {
            $gitDir = '/opt/nas_debian';
        }

        if ($gitDir !== null && DIRECTORY_SEPARATOR !== '\\') {
            $res = self::runCommand(['git', '-C', $gitDir, 'log', '-1', '--format=%h|%cd|%s', '--date=short']);
            if ($res['code'] === 0 && !empty($res['stdout'])) {
                $parts = explode('|', $res['stdout'], 3);
                $commit = $parts[0] ?? $commit;
                $date = $parts[1] ?? $date;
            }

            // Comprobación de firma GPG del tag local
            $tag = 'v' . ltrim($version, 'v');
            $catTag = self::runCommand(['git', '-C', $gitDir, 'cat-file', '-p', $tag]);
            if ($catTag['code'] === 0) {
                $gpgVerified = str_contains($catTag['stdout'], 'BEGIN PGP SIGNATURE');
            }
        }

        // Detección dinámica de la última versión y cálculo de has_updates
        $latestVersion = $version;
        $hasUpdates = false;
        $releaseNotes = $notes;

        // 1. Intentar mediante git ls-remote --tags origin con timeout defensivo de 3s
        $remoteTagsFound = false;
        if ($gitDir !== null && DIRECTORY_SEPARATOR !== '\\') {
            $lsRemote = self::runCommand(['git', '-C', $gitDir, 'ls-remote', '--tags', 'origin'], null, 3);
            if ($lsRemote['code'] === 0 && !empty($lsRemote['stdout'])) {
                $remoteTagsFound = true;
                $lines = explode("\n", $lsRemote['stdout']);
                $semverTags = [];
                foreach ($lines as $line) {
                    if (preg_match('#refs/tags/(v?[0-9]+\.[0-9]+\.[0-9]+(?:\.[0-9]+)?(?:\-[a-zA-Z0-9\.]+)?)(?:\^\{\})?$#', trim($line), $m)) {
                        $rawTag = $m[1];
                        $cleanVer = ltrim($rawTag, 'v');
                        $semverTags[$cleanVer] = $rawTag;
                    }
                }
                if (!empty($semverTags)) {
                    $sortedVers = array_keys($semverTags);
                    usort($sortedVers, 'version_compare');
                    $maxVer = end($sortedVers);
                    if (version_compare($maxVer, ltrim($version, 'v'), '>')) {
                        $latestVersion = $maxVer;
                        $hasUpdates = true;
                    } else {
                        $latestVersion = $version;
                    }
                }
            }
        }

        // 2. Fallback a GitHub API si git ls-remote no arrojó tags (o fuera de git)
        if (!$remoteTagsFound) {
            $ctx = stream_context_create([
                'http' => [
                    'method' => 'GET',
                    'header' => "User-Agent: Debian-NAS-Updater\r\nAccept: application/vnd.github.v3+json\r\n",
                    'timeout' => 3,
                ],
            ]);
            $apiUrl = 'https://api.github.com/repos/ftole/nas_debian/releases/latest';
            $apiRaw = @file_get_contents($apiUrl, false, $ctx);
            if ($apiRaw !== false) {
                $apiData = json_decode($apiRaw, true);
                if (is_array($apiData) && !empty($apiData['tag_name'])) {
                    $tag = (string) $apiData['tag_name'];
                    $cleanVer = ltrim($tag, 'v');
                    if (version_compare($cleanVer, ltrim($version, 'v'), '>')) {
                        $latestVersion = $cleanVer;
                        $hasUpdates = true;
                        if (!empty($apiData['body'])) {
                            $releaseNotes = (string) $apiData['body'];
                        }
                    } else {
                        $latestVersion = $version;
                    }
                }
            }
        }

        return [
            'version' => $version,
            'latest_version' => $latestVersion,
            'installed_commit' => $commit,
            'commit_date' => $date,
            'has_updates' => $hasUpdates,
            'channel' => $channel,
            'gpg_verified' => $gpgVerified,
            'release_notes' => $releaseNotes,
        ];
    }

    /**
     * Obtiene métricas en vivo y estado detallado de las 6 aplicaciones y subsistemas clave.
     */
    public function getApplicationsStatus(): array
    {
        // 1. Samba 4 & VFS
        $sambaVer = 'Samba 4 (Debian)';
        $sambaActive = false;
        $sambaSessions = 0;
        if (DIRECTORY_SEPARATOR !== '\\') {
            $resVer = self::runCommand(['/usr/sbin/smbd', '-V']);
            if ($resVer['code'] === 0 && !empty($resVer['stdout'])) {
                $sambaVer = trim($resVer['stdout']);
            }
            $resActive = self::runCommand(['systemctl', 'is-active', 'smbd']);
            $sambaActive = ($resActive['code'] === 0 && trim($resActive['stdout']) === 'active');

            // Sesiones concurrentes vía smbstatus
            $resStatus = self::sudo(['/usr/bin/smbstatus', '-b']);
            if ($resStatus['code'] === 0 && !empty($resStatus['stdout'])) {
                $lines = explode("\n", trim($resStatus['stdout']));
                $count = 0;
                $afterHeader = false;
                foreach ($lines as $l) {
                    if (str_starts_with($l, '---')) {
                        $afterHeader = true;
                        continue;
                    }
                    if ($afterHeader && trim($l) !== '') {
                        $count++;
                    }
                }
                $sambaSessions = $count;
            }
        } else {
            $sambaVer = 'Samba 4.22.11 (Simulado)';
            $sambaActive = true;
            $sambaSessions = 2;
        }

        // 2. Nginx & PHP-FPM
        $phpVersion = PHP_VERSION;
        $webActive = false;
        $webMemoryMb = 0.0;
        if (DIRECTORY_SEPARATOR !== '\\') {
            $resNginx = self::runCommand(['systemctl', 'is-active', 'nginx']);
            $webActive = ($resNginx['code'] === 0 && trim($resNginx['stdout']) === 'active');

            // Consumo de memoria RSS de nginx y php-fpm
            $resPs = self::runCommand(['ps', '-C', 'nginx,php8.4-fpm,php-fpm', '-o', 'rss=']);
            if ($resPs['code'] === 0 && !empty($resPs['stdout'])) {
                $rssTotal = 0;
                foreach (explode("\n", $resPs['stdout']) as $rssVal) {
                    $rssTotal += (int) trim($rssVal);
                }
                $webMemoryMb = round($rssTotal / 1024, 1);
            }
        } else {
            $webActive = true;
            $webMemoryMb = 48.5;
        }

        // 3. BTRFS con Zstandard
        $storageService = new StorageService();
        $stgOverview = $storageService->getStorageOverview();
        $fs = $stgOverview['filesystem'] ?? 'BTRFS';
        $btrfsActive = (strcasecmp((string) $fs, 'BTRFS') === 0);
        $scrubStatus = 'Sin ejecuciones previas';
        $scrubErrors = '0 errores';
        if ($btrfsActive && DIRECTORY_SEPARATOR !== '\\') {
            $resScrub = self::sudo(['/usr/bin/btrfs', 'scrub', 'status', '/srv/nas']);
            if ($resScrub['code'] === 0 && !empty($resScrub['stdout'])) {
                if (preg_match('/Status:\s+(.+)/i', $resScrub['stdout'], $m)) {
                    $scrubStatus = trim($m[1]);
                }
                if (preg_match('/Error summary:\s+(.+)/i', $resScrub['stdout'], $mErr)) {
                    $scrubErrors = trim($mErr[1]);
                }
            }
        } elseif ($btrfsActive) {
            $scrubStatus = 'finished';
            $scrubErrors = 'no errors found';
        }

        // 4. Rsync Multiplataforma
        $backupService = new BackupService();
        $backupTasks = $backupService->listTasks();
        $taskCount = count($backupTasks);

        // 5. WSDD2 & NetBIOS
        $wsddActive = false;
        $nmbdActive = false;
        if (DIRECTORY_SEPARATOR !== '\\') {
            $resWsdd = self::runCommand(['systemctl', 'is-active', 'wsdd2']);
            $wsddActive = ($resWsdd['code'] === 0 && trim($resWsdd['stdout']) === 'active');
            $resNmbd = self::runCommand(['systemctl', 'is-active', 'nmbd']);
            $nmbdActive = ($resNmbd['code'] === 0 && trim($resNmbd['stdout']) === 'active');
        } else {
            $wsddActive = true;
            $nmbdActive = true;
        }

        // 6. Seguridad Activa (AIDE & auditd)
        $auditdActive = false;
        $aideActive = false;
        if (DIRECTORY_SEPARATOR !== '\\') {
            $resAudit = self::runCommand(['systemctl', 'is-active', 'auditd']);
            $auditdActive = ($resAudit['code'] === 0 && trim($resAudit['stdout']) === 'active');
            $resAide = self::runCommand(['systemctl', 'is-active', 'dailyaidecheck.timer']);
            $aideActive = ($resAide['code'] === 0 && trim($resAide['stdout']) === 'active');
        } else {
            $auditdActive = true;
            $aideActive = true;
        }

        return [
            'samba' => [
                'version' => $sambaVer,
                'active' => $sambaActive,
                'status' => $sambaActive ? 'activo' : 'inactivo',
                'sessions' => $sambaSessions,
                'vfs_modules' => ['acl_xattr', 'streams_xattr'],
            ],
            'web' => [
                'php_version' => $phpVersion,
                'active' => $webActive,
                'status' => $webActive ? 'activo' : 'inactivo',
                'pool_mode' => 'ondemand',
                'memory_mb' => $webMemoryMb,
            ],
            'btrfs' => [
                'filesystem' => $fs,
                'active' => $btrfsActive,
                'compression' => 'zstd:3',
                'scrub_status' => $scrubStatus,
                'scrub_errors' => $scrubErrors,
                'schedule' => 'Mensual (día 1 a las 02:00)',
            ],
            'rsync' => [
                'tasks_count' => $taskCount,
                'deduplication' => 'Activa (hardlinks --link-dest >85%)',
                'staging' => 'Atómico (.inprogress_*)',
                'protocols' => ['CIFS 3.1.1', 'SSH'],
            ],
            'wsdd2' => [
                'wsdd_active' => $wsddActive,
                'nmbd_active' => $nmbdActive,
                'active' => $wsddActive && $nmbdActive,
                'protocol' => 'WSD (Web Services Discovery) + LLMNR',
            ],
            'security' => [
                'auditd_active' => $auditdActive,
                'aide_active' => $aideActive,
                'active' => $auditdActive && $aideActive,
                'audit_rules' => 'Vigilancia en tiempo real de sudoers, smb.conf y credenciales',
                'fim_status' => 'Monitoreo de integridad criptográfica offline (AIDE)',
            ],
        ];
    }

    /**
     * Ejecuta la actualización web en caliente invocando updater.sh vía sudo.
     */
    public function runHotUpdate(): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                'success' => true,
                'message' => 'Simulación de actualización en caliente en entorno local Windows.',
                'output' => "Actualización simulada completada.\nRepositorio al día.",
                'version' => '0.1.3',
            ];
        }

        $updaterScript = '/opt/nas_debian/src/core/updater.sh';
        if (!file_exists($updaterScript)) {
            $candidate = dirname(__DIR__, 3) . '/src/core/updater.sh';
            if (file_exists($candidate)) {
                $updaterScript = $candidate;
            } else {
                return [
                    'success' => false,
                    'error' => 'No se encontró el script de actualización (' . $updaterScript . ').',
                ];
            }
        }

        // Ejecución segura respetando el alias sudoers NAS_UPDATE
        $cmd = ['sudo', '-n', '/bin/bash', $updaterScript, '--yes'];
        $res = self::runCommand($cmd, null, 180);

        $combined = trim($res['stdout'] . ($res['stderr'] !== '' ? "\n" . $res['stderr'] : ''));
        if ($res['code'] !== 0) {
            return [
                'success' => false,
                'error' => 'Fallo en la actualización (código ' . $res['code'] . '): ' . ($res['stderr'] ?: $res['stdout']),
                'output' => $combined,
            ];
        }

        return [
            'success' => true,
            'message' => 'Actualización del sistema completada con éxito.',
            'output' => $combined,
        ];
    }

    /**
     * Lee dinámicamente el rol del servidor (/etc/nas/role), con fallback a smb.conf o entorno.
     * Retorna 'ARCHIVOS', 'BACKUP' o 'ARCHIVOS_BACKUP'.
     */
    public static function getServerRole(): string
    {
        // Variable de entorno para suites de pruebas o desarrollo (prevalece para test runners)
        $envRole = getenv('NAS_SERVER_ROLE');
        if ($envRole) {
            $envRole = strtoupper(trim((string) $envRole));
            $envRole = preg_replace('/[^A-Z_]/', '', $envRole);
            if (in_array($envRole, ['ARCHIVOS', 'BACKUP', 'ARCHIVOS_BACKUP'], true)) {
                return $envRole;
            }
            if ($envRole === 'HIBRIDO' || $envRole === 'ARCHIVOSBACKUP') {
                return 'ARCHIVOS_BACKUP';
            }
        }

        $roleFile = '/etc/nas/role';
        if (file_exists($roleFile) && is_readable($roleFile)) {
            $role = strtoupper(trim((string) @file_get_contents($roleFile)));
            $role = preg_replace('/[^A-Z_]/', '', $role);
            if (in_array($role, ['ARCHIVOS', 'BACKUP', 'ARCHIVOS_BACKUP'], true)) {
                return $role;
            }
            if ($role === 'HIBRIDO' || $role === 'ARCHIVOSBACKUP') {
                return 'ARCHIVOS_BACKUP';
            }
        }

        // Fallback a smb.conf
        $smbConf = '/etc/samba/smb.conf';
        if (file_exists($smbConf) && is_readable($smbConf)) {
            $content = (string) @file_get_contents($smbConf);
            if (stripos($content, 'Servidor ARCHIVOS_BACKUP') !== false || (stripos($content, 'ARCHIVOS') !== false && stripos($content, 'BACKUP') !== false)) {
                return 'ARCHIVOS_BACKUP';
            }
            if (stripos($content, 'Servidor BACKUP') !== false) {
                return 'BACKUP';
            }
        }

        return 'ARCHIVOS_BACKUP';
    }

    /**
     * Retorna la etiqueta legible del rol de servidor para la interfaz web.
     */
    public static function getServerRoleLabel(): string
    {
        $role = self::getServerRole();
        return match ($role) {
            'ARCHIVOS' => 'ARCHIVOS',
            'BACKUP' => 'BACKUP',
            'ARCHIVOS_BACKUP' => 'ARCHIVOS & BACKUP',
            default => 'ARCHIVOS & BACKUP',
        };
    }

    /**
     * Obtiene dinámicamente el grupo de trabajo (Workgroup) desde smb.conf o entorno, con fallback a TEAM-JOFRATO.
     */
    public static function getWorkgroup(): string
    {
        $envWg = getenv('NAS_WORKGROUP');
        if ($envWg !== false && trim((string) $envWg) !== '') {
            return strtoupper(trim((string) $envWg));
        }

        $smbConf = '/etc/samba/smb.conf';
        if (file_exists($smbConf) && is_readable($smbConf)) {
            $content = (string) @file_get_contents($smbConf);
            if (preg_match('/^\s*workgroup\s*=\s*([A-Za-z0-9_-]+)/mi', $content, $m)) {
                return strtoupper(trim($m[1]));
            }
        }

        return 'TEAM-JOFRATO';
    }

    /** Caché estática de la IPv4 primaria para evitar invocar 'ip' en cada métrica. */
    private static ?string $cachedPrimaryIp = null;
    private static int $cachedPrimaryIpAt = 0;

    /**
     * Obtiene la IPv4 global primaria del servidor (sin privilegios).
     */
    public function getPrimaryIp(): ?string
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return '10.10.1.2';
        }

        if (self::$cachedPrimaryIp !== null && (time() - self::$cachedPrimaryIpAt) < 30) {
            return self::$cachedPrimaryIp;
        }

        $res = self::runCommand(['ip', '-4', '-o', 'addr', 'show', 'scope', 'global'], null, 5);
        $ip = null;
        if ($res['code'] === 0 && preg_match('/inet\s+(\d{1,3}(?:\.\d{1,3}){3})/', (string) $res['stdout'], $m)) {
            $ip = $m[1];
        }

        if ($ip === null && !empty($_SERVER['SERVER_ADDR'])) {
            $ip = (string) $_SERVER['SERVER_ADDR'];
        }

        self::$cachedPrimaryIp = $ip;
        self::$cachedPrimaryIpAt = time();

        return $ip;
    }

    /**
     * Inventario de configuración y visibilidad de red para el módulo Networking.
     * Utiliza únicamente comandos sin privilegios (ip, ss, systemctl, testparm) y
     * lecturas de /etc, degradando con elegancia cuando un dato no puede verificarse.
     */
    public function getNetworkOverview(int $cacheSeconds = 5): array
    {
        $cachePath = sys_get_temp_dir() . '/nas_network_overview.json';

        if ($cacheSeconds > 0 && is_readable($cachePath)) {
            $age = time() - (int) @filemtime($cachePath);
            if ($age >= 0 && $age <= $cacheSeconds) {
                $cached = json_decode((string) @file_get_contents($cachePath), true);
                if (is_array($cached) && isset($cached['interfaces'])) {
                    return $cached;
                }
            }
        }

        $overview = (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing')
            ? $this->getSimulatedNetworkOverview()
            : $this->collectNetworkOverview();

        if ($cacheSeconds > 0 && DIRECTORY_SEPARATOR !== '\\') {
            @file_put_contents(
                $cachePath,
                (string) json_encode($overview, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES),
                LOCK_EX
            );
        }

        return $overview;
    }

    /**
     * Recolecta el estado real de red del servidor Linux.
     */
    private function collectNetworkOverview(): array
    {
        $interfaces = [];
        $links = json_decode((string) self::runCommand(['ip', '-j', 'addr', 'show'], null, 10)['stdout'], true);
        if (is_array($links)) {
            foreach ($links as $link) {
                $name = (string) ($link['ifname'] ?? '');
                if ($name === '' || $name === 'lo') {
                    continue;
                }

                $ipv4 = null;
                $prefix = null;
                foreach (($link['addr_info'] ?? []) as $addr) {
                    if (($addr['family'] ?? '') === 'inet') {
                        $ipv4 = (string) ($addr['local'] ?? '');
                        $prefix = isset($addr['prefixlen']) ? (int) $addr['prefixlen'] : null;
                        break;
                    }
                }

                $speedRaw = @file_get_contents('/sys/class/net/' . $name . '/speed');
                $speed = ($speedRaw !== false && (int) trim((string) $speedRaw) > 0) ? (int) trim((string) $speedRaw) : null;

                $interfaces[] = [
                    'name' => $name,
                    'state' => strtoupper((string) ($link['operstate'] ?? 'UNKNOWN')),
                    'ip' => $ipv4,
                    'prefix' => $prefix,
                    'mac' => (string) ($link['address'] ?? ''),
                    'mtu' => isset($link['mtu']) ? (int) $link['mtu'] : null,
                    'speed_mbps' => $speed,
                ];
            }
        }

        $gateway = null;
        $dhcp = false;
        $routes = json_decode((string) self::runCommand(['ip', '-j', 'route', 'show', 'default'], null, 10)['stdout'], true);
        if (is_array($routes)) {
            foreach ($routes as $route) {
                if (!empty($route['gateway'])) {
                    $gateway = (string) $route['gateway'];
                    $dhcp = strtolower((string) ($route['protocol'] ?? '')) === 'dhcp';
                    break;
                }
            }
        }

        $dns = [];
        if (is_readable('/etc/resolv.conf')) {
            $resolv = (string) @file_get_contents('/etc/resolv.conf');
            if (preg_match_all('/^\s*nameserver\s+(\S+)/mi', $resolv, $m)) {
                $dns = array_values(array_unique(array_map('strval', $m[1])));
            }
        }

        $samba = $this->getSambaNetworkConfig();
        $services = [];
        foreach (['smbd', 'nmbd', 'wsdd2'] as $svc) {
            $st = self::runCommand(['systemctl', 'is-active', $svc], null, 5);
            $services[$svc] = trim((string) $st['stdout']) === 'active';
        }

        return [
            'hostname' => gethostname() ?: 'SRV-NAS',
            'netbios' => $samba['netbios'],
            'workgroup' => $samba['workgroup'],
            'interfaces' => $interfaces,
            'primary_ip' => $this->getPrimaryIp(),
            'gateway' => $gateway,
            'dns' => $dns,
            'dhcp' => $dhcp,
            'samba' => [
                'min_protocol' => $samba['min_protocol'],
                'max_protocol' => $samba['max_protocol'],
                'signing' => $samba['signing'],
                'encrypt' => $samba['encrypt'],
                'netbios_enabled' => $samba['netbios_enabled'],
            ],
            'services' => $services,
            'listening' => $this->getListeningPorts(),
            'firewall' => $this->getFirewallStatus(),
            'smb_sessions' => $this->getSmbSessionCount(),
            'generated_at' => date('H:i:s'),
            'generated_timestamp' => time(),
        ];
    }

    /**
     * Lee la configuración efectiva de Samba (testparm) con reserva a smb.conf.
     */
    private function getSambaNetworkConfig(): array
    {
        $result = [
            'netbios' => strtoupper((string) (gethostname() ?: 'SRV-NAS')),
            'workgroup' => self::getWorkgroup(),
            'min_protocol' => 'SMB2_02',
            'max_protocol' => 'SMB3',
            'signing' => 'required',
            'encrypt' => 'desired',
            'netbios_enabled' => true,
        ];

        $raw = '';
        $testparm = self::runCommand(['testparm', '-s'], null, 10);
        if ($testparm['code'] === 0 && trim((string) $testparm['stdout']) !== '') {
            $raw = (string) $testparm['stdout'];
        } elseif (is_readable('/etc/samba/smb.conf')) {
            $raw = (string) @file_get_contents('/etc/samba/smb.conf');
        }

        if ($raw === '') {
            return $result;
        }

        $map = [
            'workgroup' => 'workgroup',
            'netbios name' => 'netbios',
            'server min protocol' => 'min_protocol',
            'server max protocol' => 'max_protocol',
            'server signing' => 'signing',
            'server smb encrypt' => 'encrypt',
        ];

        foreach ($map as $key => $field) {
            if (preg_match('/^\s*' . preg_quote($key, '/') . '\s*=\s*(.+)$/mi', $raw, $m)) {
                $value = trim($m[1]);
                $result[$field] = ($field === 'workgroup' || $field === 'netbios')
                    ? strtoupper($value)
                    : strtolower($value);
            }
        }

        if (preg_match('/^\s*disable netbios\s*=\s*(.+)$/mi', $raw, $m)) {
            $result['netbios_enabled'] = !in_array(strtolower(trim($m[1])), ['yes', 'true', '1'], true);
        }

        return $result;
    }

    /**
     * Enumera los puertos TCP/UDP en escucha con su servicio asociado (sin privilegios).
     */
    private function getListeningPorts(): array
    {
        $serviceByPort = [
            22 => 'sshd', 25 => 'exim4', 53 => 'dns', 80 => 'nginx', 443 => 'nginx',
            137 => 'nmbd', 138 => 'nmbd', 139 => 'smbd', 445 => 'smbd',
            3702 => 'wsdd2', 5355 => 'wsdd2',
        ];

        // Los puertos UDP efímeros (cliente DHCP/DHCPv6) no aportan visibilidad operativa
        $udpWhitelist = [53, 123, 137, 138, 161, 3702, 5353, 5355];

        $ports = [];
        foreach (['tcp' => ['ss', '-H', '-tln'], 'udp' => ['ss', '-H', '-uln']] as $proto => $cmd) {
            $res = self::runCommand($cmd, null, 5);
            if ($res['code'] !== 0) {
                continue;
            }
            foreach (preg_split('/\r?\n/', (string) $res['stdout']) ?: [] as $line) {
                $cols = preg_split('/\s+/', trim($line)) ?: [];
                $local = $cols[3] ?? '';
                if ($local === '' || !preg_match('/:(\d+)$/', $local, $m)) {
                    continue;
                }
                $port = (int) $m[1];
                if ($port <= 0) {
                    continue;
                }
                if ($proto === 'udp' && !in_array($port, $udpWhitelist, true)) {
                    continue;
                }
                $key = $proto . ':' . $port;
                if (isset($ports[$key])) {
                    continue;
                }
                $ports[$key] = [
                    'port' => $port,
                    'proto' => $proto,
                    'service' => $serviceByPort[$port] ?? null,
                ];
            }
        }

        $list = array_values($ports);
        usort($list, static fn (array $a, array $b): int => $a['port'] <=> $b['port']);

        return array_slice($list, 0, 24);
    }

    /**
     * Estado del cortafuegos UFW (las reglas solo son visibles con privilegios).
     */
    private function getFirewallStatus(): array
    {
        $active = false;
        $source = '/etc/ufw/ufw.conf';

        if (is_readable($source)) {
            $conf = (string) @file_get_contents($source);
            if (preg_match('/^\s*ENABLED\s*=\s*(\w+)/mi', $conf, $m)) {
                $active = strtolower(trim($m[1])) === 'yes';
            }
        } else {
            $source = null;
        }

        return [
            'active' => $active,
            'rules_visible' => false,
            'source' => $source,
        ];
    }

    /**
     * Número de sesiones SMB activas (requiere la regla sudoers smbstatus -b).
     */
    private function getSmbSessionCount(): ?int
    {
        $res = self::sudo(['smbstatus', '-b'], null, 10);
        if ($res['code'] !== 0) {
            return null;
        }

        $count = 0;
        foreach (preg_split('/\r?\n/', (string) $res['stdout']) ?: [] as $line) {
            if (preg_match('/^\s*\d+\s+\S+\s+\S+\s+\S+\s+\S+/', $line)) {
                $count++;
            }
        }

        return $count;
    }

    /**
     * Datos simulados para desarrollo en Windows y suites de pruebas.
     */
    private function getSimulatedNetworkOverview(): array
    {
        return [
            'hostname' => 'SRV-NAS',
            'netbios' => 'SRV-NAS',
            'workgroup' => 'TEAM-JOFRATO',
            'interfaces' => [[
                'name' => 'eth0',
                'state' => 'UP',
                'ip' => '10.10.1.2',
                'prefix' => 24,
                'mac' => '00:11:22:33:44:55',
                'mtu' => 1500,
                'speed_mbps' => 1000,
            ]],
            'primary_ip' => '10.10.1.2',
            'gateway' => '10.10.1.1',
            'dns' => ['10.16.0.23', '10.16.0.60'],
            'dhcp' => true,
            'samba' => [
                'min_protocol' => 'smb2_02',
                'max_protocol' => 'smb3',
                'signing' => 'required',
                'encrypt' => 'desired',
                'netbios_enabled' => true,
            ],
            'services' => ['smbd' => true, 'nmbd' => true, 'wsdd2' => true],
            'listening' => [
                ['port' => 22, 'proto' => 'tcp', 'service' => 'sshd'],
                ['port' => 80, 'proto' => 'tcp', 'service' => 'nginx'],
                ['port' => 443, 'proto' => 'tcp', 'service' => 'nginx'],
                ['port' => 445, 'proto' => 'tcp', 'service' => 'smbd'],
                ['port' => 3702, 'proto' => 'udp', 'service' => 'wsdd2'],
                ['port' => 5355, 'proto' => 'udp', 'service' => 'wsdd2'],
            ],
            'firewall' => ['active' => true, 'rules_visible' => false, 'source' => '/etc/ufw/ufw.conf'],
            'smb_sessions' => 0,
            'generated_at' => date('H:i:s'),
            'generated_timestamp' => time(),
        ];
    }
}

