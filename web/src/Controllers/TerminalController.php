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

    public function session(Request $request): void
    {
        $body = $request->getBody();
        $cols = max(40, min((int) ($body['cols'] ?? 200), 400));
        $rows = max(10, min((int) ($body['rows'] ?? 50), 200));
        $res = $this->terminalService->startSession($cols, $rows);
        if (!$res['success']) {
            Response::error($res['error'] ?? 'No se pudo iniciar la sesión de terminal.');
            return;
        }
        Response::success(['output' => $res['output'] ?? '']);
    }

    public function send(Request $request): void
    {
        $body = $request->getBody();
        $data = (string) ($body['data'] ?? '');
        $res = $this->terminalService->send($data);
        if (!$res['success']) {
            Response::error($res['error'] ?? 'No se pudo enviar la entrada a la terminal.');
            return;
        }
        Response::success(['sent' => $data]);
    }

    public function capture(Request $request): void
    {
        $res = $this->terminalService->capture();
        if (!$res['success']) {
            Response::error($res['error'] ?? 'No se pudo capturar la terminal.');
            return;
        }
        Response::success(['output' => $res['output'] ?? '']);
    }

    public function resize(Request $request): void
    {
        $body = $request->getBody();
        $cols = max(40, min((int) ($body['cols'] ?? 200), 400));
        $rows = max(10, min((int) ($body['rows'] ?? 50), 200));
        $this->terminalService->resizeSession($cols, $rows);
        Response::success(null);
    }

    public function kill(Request $request): void
    {
        $this->terminalService->killSession();
        Response::success(null, 'Sesión de terminal finalizada.');
    }
}
