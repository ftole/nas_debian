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

    public function disks(Request $request): void
    {
        $disks = $this->storage->getDisks();
        $overview = $this->storage->getStorageOverview();

        Response::success([
            'disks' => $disks,
            'overview' => $overview,
        ]);
    }

    public function scrubStart(Request $request): void
    {
        $res = $this->storage->startBtrfsScrub('/srv/nas');
        if (!$res['success']) {
            AuditService::log('scrub_start', '/srv/nas', 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al iniciar scrub BTRFS.', 400, $res);
            return;
        }

        AuditService::log('scrub_start', '/srv/nas', 'SUCCESS');
        Response::success($res, $res['message'] ?? 'Scrub iniciado.');
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
            Response::error($res['error'] ?? 'Error al ejecutar fstrim.', 400, $res);
            return;
        }

        AuditService::log('trim_start', '/srv/nas', 'SUCCESS', ['trimmed' => $res['bytes_trimmed'] ?? '']);
        Response::success($res, $res['message'] ?? 'fstrim completado.');
    }

    public function format(Request $request): void
    {
        $data = $request->getBody();
        try {
            $res = $this->storage->formatAndMount(
                (string) ($data['device'] ?? ''),
                (string) ($data['fstype'] ?? 'ext4'),
                (string) ($data['confirm'] ?? ''),
                (bool) ($data['unmount'] ?? false)
            );
            if (!$res['success']) {
                AuditService::log('storage_format', (string) ($data['device'] ?? ''), 'FAILED', ['error' => $res['error'] ?? '']);
                Response::error($res['error'] ?? 'Error al formatear el disco.');
                return;
            }
            AuditService::log('storage_format', (string) ($data['device'] ?? ''), 'SUCCESS', ['fstype' => $data['fstype'] ?? 'ext4']);
            Response::success(null, $res['message'] ?? 'Disco formateado y montado.');
        } catch (\Throwable $e) {
            AuditService::log('storage_format', (string) ($data['device'] ?? ''), 'ERROR', ['exception' => $e->getMessage()]);
            Response::error('Error inesperado al formatear el disco: ' . $e->getMessage(), 500);
        }
    }

    public function lvm(Request $request): void
    {
        $data = $request->getBody();
        try {
            $res = $this->storage->lvmCreate(
                (string) ($data['disk'] ?? ''),
                (string) ($data['vg'] ?? ''),
                (string) ($data['lv'] ?? ''),
                (string) ($data['size'] ?? '100%FREE'),
                (string) ($data['fstype'] ?? 'ext4'),
                (string) ($data['confirm'] ?? ''),
                (bool) ($data['unmount'] ?? false)
            );
            if (!$res['success']) {
                AuditService::log('storage_lvm', (string) ($data['vg'] ?? ''), 'FAILED', ['error' => $res['error'] ?? '']);
                Response::error($res['error'] ?? 'Error al crear el volumen LVM.');
                return;
            }
            AuditService::log('storage_lvm', (string) ($data['vg'] ?? ''), 'SUCCESS', ['lv' => $data['lv'] ?? '']);
            Response::success(null, $res['message'] ?? 'Volumen LVM creado y montado.');
        } catch (\Throwable $e) {
            AuditService::log('storage_lvm', (string) ($data['vg'] ?? 'unknown'), 'ERROR', ['exception' => $e->getMessage()]);
            Response::error('Error inesperado al crear volumen LVM: ' . $e->getMessage(), 500);
        }
    }

    public function subvolume(Request $request): void
    {
        $data = $request->getBody();
        try {
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
        } catch (\Throwable $e) {
            AuditService::log('storage_subvolume', (string) ($data['subvolume'] ?? 'unknown'), 'ERROR', ['exception' => $e->getMessage()]);
            Response::error('Error inesperado al crear subvolumen: ' . $e->getMessage(), 500);
        }
    }

    public function units(Request $request): void
    {
        $units = $this->storage->getUnits();
        Response::success(['units' => $units]);
    }

    public function subvolumes(Request $request): void
    {
        $path = (string) ($request->getQuery('path', '/srv/nas') ?: '/srv/nas');
        $subvols = $this->storage->btrfsListSubvolumes($path);
        Response::success(['subvolumes' => $subvols]);
    }

    public function inspect(Request $request): void
    {
        $data = $request->getBody();
        $device = (string) ($data['device'] ?? '');
        $res = $this->storage->inspectDevice($device);
        if (empty($res['success'])) {
            Response::error($res['error'] ?? 'Error al inspeccionar el dispositivo.', 400, $res);
            return;
        }
        Response::success($res);
    }

    public function extendPool(Request $request): void
    {
        $data = $request->getBody();
        $device = (string) ($data['device'] ?? '');
        $confirm = (string) ($data['confirm'] ?? '');
        try {
            $res = $this->storage->extendPool($device, $confirm);
            if (!$res['success']) {
                AuditService::log('storage_extend_pool', $device, 'FAILED', ['error' => $res['error'] ?? '']);
                Response::error($res['error'] ?? 'Error al expandir pool de almacenamiento.');
                return;
            }
            AuditService::log('storage_extend_pool', $device, 'SUCCESS', ['pool' => $res['pool'] ?? '']);
            Response::success($res, $res['message'] ?? 'Capacidad expandida exitosamente.');
        } catch (\Throwable $e) {
            AuditService::log('storage_extend_pool', $device, 'ERROR', ['exception' => $e->getMessage()]);
            Response::error('Error inesperado al expandir pool: ' . $e->getMessage(), 500);
        }
    }

    public function addUnit(Request $request): void
    {
        $data = $request->getBody();
        $device = (string) ($data['device'] ?? '');
        $unitId = (string) ($data['unit_id'] ?? '');
        $fstype = (string) ($data['fstype'] ?? 'ext4');
        $confirm = (string) ($data['confirm'] ?? '');
        try {
            $res = $this->storage->addUnit($device, $unitId, $fstype, $confirm);
            if (!$res['success']) {
                AuditService::log('storage_add_unit', $unitId, 'FAILED', ['device' => $device, 'error' => $res['error'] ?? '']);
                Response::error($res['error'] ?? 'Error al aprovisionar unidad adicional.');
                return;
            }
            AuditService::log('storage_add_unit', $unitId, 'SUCCESS', ['device' => $device, 'fstype' => $fstype]);
            Response::success($res, $res['message'] ?? 'Unidad adicional creada y montada.');
        } catch (\Throwable $e) {
            AuditService::log('storage_add_unit', $unitId, 'ERROR', ['exception' => $e->getMessage()]);
            Response::error('Error inesperado al agregar unidad: ' . $e->getMessage(), 500);
        }
    }
}
