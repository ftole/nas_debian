<?php

declare(strict_types=1);

namespace App\Services;

/**
 * Servicio de administración de almacenamiento, monitoreo de /srv/nas,
 * auditoría BTRFS scrub y optimización SSD TRIM.
 */
class StorageService
{
    /**
     * Resumen de capacidad y sistema de archivos de /srv/nas.
     */
    public function getStorageOverview(): array
    {
        $path = '/srv/nas';
        $totalBytes = 0;
        $usedBytes = 0;
        $freeBytes = 0;
        $usagePct = 0;
        $fsType = 'ext4';
        $isHdd = true;

        if (DIRECTORY_SEPARATOR !== '\\') {
            // df -Pk /srv/nas
            $dfRes = SystemService::runCommand(['df', '-Pk', $path]);
            if ($dfRes['code'] === 0) {
                $lines = explode("\n", trim($dfRes['stdout']));
                if (isset($lines[1])) {
                    $parts = preg_split('/\s+/', $lines[1]);
                    if (count($parts) >= 5) {
                        $totalKb = (int) $parts[1];
                        $usedKb = (int) $parts[2];
                        $availKb = (int) $parts[3];
                        $totalBytes = $totalKb * 1024;
                        $usedBytes = $usedKb * 1024;
                        $freeBytes = $availKb * 1024;
                        $usagePct = $totalKb > 0 ? round(($usedKb / $totalKb) * 100, 1) : 0;
                    }
                }
            }

            // Detección de Filesystem
            $fsRes = SystemService::runCommand(['findmnt', '-n', '-o', 'FSTYPE', $path]);
            if ($fsRes['code'] === 0 && !empty($fsRes['stdout'])) {
                $fsType = strtolower(trim($fsRes['stdout']));
            }

            // Detección de HDD vs SSD
            $devRes = SystemService::runCommand(['findmnt', '-n', '-o', 'SOURCE', $path]);
            if ($devRes['code'] === 0 && !empty($devRes['stdout'])) {
                $dev = basename(trim($devRes['stdout']));
                $diskBase = preg_replace('/[0-9p]+$/', '', $dev);
                $rotPath = "/sys/block/$diskBase/queue/rotational";
                if (file_exists($rotPath)) {
                    $isHdd = trim((string) file_get_contents($rotPath)) === '1';
                }
            }
        } else {
            // Valores de prueba en desarrollo
            $totalBytes = 4000 * 1024 * 1024 * 1024;
            $usedBytes = 1120 * 1024 * 1024 * 1024;
            $freeBytes = $totalBytes - $usedBytes;
            $usagePct = 28.0;
            $fsType = 'btrfs';
            $isHdd = false;
        }

        return [
            'path' => $path,
            'total_gb' => round($totalBytes / (1024 ** 3), 2),
            'used_gb' => round($usedBytes / (1024 ** 3), 2),
            'free_gb' => round($freeBytes / (1024 ** 3), 2),
            'usage_percent' => $usagePct,
            'filesystem' => $fsType,
            'is_btrfs' => ($fsType === 'btrfs'),
            'device_type' => $isHdd ? 'HDD (Mecánico)' : 'SSD (Estado Sólido)',
            'supports_trim' => !$isHdd,
        ];
    }

    /**
     * Lista discos físicos y particiones reconocidas en el sistema.
     */
    public function getDisks(): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                [
                    'name' => 'sda',
                    'size' => '4.0 TB',
                    'model' => 'WD Red Plus NAS',
                    'type' => 'disk',
                    'mount' => '/srv/nas',
                    'fstype' => 'btrfs',
                    'rotational' => true,
                ],
                [
                    'name' => 'nvme0n1',
                    'size' => '256 GB',
                    'model' => 'Samsung SSD 980',
                    'type' => 'disk',
                    'mount' => '/',
                    'fstype' => 'ext4',
                    'rotational' => false,
                ],
            ];
        }

        $res = SystemService::runCommand(['lsblk', '-J', '-b', '-o', 'NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,ROTA']);
        if ($res['code'] !== 0) {
            return [];
        }

        $data = json_decode($res['stdout'], true);
        $devices = $data['blockdevices'] ?? [];
        $result = [];

        foreach ($devices as $dev) {
            $sizeGb = round(((int) ($dev['size'] ?? 0)) / (1024 ** 3), 1);
            $result[] = [
                'name' => $dev['name'] ?? '',
                'size' => $sizeGb > 1000 ? round($sizeGb / 1024, 2) . ' TB' : $sizeGb . ' GB',
                'model' => trim($dev['model'] ?? 'N/A'),
                'type' => $dev['type'] ?? '',
                'mount' => $dev['mountpoint'] ?? '',
                'fstype' => $dev['fstype'] ?? '',
                'rotational' => ($dev['rota'] ?? true),
            ];
        }

        return $result;
    }

    /**
     * Inicia una auditoría de integridad criptográfica BTRFS scrub.
     */
    public function startBtrfsScrub(string $path = '/srv/nas'): array
    {
        $res = SystemService::sudo(['btrfs', 'scrub', 'start', $path]);
        if ($res['code'] !== 0) {
            return [
                'success' => false,
                'error' => 'No se pudo iniciar el scrub: ' . ($res['stderr'] ?: $res['stdout']),
            ];
        }

        return [
            'success' => true,
            'message' => 'Scrub BTRFS iniciado en ' . $path . '.',
        ];
    }

    /**
     * Consulta el estado del scrub BTRFS en ejecución o el último reporte.
     */
    public function getBtrfsScrubStatus(string $path = '/srv/nas'): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                'success' => true,
                'status' => 'finished',
                'summary' => 'Scrub completado sin errores. 0 bytes con corrupción.',
            ];
        }

        $res = SystemService::sudo(['btrfs', 'scrub', 'status', $path]);
        return [
            'success' => true,
            'output' => $res['stdout'] ?: $res['stderr'],
        ];
    }

    /**
     * Ejecuta descarte de bloques (TRIM) para unidades SSD.
     */
    public function runTrim(string $path = '/srv/nas'): array
    {
        $res = SystemService::sudo(['fstrim', '-v', $path]);
        if ($res['code'] !== 0) {
            return [
                'success' => false,
                'error' => 'No se pudo ejecutar fstrim: ' . ($res['stderr'] ?: $res['stdout']),
            ];
        }

        return [
            'success' => true,
            'message' => $res['stdout'] ?: 'fstrim completado.',
        ];
    }
}
