<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\AuditService;
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
            AuditService::log('scrub_start', '/srv/nas', 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al iniciar scrub BTRFS.');
            return;
        }

        AuditService::log('scrub_start', '/srv/nas', 'SUCCESS');
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
            AuditService::log('trim_start', '/srv/nas', 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al ejecutar fstrim.');
            return;
        }

        AuditService::log('trim_start', '/srv/nas', 'SUCCESS');
        Response::success(null, $res['message'] ?? 'fstrim completado.');
    }

    public function format(Request $request): void
    {
        $data = $request->getBody();
        $res = $this->storage->formatAndMount(
            (string) ($data['device'] ?? ''),
            (string) ($data['fstype'] ?? 'ext4'),
            (string) ($data['confirm'] ?? '')
        );
        if (!$res['success']) {
            AuditService::log('storage_format', (string) ($data['device'] ?? ''), 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al formatear el disco.');
            return;
        }
        AuditService::log('storage_format', (string) ($data['device'] ?? ''), 'SUCCESS', ['fstype' => $data['fstype'] ?? 'ext4']);
        Response::success(null, $res['message'] ?? 'Disco formateado y montado.');
    }

    public function lvm(Request $request): void
    {
        $data = $request->getBody();
        $res = $this->storage->lvmCreate(
            (string) ($data['disk'] ?? ''),
            (string) ($data['vg'] ?? ''),
            (string) ($data['lv'] ?? ''),
            (string) ($data['size'] ?? '100%FREE'),
            (string) ($data['fstype'] ?? 'ext4'),
            (string) ($data['confirm'] ?? '')
        );
        if (!$res['success']) {
            AuditService::log('storage_lvm', (string) ($data['vg'] ?? ''), 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al crear el volumen LVM.');
            return;
        }
        AuditService::log('storage_lvm', (string) ($data['vg'] ?? ''), 'SUCCESS', ['lv' => $data['lv'] ?? '']);
        Response::success(null, $res['message'] ?? 'Volumen LVM creado y montado.');
    }

    public function subvolume(Request $request): void
    {
        $data = $request->getBody();
        $res = $this->storage->btrfsSubvolume(
            (string) ($data['device'] ?? ''),
            (string) ($data['subvolume'] ?? ''),
            (string) ($data['confirm'] ?? '')
        );
        if (!$res['success']) {
            AuditService::log('storage_subvolume', (string) ($data['subvolume'] ?? ''), 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al crear el subvolumen.');
            return;
        }
        AuditService::log('storage_subvolume', (string) ($data['subvolume'] ?? ''), 'SUCCESS');
        Response::success(null, $res['message'] ?? 'Subvolumen creado.');
    }
}
