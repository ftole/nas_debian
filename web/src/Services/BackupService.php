<?php

declare(strict_types=1);

namespace App\Services;

if (!class_exists(__NAMESPACE__ . '\\SystemService')) {
    require_once __DIR__ . '/SystemService.php';
}
if (!class_exists(__NAMESPACE__ . '\\DatabaseService')) {
    require_once __DIR__ . '/DatabaseService.php';
}
if (!class_exists(__NAMESPACE__ . '\\SambaService')) {
    require_once __DIR__ . '/SambaService.php';
}

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
    public string $storageRoot = '/srv/nas';
    public ?SambaService $sambaService = null;

    /**
     * Lista todas las tareas de backup programadas leyendo los runners en /usr/local/bin/backup_*.sh.
     */
    public function listTasks(): array
    {
        $runners = glob($this->binDir . '/backup_*.sh') ?: [];
        if ($runners === [] && DIRECTORY_SEPARATOR === '\\') {
            return [
                [
                    'id' => 'win_contabilidad',
                    'protocol' => 'CIFS (Windows)',
                    'source' => '//10.10.1.50/Contabilidad',
                    'destination' => '//10.10.1.50/Contabilidad',
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
                    'destination' => 'root@10.10.1.55:/var/www',
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

        $tasks = [];

        foreach ($runners as $runner) {
            $base = basename($runner);
            $taskId = preg_replace('/^backup_|\.sh$/', '', $base);
            $content = (string) @file_get_contents($runner);

            $proto = 'Local';
            $src = 'N/A';
            $ip = '127.0.0.1';
            $retention = 30;

            if (str_contains($content, 'SRC_SHARE=')) {
                $proto = 'CIFS (Windows)';
                preg_match('/SRC_IP=[\'"]?([^\'"\r\n]+)[\'"]?/', $content, $mIp);
                preg_match('/SRC_SHARE=[\'"]?([^\'"\r\n]+)[\'"]?/', $content, $mSh);
                $ip = $mIp[1] ?? '127.0.0.1';
                $src = '//' . ($mIp[1] ?? 'IP') . '/' . ($mSh[1] ?? 'SHARE');
            } elseif (str_contains($content, 'SRC_PORT=') || str_contains($content, 'sshpass')) {
                $proto = 'SSH (Linux)';
                preg_match('/SRC_IP=[\'"]?([^\'"\r\n]+)[\'"]?/', $content, $mIp);
                preg_match('/SRC_PATH=[\'"]?([^\'"\r\n]+)[\'"]?/', $content, $mPt);
                preg_match('/SRC_USER=[\'"]?([^\'"\r\n]+)[\'"]?/', $content, $mUs);
                preg_match('/SRC_PORT=[\'"]?(\d+)[\'"]?/', $content, $mPort);
                $ip = $mIp[1] ?? '127.0.0.1';
                $portVal = (int) ($mPort[1] ?? 22);
                $portSuffix = ($portVal > 0 && $portVal !== 22) ? (':' . $portVal) : '';
                $src = ($mUs[1] ?? 'root') . '@' . ($mIp[1] ?? 'IP') . $portSuffix . ':' . ($mPt[1] ?? '/');
            } else {
                preg_match('/SRC_PATH=[\'"]?([^\'"\r\n]+)[\'"]?/', $content, $mPt);
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
                'ip' => $ip,
                'source' => $src,
                'destination' => $src,
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
        $taskId = strtolower(trim($data['id'] ?? ($data['task_id'] ?? '')));
        $proto = strtolower(trim($data['proto'] ?? ($data['source_type'] ?? 'cifs')));
        if ($proto === 'linux') {
            $proto = 'ssh';
        } elseif ($proto === 'windows') {
            $proto = 'cifs';
        }
        $cronExpr = trim($data['cron'] ?? '0 23 * * *');
        $retention = max(1, (int) ($data['retention'] ?? 30));

        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $taskId)) {
            return ['success' => false, 'error' => 'Identificador de tarea inválido (2-32 caracteres a-z, 0-9, guiones).'];
        }

        if (!$this->validateCron($cronExpr)) {
            return ['success' => false, 'error' => 'Expresión cron no válida (debe tener exactamente 5 campos).'];
        }

        $role = (string) ($_SESSION['nas_user']['role'] ?? '');
        $isOperator = ($role === 'operator');
        if ($isOperator && (!empty($data['script']) || !empty($data['pre_script']) || !empty($data['post_script']))) {
            return ['success' => false, 'error' => 'Operación no permitida: el rol operador no puede configurar scripts personalizados de respaldo.'];
        }

        // Los orígenes se interpolan en runners Bash que se ejecutan como root (cron / sudo):
        // se rechaza cualquier caracter que permita romper el literal o encadenar comandos.
        foreach (['ip', 'target', 'share', 'path', 'source_path', 'user', 'username', 'source_ip', 'source_share', 'source_user', 'local_subfolder'] as $field) {
            if (!array_key_exists($field, $data)) {
                continue;
            }
            $val = (string) $data[$field];
            if ($field === 'local_subfolder') {
                $subErr = $this->validateLocalSubfolder($val, $role);
                if ($subErr !== null) {
                    return ['success' => false, 'error' => $subErr];
                }
            }
            if (preg_match('/[^A-Za-z0-9._:@\/\\\\ -]/', $val) === 1) {
                return ['success' => false, 'error' => "El campo '$field' contiene caracteres no permitidos."];
            }
        }

        if ($proto === 'cifs') {
            $ip = trim($data['ip'] ?? ($data['target'] ?? ($data['source_ip'] ?? '')));
            $share = trim($data['share'] ?? ($data['path'] ?? ($data['source_share'] ?? '')));
            $user = trim($data['user'] ?? ($data['source_user'] ?? ($data['username'] ?? 'Administrador')));
            $pass = (string) ($data['password'] ?? '');

            if (empty($ip) || empty($share)) {
                return ['success' => false, 'error' => 'La IP del servidor Windows y el recurso compartido son obligatorios.'];
            }

            if (str_contains($pass, "\r") || str_contains($pass, "\n")) {
                return ['success' => false, 'error' => 'La contraseña no puede contener saltos de línea.'];
            }

            $originErr = $this->validateOriginHost($ip);
            if ($originErr !== null) {
                return ['success' => false, 'error' => $originErr];
            }

            if (!$this->isAllowedRemoteHost($ip, $role)) {
                return ['success' => false, 'error' => "El host de origen '$ip' no está autorizado para este rol."];
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
            $ip = trim($data['ip'] ?? ($data['target'] ?? ($data['source_ip'] ?? '')));
            $port = (int) ($data['port'] ?? 22);
            $user = trim($data['user'] ?? ($data['source_user'] ?? ($data['username'] ?? 'root')));
            $path = trim($data['path'] ?? ($data['share'] ?? ($data['source_path'] ?? '/')));
            $pass = (string) ($data['password'] ?? '');

            if (empty($ip) || empty($path)) {
                return ['success' => false, 'error' => 'La IP y la ruta del servidor Linux son obligatorias.'];
            }

            if (str_starts_with($user, '-')) {
                return ['success' => false, 'error' => 'El usuario no puede iniciar con guión (-).'];
            }

            if (str_contains($pass, "\r") || str_contains($pass, "\n")) {
                return ['success' => false, 'error' => 'La contraseña no puede contener saltos de línea.'];
            }

            $originErr = $this->validateOriginHost($ip);
            if ($originErr !== null) {
                return ['success' => false, 'error' => $originErr];
            }

            if (!$this->isAllowedRemoteHost($ip, $role)) {
                return ['success' => false, 'error' => "El host de origen '$ip' no está autorizado para este rol."];
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

            $wCred = $this->writeFileSecure($credFile, $pass . "\n", 0600);
            if (!$wCred) {
                return ['success' => false, 'error' => 'No se pudo guardar el archivo de credenciales de respaldo.'];
            }
            $runnerContent = $this->buildSshRunner($taskId, $ip, $port, $user, $path, $credFile, $retention);
        } elseif ($proto === 'local') {
            $path = trim($data['path'] ?? ($data['source_path'] ?? ($data['share'] ?? '/srv/nas/SISTEMAS')));
            if (!$this->isAllowedLocalPath($path)) {
                $storageRoot = getenv('NAS_STORAGE_ROOT') ?: $this->storageRoot;
                return ['success' => false, 'error' => "La ruta local debe estar dentro de $storageRoot."];
            }
            $realPath = realpath($path) ?: $path;

            // Crear directorios de soporte
            $this->ensureDirectory($this->credDir);
            $this->ensureDirectory($this->bkpRoot . '/' . $taskId);
            $this->ensureDirectory($this->logRoot);
            $this->ensureDirectory($this->binDir);
            $this->ensureDirectory($this->cronDir);

            $runnerFile = $this->binDir . '/backup_' . $taskId . '.sh';
            $cronFile = $this->cronDir . '/backup_' . $taskId;

            $runnerContent = $this->buildLocalRunner($taskId, $realPath, $retention);
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
                if (DIRECTORY_SEPARATOR !== '\\') {
                    SystemService::sudo(['chattr', '-R', '-i', $dir]);
                }
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

    /**
     * Lee solo los últimos bytes (por defecto 64 KB) del registro de una tarea mediante fseek
     * para prevenir el consumo excesivo de memoria en bitácoras masivas.
     */
    public function readTaskLog(string $logPathOrTaskId, int $bytes = 65536): string
    {
        $logPath = $logPathOrTaskId;
        if (!file_exists($logPath)) {
            $candidate = $this->logRoot . '/backup_' . strtolower(trim($logPathOrTaskId)) . '.log';
            if (file_exists($candidate)) {
                $logPath = $candidate;
            }
        }

        if (!file_exists($logPath) || !is_readable($logPath)) {
            return '';
        }

        $size = @filesize($logPath);
        if ($size === false || $size === 0) {
            return '';
        }

        $fp = @fopen($logPath, 'rb');
        if (!$fp) {
            return '';
        }

        if ($size > $bytes) {
            fseek($fp, -$bytes, SEEK_END);
        }

        $content = (string) stream_get_contents($fp);
        fclose($fp);

        return $content;
    }

    private function isDev(): bool
    {
        return DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing';
    }

    /**
     * Valida si un host de origen es seguro. Bloquea loopback, link-local y multicast,
     * permitiendo whitelist opcional si NAS_BACKUP_ALLOWED_ORIGINS está definido.
     */
    public function validateOriginHost(string $host): ?string
    {
        $host = trim($host);
        if ($host === '') {
            return 'El host de origen no puede estar vacío.';
        }

        if (str_starts_with($host, '-')) {
            return 'El host de origen no puede iniciar con guión (-).';
        }

        $allowedEnv = getenv('NAS_BACKUP_ALLOWED_ORIGINS');
        if ($allowedEnv !== false && trim($allowedEnv) !== '') {
            $allowedOrigins = array_filter(array_map('trim', explode(',', $allowedEnv)));
            if (!in_array($host, $allowedOrigins, true)) {
                return "El host de origen '$host' no está autorizado. Declárelo en NAS_BACKUP_ALLOWED_ORIGINS.";
            }
            return null;
        }

        $cleanHost = trim($host, '[]');
        $lower = strtolower($cleanHost);
        $blockedNames = ['localhost', '0.0.0.0', 'localhost6', 'ip6-localhost', 'ip6-loopback', '::1', '::'];
        if (in_array($lower, $blockedNames, true)) {
            return "El origen de red '$host' está bloqueado por motivos de seguridad.";
        }

        $ipsToTest = [];
        if (filter_var($cleanHost, FILTER_VALIDATE_IP)) {
            $ipsToTest[] = $cleanHost;
        } else {
            $resolved = @gethostbyname($cleanHost);
            if ($resolved !== $cleanHost && filter_var($resolved, FILTER_VALIDATE_IP)) {
                $ipsToTest[] = $resolved;
            }
            if (function_exists('dns_get_record')) {
                $records = @dns_get_record($cleanHost, DNS_A | DNS_AAAA) ?: [];
                foreach ($records as $r) {
                    if (!empty($r['ip']) && filter_var($r['ip'], FILTER_VALIDATE_IP)) {
                        $ipsToTest[] = $r['ip'];
                    }
                    if (!empty($r['ipv6']) && filter_var($r['ipv6'], FILTER_VALIDATE_IP)) {
                        $ipsToTest[] = $r['ipv6'];
                    }
                }
            }
        }

        // Fail-closed: si el host no se resuelve a ninguna IP verificable (por ejemplo
        // formas numéricas no canónicas como 0x7f000001 que PHP no interpreta pero el
        // resolutor del sistema sí), se rechaza para evitar un diferencial
        // validación/conexión explotable como SSRF.
        if ($ipsToTest === []) {
            return "El origen de red '$host' no pudo resolverse a una dirección IP verificable; se rechaza por seguridad.";
        }

        foreach (array_unique($ipsToTest) as $ipToTest) {
            if ($this->isDangerousIp($ipToTest)) {
                return "El origen de red '$host' ($ipToTest) es una dirección restringida (loopback/link-local/multicast).";
            }
        }

        return null;
    }

    /**
     * Obtiene las direcciones IP asignadas localmente al propio servidor NAS.
     *
     * @return array<string>
     */
    public function getLocalServerIps(): array
    {
        $ips = ['127.0.0.1', '::1'];
        if (!empty($_SERVER['SERVER_ADDR']) && filter_var($_SERVER['SERVER_ADDR'], FILTER_VALIDATE_IP)) {
            $ips[] = $_SERVER['SERVER_ADDR'];
        }
        if (!empty($_SERVER['LOCAL_ADDR']) && filter_var($_SERVER['LOCAL_ADDR'], FILTER_VALIDATE_IP)) {
            $ips[] = $_SERVER['LOCAL_ADDR'];
        }
        $hostname = gethostname();
        if ($hostname !== false) {
            $hostIps = @gethostbynamel($hostname);
            if (is_array($hostIps)) {
                foreach ($hostIps as $hIp) {
                    if (filter_var($hIp, FILTER_VALIDATE_IP)) {
                        $ips[] = $hIp;
                    }
                }
            }
        }
        if (DIRECTORY_SEPARATOR !== '\\') {
            $res = SystemService::runCommand(['hostname', '-I']);
            if ($res['code'] === 0 && !empty($res['stdout'])) {
                foreach (preg_split('/\s+/', trim((string) $res['stdout'])) ?: [] as $ip) {
                    if (filter_var($ip, FILTER_VALIDATE_IP)) {
                        $ips[] = $ip;
                    }
                }
            }
        }
        return array_values(array_unique(array_filter($ips)));
    }

    /**
     * Retorna la lista consolidada de hosts de respaldo preautorizados (/etc/nas/allowed_backup_hosts.conf y SQLite).
     *
     * @return array<string>
     */
    public function getAllowedBackupHosts(): array
    {
        $hosts = [];
        $allowedFile = '/etc/nas/allowed_backup_hosts.conf';
        if (file_exists($allowedFile)) {
            $lines = file($allowedFile, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES) ?: [];
            foreach ($lines as $line) {
                $line = trim($line);
                if ($line !== '' && !str_starts_with($line, '#')) {
                    $hosts[] = $line;
                }
            }
        }

        try {
            $rows = DatabaseService::query("SELECT host FROM allowed_backup_hosts");
            foreach ($rows as $r) {
                if (!empty($r['host'])) {
                    $hosts[] = trim((string) $r['host']);
                }
            }
        } catch (\Throwable $e) {
            // Tabla no presente o base no inicializada
        }

        return array_values(array_unique(array_filter($hosts)));
    }

    /**
     * Registra un host autorizado para respaldos en /etc/nas/allowed_backup_hosts.conf y SQLite (admin/superadmin).
     */
    public function addAllowedBackupHost(string $host): array
    {
        $host = trim($host);
        if ($host === '') {
            return ['success' => false, 'error' => 'El host no puede estar vacío.'];
        }
        if ($this->validateOriginHost($host) !== null) {
            return ['success' => false, 'error' => 'El host especificado no es una dirección válida o segura.'];
        }

        $allowedFile = '/etc/nas/allowed_backup_hosts.conf';
        $dir = dirname($allowedFile);
        if (!is_dir($dir)) {
            @mkdir($dir, 0755, true);
        }
        $existing = $this->getAllowedBackupHosts();
        if (!in_array($host, $existing, true)) {
            @file_put_contents($allowedFile, $host . "\n", FILE_APPEND | LOCK_EX);
            try {
                DatabaseService::execute("CREATE TABLE IF NOT EXISTS allowed_backup_hosts (id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT UNIQUE, created_at DATETIME DEFAULT CURRENT_TIMESTAMP)");
                DatabaseService::insert('allowed_backup_hosts', ['host' => $host]);
            } catch (\Throwable $e) {
                // Silencioso
            }
        }

        return ['success' => true, 'message' => "Host '$host' registrado exitosamente."];
    }

    /**
     * Valida si un host remoto está permitido según la política y el rol de usuario.
     * Bloquea loopback, link-local y las IPs del propio servidor NAS.
     */
    public function isAllowedRemoteHost(string $host, ?string $role = null): bool
    {
        if ($this->validateOriginHost($host) !== null) {
            return false;
        }

        // Bloquear las IPs del propio servidor NAS (además de loopback y link-local)
        $cleanHost = trim($host, '[]');
        $resolvedIps = [];
        if (filter_var($cleanHost, FILTER_VALIDATE_IP)) {
            $resolvedIps[] = $cleanHost;
        } else {
            $resolved = @gethostbyname($cleanHost);
            if ($resolved !== $cleanHost && filter_var($resolved, FILTER_VALIDATE_IP)) {
                $resolvedIps[] = $resolved;
            }
        }

        $localIps = $this->getLocalServerIps();
        foreach ($resolvedIps as $rIp) {
            if (in_array($rIp, $localIps, true)) {
                return false;
            }
        }

        if ($role === null) {
            $role = (string) ($_SESSION['nas_user']['role'] ?? '');
        }

        $preauthorized = $this->getAllowedBackupHosts();

        if ($role === 'operator') {
            // El rol operador se restringe estrictamente a la lista preautorizada
            if ($preauthorized === []) {
                return false;
            }
            if (!in_array($host, $preauthorized, true) && !in_array($cleanHost, $preauthorized, true)) {
                $matched = false;
                foreach ($resolvedIps as $rIp) {
                    if (in_array($rIp, $preauthorized, true)) {
                        $matched = true;
                        break;
                    }
                }
                if (!$matched) {
                    return false;
                }
            }
        }

        return true;
    }

    /**
     * Valida que un subdirectorio local no apunte a rutas prohibidas ni a carpetas
     * compartidas sobre las que el operador carezca de permisos.
     */
    public function validateLocalSubfolder(string $subfolder, ?string $role = null): ?string
    {
        $subfolder = trim($subfolder);
        if ($subfolder === '') {
            return null;
        }

        if (str_contains($subfolder, '..') || str_starts_with($subfolder, '/') || str_starts_with($subfolder, '\\')) {
            return "El subdirectorio local '$subfolder' no es válido o contiene secuencias prohibidas.";
        }

        if (
            str_starts_with($subfolder, '.trash') || $subfolder === '.trash'
            || str_starts_with($subfolder, '.tmp_zip') || $subfolder === '.tmp_zip'
        ) {
            return "El acceso al repositorio de papelera de reciclaje o directorio temporal está restringido.";
        }

        if ($role === null) {
            $role = (string) ($_SESSION['nas_user']['role'] ?? '');
        }

        if ($role === 'operator') {
            $firstSegment = explode('/', str_replace('\\', '/', $subfolder))[0];
            if (str_ends_with($firstSegment, '$') || str_starts_with(strtoupper($firstSegment), 'BACKUPS') || str_starts_with(strtoupper($firstSegment), 'LOGS')) {
                return "Acceso denegado: el rol operador no tiene permisos sobre la carpeta compartida '$firstSegment'.";
            }
            $samba = $this->sambaService ?? new SambaService();
            $shares = $samba->listShares();
            foreach ($shares as $sh) {
                if (strcasecmp((string) ($sh['name'] ?? ''), $firstSegment) === 0) {
                    $scheme = (int) ($sh['scheme'] ?? 1);
                    if ($scheme !== 4) {
                        $userGroups = (array) ($_SESSION['nas_user']['groups'] ?? []);
                        $validUsers = (array) ($sh['valid_users'] ?? []);
                        $hasAccess = false;
                        foreach ($validUsers as $vu) {
                            $vuClean = ltrim((string) $vu, '@');
                            if (in_array($vuClean, $userGroups, true)) {
                                $hasAccess = true;
                                break;
                            }
                        }
                        if (!$hasAccess) {
                            return "Acceso denegado: el rol operador no tiene permisos sobre la carpeta compartida '$firstSegment'.";
                        }
                    }
                    break;
                }
            }
        }

        return null;
    }

    /**
     * Actualiza una tarea de respaldo existente validando permisos y parámetros de seguridad.
     */
    public function updateTask(string $taskId, array $data): array
    {
        $taskId = strtolower(trim($taskId));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $taskId)) {
            return ['success' => false, 'error' => 'Identificador de tarea inválido.'];
        }

        $runnerFile = $this->binDir . '/backup_' . $taskId . '.sh';
        if (!file_exists($runnerFile) && !$this->isDev()) {
            return ['success' => false, 'error' => "La tarea de respaldo [$taskId] no existe."];
        }

        $role = (string) ($_SESSION['nas_user']['role'] ?? '');
        $isOperator = ($role === 'operator');
        if ($isOperator && (!empty($data['script']) || !empty($data['pre_script']) || !empty($data['post_script']))) {
            return ['success' => false, 'error' => 'Operación no permitida: el rol operador no puede configurar o modificar scripts personalizados de respaldo.'];
        }

        if (!empty($data['local_subfolder'])) {
            $subErr = $this->validateLocalSubfolder((string) $data['local_subfolder'], $role);
            if ($subErr !== null) {
                return ['success' => false, 'error' => $subErr];
            }
        }

        $runnerContent = file_exists($runnerFile) ? (string) @file_get_contents($runnerFile) : '';

        // Deducir o normalizar protocolo
        $proto = strtolower(trim((string) ($data['proto'] ?? ($data['source_type'] ?? ''))));
        if ($proto === 'windows') $proto = 'cifs';
        if ($proto === 'linux') $proto = 'ssh';
        if (empty($proto) && $runnerContent !== '') {
            if (str_contains($runnerContent, 'SRC_SHARE=')) {
                $proto = 'cifs';
            } elseif (str_contains($runnerContent, 'SRC_PORT=') || str_contains($runnerContent, 'sshpass')) {
                $proto = 'ssh';
            } else {
                $proto = 'local';
            }
        }
        if (empty($proto)) {
            $proto = 'cifs';
        }
        $data['proto'] = $proto;

        // Si la contraseña viene vacía, conservar la existente en el archivo .cred
        $credFile = $this->credDir . '/' . $taskId . '.cred';
        if (empty($data['password']) && file_exists($credFile)) {
            $credContent = (string) @file_get_contents($credFile);
            if ($proto === 'cifs') {
                if (preg_match('/^password=(.*)$/m', $credContent, $mPass)) {
                    $data['password'] = $mPass[1];
                }
            } else {
                $data['password'] = trim($credContent);
            }
        }

        // Si faltan campos en $data, recuperar del runner existente si está disponible
        if ($runnerContent !== '') {
            if (empty($data['ip']) && empty($data['target']) && empty($data['source_ip'])) {
                if (preg_match('/SRC_IP=[\'"]?([^\'"\r\n]+)[\'"]?/', $runnerContent, $mIp)) {
                    $data['ip'] = $mIp[1];
                }
            }
            if ($proto === 'cifs') {
                if (empty($data['share']) && empty($data['path']) && empty($data['source_share'])) {
                    if (preg_match('/SRC_SHARE=[\'"]?([^\'"\r\n]+)[\'"]?/', $runnerContent, $mSh)) {
                        $data['share'] = $mSh[1];
                    }
                }
                if (empty($data['user']) && empty($data['username']) && empty($data['source_user']) && file_exists($credFile)) {
                    $credRaw = (string) @file_get_contents($credFile);
                    if (preg_match('/^username=(.*)$/m', $credRaw, $mUsr)) {
                        $usrVal = $mUsr[1];
                        if (preg_match('/^domain=(.*)$/m', $credRaw, $mDom) && !empty($mDom[1])) {
                            $usrVal = $mDom[1] . '\\' . $usrVal;
                        }
                        $data['user'] = $usrVal;
                    }
                }
            } elseif ($proto === 'ssh') {
                if (empty($data['path']) && empty($data['share']) && empty($data['source_path'])) {
                    if (preg_match('/SRC_PATH=[\'"]?([^\'"\r\n]+)[\'"]?/', $runnerContent, $mPt)) {
                        $data['path'] = $mPt[1];
                    }
                }
                if (empty($data['user']) && empty($data['username']) && empty($data['source_user'])) {
                    if (preg_match('/SRC_USER=[\'"]?([^\'"\r\n]+)[\'"]?/', $runnerContent, $mUs)) {
                        $data['user'] = $mUs[1];
                    }
                }
                if (empty($data['port'])) {
                    if (preg_match('/SRC_PORT=[\'"]?(\d+)[\'"]?/', $runnerContent, $mPo)) {
                        $data['port'] = (int) $mPo[1];
                    }
                }
            }
            if (empty($data['retention'])) {
                if (preg_match('/RETENTION=(\d+)/', $runnerContent, $mRet)) {
                    $data['retention'] = (int) $mRet[1];
                }
            }
            if (empty($data['cron'])) {
                $cronFile = $this->cronDir . '/backup_' . $taskId;
                if (file_exists($cronFile)) {
                    $cLine = trim((string) @file_get_contents($cronFile));
                    $parts = preg_split('/\s+/', $cLine);
                    if (count($parts) >= 5) {
                        $data['cron'] = implode(' ', array_slice($parts, 0, 5));
                    }
                }
            }
        }

        $data['id'] = $taskId;
        $data['task_id'] = $taskId;

        return $this->createTask($data);
    }

    /**
     * Comprueba si existen respaldos en curso verificando bloqueos /var/lock/backup_*.lock
     * y procesos rsync o runners activos.
     *
     * @return array{running: bool, count: int, tasks: array<string>, rsync_active: bool, locks: array<string>}
     */
    public function getRunningBackups(): array
    {
        $runningTasks = [];
        $activeLocks = [];

        // Buscar bloqueos en /var/lock y /run/lock
        $dirs = ['/var/lock', '/run/lock'];
        foreach ($dirs as $dir) {
            $lockFiles = glob($dir . '/backup_*.lock') ?: [];
            foreach ($lockFiles as $lf) {
                $fp = @fopen($lf, 'c+');
                if ($fp) {
                    if (!@flock($fp, LOCK_EX | LOCK_NB)) {
                        // El bloqueo flock está adquirido activamente por un runner
                        $baseName = basename($lf);
                        $activeLocks[] = $baseName;
                        if (preg_match('/^backup_(.+)\.lock$/', $baseName, $m)) {
                            $runningTasks[] = $m[1];
                        }
                    } else {
                        @flock($fp, LOCK_UN);
                    }
                    @fclose($fp);
                }
            }
        }

        // Verificar procesos rsync activos
        $rsyncActive = false;
        $pgRsync = SystemService::runCommand(['pgrep', '-x', 'rsync']);
        if ($pgRsync['code'] === 0 && trim((string) $pgRsync['stdout']) !== '') {
            $rsyncActive = true;
        }

        // Verificar procesos de runners de respaldo
        $pgRunners = SystemService::runCommand(['pgrep', '-f', 'backup_.*\.sh']);
        if ($pgRunners['code'] === 0 && trim((string) $pgRunners['stdout']) !== '') {
            $lines = explode("\n", trim((string) $pgRunners['stdout']));
            foreach ($lines as $line) {
                if (trim($line) === '') continue;
                $cmdInfo = SystemService::runCommand(['ps', '-p', trim($line), '-o', 'args=']);
                if ($cmdInfo['code'] === 0 && preg_match('/backup_([a-z0-9_-]+)\.sh/', (string) $cmdInfo['stdout'], $m)) {
                    $runningTasks[] = $m[1];
                }
            }
        }

        // Revisar tareas según getTaskStatus() para mayor consistencia
        if (empty($runningTasks) && !$this->isDev()) {
            $runners = glob($this->binDir . '/backup_*.sh') ?: [];
            foreach ($runners as $runner) {
                $base = basename($runner);
                $tid = preg_replace('/^backup_|\.sh$/', '', $base);
                if ($this->getTaskStatus($tid) === 'running') {
                    $runningTasks[] = $tid;
                }
            }
        }

        $runningTasks = array_values(array_unique($runningTasks));
        $isRunning = !empty($runningTasks) || !empty($activeLocks) || $rsyncActive;

        return [
            'running' => $isRunning,
            'count' => count($runningTasks),
            'tasks' => $runningTasks,
            'rsync_active' => $rsyncActive,
            'locks' => array_values(array_unique($activeLocks)),
        ];
    }

    public function isDangerousIp(string $ip): bool
    {
        $bin = @inet_pton($ip);
        if ($bin === false) {
            return false;
        }
        // IPv4 (4 bytes)
        if (strlen($bin) === 4) {
            $b0 = ord($bin[0]);
            $b1 = ord($bin[1]);
            if ($b0 === 0) return true; // 0.0.0.0/8
            if ($b0 === 127) return true; // 127.0.0.0/8 Loopback
            if ($b0 === 169 && $b1 === 254) return true; // 169.254.0.0/16 Link-local
            if ($b0 >= 224 && $b0 <= 239) return true; // 224.0.0.0/4 Multicast
            if ($b0 >= 240) return true; // 240.0.0.0/4 Reserved
            return false;
        }
        // IPv6 (16 bytes)
        if (strlen($bin) === 16) {
            if ($bin === str_repeat("\0", 16)) return true; // ::
            if ($bin === (str_repeat("\0", 15) . "\1")) return true; // ::1
            $b0 = ord($bin[0]);
            $b1 = ord($bin[1]);
            if ($b0 === 0xfe && ($b1 & 0xc0) === 0x80) return true; // fe80::/10 Link-local
            if ($b0 === 0xff) return true; // ff00::/8 Multicast
            if (str_starts_with($bin, str_repeat(chr(0), 10) . chr(0xff) . chr(0xff))) {
                $ipv4Part = inet_ntop(substr($bin, 12));
                if ($ipv4Part !== false) {
                    return $this->isDangerousIp($ipv4Part);
                }
            }
            return false;
        }
        return false;
    }

    /**
     * Valida que una ruta local exista y esté contenida estrictamente en la raíz de almacenamiento permitida.
     * Excluye el directorio de repositorios de respaldo para evitar recursión destructiva.
     */
    public function isAllowedLocalPath(string $path): bool
    {
        $realPath = realpath($path);
        if ($realPath === false) {
            return false;
        }

        // El repositorio de copias de seguridad no debe utilizarse recursivamente como origen
        $bkpRoot = $this->bkpRoot;
        $realBkp = realpath($bkpRoot) ?: $bkpRoot;
        if ($realPath === $realBkp || str_starts_with($realPath, rtrim($realBkp, DIRECTORY_SEPARATOR) . DIRECTORY_SEPARATOR)) {
            return false;
        }

        $allowedRoots = [
            getenv('NAS_STORAGE_ROOT') ?: $this->storageRoot,
            $this->storageRoot,
            dirname($this->bkpRoot),
        ];

        foreach ($allowedRoots as $root) {
            $realRoot = realpath($root);
            if ($realRoot !== false) {
                if ($realPath === $realRoot || str_starts_with($realPath, rtrim($realRoot, DIRECTORY_SEPARATOR) . DIRECTORY_SEPARATOR)) {
                    return true;
                }
            }
        }
        return false;
    }

    /**
     * Prueba la conexión al origen de la tarea (CIFS/SMB, SSH o Local) sin crear la tarea.
     */
    public function testConnection(array $data): array
    {
        $proto = strtolower(trim($data['proto'] ?? 'cifs'));

        if ($proto === 'cifs') {
            $ip = trim($data['ip'] ?? ($data['target'] ?? ($data['source_ip'] ?? '')));
            $share = trim($data['share'] ?? ($data['path'] ?? ($data['source_share'] ?? '')));
            $user = trim($data['user'] ?? ($data['source_user'] ?? ''));
            $pass = (string) ($data['password'] ?? '');
            if ($ip === '' || $share === '') {
                return ['success' => false, 'error' => 'IP y recurso compartido son obligatorios.'];
            }
            if (str_contains($pass, "\r") || str_contains($pass, "\n")) {
                return ['success' => false, 'error' => 'La contraseña no puede contener saltos de línea.'];
            }
            $originErr = $this->validateOriginHost($ip);
            if ($originErr !== null) {
                return ['success' => false, 'error' => $originErr];
            }
            if (!$this->isAllowedRemoteHost($ip)) {
                return ['success' => false, 'error' => "El host de origen '$ip' no está autorizado."];
            }
            if ($this->isDev()) {
                return ['success' => true, 'message' => 'Conexión de prueba correcta (modo dev).'];
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
            $ip = trim($data['ip'] ?? ($data['target'] ?? ($data['source_ip'] ?? '')));
            $port = (int) ($data['port'] ?? 22);
            $user = trim($data['user'] ?? ($data['source_user'] ?? 'root'));
            $pass = (string) ($data['password'] ?? '');
            if ($ip === '') {
                return ['success' => false, 'error' => 'La IP del servidor Linux es obligatoria.'];
            }
            if (str_starts_with($user, '-') || !preg_match('/^[A-Za-z0-9_.-]{1,32}$/', $user)) {
                return ['success' => false, 'error' => 'Usuario SSH no válido.'];
            }
            if (!preg_match('/^[A-Za-z0-9_.:-]{1,255}$/', $ip)) {
                return ['success' => false, 'error' => 'Host o IP SSH no válido.'];
            }
            if (str_contains($pass, "\r") || str_contains($pass, "\n")) {
                return ['success' => false, 'error' => 'La contraseña no puede contener saltos de línea.'];
            }
            $originErr = $this->validateOriginHost($ip);
            if ($originErr !== null) {
                return ['success' => false, 'error' => $originErr];
            }
            if (!$this->isAllowedRemoteHost($ip)) {
                return ['success' => false, 'error' => "El host de origen '$ip' no está autorizado."];
            }
            if ($this->isDev()) {
                return ['success' => true, 'message' => 'Conexión de prueba correcta (modo dev).'];
            }
            $tmp = @tempnam(sys_get_temp_dir(), 'nas_ssh_');
            if ($tmp === false) {
                return ['success' => false, 'error' => 'No se pudo preparar la prueba SSH.'];
            }
            file_put_contents($tmp, $pass . "\n");
            @chmod($tmp, 0600);
            $res = SystemService::runCommand(
                ['sshpass', '-f', $tmp, 'ssh', '-p', (string) $port, '-o', 'StrictHostKeyChecking=accept-new',
                 '-o', 'UserKnownHostsFile=/dev/null', '-o', 'ConnectTimeout=8', '-l', $user, '--', $ip, 'echo', 'NAS_OK'], null, 20);
            @unlink($tmp);
            if ($res['code'] === 0 && str_contains($res['stdout'], 'NAS_OK')) {
                return ['success' => true, 'message' => "Conexión SSH correcta a $user@$ip:$port."];
            }
            return ['success' => false, 'error' => 'No se pudo conectar por SSH: ' . trim($res['stderr'] ?: $res['stdout'])];
        }

        if ($proto === 'local') {
            $path = trim($data['path'] ?? ($data['source_path'] ?? ($data['share'] ?? '')));
            if ($path === '') {
                return ['success' => false, 'error' => 'La ruta local es obligatoria.'];
            }
            if (!$this->isAllowedLocalPath($path)) {
                $storageRoot = getenv('NAS_STORAGE_ROOT') ?: $this->storageRoot;
                return ['success' => false, 'error' => "La ruta local debe estar dentro de $storageRoot."];
            }
            $realPath = realpath($path) ?: $path;
            if ($this->isDev()) {
                return ['success' => true, 'message' => "Ruta local accesible: $realPath."];
            }
            $res = SystemService::runCommand(['test', '-d', $realPath]);
            if ($res['code'] === 0) {
                return ['success' => true, 'message' => "Ruta local accesible: $realPath."];
            }
            return ['success' => false, 'error' => "La ruta local no existe o no es accesible: $realPath."];
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

        $classified = $this->classifyLatestRun($this->readTaskLog($log));
        return $classified === 'idle' ? 'idle' : $classified;
    }

    /**
     * Clasifica el resultado de la ÚLTIMA ejecución a partir del contenido del log.
     * Los logs son acumulativos: se analiza únicamente el segmento posterior al
     * último LOCK_ACQUIRED/INICIANDO BACKUP para no arrastrar fallos anteriores.
     *
     * @return string 'ok' | 'error' | 'idle'
     */
    public function classifyLatestRun(string $content): string
    {
        $lines = preg_split('/\r\n|\r|\n/', $content);
        if (!is_array($lines)) {
            return 'idle';
        }

        $startIdx = 0;
        for ($i = count($lines) - 1; $i >= 0; $i--) {
            if (str_contains($lines[$i], 'LOCK_ACQUIRED') || str_contains($lines[$i], 'INICIANDO BACKUP')) {
                $startIdx = $i;
                break;
            }
        }
        $segment = strtoupper(implode("\n", array_slice($lines, $startIdx)));

        if (str_contains($segment, 'BACKUP FINALIZADO CON ÉXITO')
            || str_contains($segment, 'PROMOVIDO EXITOSAMENTE')
            || str_contains($segment, 'BACKUP_COMPLETED')) {
            return 'ok';
        }
        if (str_contains($segment, 'BACKUP_FAILED')
            || str_contains($segment, 'RSYNC_FAILED')
            || str_contains($segment, 'MOUNT_FAILED')
            || str_contains($segment, 'ABORTADO')) {
            return 'error';
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
        $lines = preg_split('/\r\n|\r|\n/', $this->readTaskLog($log));
        if (!is_array($lines)) {
            return $result;
        }

        $start = null;
        $startIdx = 0;
        for ($i = count($lines) - 1; $i >= 0; $i--) {
            if (str_contains($lines[$i], 'LOCK_ACQUIRED') && preg_match('/\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]/', $lines[$i], $m)) {
                $start = strtotime($m[1]);
                $startIdx = $i;
                break;
            }
        }
        if ($start === null) {
            foreach ($lines as $ln) {
                if (preg_match('/^\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]/', $ln, $m)) {
                    $start = strtotime($m[1]);
                    break;
                }
            }
        }

        // El progreso también debe provenir solo de la última ejecución.
        $percent = 0;
        $speed = '';
        for ($i = count($lines) - 1; $i >= $startIdx; $i--) {
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
        if ($status === 'ok') {
            $result['percent'] = 100;
            $result['remaining'] = '00:00:00';
        } elseif ($percent > 0 && $elapsed > 0) {
            $result['remaining'] = $this->formatDuration((int) round($elapsed * (100 - $percent) / $percent));
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

        $stagingDir = '/var/lib/nas/staging';
        $tmpDir = (is_dir($stagingDir) && is_writable($stagingDir)) ? $stagingDir : sys_get_temp_dir();
        $tmp = tempnam($tmpDir, 'nas_');
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
        if (is_link($dir)) {
            @unlink($dir);
            return;
        }
        $items = scandir($dir);
        if ($items === false) return;
        foreach ($items as $item) {
            if ($item === '.' || $item === '..') continue;
            $path = $dir . '/' . $item;
            if (is_link($path)) {
                @unlink($path);
            } elseif (is_dir($path)) {
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
        $escTask = escapeshellarg($task);
        $escIp = escapeshellarg($ip);
        $escShare = escapeshellarg($share);
        $escCred = escapeshellarg($cred);

        return <<<BASH
#!/bin/bash
set -e
TASK=$escTask
SRC_IP=$escIp
SRC_SHARE=$escShare
CRED_FILE=$escCred
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
    local entry="[\$ts] [\$TASK] [\$SRC_IP] [\$evt] [\$sev] \$msg"
    echo "\$entry" >> "\$LOG_FILE"
    mkdir -p "\$(dirname "\$MASTER_LOG")" 2>/dev/null || true
    echo "\$entry" >> "\$MASTER_LOG"
    logger -t nas_backup -p "local4.\$sev" "[\$TASK] [\$SRC_IP] [\$evt] \$msg" 2>/dev/null || true
}

cleanup() {
    local status=\$?
    umount "\$MOUNT_POINT" 2>/dev/null || true
    if [ "\$SNAPSHOT_OK" != "true" ] && [ "\$status" -ne 0 ]; then
        echo "=== se descarta el snapshot parcial ===" >> "\$LOG_FILE"
        log_backup_event "BACKUP_FAILED" "err" "Respaldo fallido; se descarta el snapshot parcial"
        if btrfs subvolume show "\$FINAL_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$FINAL_SNAPSHOT" ro false 2>/dev/null || true; fi
        chattr -R -i "\$FINAL_SNAPSHOT" 2>/dev/null || true
        rm -rf "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
    fi
    if [ -n "\${LAST_SNAPSHOT:-}" ] && [ -d "\$LAST_SNAPSHOT" ]; then
        chattr -R +i "\$LAST_SNAPSHOT" 2>/dev/null || true
        if btrfs subvolume show "\$LAST_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$LAST_SNAPSHOT" ro true 2>/dev/null || true; fi
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
RSYNC_OPTS=(-aAXH --safe-links --numeric-ids --timeout=60 --delete)
if [ -n "\$LAST_SNAPSHOT" ] && [ -d "\$LAST_SNAPSHOT" ]; then
    if btrfs subvolume show "\$LAST_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$LAST_SNAPSHOT" ro false 2>/dev/null || true; fi
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

if btrfs subvolume show "\$FINAL_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$FINAL_SNAPSHOT" ro true 2>/dev/null || true; fi
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
            if btrfs subvolume show "\$old_snap" >/dev/null 2>&1; then btrfs property set "\$old_snap" ro false 2>/dev/null || true; fi
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
        $escTask = escapeshellarg($task);
        $escIp = escapeshellarg($ip);
        $escUser = escapeshellarg($user);
        $escPath = escapeshellarg($path);
        $escCred = escapeshellarg($cred);

        return <<<BASH
#!/bin/bash
set -e
TASK=$escTask
SRC_IP=$escIp
SRC_PORT=$port
SRC_USER=$escUser
SRC_PATH=$escPath
CRED_FILE=$escCred
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
    local entry="[\$ts] [\$TASK] [\$SRC_IP] [\$evt] [\$sev] \$msg"
    echo "\$entry" >> "\$LOG_FILE"
    mkdir -p "\$(dirname "\$MASTER_LOG")" 2>/dev/null || true
    echo "\$entry" >> "\$MASTER_LOG"
    logger -t nas_backup -p "local4.\$sev" "[\$TASK] [\$SRC_IP] [\$evt] \$msg" 2>/dev/null || true
}

cleanup() {
    local status=\$?
    if [ "\$SNAPSHOT_OK" != "true" ] && [ "\$status" -ne 0 ]; then
        echo "=== se descarta el snapshot parcial ===" >> "\$LOG_FILE"
        log_backup_event "BACKUP_FAILED" "err" "Respaldo fallido; se descarta el snapshot parcial"
        if btrfs subvolume show "\$FINAL_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$FINAL_SNAPSHOT" ro false 2>/dev/null || true; fi
        chattr -R -i "\$FINAL_SNAPSHOT" 2>/dev/null || true
        rm -rf "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
    fi
    if [ -n "\${LAST_SNAPSHOT:-}" ] && [ -d "\$LAST_SNAPSHOT" ]; then
        chattr -R +i "\$LAST_SNAPSHOT" 2>/dev/null || true
        if btrfs subvolume show "\$LAST_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$LAST_SNAPSHOT" ro true 2>/dev/null || true; fi
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
RSYNC_OPTS=(-aAXH --safe-links --numeric-ids -v -z --timeout=60 --delete)
if [ -n "\$LAST_SNAPSHOT" ] && [ -d "\$LAST_SNAPSHOT" ]; then
    if btrfs subvolume show "\$LAST_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$LAST_SNAPSHOT" ro false 2>/dev/null || true; fi
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

if btrfs subvolume show "\$FINAL_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$FINAL_SNAPSHOT" ro true 2>/dev/null || true; fi
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
            if btrfs subvolume show "\$old_snap" >/dev/null 2>&1; then btrfs property set "\$old_snap" ro false 2>/dev/null || true; fi
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
        $escTask = escapeshellarg($task);
        $escPath = escapeshellarg($path);

        return <<<BASH
#!/bin/bash
set -e
TASK=$escTask
SRC_IP="127.0.0.1"
SRC_PATH=$escPath
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
    local entry="[\$ts] [\$TASK] [\$SRC_IP] [\$evt] [\$sev] \$msg"
    echo "\$entry" >> "\$LOG_FILE"
    mkdir -p "\$(dirname "\$MASTER_LOG")" 2>/dev/null || true
    echo "\$entry" >> "\$MASTER_LOG"
    logger -t nas_backup -p "local4.\$sev" "[\$TASK] [\$SRC_IP] [\$evt] \$msg" 2>/dev/null || true
}

cleanup() {
    local status=\$?
    if [ "\$SNAPSHOT_OK" != "true" ] && [ "\$status" -ne 0 ]; then
        echo "=== se descarta el snapshot parcial ===" >> "\$LOG_FILE"
        log_backup_event "BACKUP_FAILED" "err" "Respaldo fallido; se descarta el snapshot parcial"
        if btrfs subvolume show "\$FINAL_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$FINAL_SNAPSHOT" ro false 2>/dev/null || true; fi
        chattr -R -i "\$FINAL_SNAPSHOT" 2>/dev/null || true
        rm -rf "\$STAGE_SNAPSHOT" "\$FINAL_SNAPSHOT"
    fi
    if [ -n "\${LAST_SNAPSHOT:-}" ] && [ -d "\$LAST_SNAPSHOT" ]; then
        chattr -R +i "\$LAST_SNAPSHOT" 2>/dev/null || true
        if btrfs subvolume show "\$LAST_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$LAST_SNAPSHOT" ro true 2>/dev/null || true; fi
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
RSYNC_OPTS=(-aAXH --safe-links --numeric-ids --timeout=60 --delete)
if [ -n "\$LAST_SNAPSHOT" ] && [ -d "\$LAST_SNAPSHOT" ]; then
    if btrfs subvolume show "\$LAST_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$LAST_SNAPSHOT" ro false 2>/dev/null || true; fi
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

if btrfs subvolume show "\$FINAL_SNAPSHOT" >/dev/null 2>&1; then btrfs property set "\$FINAL_SNAPSHOT" ro true 2>/dev/null || true; fi
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
            if btrfs subvolume show "\$old_snap" >/dev/null 2>&1; then btrfs property set "\$old_snap" ro false 2>/dev/null || true; fi
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
