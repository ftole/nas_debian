<?php

declare(strict_types=1);

namespace App\Services;

use Throwable;

/**
 * Servicio de Ejecución Real de Comandos de Terminal Web para Debian 13.
 * Ejecuta comandos en Bash mediante proc_open no bloqueante, manteniendo el
 * directorio de trabajo (cwd), evitando bloqueos de paginadores y registrando
 * el historial en SQLite.
 */
class TerminalService
{
    private const DEFAULT_CWD = '/srv/nas';
    private const TIMEOUT_SECONDS = 15;

    public function execute(string $rawCommand, string $cwd = self::DEFAULT_CWD): array
    {
        $command = trim($rawCommand);
        $cleanCwd = $this->resolveCwd($cwd);

        if ($command === '') {
            $emptyRes = [
                'success' => true,
                'output' => '',
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
            $emptyRes['data'] = $emptyRes;
            return $emptyRes;
        }

        // 1. Manejo nativo de comandos de cierre de sesion de consola
        if ($command === 'exit' || $command === 'logout') {
            $exitMsg = "Esta es una consola web de administración integrada persistente.\n" .
                "• La consola se mantiene abierta para continuar ejecutando comandos.\n" .
                "• Para cerrar la sesión de su cuenta en el panel web, pulse 'Cerrar Sesión' en la barra superior.\n";
            $exitRes = [
                'success' => true,
                'output' => $exitMsg,
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
            $exitRes['data'] = $exitRes;
            return $exitRes;
        }

        // 2. Manejo nativo de comando 'cd' para persistir navegación de directorios (comandos cd puros)
        if ($command === 'cd' || (str_starts_with($command, 'cd ') && !preg_match('/[&|;]/', $command))) {
            return $this->handleCdCommand($command, $cleanCwd);
        }

        // 2. Manejo nativo de 'pwd', 'clear' y 'help'
        if ($command === 'pwd') {
            $pwdRes = [
                'success' => true,
                'output' => $cleanCwd . "\n",
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
            $pwdRes['data'] = $pwdRes;
            return $pwdRes;
        }

        if ($command === 'clear' || $command === 'cls') {
            $clearRes = [
                'success' => true,
                'output' => '',
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'clear' => true,
                'time_ms' => 0,
            ];
            $clearRes['data'] = $clearRes;
            return $clearRes;
        }

        if ($command === 'help' || $command === 'ayuda') {
            $helpOutput = "Servidor NAS Debian 13 (Trixie) - Consola Web de Administración\n" .
                "Directorio de trabajo actual: {$cleanCwd}\n\n" .
                "Comandos rápidos del sistema:\n" .
                "  • ls -la                   Listar archivos con detalles y permisos\n" .
                "  • cd <directorio>          Navegar por el sistema de archivos (/srv/nas, /var/log, etc.)\n" .
                "  • pwd                      Mostrar directorio de trabajo actual\n" .
                "  • cat <archivo>            Visualizar contenido de archivos de texto\n" .
                "  • df -h                    Verificar espacio libre en discos y particiones\n" .
                "  • free -m                  Consultar memoria RAM y Swap en megabytes\n" .
                "  • systemctl status smbd    Consultar estado de servicios (smbd, nmbd, nginx, etc.)\n" .
                "  • sudo <comando>           Ejecutar comandos con privilegios administrativos\n" .
                "  • clear / cls              Limpiar la pantalla de la consola\n\n" .
                "Nota: Comandos interactivos a pantalla completa (su, nano, top, ssh) requieren conexión SSH directa.\n";

            $helpRes = [
                'success' => true,
                'output' => $helpOutput,
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
            $helpRes['data'] = $helpRes;
            return $helpRes;
        }

        // 3. Advertencia preventiva sobre comandos interactivos que requieren TTY
        $interactiveWarning = $this->checkInteractiveCommand($command);
        if ($interactiveWarning !== null) {
            $formattedWarning = "[Aviso de terminal interactiva]:\n" . $interactiveWarning . "\n";
            $warnRes = [
                'success' => true,
                'output' => $formattedWarning,
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 1,
            ];
            $warnRes['data'] = $warnRes;
            return $warnRes;
        }

        // 4. Filtro preventivo de seguridad contra comandos destructivos del SO
        if (preg_match('/rm\s+(-[a-zA-Z]*r[a-zA-Z]*f\s+|-[a-zA-Z]*f[a-zA-Z]*r\s+)\s*\/($|\s)/', $command) ||
            preg_match('/mkfs\s+.*(\/dev\/sd[a-z]|\/dev\/nvme[0-9]n[0-9]p?)[1-9]?/', $command) && !str_contains($command, '--help')) {
            $denyRes = [
                'success' => true,
                'output' => "Acceso denegado: Comando destructivo de sistema no permitido en la consola web.\n",
                'exit_code' => 126,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
            $denyRes['data'] = $denyRes;
            return $denyRes;
        }

        // 5. Modificar 'sudo' para que opere con flag -n (no-interactivo) evitando cuelgues
        $execCommand = $command;
        if (preg_match('/^sudo\s+(?!-n\b)/', $execCommand)) {
            $execCommand = preg_replace('/^sudo\s+/', 'sudo -n ', $execCommand);
        }

        // 6. Si es un comando ping sin límite de paquetes (-c), limitar a 4 para evitar bloqueos infinitos
        if (preg_match('/^(sudo -n\s+)?ping\s+(?!.*-c\b)/i', $execCommand)) {
            $execCommand = preg_replace('/(\bping\s+)/i', '$1-c 4 ', $execCommand);
        }

        // 7. Entorno local Windows (Simulación transparente)
        if (DIRECTORY_SEPARATOR === '\\') {
            $simOutput = sprintf("Simulación de comando en Windows: %s (cwd: %s)\n", $execCommand, $cleanCwd);
            $winRes = [
                'success' => true,
                'output' => $simOutput,
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 5,
            ];
            $winRes['data'] = $winRes;
            return $winRes;
        }

        // 8. Ejecución real con proc_open en Debian 13
        $startTime = microtime(true);
        $descriptors = [
            0 => ['pipe', 'r'],
            1 => ['pipe', 'w'],
            2 => ['pipe', 'w'],
        ];

        $env = array_merge($_ENV, [
            'TERM' => 'xterm-256color',
            'LANG' => 'C.UTF-8',
            'PATH' => '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin',
            'HOME' => self::DEFAULT_CWD,
            'PAGER' => 'cat',
            'SYSTEMD_PAGER' => 'cat',
        ]);

        $cwdToken = '__NAS_CWD_' . bin2hex(random_bytes(6)) . '__';
        $bashScript = sprintf(
            'll() { ls -la "$@"; }; la() { ls -A "$@"; }; l() { ls -CF "$@"; }; %s; __nas_ec=$?; printf "\n%s%%s\n" "$PWD"; exit $__nas_ec',
            $execCommand,
            $cwdToken
        );

        $process = @proc_open(['/bin/bash', '-c', $bashScript], $descriptors, $pipes, $cleanCwd, $env);
        if (!is_resource($process)) {
            $failRes = [
                'success' => true,
                'output' => "Error: No se pudo instanciar el proceso de terminal.\n",
                'exit_code' => 1,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
            $failRes['data'] = $failRes;
            return $failRes;
        }

        fclose($pipes[0]);
        stream_set_blocking($pipes[1], false);
        stream_set_blocking($pipes[2], false);

        $stdout = '';
        $stderr = '';
        $timeout = self::TIMEOUT_SECONDS;
        $deadline = time() + $timeout;

        while (true) {
            $read = [$pipes[1], $pipes[2]];
            $write = null;
            $except = null;
            $numChanged = @stream_select($read, $write, $except, 0, 200000);

            if ($numChanged > 0) {
                foreach ($read as $stream) {
                    $chunk = fread($stream, 8192);
                    if ($chunk !== false && $chunk !== '') {
                        if ($stream === $pipes[1]) {
                            $stdout .= $chunk;
                        } else {
                            $stderr .= $chunk;
                        }
                    }
                }
            }

            $status = proc_get_status($process);
            if (!$status['running']) {
                break;
            }

            if (time() >= $deadline) {
                @proc_terminate($process, 9);
                $stderr .= sprintf("\n[!] Tiempo de ejecución agotado (límite: %d segundos).\n", $timeout);
                break;
            }
        }

        // Leer restos de los buffers
        while (($chunk = fread($pipes[1], 8192)) !== false && $chunk !== '') {
            $stdout .= $chunk;
        }
        while (($chunk = fread($pipes[2], 8192)) !== false && $chunk !== '') {
            $stderr .= $chunk;
        }

        fclose($pipes[1]);
        fclose($pipes[2]);
        $exitCode = proc_close($process);

        // Extraer el nuevo CWD si cambió durante la ejecución de bash (ej: cd compuesto, pushd, etc.)
        if (preg_match('/\n' . preg_quote($cwdToken, '/') . '(.*?)\n?$/s', $stdout, $cwdMatch)) {
            $detectedCwd = trim($cwdMatch[1]);
            if ($detectedCwd !== '' && is_dir($detectedCwd)) {
                $cleanCwd = realpath($detectedCwd) ?: $detectedCwd;
            }
            $stdout = substr($stdout, 0, -strlen($cwdMatch[0]));
        }

        $elapsedMs = (int) round((microtime(true) - $startTime) * 1000);
        $fullOutput = $stdout !== '' ? $stdout : '';
        if ($stderr !== '') {
            $fullOutput .= ($fullOutput !== '' ? "\n" : '') . $stderr;
        }

        // Registrar en historial SQLite
        try {
            $username = $_SESSION['nas_user']['username'] ?? 'sistemas';
            DatabaseService::insert('terminal_history', [
                'command' => $command,
                'username' => (string) $username,
                'cwd' => $cleanCwd,
                'exit_code' => $exitCode,
            ]);
        } catch (Throwable) {
            // Tolerante
        }

        $res = [
            'success' => true,
            'output' => $fullOutput,
            'exit_code' => $exitCode,
            'cwd' => $cleanCwd,
            'time_ms' => $elapsedMs,
        ];
        $res['data'] = $res;
        return $res;
    }

    /**
     * Detecta comandos interactivos incompatibles con consolas web sin pseudo-terminal (PTY).
     */
    public function checkInteractiveCommand(string $command): ?string
    {
        $clean = trim($command);
        if ($clean === '') {
            return null;
        }

        // Quitar sudo y sus modificadores (-u user, -n, etc.) para analizar el binario subyacente
        $cmdWithoutSudo = $clean;
        $isSudo = false;
        if (preg_match('/^sudo(\s+-[a-zA-Z0-9]+(\s+\S+)?)*\s+(.*)$/i', $clean, $m)) {
            $cmdWithoutSudo = trim($m[3]);
            $isSudo = true;
        }

        // 1. Sesiones de cambio de usuario interactivas (su, sudo -i, sudo -s, sudo su, sudo bash, sudo sh)
        if (preg_match('/^su(\s+.*)?$/i', $clean) ||
            preg_match('/^sudo\s+(-i|-s)(\s+.*)?$/i', $clean) ||
            ($isSudo && preg_match('/^su(\s+.*)?$/i', $cmdWithoutSudo)) ||
            ($isSudo && preg_match('/^(bash|sh|zsh|dash|csh|tcsh)(\s+.*)?$/i', $cmdWithoutSudo))) {
            return "El comando '{$clean}' requiere autenticación interactiva en una terminal TTY.\n" .
                "• En esta consola web ya opera con los permisos autorizados de administración.\n" .
                "• Para ejecutar tareas administrativas use directamente: sudo <comando> (ej. 'sudo systemctl restart smbd').\n" .
                "• Para una sesión interactiva root completa con TTY, conéctese directamente por SSH al servidor.";
        }

        // 2. Intento de lanzar subshells interactivas (bash, sh, zsh) sin argumentos de script
        if (preg_match('/^(bash|sh|zsh|dash|csh|tcsh)$/i', $clean)) {
            return "La consola web ya ejecuta cada comando dentro de una subshell Bash interactiva.\n" .
                "• No es necesario invocar '{$clean}'. Escriba directamente cualquier comando que desee ejecutar.\n" .
                "• Para una sesión interactiva SSH completa con TTY, conéctese directamente por SSH al servidor.";
        }

        // 3. Seguimiento continuo de archivos y registros que bloquean indefinidamente (tail -f, journalctl -f)
        if (preg_match('/^tail\s+.*(-f|-F|--follow)\b/i', $cmdWithoutSudo)) {
            return "El comando '{$clean}' realiza un seguimiento continuo bloqueante que requiere Ctrl+C (no disponible en consola web).\n" .
                "• Para consultar los últimos registros en esta consola use: 'tail -n 50 <archivo>'.\n" .
                "• O consulte las bitácoras en tiempo real en la pestaña 'Bitácoras y Auditoría' del panel web.";
        }

        if (preg_match('/^journalctl\s+.*(-f|--follow)\b/i', $cmdWithoutSudo)) {
            return "El parámetro '-f' (follow) en journalctl realiza un seguimiento continuo bloqueante que requiere Ctrl+C.\n" .
                "• Para consultar los últimos eventos del sistema use: 'journalctl -n 50' o 'journalctl -u <servicio> -n 50'.\n" .
                "• O consulte las bitácoras del sistema en la pestaña 'Bitácoras y Auditoría'.";
        }

        // 4. Monitores continuos en bucle infinito
        if (preg_match('/^watch(\s+.*)?$/i', $cmdWithoutSudo)) {
            return "El comando 'watch' es un monitor interactivo en bucle continuo que requiere Ctrl+C.\n" .
                "• Para ejecutar el comando una sola vez retire 'watch' (ej. 'df -h' en lugar de 'watch df -h').\n" .
                "• Para métricas en tiempo real consulte la pestaña 'Dashboard' de este panel web.";
        }

        // Extraer el primer binario del comando
        $parts = preg_split('/\s+/', $cmdWithoutSudo);
        $bin = strtolower(basename($parts[0] ?? ''));

        // 5. Editores de texto interactivos
        if (in_array($bin, ['nano', 'vi', 'vim', 'nvim', 'pico', 'emacs', 'joe', 'jed'], true)) {
            return "El editor interactivo '{$bin}' requiere una terminal interactiva (TTY).\n" .
                "• Para previsualizar o editar archivos en la interfaz gráfica, use el Explorador de Archivos (haga clic en el archivo para el visor/editor integrado con guardado).\n" .
                "• Para visualizar archivos en esta consola use: 'cat <archivo>', 'head -n 30 <archivo>' o 'tail -n 50 <archivo>'.\n" .
                "• Para editar mediante consola interactiva a pantalla completa, conéctese por SSH.";
        }

        // 6. Monitores interactivos y herramientas de pantalla completa
        if (in_array($bin, ['top', 'htop', 'btop', 'atop', 'iotop', 'iftop', 'nmon', 'glances'], true)) {
            return "El monitor interactivo '{$bin}' requiere una terminal interactiva de pantalla completa.\n" .
                "• Para ver procesos de una sola vez use: 'ps aux | head -n 30' o 'ps -ef'.\n" .
                "• Para ver métricas de recursos use: 'free -m', 'df -h' o 'uptime'.\n" .
                "• O consulte las gráficas y métricas en vivo en la pestaña 'Dashboard' de este panel web.";
        }

        // 7. Conexiones remotas y multiplexores interactivos
        if (in_array($bin, ['ssh', 'telnet', 'ftp', 'sftp', 'tmux', 'screen'], true)) {
            return "El comando '{$bin}' requiere una sesión interactiva no soportada en la consola web.\n" .
                "• Ejecute conexiones remotas interactivas directamente desde la consola SSH de su equipo.";
        }

        // 8. Cambio interactivo de credenciales
        if ($bin === 'passwd') {
            return "El comando 'passwd' requiere ingreso interactivo de contraseñas en TTY.\n" .
                "• Para gestionar usuarios y contraseñas utilice la pestaña 'Usuarios y Grupos' del panel web.";
        }

        // 9. Paginadores manuales interactivos
        if (in_array($bin, ['less', 'more', 'man'], true)) {
            return "El comando '{$bin}' requiere control interactivo de teclado (TTY).\n" .
                "• Para visualizar contenido utilice 'cat <archivo>', 'tail -n 50 <archivo>' o agregue '| cat'.";
        }

        return null;
    }

    private function handleCdCommand(string $cmd, string $currentCwd): array
    {
        $target = trim(substr($cmd, 2));

        // Deshacer comillas que puedan envolver la ruta (ej: cd "/tmp" o cd 'VENTAS')
        if ((str_starts_with($target, '"') && str_ends_with($target, '"') && strlen($target) >= 2) ||
            (str_starts_with($target, "'") && str_ends_with($target, "'") && strlen($target) >= 2)) {
            $target = substr($target, 1, -1);
        }
        $target = trim($target);

        // Deshacer escapes de barra invertida en espacios (ej: cd Mi\ Carpeta)
        $target = str_replace('\ ', ' ', $target);

        if ($target === '' || $target === '~') {
            $target = self::DEFAULT_CWD;
        } elseif (str_starts_with($target, '~/')) {
            $target = self::DEFAULT_CWD . substr($target, 1);
        } elseif ($target === '-') {
            $target = $_SESSION['terminal_old_cwd'] ?? self::DEFAULT_CWD;
        }

        if (!str_starts_with($target, '/')) {
            $target = rtrim($currentCwd, '/') . '/' . $target;
        }

        $realTarget = realpath($target);
        if ($realTarget === false || !is_dir($realTarget)) {
            $errRes = [
                'success' => true,
                'output' => sprintf("bash: cd: %s: No existe el fichero o el directorio\n", $target),
                'exit_code' => 1,
                'cwd' => $currentCwd,
                'time_ms' => 1,
            ];
            $errRes['data'] = $errRes;
            return $errRes;
        }

        if ($realTarget !== $currentCwd) {
            $_SESSION['terminal_old_cwd'] = $currentCwd;
        }

        $output = '';
        if (trim(substr($cmd, 2)) === '-') {
            $output = $realTarget . "\n";
        }

        $okRes = [
            'success' => true,
            'output' => $output,
            'exit_code' => 0,
            'cwd' => $realTarget,
            'time_ms' => 1,
        ];
        $okRes['data'] = $okRes;
        return $okRes;
    }

    private function resolveCwd(string $cwd): string
    {
        $clean = trim($cwd);
        if ($clean === '' || !is_dir($clean)) {
            if (is_dir(self::DEFAULT_CWD)) {
                return self::DEFAULT_CWD;
            }
            return '/';
        }
        return realpath($clean) ?: $clean;
    }

    public function getHistory(int $limit = 20): array
    {
        try {
            return DatabaseService::query(
                'SELECT command, executed_at, username, cwd, exit_code FROM terminal_history ORDER BY id DESC LIMIT :lim',
                ['lim' => $limit]
            );
        } catch (Throwable) {
            return [];
        }
    }
}
