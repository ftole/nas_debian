<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\BackupService;
use App\Services\SambaService;
use App\Services\StorageService;
use App\Services\SystemService;
use App\Services\UserService;

/**
 * Controlador de la vista principal y agregador de métricas en tiempo real.
 */
class DashboardController
{
    private SystemService $system;
    private StorageService $storage;
    private SambaService $samba;
    private BackupService $backup;
    private UserService $user;

    public function __construct()
    {
        $this->system = new SystemService();
        $this->storage = new StorageService();
        $this->samba = new SambaService();
        $this->backup = new BackupService();
        $this->user = new UserService();
    }

    /**
     * Renderiza la plantilla principal de la aplicación con la vista inicial.
     */
    public function index(Request $request, array $params = []): void
    {
        $templatePath = dirname(__DIR__, 2) . '/templates/layout.php';
        $metrics = $this->system->getSystemMetrics();
        $storage = $this->storage->getStorageOverview();
        $services = $this->system->getServicesStatus();

        Response::html($templatePath, [
            'metrics' => $metrics,
            'storage' => $storage,
            'services' => $services,
            'activeView' => $params['view'] ?? 'dashboard',
        ]);
    }

    /**
     * Endpoint API para recarga en vivo de métricas del panel (polling asíncrono).
     */
    public function metrics(Request $request): void
    {
        $metrics = $this->system->getSystemMetrics();
        $storage = $this->storage->getStorageOverview();
        $services = $this->system->getServicesStatus();
        $shares = $this->samba->listShares();
        $backups = $this->backup->listTasks();
        $users = $this->user->listUsers();

        Response::success([
            'system' => $metrics,
            'storage' => $storage,
            'services' => $services,
            'counts' => [
                'shares' => count($shares),
                'backups' => count($backups),
                'users' => count($users),
            ],
        ]);
    }
}
