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
                    'device' => '/dev/sda',
                    'size' => '4.0 TB',
                    'model' => 'WD Red Plus NAS',
                    'type' => 'disk',
                    'mount' => '/srv/nas',
                    'fstype' => 'btrfs',
                    'rotational' => true,
                    'protected' => false,
                    'in_use' => true,
                ],
                [
                    'name' => 'nvme0n1',
                    'device' => '/dev/nvme0n1',
                    'size' => '256 GB',
                    'model' => 'Samsung SSD 980',
                    'type' => 'disk',
                    'mount' => '/',
                    'fstype' => 'ext4',
                    'rotational' => false,
                    'protected' => true,
                    'in_use' => true,
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
            $name = $dev['name'] ?? '';
            $result[] = [
                'name' => $name,
                'device' => '/dev/' . $name,
                'size' => $sizeGb > 1000 ? round($sizeGb / 1024, 2) . ' TB' : $sizeGb . ' GB',
                'model' => trim($dev['model'] ?? 'N/A'),
                'type' => $dev['type'] ?? '',
                'mount' => $dev['mountpoint'] ?? '',
                'fstype' => $dev['fstype'] ?? '',
                'rotational' => ($dev['rota'] ?? true),
                'protected' => ($dev['type'] ?? '') === 'disk' ? $this->isOsDisk($name) : false,
                'in_use' => $this->deviceHasMount('/dev/' . $name),
            ];
        }

        return $result;
    }

    /**
     * Indica si un dispositivo o cualquiera de sus particiones hijas está montado.
     */
    private function deviceHasMount(string $device): bool
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return false;
        }
        $out = SystemService::runCommand(['lsblk', '-ln', '-o', 'MOUNTPOINT', $device])['stdout'] ?? '';
        foreach (preg_split('/\r\n|\r|\n/', (string) $out) as $line) {
            if (trim($line) !== '') {
                return true;
            }
        }
        return false;
    }

    /**
     * Devuelve los puntos de montaje activos de un dispositivo y sus particiones (más profundos primero).
     *
     * @return string[]
     */
    private function mountedPointsOf(string $device): array
    {
        $out = SystemService::runCommand(['lsblk', '-ln', '-o', 'MOUNTPOINT', $device])['stdout'] ?? '';
        $points = [];
        foreach (preg_split('/\r\n|\r|\n/', (string) $out) as $line) {
            $mp = trim($line);
            if ($mp !== '' && str_starts_with($mp, '/')) {
                $points[] = $mp;
            }
        }
        // Desmontar primero los puntos más profundos para evitar "target is busy".
        usort($points, fn($a, $b) => substr_count($b, '/') <=> substr_count($a, '/'));
        return array_values(array_unique($points));
    }

    /**
     * Desmonta /srv/nas y todos los puntos de montaje del dispositivo indicado.
     */
    private function unmountDevice(string $device): bool
    {
        $points = $this->mountedPointsOf($device);
        if (!in_array('/srv/nas', $points, true) && is_dir('/srv/nas')) {
            $points[] = '/srv/nas';
        }
        $ok = true;
        foreach ($points as $mp) {
            $res = SystemService::sudo(['umount', $mp]);
            if ($res['code'] !== 0) {
                // Reintento con lazy unmount si el punto está ocupado.
                if (SystemService::sudo(['umount', '-l', $mp])['code'] !== 0) {
                    $ok = false;
                }
            }
        }
        return $ok;
    }

    /**
     * Determina si un disco aloja el sistema operativo (o pertenece a su árbol físico).
     */
    public function isOsDisk(string $name): bool
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return false;
        }
        $rootSrc = trim(SystemService::runCommand(['findmnt', '-n', '-o', 'SOURCE', '/'])['stdout'] ?? '');
        if ($rootSrc === '') {
            return false;
        }
        $rootDisk = trim(SystemService::runCommand(['lsblk', '-no', 'PKNAME', $rootSrc])['stdout'] ?? '');
        if ($rootDisk === '') {
            $rootDisk = basename($rootSrc);
        }
        $parents = SystemService::runCommand(['lsblk', '-s', '-n', '-o', 'NAME', '/dev/' . $name])['stdout'] ?? '';
        foreach (preg_split('/\s+/', trim($parents)) as $p) {
            if ($p !== '' && $p === $rootDisk) {
                return true;
            }
        }
        return false;
    }

    /**
     * Formatea (GPT + mkfs) y monta un disco/partición de datos en /srv/nas.
     * Requiere la confirmación textual 'SI-FORMATEAR'. Nunca opera sobre el disco del SO.
     */
    public function formatAndMount(string $device, string $fstype, string $confirm, bool $unmount = false): array
    {
        $device = trim($device);
        $fstype = strtolower(trim($fstype));
        if ($confirm !== 'SI-FORMATEAR') {
            return ['success' => false, 'error' => "Confirmación requerida (escribe SI-FORMATEAR)."];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/]+$#', $device)) {
            return ['success' => false, 'error' => 'Dispositivo inválido.'];
        }
        if (!in_array($fstype, ['ext4', 'btrfs'], true)) {
            return ['success' => false, 'error' => 'Sistema de archivos no soportado (ext4|btrfs).'];
        }
        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Formateo simulado de $device (modo dev)."];
        }

        $name = basename($device);
        if ($this->isOsDisk($name)) {
            return ['success' => false, 'error' => 'Operación prohibida: es el disco del sistema operativo.'];
        }

        if ($this->deviceHasMount($device)) {
            if (!$unmount) {
                return ['success' => false, 'error' => 'El dispositivo (o una de sus particiones) está montado; habilita la opción de desmontar para continuar.'];
            }
            if (!$this->unmountDevice($device)) {
                return ['success' => false, 'error' => 'No se pudieron desmontar todos los puntos de montaje del dispositivo.'];
            }
        }

        $type = trim(SystemService::runCommand(['lsblk', '-no', 'TYPE', $device])['stdout'] ?? '');
        $target = $device;
        if ($type === 'disk') {
            SystemService::sudo(['parted', '-s', $device, 'mklabel', 'gpt', 'mkpart', 'primary', '0%', '100%']);
            SystemService::sudo(['partprobe', $device]);
            $target = str_contains($name, 'nvme') || preg_match('/[0-9]$/', $name) ? $device . 'p1' : $device . '1';
        }

        $mkfs = SystemService::sudo(
            $fstype === 'btrfs'
                ? ['mkfs.btrfs', '-f', '-L', 'NAS_DATA', $target]
                : ['mkfs.ext4', '-F', '-L', 'NAS_DATA', $target]
        );
        if ($mkfs['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al formatear: ' . ($mkfs['stderr'] ?: $mkfs['stdout'])];
        }

        return $this->mountAtNas($target, $fstype);
    }

    /**
     * Crea LVM (PV/VG/LV), formatea y monta en /srv/nas.
     */
    public function lvmCreate(string $disk, string $vg, string $lv, string $size, string $fstype, string $confirm, bool $unmount = false): array
    {
        if ($confirm !== 'SI-FORMATEAR') {
            return ['success' => false, 'error' => 'Confirmación requerida (escribe SI-FORMATEAR).'];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/]+$#', $disk) || !preg_match('/^[a-z0-9_]{2,20}$/', $vg) || !preg_match('/^[a-z0-9_]{2,20}$/', $lv)) {
            return ['success' => false, 'error' => 'Parámetros LVM inválidos.'];
        }
        if (!preg_match('/^([0-9]+%|[0-9]+[KMGT])$/', $size)) {
            return ['success' => false, 'error' => 'Tamaño LVM inválido (ej. 100%FREE, 500G).'];
        }
        if ($this->isOsDisk(basename($disk))) {
            return ['success' => false, 'error' => 'Operación prohibida sobre el disco del sistema.'];
        }
        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "LVM simulado ($vg/$lv) (modo dev)."];
        }
        if ($this->deviceHasMount($disk)) {
            if (!$unmount) {
                return ['success' => false, 'error' => 'El disco (o una de sus particiones) está montado; habilita la opción de desmontar para continuar.'];
            }
            if (!$this->unmountDevice($disk)) {
                return ['success' => false, 'error' => 'No se pudieron desmontar todos los puntos de montaje del disco.'];
            }
        }
        $always = !str_ends_with($size, '%') ? '--yes' : null;
        SystemService::sudo(['pvcreate', '-f', '-y', $disk]);
        SystemService::sudo(['vgcreate', $vg, $disk]);
        $lvArgs = ['lvcreate', '-y', '-n', $lv, '-L', $size, $vg];
        if ($always === null) {
            $lvArgs = ['lvcreate', '-y', '-l', $size, '-n', $lv, $vg];
        }
        $lvRes = SystemService::sudo($lvArgs);
        if ($lvRes['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al crear LV: ' . ($lvRes['stderr'] ?: $lvRes['stdout'])];
        }
        $lvPath = '/dev/' . $vg . '/' . $lv;
        $fstype = in_array($fstype, ['ext4', 'btrfs'], true) ? $fstype : 'ext4';
        $mkfs = SystemService::sudo($fstype === 'btrfs' ? ['mkfs.btrfs', '-f', '-L', 'NAS_DATA', $lvPath] : ['mkfs.ext4', '-F', '-L', 'NAS_DATA', $lvPath]);
        if ($mkfs['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al formatear el LV: ' . ($mkfs['stderr'] ?: $mkfs['stdout'])];
        }
        return $this->mountAtNas($lvPath, $fstype);
    }

    /**
     * Crea un subvolumen Btrfs.
     */
    public function btrfsSubvolume(string $device, string $subvol, string $confirm): array
    {
        if ($confirm !== 'SI-FORMATEAR') {
            return ['success' => false, 'error' => 'Confirmación requerida (escribe SI-FORMATEAR).'];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/]+$#', $device) || !preg_match('/^[a-zA-Z0-9._-]{1,60}$/', $subvol)) {
            return ['success' => false, 'error' => 'Parámetros inválidos para subvolumen Btrfs.'];
        }
        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Subvolumen Btrfs $subvol simulado (modo dev)."];
        }
        $mountPoint = '/mnt/nas-btrfs-tmp';
        SystemService::sudo(['mkdir', '-p', $mountPoint]);
        if (SystemService::sudo(['mount', $device, $mountPoint])['code'] !== 0) {
            return ['success' => false, 'error' => 'No se pudo montar el dispositivo Btrfs.'];
        }
        $res = SystemService::sudo(['btrfs', 'subvolume', 'create', $mountPoint . '/' . $subvol]);
        SystemService::sudo(['umount', $mountPoint]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'No se pudo crear el subvolumen: ' . ($res['stderr'] ?: $res['stdout'])];
        }
        return ['success' => true, 'message' => "Subvolumen Btrfs '$subvol' creado en $device."];
    }

    private function mountAtNas(string $device, string $fstype): array
    {
        SystemService::sudo(['umount', '/srv/nas']);
        SystemService::sudo(['mkdir', '-p', '/srv/nas']);
        $uuid = trim(SystemService::sudo(['blkid', '-s', 'UUID', '-o', 'value', $device])['stdout'] ?? '');
        $fstabTmp = '/tmp/nas_fstab_' . bin2hex(random_bytes(4));
        $current = @file_get_contents('/etc/fstab') ?: '';
        $clean = preg_replace('/# BEGIN NAS_DEBIAN \/srv\/nas.*?# END NAS_DEBIAN \/srv\/nas\n?/s', '', $current) ?? $current;
        $ref = $uuid !== '' ? "UUID=$uuid" : $device;
        $clean .= "\n# BEGIN NAS_DEBIAN /srv/nas\n$ref /srv/nas $fstype defaults,noatime 0 2\n# END NAS_DEBIAN /srv/nas\n";
        file_put_contents($fstabTmp, $clean);
        SystemService::sudo(['cp', $fstabTmp, '/etc/fstab']);
        @unlink($fstabTmp);
        $mnt = SystemService::sudo(['mount', '/srv/nas']);
        if ($mnt['code'] !== 0) {
            return ['success' => false, 'error' => 'No se pudo montar /srv/nas: ' . ($mnt['stderr'] ?: $mnt['stdout'])];
        }
        return ['success' => true, 'message' => "Dispositivo $device formateado ($fstype) y montado en /srv/nas."];
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
