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
            return [
                'success' => true,
                'output' => '',
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
        }

        // 1. Manejo nativo de comando 'cd' para persistir navegación de directorios
        if ($command === 'cd' || str_starts_with($command, 'cd ')) {
            return $this->handleCdCommand($command, $cleanCwd);
        }

        // 2. Manejo nativo de 'pwd' y 'clear'
        if ($command === 'pwd') {
            return [
                'success' => true,
                'output' => $cleanCwd . "\n",
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
        }

        if ($command === 'clear' || $command === 'cls') {
            return [
                'success' => true,
                'output' => '',
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'clear' => true,
                'time_ms' => 0,
            ];
        }

        // 3. Filtro preventivo de seguridad contra comandos destructivos del SO
        if (preg_match('/rm\s+(-[a-zA-Z]*r[a-zA-Z]*f\s+|-[a-zA-Z]*f[a-zA-Z]*r\s+)\s*\/($|\s)/', $command) ||
            preg_match('/mkfs\s+.*(\/dev\/sd[a-z]|\/dev\/nvme[0-9]n[0-9]p?)[1-9]?/', $command) && !str_contains($command, '--help')) {
            return [
                'success' => false,
                'output' => "Acceso denegado: Comando destructivo de sistema no permitido en la consola web.\n",
                'exit_code' => 126,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
        }

        // 4. Modificar 'sudo' para que opere con flag -n (no-interactivo) evitando cuelgues
        $execCommand = $command;
        if (preg_match('/^sudo\s+(?!-n\b)/', $execCommand)) {
            $execCommand = preg_replace('/^sudo\s+/', 'sudo -n ', $execCommand);
        }

        // 5. Entorno local Windows (Simulación transparente)
        if (DIRECTORY_SEPARATOR === '\\') {
            $simOutput = sprintf("Simulación de comando en Windows: %s (cwd: %s)\n", $execCommand, $cleanCwd);
            return [
                'success' => true,
                'output' => $simOutput,
                'exit_code' => 0,
                'cwd' => $cleanCwd,
                'time_ms' => 5,
            ];
        }

        // 6. Ejecución real con proc_open en Debian 13
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
            'PAGER' => 'cat',
            'SYSTEMD_PAGER' => 'cat',
        ]);

        $process = @proc_open(['/bin/bash', '-c', $execCommand], $descriptors, $pipes, $cleanCwd, $env);
        if (!is_resource($process)) {
            return [
                'success' => false,
                'output' => "Error: No se pudo instanciar el proceso de terminal.\n",
                'exit_code' => 1,
                'cwd' => $cleanCwd,
                'time_ms' => 0,
            ];
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

        return [
            'success' => $exitCode === 0,
            'output' => $fullOutput,
            'exit_code' => $exitCode,
            'cwd' => $cleanCwd,
            'time_ms' => $elapsedMs,
        ];
    }

    private function handleCdCommand(string $cmd, string $currentCwd): array
    {
        $target = trim(substr($cmd, 2));
        if ($target === '' || $target === '~') {
            $target = self::DEFAULT_CWD;
        }

        if (!str_starts_with($target, '/')) {
            $target = $currentCwd . '/' . $target;
        }

        $realTarget = realpath($target);
        if ($realTarget === false || !is_dir($realTarget)) {
            return [
                'success' => false,
                'output' => sprintf("bash: cd: %s: No existe el directorio o sin permisos.\n", $target),
                'exit_code' => 1,
                'cwd' => $currentCwd,
                'time_ms' => 1,
            ];
        }

        return [
            'success' => true,
            'output' => '',
            'exit_code' => 0,
            'cwd' => $realTarget,
            'time_ms' => 1,
        ];
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
