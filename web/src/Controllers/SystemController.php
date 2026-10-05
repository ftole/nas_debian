<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\AuditService;
use App\Services\SystemService;

/**
 * Controlador API para operaciones del sistema, control de demonios, logs y reinicio.
 */
class SystemController
{
    private SystemService $system;

    public function __construct()
    {
        $this->system = new SystemService();
    }

    public function services(Request $request): void
    {
        $services = $this->system->getServicesStatus();
        Response::success($services);
    }

    public function manageService(Request $request): void
    {
        $data = $request->getBody();
        $service = trim($data['service'] ?? '');
        $action = trim($data['action'] ?? 'restart');

        if (empty($service)) {
            Response::error('El nombre del servicio es obligatorio.');
            return;
        }

        $res = $this->system->manageService($service, $action);
        if (!$res['success']) {
            AuditService::log('service_manage', $service, 'FAILED', ['action' => $action, 'error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al gestionar servicio.');
            return;
        }

        AuditService::log('service_manage', $service, 'SUCCESS', ['action' => $action]);
        Response::success(null, $res['message'] ?? 'Servicio actualizado.');
    }

    public function logs(Request $request): void
    {
        $limit = max(5, min(500, (int) $request->getQuery('limit', 100)));
        $source = (string) $request->getQuery('source', 'all');
        $query = $request->getQuery('q');
        $qStr = (is_string($query) && trim($query) !== '') ? trim($query) : null;

        $logs = $this->system->getLogs($source, $limit, $qStr);
        Response::success($logs);
    }

    public function reboot(Request $request): void
    {
        $res = $this->system->rebootServer();
        if ($res['code'] !== 0) {
            AuditService::log('server_reboot', 'sistema', 'FAILED', ['error' => $res['stderr'] ?: $res['stdout']]);
            Response::error('Error al solicitar reinicio: ' . ($res['stderr'] ?: $res['stdout']));
            return;
        }

        AuditService::log('server_reboot', 'sistema', 'SUCCESS');
        Response::success(null, 'Reinicio del servidor programado.');
    }

    public function updates(Request $request): void
    {
        $info = $this->system->checkUpdates();
        Response::success($info);
    }

    public function diagnostics(Request $request): void
    {
        $services = $this->system->getServicesStatus();
        $metrics = $this->system->getSystemMetrics();
        $storage = (new \App\Services\StorageService())->getStorageOverview();
        $shares = (new \App\Services\SambaService())->listShares();
        $backups = (new \App\Services\BackupService())->listTasks();

        $activeServices = 0;
        $totalServices = count($services);
        foreach ($services as $s) {
            if (!empty($s['active'])) {
                $activeServices++;
            }
        }

        // Validación testparm de Samba
        $testparmOk = true;
        if (DIRECTORY_SEPARATOR !== '\\' && getenv('APP_ENV') !== 'testing') {
            $tpRes = SystemService::sudo(['testparm', '-s']);
            if ($tpRes['code'] !== 0) {
                $testparmOk = false;
            }
        }

        $usagePct = (float) ($storage['usage_percent'] ?? 0);
        $overall = ($activeServices === $totalServices && $testparmOk && $usagePct < 90) ? 'OK' : 'Warning';

        Response::success([
            'overall_status' => $overall,
            'services_active' => $activeServices,
            'services_total' => $totalServices,
            'services' => $services,
            'storage' => $storage,
            'shares_count' => count($shares),
            'backups_count' => count($backups),
            'testparm_ok' => $testparmOk,
            'hostname' => $metrics['hostname'] ?? 'SRV-NAS',
            'uptime' => $metrics['uptime'] ?? 'N/A',
            'kernel' => $metrics['kernel'] ?? php_uname('r'),
        ]);
    }
}
