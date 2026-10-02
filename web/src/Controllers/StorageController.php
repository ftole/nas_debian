<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\StorageService;

/**
 * Controlador API para gestión de discos, BTRFS y optimizaciones de almacenamiento.
 */
class StorageController
{
    private StorageService $storage;

    public function __construct()
    {
        $this->storage = new StorageService();
    }

    public function overview(Request $request): void
    {
        $overview = $this->storage->getStorageOverview();
        $disks = $this->storage->getDisks();

        Response::success([
            'overview' => $overview,
            'disks' => $disks,
        ]);
    }

    public function scrubStart(Request $request): void
    {
        $res = $this->storage->startBtrfsScrub('/srv/nas');
        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al iniciar scrub BTRFS.');
            return;
        }

        Response::success(null, $res['message'] ?? 'Scrub iniciado.');
    }

    public function scrubStatus(Request $request): void
    {
        $res = $this->storage->getBtrfsScrubStatus('/srv/nas');
        Response::success($res);
    }

    public function trim(Request $request): void
    {
        $res = $this->storage->runTrim('/srv/nas');
        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al ejecutar fstrim.');
            return;
        }

        Response::success(null, $res['message'] ?? 'fstrim completado.');
    }
}
