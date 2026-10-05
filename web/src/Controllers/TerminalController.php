<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\TerminalService;

/**
 * Controlador de la Consola de Terminal Web.
 * Atiende peticiones de ejecución real de comandos del sistema.
 */
class TerminalController
{
    private TerminalService $terminalService;

    public function __construct(?TerminalService $service = null)
    {
        $this->terminalService = $service ?? new TerminalService();
    }

    public function exec(Request $request): void
    {
        $body = $request->getBody();
        $cmd = (string) ($body['cmd'] ?? $body['command'] ?? '');
        $cwd = (string) ($body['cwd'] ?? '/srv/nas');

        $result = $this->terminalService->execute($cmd, $cwd);
        $payload = [
            'success' => true,
            'output' => $result['output'] ?? '',
            'exit_code' => $result['exit_code'] ?? 0,
            'cwd' => $result['cwd'] ?? $cwd,
            'time_ms' => $result['time_ms'] ?? 0,
            'clear' => $result['clear'] ?? false,
            'data' => [
                'output' => $result['output'] ?? '',
                'exit_code' => $result['exit_code'] ?? 0,
                'cwd' => $result['cwd'] ?? $cwd,
                'time_ms' => $result['time_ms'] ?? 0,
                'clear' => $result['clear'] ?? false,
            ],
        ];
        Response::json($payload);
    }

    public function history(Request $request): void
    {
        $limit = (int) ($request->getQuery()['limit'] ?? 20);
        $history = $this->terminalService->getHistory(max(1, min($limit, 100)));
        Response::json([
            'success' => true,
            'data' => $history,
        ]);
    }
}
