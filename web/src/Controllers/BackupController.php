<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\BackupService;

/**
 * Controlador API para la central de respaldos y réplicas multiplataforma.
 */
class BackupController
{
    private BackupService $backup;

    public function __construct()
    {
        $this->backup = new BackupService();
    }

    public function list(Request $request): void
    {
        $tasks = $this->backup->listTasks();
        Response::success($tasks);
    }

    public function create(Request $request): void
    {
        $data = $request->getBody();
        if (empty($data['id'])) {
            Response::error('El identificador de la tarea de backup es obligatorio.');
            return;
        }

        $res = $this->backup->createTask($data);
        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al programar tarea de backup.');
            return;
        }

        Response::success(null, $res['message'] ?? 'Tarea programada.');
    }

    public function delete(Request $request, array $params): void
    {
        $id = $params['id'] ?? $request->get('id');
        if (empty($id)) {
            Response::error('Identificador de tarea no especificado.');
            return;
        }

        $deleteBackups = (bool) $request->get('delete_backups', false);
        $res = $this->backup->deleteTask($id, $deleteBackups);

        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al eliminar tarea de backup.');
            return;
        }

        Response::success(null, $res['message'] ?? 'Tarea eliminada.');
    }

    public function run(Request $request, array $params): void
    {
        $id = $params['id'] ?? $request->get('id');
        if (empty($id)) {
            Response::error('Identificador de tarea no especificado.');
            return;
        }

        $res = $this->backup->runTaskNow($id);
        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al lanzar respaldo.');
            return;
        }

        Response::success(null, $res['message'] ?? 'Respaldo iniciado.');
    }

    public function logs(Request $request, array $params): void
    {
        $id = $params['id'] ?? $request->get('id');
        if (empty($id)) {
            Response::error('Identificador de tarea no especificado.');
            return;
        }

        $lines = (int) $request->getQuery('lines', 100);
        $logs = $this->backup->getTaskLogs($id, $lines);

        Response::success(['id' => $id, 'logs' => $logs]);
    }
}
