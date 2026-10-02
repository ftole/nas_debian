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

        // Ajustamos timeout de lectura
        stream_set_timeout($pipes[1], $timeout);
        stream_set_timeout($pipes[2], $timeout);

        $stdout = (string) stream_get_contents($pipes[1]);
        fclose($pipes[1]);

        $stderr = (string) stream_get_contents($pipes[2]);
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

        $repoDir = dirname(__DIR__, 2);
        if (file_exists($repoDir . '/.git')) {
            $res = self::runCommand(['git', '-C', $repoDir, 'log', '-1', '--format=%h|%cd|%s', '--date=short']);
            if ($res['code'] === 0 && !empty($res['stdout'])) {
                $parts = explode('|', $res['stdout'], 3);
                $commit = $parts[0] ?? '';
                $date = $parts[1] ?? '';
            }
        }

        return [
            'installed_commit' => $commit,
            'commit_date' => $date,
            'has_updates' => false,
            'channel' => 'GitHub main',
        ];
    }
}
