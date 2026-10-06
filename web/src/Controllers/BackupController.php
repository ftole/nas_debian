<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\AuditService;
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
        $id = trim($data['id'] ?? '');
        if (empty($id)) {
            Response::error('El identificador de la tarea de backup es obligatorio.');
            return;
        }

        $res = $this->backup->createTask($data);
        if (!$res['success']) {
            AuditService::log('backup_create', $id, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al programar tarea de backup.');
            return;
        }

        AuditService::log('backup_create', $id, 'SUCCESS', [
            'proto' => $data['proto'] ?? 'cifs',
            'cron' => $data['cron'] ?? '',
            'retention' => $data['retention'] ?? 30,
        ]);
        Response::success(null, $res['message'] ?? 'Tarea programada.');
    }

    public function delete(Request $request, array $params = []): void
    {
        $id = $params['id'] ?? $request->get('id');
        if (empty($id)) {
            Response::error('Identificador de tarea no especificado.');
            return;
        }

        $cleanId = strtolower(trim((string) $id));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $cleanId)) {
            Response::error('Identificador de tarea inválido.');
            return;
        }

        $deleteBackups = (bool) $request->get('delete_backups', false);
        $res = $this->backup->deleteTask($cleanId, $deleteBackups);

        if (!$res['success']) {
            AuditService::log('backup_delete', $cleanId, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al eliminar tarea de backup.');
            return;
        }

        AuditService::log('backup_delete', $cleanId, 'SUCCESS', ['delete_backups' => $deleteBackups]);
        Response::success(null, $res['message'] ?? 'Tarea eliminada.');
    }

    public function run(Request $request, array $params = []): void
    {
        $id = $params['id'] ?? $request->get('id');
        if (empty($id)) {
            Response::error('Identificador de tarea no especificado.');
            return;
        }

        $cleanId = strtolower(trim((string) $id));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $cleanId)) {
            Response::error('Identificador de tarea inválido.');
            return;
        }

        $res = $this->backup->runTaskNow($cleanId);
        if (!$res['success']) {
            AuditService::log('backup_run_manual', $cleanId, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al lanzar respaldo.');
            return;
        }

        AuditService::log('backup_run_manual', $cleanId, 'SUCCESS');
        Response::success(null, $res['message'] ?? 'Respaldo iniciado.');
    }

    public function logs(Request $request, array $params = []): void
    {
        $id = $params['id'] ?? $request->get('id');
        if (empty($id)) {
            Response::error('Identificador de tarea no especificado.');
            return;
        }

        $cleanId = strtolower(trim((string) $id));
        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $cleanId)) {
            Response::error('Identificador de tarea inválido.');
            return;
        }

        $lines = (int) $request->getQuery('lines', 100);
        $logs = $this->backup->getTaskLogs($cleanId, $lines);

        Response::success(['id' => $cleanId, 'logs' => $logs]);
    }
}
