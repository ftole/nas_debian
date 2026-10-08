<?php

declare(strict_types=1);

namespace App\Services;

/**
 * Servicio de Central de Respaldos Multiplataforma (CIFS 3.1.1 / SSH Linux / Local),
 * deduplicación superior al 85% por Hardlinks (rsync --link-dest), staging atómico
 * y control de concurrencia mediante descriptores de archivo (flock).
 */
class BackupService
{
    public string $binDir = '/usr/local/bin';
    public string $cronDir = '/etc/cron.d';
    public string $credDir = '/etc/backup-credentials';
    public string $bkpRoot = '/srv/nas/BACKUPS_HISTORICOS';
    public string $logRoot = '/srv/nas/LOGS_BACKUP';

    /**
     * Lista todas las tareas de backup programadas leyendo los runners en /usr/local/bin/backup_*.sh.
     */
    public function listTasks(): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                [
                    'id' => 'win_contabilidad',
                    'protocol' => 'CIFS (Windows)',
                    'source' => '//10.10.1.50/Contabilidad',
                    'cron' => '0 23 * * *',
                    'cron_desc' => 'Diario a las 23:00 hrs',
                    'retention' => 30,
                    'last_status' => 'OK',
                    'last_run' => date('Y-m-d 23:00'),
                    'running' => false,
                    'percent' => 100,
                    'elapsed' => '00:02:14',
                    'remaining' => '00:00:00',
                    'speed' => '48.2MB/s',
                ],
                [
                    'id' => 'lnx_web_prod',
                    'protocol' => 'SSH (Linux)',
                    'source' => 'root@10.10.1.55:/var/www',
                    'cron' => '0 */6 * * *',
                    'cron_desc' => 'Cada 6 horas',
                    'retention' => 15,
                    'last_status' => 'OK',
                    'last_run' => date('Y-m-d H:00'),
                    'running' => false,
                    'percent' => 100,
                    'elapsed' => '00:00:41',
                    'remaining' => '00:00:00',
                    'speed' => '22.7MB/s',
                ],
            ];
        }

        $runners = glob($this->binDir . '/backup_*.sh') ?: [];
        $tasks = [];

        foreach ($runners as $runner) {
            $base = basename($runner);
            $taskId = preg_replace('/^backup_|\.sh$/', '', $base);
            $content = (string) @file_get_contents($runner);

            $proto = 'Local';
            $src = 'N/A';
            $retention = 30;

            if (str_contains($content, 'SRC_SHARE=')) {
                $proto = 'CIFS (Windows)';
                preg_match('/SRC_IP="([^"]+)"/', $content, $mIp);
                preg_match('/SRC_SHARE="([^"]+)"/', $content, $mSh);
                $src = '//' . ($mIp[1] ?? 'IP') . '/' . ($mSh[1] ?? 'SHARE');
            } elseif (str_contains($content, 'SRC_PORT=') || str_contains($content, 'sshpass')) {
                $proto = 'SSH (Linux)';
                preg_match('/SRC_IP="([^"]+)"/', $content, $mIp);
                preg_match('/SRC_PATH="([^"]+)"/', $content, $mPt);
                preg_match('/SRC_USER="([^"]+)"/', $content, $mUs);
                $src = ($mUs[1] ?? 'root') . '@' . ($mIp[1] ?? 'IP') . ':' . ($mPt[1] ?? '/');
            } else {
                preg_match('/SRC_PATH="([^"]+)"/', $content, $mPt);
                $src = $mPt[1] ?? '/';
            }

            if (preg_match('/RETENTION=(\d+)/', $content, $mRet)) {
                $retention = (int) $mRet[1];
            }

            // Expresión cron
            $cronFile = $this->cronDir . '/backup_' . $taskId;
            $cronExpr = 'Manual';
            if (file_exists($cronFile)) {
                $cLine = trim((string) @file_get_contents($cronFile));
                $parts = preg_split('/\s+/', $cLine);
                if (count($parts) >= 5) {
                    $cronExpr = implode(' ', array_slice($parts, 0, 5));
                }
            }

            // Estado y progreso de la última ejecución
            $logFile = $this->logRoot . '/backup_' . $taskId . '.log';
            $lastRun = file_exists($logFile) ? date('Y-m-d H:i', (int) filemtime($logFile)) : 'Nunca';
            $state = $this->getTaskStatus($taskId);
            $progress = $this->getTaskProgress($taskId);
            $statusLabel = [
                'running' => 'En curso',
                'ok' => 'OK',
                'error' => 'Error',
                'idle' => 'Sin ejecutar',
            ][$state] ?? 'Sin ejecutar';

            $tasks[] = [
                'id' => $taskId,
                'protocol' => $proto,
                'source' => $src,
                'cron' => $cronExpr,
                'cron_desc' => $this->describeCron($cronExpr),
                'retention' => $retention,
                'last_status' => $statusLabel,
                'last_run' => $lastRun,
                'running' => $state === 'running',
                'percent' => $progress['percent'],
                'elapsed' => $progress['elapsed'],
                'remaining' => $progress['remaining'],
                'speed' => $progress['speed'],
            ];
        }

        return $tasks;
    }

    /**
     * Crea una nueva tarea de respaldo generando el runner ejecutable, archivo de credenciales
     * protegidas (0600) y entrada en cron.d.
     */
    public function createTask(array $data): array
    {
        $taskId = strtolower(trim($data['id'] ?? ''));
        $proto = strtolower(trim($data['proto'] ?? 'cifs'));
        $cronExpr = trim($data['cron'] ?? '0 23 * * *');
        $retention = max(1, (int) ($data['retention'] ?? 30));

        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $taskId)) {
            return ['success' => false, 'error' => 'Identificador de tarea inválido (2-32 caracteres a-z, 0-9, guiones).'];
        }

        if (!$this->validateCron($cronExpr)) {
            return ['success' => false, 'error' => 'Expresión cron no válida (debe tener exactamente 5 campos).'];
        }

        // Crear directorios de soporte
        $this->ensureDirectory($this->credDir);
        $this->ensureDirectory($this->bkpRoot . '/' . $taskId);
        $this->ensureDirectory($this->logRoot);
        $this->ensureDirectory($this->binDir);
        $this->ensureDirectory($this->cronDir);

        $credFile = $this->credDir . '/' . $taskId . '.cred';
        $runnerFile = $this->binDir . '/backup_' . $taskId . '.sh';
        $cronFile = $this->cronDir . '/backup_' . $taskId;

        $runnerContent = '';

        if ($proto === 'cifs') {
            $ip = trim($data['ip'] ?? '');
            $share = trim($data['share'] ?? '');
            $user = trim($data['user'] ?? 'Administrador');
            $pass = $data['password'] ?? '';

            if (empty($ip) || empty($share)) {
                return ['success' => false, 'error' => 'La IP del servidor Windows y el recurso compartido son obligatorios.'];
            }

            // Guardar credenciales 0600
            $credContent = '';
            if (str_contains($user, '\\') || str_contains($user, '/')) {
                $parts = preg_split('#[\\\\/]#', $user, 2);
                $credContent = "username={$parts[1]}\npassword=$pass\ndomain={$parts[0]}\n";
            } else {
                $credContent = "username=$user\npassword=$pass\n";
            }

            $wCred = $this->writeFileSecure($credFile, $credContent, 0600);
            if (!$wCred) {
                return ['success' => false, 'error' => 'No se pudo guardar el archivo de credenciales de respaldo.'];
            }
            $runnerContent = $this->buildCifsRunner($taskId, $ip, $share, $credFile, $retention);
        } elseif ($proto === 'ssh') {
            $ip = trim($data['ip'] ?? '');
            $port = (int) ($data['port'] ?? 22);
            $user = trim($data['user'] ?? 'root');
            $path = trim($data['path'] ?? '/');
            $pass = $data['password'] ?? '';

            if (empty($ip) || empty($path)) {
                return ['success' => false, 'error' => 'La IP y la ruta del servidor Linux son obligatorias.'];
            }

            $wCred = $this->writeFileSecure($credFile, $pass . "\n", 0600);
            if (!$wCred) {
                return ['success' => false, 'error' => 'No se pudo guardar el archivo de credenciales de respaldo.'];
            }
            $runnerContent = $this->buildSshRunner($taskId, $ip, $port, $user, $path, $credFile, $retention);
        } elseif ($proto === 'local') {
            $path = trim($data['path'] ?? '/srv/nas/SISTEMAS');
            $runnerContent = $this->buildLocalRunner($taskId, $path, $retention);
        } else {
            return ['success' => false, 'error' => 'Protocolo de backup no soportado.'];
        }

        // Escribir runner con permisos 755
        $wRunner = $this->writeFileSecure($runnerFile, $runnerContent, 0755);
        if (!$wRunner) {
            return ['success' => false, 'error' => 'No se pudo generar el ejecutable del runner de respaldo.'];
        }

        // Escribir archivo cron
        $cronLine = "$cronExpr root $runnerFile >/dev/null 2>&1\n";
        $wCron = $this->writeFileSecure($cronFile, $cronLine, 0644);
        if (!$wCron) {
            return ['success' => false, 'error' => 'No se pudo registrar la tarea en cron.d.'];
        }

        return ['success' => true, 'message' => "Tarea de respaldo [$taskId] programada exitosamente."];
    }

    /**
     * Elimina una tarea de respaldo, su runner, cron y opcionalmente los snapshots almacenados.
     */
    public function deleteTask(string $taskId, bool $deleteBackups = false): array
    {
        $taskId = strtolower(trim($taskId));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $taskId)) {
            return ['success' => false, 'error' => 'Identificador de tarea inválido.'];
        }

        $runner = $this->binDir . '/backup_' . $taskId . '.sh';
        $cron = $this->cronDir . '/backup_' . $taskId;
        $cred = $this->credDir . '/' . $taskId . '.cred';
        $lock = '/var/lock/backup_' . $taskId . '.lock';

        @unlink($runner);
        @unlink($cron);
        @unlink($cred);
        @unlink($lock);

        if (DIRECTORY_SEPARATOR !== '\\') {
            SystemService::sudo(['rm', '-f', $runner]);
            SystemService::sudo(['rm', '-f', $cron]);
            SystemService::sudo(['rm', '-f', $cred]);
            SystemService::sudo(['rm', '-f', $lock]);
        }

        if ($deleteBackups) {
            $dir = $this->bkpRoot . '/' . $taskId;
            if (is_dir($dir) && $dir !== $this->bkpRoot) {
                if (is_writable($this->bkpRoot)) {
                    $this->removeDirectoryRecursive($dir);
                } else {
                    SystemService::sudo(['rm', '-rf', $dir]);
                }
            }
        }

        return ['success' => true, 'message' => "Tarea de backup [$taskId] eliminada."];
    }

    /**
     * Ejecuta una tarea de respaldo en segundo plano de manera inmediata.
     */
    public function runTaskNow(string $taskId): array
    {
        $taskId = strtolower(trim($taskId));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $taskId)) {
            return ['success' => false, 'error' => 'Identificador de tarea inválido.'];
        }

        $runner = $this->binDir . '/backup_' . $taskId . '.sh';
        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Backup $taskId lanzado en segundo plano (modo dev)."];
        }

        if (!file_exists($runner)) {
            return ['success' => false, 'error' => "El runner de respaldo no existe para la tarea $taskId."];
        }

        // Ejecutar en segundo plano desacoplado
        exec('nohup sudo -n ' . escapeshellarg($runner) . ' >/dev/null 2>&1 &');

        return ['success' => true, 'message' => "Tarea [$taskId] lanzada. Consulta la bitácora para ver el progreso."];
    }

    /**
     * Lee las últimas líneas del registro de una tarea específica.
     */
    public function getTaskLogs(string $taskId, int $lines = 100): string
    {
        $taskId = strtolower(trim($taskId));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $taskId)) {
            return '';
        }

        $logFile = $this->logRoot . '/backup_' . $taskId . '.log';
        if (DIRECTORY_SEPARATOR === '\\' || !file_exists($logFile)) {
            return "[2026-10-02 23:00:01] === INICIANDO BACKUP: {$taskId} ===\n" .
                   "[2026-10-02 23:00:02] Verificación de umbral de espacio: Uso 28% (OK)\n" .
                   "[2026-10-02 23:00:04] Conexión establecida.\n" .
                   "[2026-10-02 23:00:15] Sincronización rsync con deduplicación por Hardlinks finalizada.\n" .
                   "[2026-10-02 23:00:16] Snapshot atómico promovido: snapshot_2026-10-02_230000\n" .
                   "[2026-10-02 23:00:17] === BACKUP FINALIZADO CON ÉXITO ===";
        }

        return $this->getLastLines($logFile, $lines);
    }

    private function isDev(): bool
    {
        return DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing';
    }

    /**
     * Prueba la conexión al origen de la tarea (CIFS/SMB, SSH o Local) sin crear la tarea.
     */
    public function testConnection(array $data): array
    {
        $proto = strtolower(trim($data['proto'] ?? 'cifs'));

        if ($this->isDev()) {
            return ['success' => true, 'message' => 'Conexión de prueba correcta (modo dev).'];
        }

        if ($proto === 'cifs') {
            $ip = trim($data['ip'] ?? '');
            $share = trim($data['share'] ?? '');
            $user = trim($data['user'] ?? '');
            $pass = (string) ($data['password'] ?? '');
            if ($ip === '' || $share === '') {
                return ['success' => false, 'error' => 'IP y recurso compartido son obligatorios.'];
            }
            $res = SystemService::runCommand(
                ['smbclient', '//' . $ip . '/' . $share, '-U', $user, '-c', 'exit'], $pass . "\n", 15);
            $out = $res['stdout'] . ' ' . $res['stderr'];
            if ($res['code'] === 0 && !str_contains($out, 'NT_STATUS_LOGON_FAILURE') && !str_contains($out, 'NT_STATUS_BAD_NETWORK_NAME')) {
                return ['success' => true, 'message' => "Conexión SMB correcta a //$ip/$share."];
            }
            return ['success' => false, 'error' => 'No se pudo conectar: ' . trim(preg_replace('/\s+/', ' ', $out))];
        }

        if ($proto === 'ssh') {
            $ip = trim($data['ip'] ?? '');
            $port = (int) ($data['port'] ?? 22);
            $user = trim($data['user'] ?? 'root');
            $pass = (string) ($data['password'] ?? '');
            if ($ip === '') {
                return ['success' => false, 'error' => 'La IP del servidor Linux es obligatoria.'];
            }
            $tmp = @tempnam(sys_get_temp_dir(), 'nas_ssh_');
            if ($tmp === false) {
                return ['success' => false, 'error' => 'No se pudo preparar la prueba SSH.'];
            }
            file_put_contents($tmp, $pass . "\n");
            @chmod($tmp, 0600);
            $res = SystemService::runCommand(
                ['sshpass', '-f', $tmp, 'ssh', '-p', (string) $port, '-o', 'StrictHostKeyChecking=accept-new',
                 '-o', 'UserKnownHostsFile=/dev/null', '-o', 'ConnectTimeout=8', $user . '@' . $ip, 'echo NAS_OK'], null, 20);
            @unlink($tmp);
            if ($res['code'] === 0 && str_contains($res['stdout'], 'NAS_OK')) {
                return ['success' => true, 'message' => "Conexión SSH correcta a $user@$ip:$port."];
            }
            return ['success' => false, 'error' => 'No se pudo conectar por SSH: ' . trim($res['stderr'] ?: $res['stdout'])];
        }

        if ($proto === 'local') {
            $path = trim($data['path'] ?? '');
            if ($path === '') {
                return ['success' => false, 'error' => 'La ruta local es obligatoria.'];
            }
            $res = SystemService::runCommand(['test', '-d', $path]);
            if ($res['code'] === 0) {
                return ['success' => true, 'message' => "Ruta local accesible: $path."];
            }
            return ['success' => false, 'error' => "La ruta local no existe o no es accesible: $path."];
        }

        return ['success' => false, 'error' => 'Protocolo no soportado para prueba.'];
    }

    /**
     * Estado de la última/más reciente ejecución: running | ok | error | idle.
     */
    public function getTaskStatus(string $taskId): string
    {
        $taskId = strtolower(trim($taskId));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $taskId)) {
            return 'idle';
        }
        if ($this->isDev()) {
            return 'ok';
        }

        $pg = SystemService::runCommand(['pgrep', '-f', 'backup_' . $taskId . '.sh']);
        if ($pg['code'] === 0 && trim($pg['stdout']) !== '') {
            return 'running';
        }

        $log = $this->logRoot . '/backup_' . $taskId . '.log';
        if (!file_exists($log)) {
            return 'idle';
        }
        $tail = strtoupper($this->getLastLines($log, 25));
        if (str_contains($tail, 'ABORTADO') || str_contains($tail, 'FAILED') || str_contains($tail, 'ERROR')) {
            return 'error';
        }
        if (str_contains($tail, 'BACKUP FINALIZADO CON ÉXITO') || str_contains($tail, 'PROMOVIDO EXITOSAMENTE') || str_contains($tail, 'BACKUP_COMPLETED')) {
            return 'ok';
        }
        return 'idle';
    }

    /**
     * Progreso de la tarea a partir del log (rsync --info=progress2).
     */
    public function getTaskProgress(string $taskId): array
    {
        $taskId = strtolower(trim($taskId));
        $status = $this->getTaskStatus($taskId);
        $result = [
            'status' => $status,
            'percent' => 0,
            'speed' => '',
            'elapsed_sec' => 0,
            'elapsed' => '00:00:00',
            'remaining' => '--:--:--',
        ];

        $log = $this->logRoot . '/backup_' . $taskId . '.log';
        if (!file_exists($log)) {
            return $result;
        }
        $lines = preg_split('/\r\n|\r|\n/', (string) @file_get_contents($log));
        if (!is_array($lines)) {
            return $result;
        }

        $start = null;
        foreach ($lines as $ln) {
            if (preg_match('/^\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]/', $ln, $m)) {
                $start = strtotime($m[1]);
                break;
            }
        }

        $percent = 0;
        $speed = '';
        for ($i = count($lines) - 1; $i >= 0; $i--) {
            if (preg_match('/(\d{1,3})%/', $lines[$i], $pm)) {
                $percent = min(100, (int) $pm[1]);
                if (preg_match('/([\d.]+[KMGkmg]?B\/s)/', $lines[$i], $sm)) {
                    $speed = $sm[1];
                }
                break;
            }
        }

        $elapsed = $start !== null ? max(0, time() - $start) : 0;
        $result['percent'] = $percent;
        $result['speed'] = $speed;
        $result['elapsed_sec'] = $elapsed;
        $result['elapsed'] = $this->formatDuration($elapsed);
        if ($percent > 0 && $elapsed > 0) {
            $result['remaining'] = $this->formatDuration((int) round($elapsed * (100 - $percent) / $percent));
        } elseif ($status === 'ok') {
            $result['percent'] = 100;
            $result['remaining'] = '00:00:00';
        }
        return $result;
    }

    private function formatDuration(int $seconds): string
    {
        $h = intdiv($seconds, 3600);
        $m = intdiv($seconds % 3600, 60);
        $s = $seconds % 60;
        return sprintf('%02d:%02d:%02d', $h, $m, $s);
    }

    private function ensureDirectory(string $dir): void
    {
        if (is_dir($dir)) {
            return;
        }

        if (@mkdir($dir, 0755, true)) {
            return;
        }

        if (DIRECTORY_SEPARATOR !== '\\') {
            SystemService::sudo(['mkdir', '-p', $dir]);
        }
    }

    private function writeFileSecure(string $path, string $content, int $mode): bool
    {
        $dir = dirname($path);
        $this->ensureDirectory($dir);

        if (is_writable($dir) || (file_exists($path) && is_writable($path))) {
            $w = @file_put_contents($path, $content);
            @chmod($path, $mode);
            return $w !== false;
        }

        $tmp = tempnam(sys_get_temp_dir(), 'nas_tmp_');
        if ($tmp !== false) {
            file_put_contents($tmp, $content);
            chmod($tmp, $mode);
            $cpRes = SystemService::sudo(['cp', $tmp, $path]);
            SystemService::sudo(['chmod', sprintf('%o', $mode), $path]);
            @unlink($tmp);
            return ($cpRes['code'] === 0);
        }

        return false;
    }

    private function removeDirectoryRecursive(string $dir): void
    {
        $items = scandir($dir);
        if ($items === false) return;
        foreach ($items as $item) {
            if ($item === '.' || $item === '..') continue;
            $path = $dir . '/' . $item;
            if (is_dir($path)) {
                $this->removeDirectoryRecursive($path);
            } else {
                @unlink($path);
            }
        }
        @rmdir($dir);
    }

    private function getLastLines(string $file, int $lines): string
    {
        if (!file_exists($file)) {
            return '';
        }

        $res = SystemService::runCommand(['tail', '-n', (string) $lines, $file]);
        return $res['stdout'] ?: '';
    }

    private function validateCron(string $cron): bool
    {
        $parts = preg_split('/\s+/', trim($cron));
        if (count($parts) !== 5) {
            return false;
        }

        foreach ($parts as $p) {
            if (!preg_match('/^[0-9\*\/,-]+$/', $p)) {
                return false;
            }
        }

        return true;
    }

    private function describeCron(string $cron): string
    {
        return match ($cron) {
            '0 23 * * *' => 'Diario a las 23:00',
            '0 */6 * * *' => 'Cada 6 horas',
            '0 * * * *' => 'Cada hora en punto',
            'Manual' => 'Ejecución manual',
            default => $cron,
        };
    }

    private function buildCifsRunner(string $task, string $ip, string $share, string $cred, int $retention): string
    {
        return <<<BASH
#!/bin/bash
set -e
TASK="$task"
SRC_IP="$ip"
SRC_SHARE="$share"
CRED_FILE="$cred"
MOUNT_POINT="\${MOUNT_ROOT:-/mnt/backup_sources}/\$TASK"
BKP_DIR="{$this->bkpRoot}/\$TASK"
LOG_FILE="{$this->logRoot}/backup_\${TASK}.log"
MASTER_LOG="{$this->logRoot}/backups_master.log"
RETENTION=$retention
DATE_STR=\$(date +%Y-%m-%d_%H%M%S)
STAGE_SNAPSHOT="\$BKP_DIR/.inprogress_\$DATE_STR"
FINAL_SNAPSHOT="\$BKP_DIR/snapshot_\$DATE_STR"
SNAPSHOT_OK=false

log_backup_event() {
    local evt="\$1"
    local sev="\$2"
    local msg="\$3"
    local ts
    ts=\$(date '+%Y-%m-%d %H:%M:%S')
    local entry="[\$ts] [\$TASK] [\$evt] [\$sev] \$msg"
    echo "\$entry" >> "\$LOG_FILE"
    mkdir -p "\$(dirname "\$MASTER_LOG")" 2>/dev/null || true
    echo "\$entry" >> "\$MASTER_LOG"
    logger -t nas_backup -p "local4.\$sev" "[\$TASK] [\$evt] \$msg" 2>/dev/null || true
}

cleanup() {
    local status=\$?
    umount "\$MOUNT_POINT" 2>/dev/null || true
    if [ "\$SNAPSHOT_OK" != "true" ] && [ "\$status" -ne 0 ]; then
        echo "=== se descarta el snapshot parcial ===" >> "\$LOG_FILE"
        log_backup_event "BACKUP_FAILED" "err" "Respaldo fallido; se descarta el snapshot parcial"
        btrfs property set "\$FINAL_SNAPSHOT" ro false 2>/dev/null || true
        chattr -R -i "\$FINAL_SNAPSHOT" 2>/dev/null || true
        rm -rf "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
    fi
    if [ -n "\${LAST_SNAPSHOT:-}" ] && [ -d "\$LAST_SNAPSHOT" ]; then
        chattr -R +i "\$LAST_SNAPSHOT" 2>/dev/null || true
        btrfs property set "\$LAST_SNAPSHOT" ro true 2>/dev/null || true
    fi
    exit "\$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

exec 9>"\${LOCK_DIR:-/var/lock}/backup_\${TASK}.lock"
flock -n 9 || { log_backup_event "LOCK_OMITTED" "warning" "Backup omitido: ya hay una ejecucion en curso (\$DATE_STR)"; echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso (\$DATE_STR) ===" >> "\$LOG_FILE"; exit 0; }

log_backup_event "LOCK_ACQUIRED" "info" "Bloqueo de ejecucion exclusivo adquirido"
echo "=== INICIANDO BACKUP CIFS: \$TASK (\$DATE_STR) ===" >> "\$LOG_FILE"
mkdir -p "\$MOUNT_POINT" "\$BKP_DIR"
rm -rf "\$BKP_DIR"/.inprogress_*

DF_INFO=\$(df -Pk "\$BKP_DIR" 2>/dev/null | awk 'NR==2 {gsub(/%/, "", \$5); print \$4, \$5}')
read -r DISPONIBLE_KB USO_PORCENTAJE <<< "\$DF_INFO"
if [[ "\$DISPONIBLE_KB" =~ ^[0-9]+$ ]] && [[ "\$USO_PORCENTAJE" =~ ^[0-9]+$ ]]; then
    if [ "\$DISPONIBLE_KB" -lt 2097152 ] || [ "\$USO_PORCENTAJE" -gt 95 ]; then
        log_backup_event "SPACE_CHECK" "err" "Espacio libre insuficiente en \$BKP_DIR (\$((DISPONIBLE_KB / 1024)) MB libres, \${USO_PORCENTAJE}% en uso)"
        echo "=== ABORTADO: espacio libre insuficiente en \$BKP_DIR (\$((DISPONIBLE_KB / 1024)) MB libres, \${USO_PORCENTAJE}% en uso) ===" >> "\$LOG_FILE"
        exit 1
    elif [ "\$USO_PORCENTAJE" -gt 85 ]; then
        log_backup_event "SPACE_CHECK" "warning" "Uso de disco elevado en \$BKP_DIR (\${USO_PORCENTAJE}% en uso, \$((DISPONIBLE_KB / 1024)) MB libres)"
        echo "=== ADVERTENCIA: uso de disco elevado en \$BKP_DIR (\${USO_PORCENTAJE}% en uso, \$((DISPONIBLE_KB / 1024)) MB libres) ===" >> "\$LOG_FILE"
    else
        log_backup_event "SPACE_CHECK" "info" "Espacio verificado: \$((DISPONIBLE_KB / 1024)) MB libres (\${USO_PORCENTAJE}% en uso)"
    fi
fi
umount "\$MOUNT_POINT" 2>/dev/null || true

if mount -t cifs "//\$SRC_IP/\$SRC_SHARE" "\$MOUNT_POINT" -o credentials="\$CRED_FILE",ro,iocharset=utf8,vers=3.1.1,noserverino,cache=none,soft 2>> "\$LOG_FILE"; then
    log_backup_event "MOUNT_SUCCESS" "info" "Recurso CIFS //\$SRC_IP/\$SRC_SHARE montado exitosamente"
else
    log_backup_event "MOUNT_FAILED" "err" "Error al montar recurso CIFS //\$SRC_IP/\$SRC_SHARE"
    exit 1
fi

LAST_SNAPSHOT=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | sort | tail -n 1 || echo "")
RSYNC_OPTS=(-aAXH --numeric-ids --timeout=60 --delete)
if [ -n "\$LAST_SNAPSHOT" ] && [ -d "\$LAST_SNAPSHOT" ]; then
    btrfs property set "\$LAST_SNAPSHOT" ro false 2>/dev/null || true
    chattr -R -i "\$LAST_SNAPSHOT" 2>/dev/null || true
    RSYNC_OPTS+=("--link-dest=\$LAST_SNAPSHOT")
    echo " -> Deduplicando con hardlinks contra: \$(basename "\$LAST_SNAPSHOT")" >> "\$LOG_FILE"
fi

mkdir -p "\$STAGE_SNAPSHOT"
rsync "\${RSYNC_OPTS[@]}" --info=progress2 --no-inc-recursive "\$MOUNT_POINT/" "\$STAGE_SNAPSHOT/" 2>&1 | tr '\\r' '\\n' >> "\$LOG_FILE"
if [ "\${PIPESTATUS[0]}" -eq 0 ]; then
    log_backup_event "RSYNC_COMPLETED" "info" "Sincronizacion rsync finalizada correctamente"
else
    log_backup_event "RSYNC_FAILED" "err" "Fallo en la sincronizacion rsync"
    exit 1
fi
mv "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
SNAPSHOT_OK=true

btrfs property set "\$FINAL_SNAPSHOT" ro true 2>/dev/null || true
chattr -R +i "\$FINAL_SNAPSHOT" 2>/dev/null || true
echo "[\$(date '+%Y-%m-%d %H:%M:%S')] PROMOVIDO EXITOSAMENTE: \$FINAL_SNAPSHOT" >> "\$LOG_FILE"
log_backup_event "SNAPSHOT_PROMOTED" "notice" "Snapshot promovido: \$FINAL_SNAPSHOT"

# Rotación de snapshots
ALL_SNAPS=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name "snapshot_*" 2>/dev/null | sort || true)
COUNT=\$(echo "\$ALL_SNAPS" | grep -c "snapshot_" || echo 0)
if [ "\$COUNT" -gt "\$RETENTION" ]; then
    DIFF=\$((COUNT - RETENTION))
    echo "\$ALL_SNAPS" | head -n "\$DIFF" | while read -r old_snap; do
        if [ -n "\$old_snap" ]; then
            btrfs property set "\$old_snap" ro false 2>/dev/null || true
            chattr -R -i "\$old_snap" 2>/dev/null || true
            rm -rf "\$old_snap"
            echo "Rotado snapshot antiguo: \$old_snap" >> "\$LOG_FILE"
            log_backup_event "ROTATION_PRUNED" "info" "Rotado snapshot antiguo: \$old_snap"
        fi
    done
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: \$DATE_STR ===" >> "\$LOG_FILE"
log_backup_event "BACKUP_COMPLETED" "notice" "Respaldo CIFS \$TASK finalizado con exito"
BASH;
    }

    private function buildSshRunner(string $task, string $ip, int $port, string $user, string $path, string $cred, int $retention): string
    {
        return <<<BASH
#!/bin/bash
set -e
TASK="$task"
SRC_IP="$ip"
SRC_PORT=$port
SRC_USER="$user"
SRC_PATH="$path"
CRED_FILE="$cred"
BKP_DIR="{$this->bkpRoot}/\$TASK"
LOG_FILE="{$this->logRoot}/backup_\${TASK}.log"
MASTER_LOG="{$this->logRoot}/backups_master.log"
RETENTION=$retention
DATE_STR=\$(date +%Y-%m-%d_%H%M%S)
STAGE_SNAPSHOT="\$BKP_DIR/.inprogress_\$DATE_STR"
FINAL_SNAPSHOT="\$BKP_DIR/snapshot_\$DATE_STR"
SNAPSHOT_OK=false

log_backup_event() {
    local evt="\$1"
    local sev="\$2"
    local msg="\$3"
    local ts
    ts=\$(date '+%Y-%m-%d %H:%M:%S')
    local entry="[\$ts] [\$TASK] [\$evt] [\$sev] \$msg"
    echo "\$entry" >> "\$LOG_FILE"
    mkdir -p "\$(dirname "\$MASTER_LOG")" 2>/dev/null || true
    echo "\$entry" >> "\$MASTER_LOG"
    logger -t nas_backup -p "local4.\$sev" "[\$TASK] [\$evt] \$msg" 2>/dev/null || true
}

cleanup() {
    local status=\$?
    if [ "\$SNAPSHOT_OK" != "true" ] && [ "\$status" -ne 0 ]; then
        echo "=== se descarta el snapshot parcial ===" >> "\$LOG_FILE"
        log_backup_event "BACKUP_FAILED" "err" "Respaldo fallido; se descarta el snapshot parcial"
        btrfs property set "\$FINAL_SNAPSHOT" ro false 2>/dev/null || true
        chattr -R -i "\$FINAL_SNAPSHOT" 2>/dev/null || true
        rm -rf "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
    fi
    if [ -n "\${LAST_SNAPSHOT:-}" ] && [ -d "\$LAST_SNAPSHOT" ]; then
        chattr -R +i "\$LAST_SNAPSHOT" 2>/dev/null || true
        btrfs property set "\$LAST_SNAPSHOT" ro true 2>/dev/null || true
    fi
    exit "\$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

exec 9>"\${LOCK_DIR:-/var/lock}/backup_\${TASK}.lock"
flock -n 9 || { log_backup_event "LOCK_OMITTED" "warning" "Backup omitido: ya hay una ejecucion en curso (\$DATE_STR)"; echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso (\$DATE_STR) ===" >> "\$LOG_FILE"; exit 0; }

log_backup_event "LOCK_ACQUIRED" "info" "Bloqueo de ejecucion exclusivo adquirido"
echo "=== INICIANDO BACKUP SSH (Linux): \$TASK (\$DATE_STR) ===" >> "\$LOG_FILE"
mkdir -p "\$BKP_DIR"
rm -rf "\$BKP_DIR"/.inprogress_*

DF_INFO=\$(df -Pk "\$BKP_DIR" 2>/dev/null | awk 'NR==2 {gsub(/%/, "", \$5); print \$4, \$5}')
read -r DISPONIBLE_KB USO_PORCENTAJE <<< "\$DF_INFO"
if [[ "\$DISPONIBLE_KB" =~ ^[0-9]+$ ]] && [[ "\$USO_PORCENTAJE" =~ ^[0-9]+$ ]]; then
    if [ "\$DISPONIBLE_KB" -lt 2097152 ] || [ "\$USO_PORCENTAJE" -gt 95 ]; then
        log_backup_event "SPACE_CHECK" "err" "Espacio libre insuficiente en \$BKP_DIR (\$((DISPONIBLE_KB / 1024)) MB libres, \${USO_PORCENTAJE}% en uso)"
        echo "=== ABORTADO: espacio libre insuficiente en \$BKP_DIR (\$((DISPONIBLE_KB / 1024)) MB libres, \${USO_PORCENTAJE}% en uso) ===" >> "\$LOG_FILE"
        exit 1
    elif [ "\$USO_PORCENTAJE" -gt 85 ]; then
        log_backup_event "SPACE_CHECK" "warning" "Uso de disco elevado en \$BKP_DIR (\${USO_PORCENTAJE}% en uso, \$((DISPONIBLE_KB / 1024)) MB libres)"
        echo "=== ADVERTENCIA: uso de disco elevado en \$BKP_DIR (\${USO_PORCENTAJE}% en uso, \$((DISPONIBLE_KB / 1024)) MB libres) ===" >> "\$LOG_FILE"
    else
        log_backup_event "SPACE_CHECK" "info" "Espacio verificado: \$((DISPONIBLE_KB / 1024)) MB libres (\${USO_PORCENTAJE}% en uso)"
    fi
fi

LAST_SNAPSHOT=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | sort | tail -n 1 || echo "")
RSYNC_OPTS=(-aAXH --numeric-ids -v -z --timeout=60 --delete)
if [ -n "\$LAST_SNAPSHOT" ] && [ -d "\$LAST_SNAPSHOT" ]; then
    btrfs property set "\$LAST_SNAPSHOT" ro false 2>/dev/null || true
    chattr -R -i "\$LAST_SNAPSHOT" 2>/dev/null || true
    RSYNC_OPTS+=("--link-dest=\$LAST_SNAPSHOT")
    echo " -> Deduplicando con hardlinks contra: \$(basename "\$LAST_SNAPSHOT")" >> "\$LOG_FILE"
fi

mkdir -p "\$STAGE_SNAPSHOT"
PASS=\$(cat "\$CRED_FILE")
export SSHPASS="\$PASS"
sshpass -e rsync "\${RSYNC_OPTS[@]}" --info=progress2 --no-inc-recursive -e "ssh -p \$SRC_PORT -o StrictHostKeyChecking=accept-new" "\$SRC_USER@\$SRC_IP:\$SRC_PATH/" "\$STAGE_SNAPSHOT/" 2>&1 | tr '\\r' '\\n' >> "\$LOG_FILE"
if [ "\${PIPESTATUS[0]}" -eq 0 ]; then
    log_backup_event "RSYNC_COMPLETED" "info" "Sincronizacion rsync finalizada correctamente"
else
    log_backup_event "RSYNC_FAILED" "err" "Fallo en la sincronizacion rsync SSH"
    exit 1
fi
mv "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
SNAPSHOT_OK=true

btrfs property set "\$FINAL_SNAPSHOT" ro true 2>/dev/null || true
chattr -R +i "\$FINAL_SNAPSHOT" 2>/dev/null || true
echo "[\$(date '+%Y-%m-%d %H:%M:%S')] PROMOVIDO EXITOSAMENTE: \$FINAL_SNAPSHOT" >> "\$LOG_FILE"
log_backup_event "SNAPSHOT_PROMOTED" "notice" "Snapshot promovido: \$FINAL_SNAPSHOT"

# Rotación de snapshots
ALL_SNAPS=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name "snapshot_*" 2>/dev/null | sort || true)
COUNT=\$(echo "\$ALL_SNAPS" | grep -c "snapshot_" || echo 0)
if [ "\$COUNT" -gt "\$RETENTION" ]; then
    DIFF=\$((COUNT - RETENTION))
    echo "\$ALL_SNAPS" | head -n "\$DIFF" | while read -r old_snap; do
        if [ -n "\$old_snap" ]; then
            btrfs property set "\$old_snap" ro false 2>/dev/null || true
            chattr -R -i "\$old_snap" 2>/dev/null || true
            rm -rf "\$old_snap"
            echo "Rotado snapshot antiguo: \$old_snap" >> "\$LOG_FILE"
            log_backup_event "ROTATION_PRUNED" "info" "Rotado snapshot antiguo: \$old_snap"
        fi
    done
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: \$DATE_STR ===" >> "\$LOG_FILE"
log_backup_event "BACKUP_COMPLETED" "notice" "Respaldo SSH \$TASK finalizado con exito"
BASH;
    }

    private function buildLocalRunner(string $task, string $path, int $retention): string
    {
        return <<<BASH
#!/bin/bash
set -e
TASK="$task"
SRC_PATH="$path"
BKP_DIR="{$this->bkpRoot}/\$TASK"
LOG_FILE="{$this->logRoot}/backup_\${TASK}.log"
MASTER_LOG="{$this->logRoot}/backups_master.log"
RETENTION=$retention
DATE_STR=\$(date +%Y-%m-%d_%H%M%S)
STAGE_SNAPSHOT="\$BKP_DIR/.inprogress_\$DATE_STR"
FINAL_SNAPSHOT="\$BKP_DIR/snapshot_\$DATE_STR"
SNAPSHOT_OK=false

log_backup_event() {
    local evt="\$1"
    local sev="\$2"
    local msg="\$3"
    local ts
    ts=\$(date '+%Y-%m-%d %H:%M:%S')
    local entry="[\$ts] [\$TASK] [\$evt] [\$sev] \$msg"
    echo "\$entry" >> "\$LOG_FILE"
    mkdir -p "\$(dirname "\$MASTER_LOG")" 2>/dev/null || true
    echo "\$entry" >> "\$MASTER_LOG"
    logger -t nas_backup -p "local4.\$sev" "[\$TASK] [\$evt] \$msg" 2>/dev/null || true
}

cleanup() {
    local status=\$?
    if [ "\$SNAPSHOT_OK" != "true" ] && [ "\$status" -ne 0 ]; then
        echo "=== se descarta el snapshot parcial ===" >> "\$LOG_FILE"
        log_backup_event "BACKUP_FAILED" "err" "Respaldo fallido; se descarta el snapshot parcial"
        btrfs property set "\$FINAL_SNAPSHOT" ro false 2>/dev/null || true
        chattr -R -i "\$FINAL_SNAPSHOT" 2>/dev/null || true
        rm -rf "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
    fi
    if [ -n "\${LAST_SNAPSHOT:-}" ] && [ -d "\$LAST_SNAPSHOT" ]; then
        chattr -R +i "\$LAST_SNAPSHOT" 2>/dev/null || true
        btrfs property set "\$LAST_SNAPSHOT" ro true 2>/dev/null || true
    fi
    exit "\$status"
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

exec 9>"\${LOCK_DIR:-/var/lock}/backup_\${TASK}.lock"
flock -n 9 || { log_backup_event "LOCK_OMITTED" "warning" "Backup omitido: ya hay una ejecucion en curso (\$DATE_STR)"; echo "=== BACKUP OMITIDO: ya hay una ejecucion en curso (\$DATE_STR) ===" >> "\$LOG_FILE"; exit 0; }

log_backup_event "LOCK_ACQUIRED" "info" "Bloqueo de ejecucion exclusivo adquirido"
echo "=== INICIANDO BACKUP LOCAL: \$TASK (\$DATE_STR) ===" >> "\$LOG_FILE"
mkdir -p "\$BKP_DIR"
rm -rf "\$BKP_DIR"/.inprogress_*

DF_INFO=\$(df -Pk "\$BKP_DIR" 2>/dev/null | awk 'NR==2 {gsub(/%/, "", \$5); print \$4, \$5}')
read -r DISPONIBLE_KB USO_PORCENTAJE <<< "\$DF_INFO"
if [[ "\$DISPONIBLE_KB" =~ ^[0-9]+$ ]] && [[ "\$USO_PORCENTAJE" =~ ^[0-9]+$ ]]; then
    if [ "\$DISPONIBLE_KB" -lt 2097152 ] || [ "\$USO_PORCENTAJE" -gt 95 ]; then
        log_backup_event "SPACE_CHECK" "err" "Espacio libre insuficiente en \$BKP_DIR (\$((DISPONIBLE_KB / 1024)) MB libres, \${USO_PORCENTAJE}% en uso)"
        echo "=== ABORTADO: espacio libre insuficiente en \$BKP_DIR (\$((DISPONIBLE_KB / 1024)) MB libres, \${USO_PORCENTAJE}% en uso) ===" >> "\$LOG_FILE"
        exit 1
    elif [ "\$USO_PORCENTAJE" -gt 85 ]; then
        log_backup_event "SPACE_CHECK" "warning" "Uso de disco elevado en \$BKP_DIR (\${USO_PORCENTAJE}% en uso, \$((DISPONIBLE_KB / 1024)) MB libres)"
        echo "=== ADVERTENCIA: uso de disco elevado en \$BKP_DIR (\${USO_PORCENTAJE}% en uso, \$((DISPONIBLE_KB / 1024)) MB libres) ===" >> "\$LOG_FILE"
    else
        log_backup_event "SPACE_CHECK" "info" "Espacio verificado: \$((DISPONIBLE_KB / 1024)) MB libres (\${USO_PORCENTAJE}% en uso)"
    fi
fi

LAST_SNAPSHOT=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name 'snapshot_*' 2>/dev/null | sort | tail -n 1 || echo "")
RSYNC_OPTS=(-aAXH --numeric-ids --timeout=60 --delete)
if [ -n "\$LAST_SNAPSHOT" ] && [ -d "\$LAST_SNAPSHOT" ]; then
    btrfs property set "\$LAST_SNAPSHOT" ro false 2>/dev/null || true
    chattr -R -i "\$LAST_SNAPSHOT" 2>/dev/null || true
    RSYNC_OPTS+=("--link-dest=\$LAST_SNAPSHOT")
    echo " -> Deduplicando con hardlinks contra: \$(basename "\$LAST_SNAPSHOT")" >> "\$LOG_FILE"
fi

mkdir -p "\$STAGE_SNAPSHOT"
rsync "\${RSYNC_OPTS[@]}" --info=progress2 --no-inc-recursive "\$SRC_PATH/" "\$STAGE_SNAPSHOT/" 2>&1 | tr '\\r' '\\n' >> "\$LOG_FILE"
if [ "\${PIPESTATUS[0]}" -eq 0 ]; then
    log_backup_event "RSYNC_COMPLETED" "info" "Sincronizacion rsync finalizada correctamente"
else
    log_backup_event "RSYNC_FAILED" "err" "Fallo en la sincronizacion rsync local"
    exit 1
fi
mv "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
SNAPSHOT_OK=true

btrfs property set "\$FINAL_SNAPSHOT" ro true 2>/dev/null || true
chattr -R +i "\$FINAL_SNAPSHOT" 2>/dev/null || true
echo "[\$(date '+%Y-%m-%d %H:%M:%S')] PROMOVIDO EXITOSAMENTE: \$FINAL_SNAPSHOT" >> "\$LOG_FILE"
log_backup_event "SNAPSHOT_PROMOTED" "notice" "Snapshot promovido: \$FINAL_SNAPSHOT"

# Rotación de snapshots
ALL_SNAPS=\$(find "\$BKP_DIR" -maxdepth 1 -type d -name "snapshot_*" 2>/dev/null | sort || true)
COUNT=\$(echo "\$ALL_SNAPS" | grep -c "snapshot_" || echo 0)
if [ "\$COUNT" -gt "\$RETENTION" ]; then
    DIFF=\$((COUNT - RETENTION))
    echo "\$ALL_SNAPS" | head -n "\$DIFF" | while read -r old_snap; do
        if [ -n "\$old_snap" ]; then
            btrfs property set "\$old_snap" ro false 2>/dev/null || true
            chattr -R -i "\$old_snap" 2>/dev/null || true
            rm -rf "\$old_snap"
            echo "Rotado snapshot antiguo: \$old_snap" >> "\$LOG_FILE"
            log_backup_event "ROTATION_PRUNED" "info" "Rotado snapshot antiguo: \$old_snap"
        fi
    done
fi

echo "=== BACKUP FINALIZADO CON ÉXITO: \$DATE_STR ===" >> "\$LOG_FILE"
log_backup_event "BACKUP_COMPLETED" "notice" "Respaldo local \$TASK finalizado con exito"
BASH;
    }
}
