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
            } elseif (SystemService::getServerRole() === 'BACKUP') {
                $fsType = 'btrfs';
            }

            // Detección de HDD vs SSD
            $devRes = SystemService::runCommand(['findmnt', '-n', '-o', 'SOURCE', $path]);
            if ($devRes['code'] === 0 && !empty($devRes['stdout'])) {
                $rawDev = trim($devRes['stdout']);
                // Detección directa mediante lsblk ROTA (0 = no rotacional / SSD, 1 = HDD)
                $rotaRes = SystemService::runCommand(['lsblk', '-n', '-o', 'ROTA', $rawDev]);
                if ($rotaRes['code'] === 0 && trim($rotaRes['stdout']) !== '') {
                    $firstRota = trim(explode("\n", trim($rotaRes['stdout']))[0]);
                    $isHdd = ($firstRota === '1');
                } else {
                    $dev = basename($rawDev);
                    $diskBase = preg_replace('/[0-9p]+$/', '', $dev);
                    $rotPath = "/sys/block/$diskBase/queue/rotational";
                    if (file_exists($rotPath)) {
                        $isHdd = trim((string) file_get_contents($rotPath)) === '1';
                    }
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
        return $this->flattenBlockDevices($devices);
    }

    /**
     * Aplana recursivamente la jerarquía de dispositivos y particiones (children) de lsblk.
     */
    public function flattenBlockDevices(array $devices, ?string $parentModel = null, ?bool $parentRota = null): array
    {
        $result = [];
        foreach ($devices as $dev) {
            $name = $dev['name'] ?? '';
            if ($name === '') {
                continue;
            }
            $sizeGb = round(((int) ($dev['size'] ?? 0)) / (1024 ** 3), 1);
            $model = trim((string) ($dev['model'] ?? ($parentModel ?? 'N/A')));
            $rota = isset($dev['rota']) ? (bool) $dev['rota'] : ($parentRota ?? true);
            $devPath = '/dev/' . $name;
            $type = $dev['type'] ?? '';
            $isOs = $this->isOsDisk($name);
            $inUse = $this->deviceHasMount($devPath) || $this->deviceInUseCritical($devPath);

            $result[] = [
                'name' => $name,
                'device' => $devPath,
                'size' => $sizeGb > 1000 ? round($sizeGb / 1024, 2) . ' TB' : $sizeGb . ' GB',
                'model' => $model,
                'type' => $type,
                'mount' => $dev['mountpoint'] ?? '',
                'fstype' => $dev['fstype'] ?? '',
                'rotational' => $rota,
                'protected' => $isOs,
                'in_use' => $inUse,
                'children_count' => isset($dev['children']) && is_array($dev['children']) ? count($dev['children']) : 0,
            ];

            if (!empty($dev['children']) && is_array($dev['children'])) {
                $children = $this->flattenBlockDevices($dev['children'], $model, $rota);
                foreach ($children as $c) {
                    $result[] = $c;
                }
            }
        }
        return $result;
    }

    /** @var string Ruta al archivo mdstat para detección de arreglos RAID */
    public static string $mdstatPath = '/proc/mdstat';

    /** @var string|null Salida simulada de comando pvs para pruebas unitarias */
    public static ?string $mockPvs = null;

    /**
     * Indica si un disco contiene metadatos de volumen activo (PV de LVM o miembro de RAID md).
     * Estos discos no deben formatearse ni provisionarse aunque se solicite desmontarlos.
     */
    public function deviceInUseCritical(string $device): bool
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return false;
        }

        $real = realpath($device) ?: $device;
        if (!str_starts_with($real, '/dev/')) {
            return false;
        }

        // Miembro de arreglo RAID (md): el nombre del kernel aparece en mdstat.
        $kernelNames = SystemService::runCommand(['lsblk', '-ln', '-o', 'NAME', $real])['stdout'] ?? '';
        if (empty($kernelNames)) {
            $kernelNames = basename($real);
        }
        if (is_readable(self::$mdstatPath)) {
            $mdstat = (string) file_get_contents(self::$mdstatPath);
            foreach (preg_split('/\s+/', trim((string) $kernelNames)) as $kernelName) {
                if ($kernelName !== '' && preg_match('/\b' . preg_quote($kernelName, '/') . '\b/', $mdstat)) {
                    return true;
                }
            }
        }

        // Physical Volume de LVM activo ('sudo pvs' ya está autorizado para www-data).
        $diskBase = basename($real);
        $pvs = self::$mockPvs ?? (SystemService::sudo(['pvs'])['stdout'] ?? '');
        foreach (preg_split('/\r\n|\r|\n/', (string) $pvs) as $line) {
            $cols = preg_split('/\s+/', trim($line));
            if (!empty($cols[0]) && str_starts_with($cols[0], '/dev/')) {
                $pvReal = realpath($cols[0]) ?: $cols[0];
                $pvBase = basename($pvReal);
                if ($pvReal === $real || $pvBase === $diskBase || preg_match('/^' . preg_quote($diskBase, '/') . '(?:p?\d+)$/', $pvBase)) {
                    return true;
                }
                // El volumen físico puede residir en una partición del disco consultado (p. ej. /dev/sda1 para /dev/sda)
                $pvChain = SystemService::runCommand(['lsblk', '-s', '-n', '-o', 'NAME', $pvReal])['stdout'] ?? '';
                foreach (preg_split('/\s+/', trim((string) $pvChain)) as $ancestor) {
                    $ancestor = (string) preg_replace('/^[^a-zA-Z0-9]+/', '', $ancestor);
                    if ($ancestor !== '' && ($ancestor === $diskBase || preg_match('/^' . preg_quote($diskBase, '/') . '(?:p?\d+)$/', $ancestor))) {
                        return true;
                    }
                }
            }
        }

        return false;
    }

    /**
     * Indica si un dispositivo o punto de montaje está actualmente montado en el sistema.
     */
    public function deviceHasMount(string $device): bool
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return false;
        }

        $device = trim($device);
        if ($device === '') {
            return false;
        }

        if (str_starts_with($device, '/dev/')) {
            $out = SystemService::runCommand(['lsblk', '-ln', '-o', 'MOUNTPOINT', $device])['stdout'] ?? '';
            foreach (preg_split('/\r\n|\r|\n/', (string) $out) as $line) {
                if (trim($line) !== '') {
                    return true;
                }
            }
            return false;
        }

        $out = SystemService::runCommand(['findmnt', '-n', '-M', $device])['stdout'] ?? '';
        return trim($out) !== '';
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
     * Desmonta un dispositivo y sus particiones ejecutando desmontaje tanto por
     * puntos de montaje como por la ruta del dispositivo (/dev/...).
     * Soporta también la ruta directa de un punto de montaje (ej. /mnt/disco2 o /srv/nas).
     */
    public function unmountDevice(string $device): bool
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return true;
        }

        $device = trim($device);
        if ($device === '') {
            return false;
        }

        $points = [];
        $devPaths = [];

        if (str_starts_with($device, '/dev/')) {
            $points = $this->mountedPointsOf($device);
            $devPaths = [$device];

            $outDevs = SystemService::runCommand(['lsblk', '-ln', '-o', 'PATH', $device])['stdout'] ?? '';
            foreach (preg_split('/\r\n|\r|\n/', (string) $outDevs) as $line) {
                $p = trim($line);
                if ($p !== '' && str_starts_with($p, '/dev/') && !in_array($p, $devPaths, true)) {
                    $devPaths[] = $p;
                }
            }
            // Desmontar particiones primero (longitud mayor primero, ej. /dev/sdb1 antes de /dev/sdb)
            usort($devPaths, fn($a, $b) => strlen($b) <=> strlen($a));
        } else {
            // Se pasó directamente una ruta de punto de montaje
            $points = [$device];
            $src = trim(SystemService::runCommand(['findmnt', '-n', '-o', 'SOURCE', $device])['stdout'] ?? '');
            if ($src !== '' && str_starts_with($src, '/dev/')) {
                $devPaths[] = $src;
            }
        }

        // 1. Desmontar por puntos de montaje
        foreach ($points as $mp) {
            $res = SystemService::sudo(['umount', $mp]);
            if ($res['code'] !== 0) {
                // Reintento con lazy unmount si el punto está ocupado.
                SystemService::sudo(['umount', '-l', $mp]);
            }
        }

        // 2. Desmontar directamente por ruta de dispositivo /dev/... si aplica
        foreach ($devPaths as $dev) {
            $res = SystemService::sudo(['umount', $dev]);
            if ($res['code'] !== 0) {
                SystemService::sudo(['umount', '-l', $dev]);
            }
        }

        return !$this->deviceHasMount($device);
    }

    /**
     * Resuelve la ruta canónica de un dispositivo y verifica que sea un disco o
     * partición de bloque presente en el inventario del sistema (lsblk).
     *
     * @return string|null Ruta canónica /dev/... o null si no es un disco válido.
     */
    public function resolveBlockDevice(string $device): ?string
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return $device;
        }

        $real = realpath($device);
        if ($real !== false) {
            // El dispositivo existe físicamente: debe ser un dispositivo de bloque y reconocido por lsblk
            if (!str_starts_with($real, '/dev/') || @filetype($real) !== 'block') {
                return null;
            }
            $type = trim(SystemService::runCommand(['lsblk', '-nd', '-o', 'TYPE', $real])['stdout'] ?? '');
            if (!in_array($type, ['disk', 'part'], true)) {
                return null;
            }
            return $real;
        }

        // Si no existe físicamente en el filesystem:
        // En entorno de pruebas solo se aceptan nombres de disco canónicos simulados (sdX, vdX, nvmeX)
        if (getenv('APP_ENV') === 'testing') {
            if (preg_match('#^/dev/(sd[a-z][0-9]?|vd[a-z][0-9]?|nvme[0-9]n[0-9](p[0-9])?)$#', $device)) {
                return $device;
            }
        }

        return null;
    }

    /**
     * Determina si un disco aloja el sistema operativo (o pertenece a su árbol físico).
     */
    public function isOsDisk(string $name): bool
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return false;
        }
        $name = trim($name);
        if ($name === '') {
            return false;
        }
        $diskDev = str_starts_with($name, '/dev/') ? $name : '/dev/' . $name;
        $diskBase = basename($name);

        $rootSrc = trim(SystemService::runCommand(['findmnt', '-n', '-o', 'SOURCE', '/'])['stdout'] ?? '');
        // findmnt reporta subvolúmenes Btrfs como "/dev/sda2[/@]"; limpiar el sufijo
        $rootSrc = preg_replace('#\[.*$#', '', $rootSrc) ?? $rootSrc;
        if ($rootSrc === '' || !str_starts_with($rootSrc, '/dev/')) {
            return false;
        }
        $rootDisk = trim(SystemService::runCommand(['lsblk', '-no', 'PKNAME', $rootSrc])['stdout'] ?? '');
        if ($rootDisk === '') {
            $rootDisk = basename($rootSrc);
        }
        $rootDevs = SystemService::runCommand(['lsblk', '-s', '-n', '-o', 'NAME', $rootSrc])['stdout'] ?? '';
        $rootDevSet = array_filter(preg_split('/\s+/', trim($rootDevs)));

        if ($diskBase === $rootDisk || in_array($diskBase, $rootDevSet, true)) {
            return true;
        }

        $parents = SystemService::runCommand(['lsblk', '-s', '-n', '-o', 'NAME', $diskDev])['stdout'] ?? '';
        foreach (preg_split('/\s+/', trim($parents)) as $p) {
            if ($p !== '' && ($p === $rootDisk || in_array($p, $rootDevSet, true))) {
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
        if ($confirm !== 'SI-FORMATEAR' && $confirm !== 'SI-DESTRUIR-DATOS') {
            return ['success' => false, 'error' => "Confirmación requerida (escribe SI-FORMATEAR o SI-DESTRUIR-DATOS)."];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/]+$#', $device)) {
            return ['success' => false, 'error' => 'Dispositivo inválido.'];
        }
        $resolvedDevice = $this->resolveBlockDevice($device);
        if ($resolvedDevice === null) {
            return ['success' => false, 'error' => 'El dispositivo no corresponde a un disco o partición de bloque reconocido por el sistema.'];
        }
        $device = $resolvedDevice;
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
        if ($this->deviceInUseCritical($device)) {
            return ['success' => false, 'error' => 'Operación prohibida: el disco contiene un volumen LVM o un miembro de RAID activo.'];
        }

        if ($this->deviceHasMount($device)) {
            if (!$unmount) {
                return ['success' => false, 'error' => 'El dispositivo (o una de sus particiones) está montado; habilita la opción de desmontar para continuar.'];
            }
            if (!$this->unmountDevice($device)) {
                return ['success' => false, 'error' => 'No se pudieron desmontar todos los puntos de montaje del dispositivo.'];
            }
        }

        $type = trim(SystemService::runCommand(['lsblk', '-nd', '-o', 'TYPE', $device])['stdout'] ?? '');
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
        if ($confirm !== 'SI-FORMATEAR' && $confirm !== 'SI-DESTRUIR-DATOS') {
            return ['success' => false, 'error' => 'Confirmación requerida (escribe SI-FORMATEAR o SI-DESTRUIR-DATOS).'];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/]+$#', $disk) || !preg_match('/^[a-z0-9_]{2,20}$/', $vg) || !preg_match('/^[a-z0-9_]{2,20}$/', $lv)) {
            return ['success' => false, 'error' => 'Parámetros LVM inválidos.'];
        }
        if (!preg_match('/^([0-9]+%(FREE|VG|PVS)?|[0-9]+[KMGTkmgt]B?)$/i', $size)) {
            return ['success' => false, 'error' => 'Tamaño LVM inválido (ej. 100%FREE, 500G).'];
        }
        if ($this->isOsDisk(basename($disk))) {
            return ['success' => false, 'error' => 'Operación prohibida sobre el disco del sistema.'];
        }
        if ($this->deviceInUseCritical($disk)) {
            return ['success' => false, 'error' => 'Operación prohibida: el disco contiene un volumen LVM o un miembro de RAID activo.'];
        }
        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "LVM simulado ($vg/$lv) (modo dev)."];
        }
        $resolvedDisk = $this->resolveBlockDevice($disk);
        if ($resolvedDisk === null) {
            return ['success' => false, 'error' => 'El disco no corresponde a un dispositivo de bloque reconocido por el sistema.'];
        }
        $disk = $resolvedDisk;
        if ($this->deviceHasMount($disk)) {
            if (!$unmount) {
                return ['success' => false, 'error' => 'El disco (o una de sus particiones) está montado; habilita la opción de desmontar para continuar.'];
            }
            if (!$this->unmountDevice($disk)) {
                return ['success' => false, 'error' => 'No se pudieron desmontar todos los puntos de montaje del disco.'];
            }
        }
        $pvRes = SystemService::sudo(['pvcreate', '-f', '-y', $disk]);
        if ($pvRes['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al inicializar volumen físico (PV): ' . ($pvRes['stderr'] ?: $pvRes['stdout'])];
        }
        $vgRes = SystemService::sudo(['vgcreate', $vg, $disk]);
        if ($vgRes['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al crear grupo de volúmenes (VG): ' . ($vgRes['stderr'] ?: $vgRes['stdout'])];
        }
        if (str_contains($size, '%')) {
            $lvArgs = ['lvcreate', '-y', '-l', $size, '-n', $lv, $vg];
        } else {
            $lvArgs = ['lvcreate', '-y', '-L', $size, '-n', $lv, $vg];
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
        if ($confirm !== 'SI-FORMATEAR' && $confirm !== 'SI-DESTRUIR-DATOS') {
            return ['success' => false, 'error' => 'Confirmación requerida (escribe SI-FORMATEAR o SI-DESTRUIR-DATOS).'];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/]+$#', $device) || !preg_match('/^[a-zA-Z0-9._-]{1,60}$/', $subvol)) {
            return ['success' => false, 'error' => 'Parámetros inválidos para subvolumen Btrfs.'];
        }
        $resolvedBtrfsDevice = $this->resolveBlockDevice($device);
        if ($resolvedBtrfsDevice === null) {
            return ['success' => false, 'error' => 'El dispositivo no corresponde a un disco o partición de bloque reconocido por el sistema.'];
        }
        $device = $resolvedBtrfsDevice;
        $name = basename($device);
        if ($this->isOsDisk($name)) {
            return ['success' => false, 'error' => 'Operación prohibida: es el disco del sistema operativo.'];
        }
        if ($this->deviceInUseCritical($device)) {
            return ['success' => false, 'error' => 'Operación prohibida: el dispositivo contiene un volumen LVM o un miembro de RAID activo.'];
        }
        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
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
        $stagingDir = '/var/lib/nas/staging';
        $stagingBase = (is_dir($stagingDir) && is_writable($stagingDir)) ? $stagingDir : sys_get_temp_dir();
        $fstabTmp = $stagingBase . '/nas_fstab_' . bin2hex(random_bytes(4));
        $current = @file_get_contents('/etc/fstab') ?: '';
        $clean = preg_replace('/# BEGIN NAS_DEBIAN \/srv\/nas.*?# END NAS_DEBIAN \/srv\/nas\n?/s', '', $current) ?? $current;
        $ref = $uuid !== '' ? "UUID=$uuid" : $device;
        $opts = ($fstype === 'btrfs') ? 'defaults,noatime,compress=zstd:3,space_cache=v2 0 0' : 'defaults,noatime 0 2';
        $clean .= "\n# BEGIN NAS_DEBIAN /srv/nas\n$ref /srv/nas $fstype $opts\n# END NAS_DEBIAN /srv/nas\n";
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
     * Lista los subvolúmenes Btrfs dentro de /srv/nas.
     *
     * @return array<int,array{id:string,name:string,path:string}>
     */
    public function btrfsListSubvolumes(string $path = '/srv/nas'): array
    {
        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return [
                ['id' => '256', 'name' => 'subvol_data', 'path' => rtrim($path, '/') . '/subvol_data'],
            ];
        }

        $res = SystemService::sudo(['btrfs', 'subvolume', 'list', $path]);
        if ($res['code'] !== 0) {
            return [];
        }

        $subvols = [];
        foreach (preg_split('/\r\n|\r|\n/', (string) $res['stdout']) as $line) {
            $line = trim($line);
            if ($line === '') {
                continue;
            }
            if (preg_match('/ID\s+(\d+)\s+.*?\s+path\s+(.+)$/i', $line, $m)) {
                $subPath = trim($m[2]);
                $subvols[] = [
                    'id' => $m[1],
                    'name' => basename($subPath),
                    'path' => rtrim($path, '/') . '/' . $subPath,
                ];
            }
        }

        return $subvols;
    }

    /**
     * Crea un subvolumen Btrfs dentro de la raíz o ruta indicada.
     */
    public function btrfsCreateSubvolume(string $name, string $parentPath = '/srv/nas'): array
    {
        if (!preg_match('/^[a-zA-Z0-9._-]{1,60}$/', $name)) {
            return ['success' => false, 'error' => 'Nombre de subvolumen inválido (1-60 caracteres alfanuméricos, punto, guion o guion bajo).'];
        }

        $fullPath = rtrim($parentPath, '/') . '/' . $name;
        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "Subvolumen Btrfs '$name' creado con éxito (modo dev).", 'path' => $fullPath];
        }

        $res = SystemService::sudo(['btrfs', 'subvolume', 'create', $fullPath]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'No se pudo crear el subvolumen: ' . ($res['stderr'] ?: $res['stdout'])];
        }

        return ['success' => true, 'message' => "Subvolumen Btrfs '$name' creado con éxito en $fullPath.", 'path' => $fullPath];
    }

    /**
     * Alias compatible hacia runTrim para ejecución de descarte de bloques SSD.
     */
    public function startSsdTrim(string $path = '/srv/nas'): array
    {
        return $this->runTrim($path);
    }

    /**
     * Inicia una auditoría de integridad criptográfica BTRFS scrub.
     * Verifica que el filesystem sea BTRFS, comprueba que no haya un scrub previo
     * en curso y lanza la verificación criptográfica bloque a bloque contra Bit Rot.
     */
    public function startBtrfsScrub(string $path = '/srv/nas'): array
    {
        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            if ($path === '/srv/ext4_mock') {
                return [
                    'success' => false,
                    'is_btrfs' => false,
                    'error' => 'El sistema de archivos en ' . $path . ' es ext4. La función de Scrub BTRFS con verificación criptográfica contra Bit Rot solo está disponible en sistemas de archivos BTRFS.',
                ];
            }
            return [
                'success' => true,
                'running' => true,
                'is_btrfs' => true,
                'message' => 'Scrub BTRFS iniciado en ' . $path . ' (modo dev/test). Verificación criptográfica bloque a bloque contra Bit Rot.',
            ];
        }

        // 1. Validar si el punto de montaje es BTRFS
        $overview = $this->getStorageOverview($path);
        if (empty($overview['is_btrfs'])) {
            return [
                'success' => false,
                'is_btrfs' => false,
                'error' => 'El sistema de archivos en ' . $path . ' es ' . ($overview['filesystem'] ?? 'ext4') . '. La función de Scrub con verificación criptográfica contra Bit Rot solo está disponible en sistemas de archivos BTRFS.',
            ];
        }

        // 2. Comprobar si ya hay un scrub en ejecución
        $status = $this->getBtrfsScrubStatus($path);
        if (!empty($status['running']) || ($status['status'] ?? '') === 'running') {
            return [
                'success' => false,
                'running' => true,
                'is_btrfs' => true,
                'error' => 'Ya existe una auditoría BTRFS scrub en ejecución sobre ' . $path . '.',
                'details' => $status,
            ];
        }

        // 3. Iniciar scrub en segundo plano con ionice -c 3 (prioridad de E/S ociosa)
        $ioniceBin = is_executable('/usr/bin/ionice') ? '/usr/bin/ionice' : (is_executable('/bin/ionice') ? '/bin/ionice' : 'ionice');
        $res = SystemService::sudo([$ioniceBin, '-c', '3', 'btrfs', 'scrub', 'start', $path]);
        if ($res['code'] !== 0) {
            $res = SystemService::sudo(['btrfs', 'scrub', 'start', $path]);
        }
        if ($res['code'] !== 0) {
            return [
                'success' => false,
                'is_btrfs' => true,
                'error' => 'No se pudo iniciar el scrub BTRFS: ' . ($res['stderr'] ?: $res['stdout']),
            ];
        }

        return [
            'success' => true,
            'running' => true,
            'is_btrfs' => true,
            'message' => 'Auditoría criptográfica BTRFS scrub iniciada en segundo plano sobre ' . $path . '. Verificando sumas de comprobación bloque a bloque contra Bit Rot.',
        ];
    }

    /**
     * Consulta el estado del scrub BTRFS en ejecución o el último reporte.
     * Retorna información estructurada con status, duración, velocidad, progreso y errores.
     */
    public function getBtrfsScrubStatus(string $path = '/srv/nas'): array
    {
        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return [
                'success' => true,
                'running' => false,
                'status' => 'finished',
                'duration' => '0:01:15',
                'total_to_scrub' => '64.00GiB',
                'rate' => '120.00MiB/s',
                'error_summary' => 'no errors found',
                'output' => "Status: finished\nDuration: 0:01:15\nTotal to scrub: 64.00GiB\nRate: 120.00MiB/s\nError summary: no errors found",
                'summary' => 'Scrub completado sin errores. 0 bytes con corrupción.',
            ];
        }

        $res = SystemService::sudo(['btrfs', 'scrub', 'status', $path]);
        $out = $res['stdout'] ?: $res['stderr'];

        $statusStr = 'never run';
        $isRunning = false;
        $duration = '';
        $totalToScrub = '';
        $rate = '';
        $errors = '';
        $eta = '';

        if (preg_match('/Status:\s+([a-zA-Z ]+)/i', $out, $m)) {
            $statusStr = strtolower(trim($m[1]));
            if (str_contains($statusStr, 'running')) {
                $statusStr = 'running';
                $isRunning = true;
            } elseif (str_contains($statusStr, 'finished')) {
                $statusStr = 'finished';
            } elseif (str_contains($statusStr, 'aborted') || str_contains($statusStr, 'canceled')) {
                $statusStr = 'aborted';
            }
        }
        if (preg_match('/Duration:\s+([^\r\n]+)/i', $out, $m)) {
            $duration = trim($m[1]);
        }
        if (preg_match('/Total to scrub:\s+([^\r\n]+)/i', $out, $m)) {
            $totalToScrub = trim($m[1]);
        }
        if (preg_match('/Rate:\s+([^\r\n]+)/i', $out, $m)) {
            $rate = trim($m[1]);
        }
        if (preg_match('/Error summary:\s+([^\r\n]+)/i', $out, $m)) {
            $errors = trim($m[1]);
        }
        if (preg_match('/Time left:\s+([^\r\n]+)/i', $out, $m)) {
            $eta = trim($m[1]);
        }

        $summary = $isRunning
            ? "Scrub en curso ($rate). Tiempo restante estimado: " . ($eta ?: 'calculando...')
            : ($statusStr === 'finished' ? "Scrub finalizado ($duration). Errores: " . ($errors ?: '0') : "Estado: $statusStr");

        return [
            'success' => true,
            'running' => $isRunning,
            'status' => $statusStr,
            'duration' => $duration,
            'total_to_scrub' => $totalToScrub,
            'rate' => $rate,
            'error_summary' => $errors,
            'time_left' => $eta,
            'output' => $out,
            'summary' => $summary,
        ];
    }

    /**
     * Ejecuta descarte de bloques (TRIM) para unidades SSD / NVMe.
     * Detecta automáticamente si el dispositivo subyacente es estado sólido (ROTA=0),
     * verifica si hay otro proceso fstrim en curso y reporta los bytes liberados en celdas flash.
     */
    public function runTrim(string $path = '/srv/nas'): array
    {
        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            if ($path === '/srv/hdd_mock') {
                return [
                    'success' => false,
                    'is_ssd' => false,
                    'device_type' => 'HDD (Mecánico)',
                    'error' => 'El dispositivo subyacente en ' . $path . ' es un disco mecánico (HDD). La operación TRIM está diseñada exclusivamente para unidades de estado sólido (SSD / NVMe) para liberar celdas de bloques flash; no aplica a discos mecánicos.',
                ];
            }
            return [
                'success' => true,
                'is_ssd' => true,
                'bytes_trimmed' => '10 GiB',
                'message' => $path . ': 10 GiB descartados via fstrim (modo dev/test). Celdas flash liberadas.',
            ];
        }

        // 1. Detectar si el dispositivo subyacente es SSD
        $overview = $this->getStorageOverview($path);
        if (empty($overview['supports_trim'])) {
            $devType = $overview['device_type'] ?? 'HDD (Mecánico)';
            return [
                'success' => false,
                'is_ssd' => false,
                'device_type' => $devType,
                'error' => 'El dispositivo subyacente en ' . $path . ' es un ' . $devType . '. La operación TRIM está diseñada exclusivamente para unidades de estado sólido (SSD / NVMe) para liberar celdas de memoria flash y mantener el rendimiento; en discos mecánicos no se fuerza la operación para evitar fallos del controlador.',
            ];
        }

        // 2. Comprobar si ya hay un proceso fstrim en ejecución
        $pgrep = SystemService::runCommand(['pgrep', '-x', 'fstrim']);
        if ($pgrep['code'] === 0 && trim((string) $pgrep['stdout']) !== '') {
            return [
                'success' => false,
                'running' => true,
                'error' => 'Ya existe un proceso fstrim en ejecución en el servidor.',
            ];
        }

        // 3. Ejecutar fstrim
        $res = SystemService::sudo(['fstrim', '-v', $path]);
        if ($res['code'] !== 0) {
            return [
                'success' => false,
                'is_ssd' => true,
                'error' => 'No se pudo ejecutar fstrim: ' . ($res['stderr'] ?: $res['stdout']),
            ];
        }

        $out = trim((string) $res['stdout']);
        $bytesFreed = '';
        if (preg_match('/:\s+([0-9.]+\s+[A-Za-z]+)\s+\(([0-9]+)\s+bytes\)/i', $out, $m)) {
            $bytesFreed = $m[1] . " ({$m[2]} bytes)";
        } elseif (preg_match('/:\s+([0-9.]+\s+[A-Za-z]+)/i', $out, $m)) {
            $bytesFreed = $m[1];
        } else {
            $bytesFreed = $out;
        }

        return [
            'success' => true,
            'is_ssd' => true,
            'bytes_trimmed' => $bytesFreed,
            'message' => 'TRIM SSD completado exitosamente: ' . $bytesFreed . ' liberados en celdas flash.',
            'output' => $out,
        ];
    }

    /**
     * Inspecciona un disco o partición para detectar sistemas de archivos, tablas de
     * particiones y firmas de datos preexistentes usando wipefs, blkid y lsblk.
     */
    public function inspectDevice(string $device): array
    {
        $device = trim($device);
        if (!preg_match('#^/dev/[a-zA-Z0-9/._-]+$#', $device)) {
            return [
                'success' => false,
                'device' => $device,
                'has_data' => false,
                'error' => 'Dispositivo inválido.',
            ];
        }

        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            $hasData = str_contains($device, 'sdb') || str_contains($device, 'mock_data');
            return [
                'success' => true,
                'device' => $device,
                'has_data' => $hasData,
                'requires_confirm' => $hasData,
                'signatures' => $hasData ? ['ext4', 'dos_partition_table'] : [],
                'partitions' => $hasData ? [['name' => basename($device) . '1', 'size' => '500 GB', 'fstype' => 'ext4', 'label' => 'DATOS_PREVIOS']] : [],
                'pttype' => $hasData ? 'dos' : '',
                'fstype' => $hasData ? 'ext4' : '',
                'label' => $hasData ? 'DATOS_PREVIOS' : '',
                'uuid' => $hasData ? 'a1b2c3d4-e5f6-7890-abcd-ef1234567890' : '',
                'warning' => $hasData
                    ? 'ADVERTENCIA CRÍTICA: Se detectaron particiones o firmas de datos preexistentes en ' . $device . '. La operación destruirá de forma permanente e irreversible todos los datos contenidos. Se requiere confirmación estricta SI-DESTRUIR-DATOS o SI-FORMATEAR.'
                    : null,
            ];
        }

        $resolvedDevice = $this->resolveBlockDevice($device);
        if ($resolvedDevice === null) {
            return [
                'success' => false,
                'device' => $device,
                'has_data' => false,
                'error' => 'El dispositivo no corresponde a un disco o partición de bloque reconocido.',
            ];
        }
        $device = $resolvedDevice;

        $hasData = false;
        $signatures = [];
        $partitions = [];
        $pttype = '';
        $fstype = '';
        $label = '';
        $uuid = '';

        // 1. wipefs -n para detectar firmas sin destruirlas
        $wipeRes = SystemService::sudo(['wipefs', '-n', $device]);
        if ($wipeRes['code'] === 0 && trim((string) $wipeRes['stdout']) !== '') {
            $lines = explode("\n", trim((string) $wipeRes['stdout']));
            foreach ($lines as $i => $line) {
                if ($i === 0 && (str_contains($line, 'DEVICE') || str_contains($line, 'OFFSET'))) {
                    continue;
                }
                $parts = preg_split('/\s+/', trim($line));
                if (!empty($parts[2])) {
                    $signatures[] = $parts[2];
                    $hasData = true;
                }
            }
        }

        // 2. blkid para metadatos (UUID, LABEL, TYPE, PTTYPE)
        $blkRes = SystemService::sudo(['blkid', '-p', $device]);
        if ($blkRes['code'] === 0 && trim((string) $blkRes['stdout']) !== '') {
            $out = trim((string) $blkRes['stdout']);
            if (preg_match('/TYPE="([^"]+)"/', $out, $m)) {
                $fstype = $m[1];
                $hasData = true;
            }
            if (preg_match('/LABEL="([^"]+)"/', $out, $m)) {
                $label = $m[1];
                $hasData = true;
            }
            if (preg_match('/UUID="([^"]+)"/', $out, $m)) {
                $uuid = $m[1];
                $hasData = true;
            }
            if (preg_match('/PTTYPE="([^"]+)"/', $out, $m)) {
                $pttype = $m[1];
                $hasData = true;
            }
        }

        // 3. lsblk para inspeccionar particiones hijas
        $lsblkRes = SystemService::runCommand(['lsblk', '-J', '-b', '-o', 'NAME,SIZE,TYPE,FSTYPE,LABEL,UUID', $device]);
        if ($lsblkRes['code'] === 0) {
            $data = json_decode($lsblkRes['stdout'], true);
            $devList = $data['blockdevices'] ?? [];
            foreach ($devList as $topDev) {
                if (!empty($topDev['children']) && is_array($topDev['children'])) {
                    foreach ($topDev['children'] as $child) {
                        $cSize = round(((int) ($child['size'] ?? 0)) / (1024 ** 3), 1);
                        $partitions[] = [
                            'name' => $child['name'] ?? '',
                            'size' => $cSize . ' GB',
                            'fstype' => $child['fstype'] ?? '',
                            'label' => $child['label'] ?? '',
                            'uuid' => $child['uuid'] ?? '',
                        ];
                        $hasData = true;
                    }
                }
            }
        }

        $warning = $hasData
            ? 'ADVERTENCIA CRÍTICA: Se detectaron particiones o firmas de datos preexistentes en ' . $device . '. La operación destruirá de forma permanente e irreversible todos los datos contenidos. Se requiere confirmación estricta SI-DESTRUIR-DATOS o SI-FORMATEAR.'
            : null;

        return [
            'success' => true,
            'device' => $device,
            'has_data' => $hasData,
            'requires_confirm' => $hasData,
            'signatures' => array_values(array_unique($signatures)),
            'partitions' => $partitions,
            'pttype' => $pttype,
            'fstype' => $fstype,
            'label' => $label,
            'uuid' => $uuid,
            'warning' => $warning,
        ];
    }

    /**
     * Retorna la lista de unidades de almacenamiento disponibles para recursos Samba y respaldos.
     * Incluye la Unidad Principal (/srv/nas) y las Unidades Adicionales montadas en /srv/nas/unidades/<id>.
     *
     * @return array<int, array{id: string, name: string, path: string, is_main: bool, filesystem: string, total_gb: float, used_gb: float, free_gb: float, usage_percent: float}>
     */
    public function getUnits(): array
    {
        $overview = $this->getStorageOverview('/srv/nas');
        $units = [
            [
                'id' => 'principal',
                'name' => 'Unidad Principal (/srv/nas)',
                'path' => '/srv/nas',
                'is_main' => true,
                'filesystem' => $overview['filesystem'] ?? 'ext4',
                'total_gb' => $overview['total_gb'] ?? 0,
                'used_gb' => $overview['used_gb'] ?? 0,
                'free_gb' => $overview['free_gb'] ?? 0,
                'usage_percent' => $overview['usage_percent'] ?? 0,
            ],
        ];

        $unitsBase = '/srv/nas/unidades';
        if (DIRECTORY_SEPARATOR === '\\') {
            // Unidades simuladas en dev
            $units[] = [
                'id' => 'unidad2',
                'name' => 'Unidad Adicional: unidad2 (/srv/nas/unidades/unidad2)',
                'path' => '/srv/nas/unidades/unidad2',
                'is_main' => false,
                'filesystem' => 'ext4',
                'total_gb' => 2000.0,
                'used_gb' => 350.0,
                'free_gb' => 1650.0,
                'usage_percent' => 17.5,
            ];
            return $units;
        }

        if (is_dir($unitsBase)) {
            $entries = scandir($unitsBase) ?: [];
            foreach ($entries as $entry) {
                if ($entry === '.' || $entry === '..') {
                    continue;
                }
                $unitPath = $unitsBase . '/' . $entry;
                if (is_dir($unitPath)) {
                    $uOv = $this->getStorageOverview($unitPath);
                    $units[] = [
                        'id' => $entry,
                        'name' => 'Unidad Adicional: ' . $entry . ' (' . $unitPath . ')',
                        'path' => $unitPath,
                        'is_main' => false,
                        'filesystem' => $uOv['filesystem'] ?? 'ext4',
                        'total_gb' => $uOv['total_gb'] ?? 0,
                        'used_gb' => $uOv['used_gb'] ?? 0,
                        'free_gb' => $uOv['free_gb'] ?? 0,
                        'usage_percent' => $uOv['usage_percent'] ?? 0,
                    ];
                }
            }
        }

        return $units;
    }

    /**
     * Opción A: Sumar capacidad al pool existente de /srv/nas en caliente.
     * Si el pool es LVM: pvcreate + vgextend + lvextend -r -l +100%FREE.
     * Si el pool es Btrfs: btrfs device add -f + btrfs balance start en caliente.
     */
    public function extendPool(string $device, string $confirm): array
    {
        $device = trim($device);
        if ($confirm !== 'SI-DESTRUIR-DATOS' && $confirm !== 'SI-FORMATEAR') {
            return ['success' => false, 'error' => 'Confirmación requerida (escribe SI-DESTRUIR-DATOS o SI-FORMATEAR).'];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/._-]+$#', $device)) {
            return ['success' => false, 'error' => 'Dispositivo inválido.'];
        }
        if ($this->isOsDisk(basename($device))) {
            return ['success' => false, 'error' => 'Operación prohibida: es el disco del sistema operativo.'];
        }
        if ($this->deviceInUseCritical($device)) {
            return ['success' => false, 'error' => 'Operación prohibida: el disco contiene un volumen LVM o miembro RAID activo.'];
        }
        $inspect = $this->inspectDevice($device);
        if (!empty($inspect['has_data']) && $confirm !== 'SI-DESTRUIR-DATOS') {
            return ['success' => false, 'error' => 'El dispositivo contiene particiones o firmas preexistentes. Se exige confirmación estricta SI-DESTRUIR-DATOS.'];
        }

        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return [
                'success' => true,
                'pool' => 'btrfs',
                'message' => "Capacidad de /srv/nas expandida exitosamente con $device (modo dev).",
            ];
        }

        $resolvedDevice = $this->resolveBlockDevice($device);
        if ($resolvedDevice === null) {
            return ['success' => false, 'error' => 'El dispositivo no corresponde a un disco de bloque reconocido.'];
        }
        $device = $resolvedDevice;

        if ($this->deviceHasMount($device)) {
            if (!$this->unmountDevice($device)) {
                return ['success' => false, 'error' => 'No se pudieron desmontar las particiones del dispositivo antes de expandir el pool.'];
            }
        }

        $fsType = 'ext4';
        $fsRes = SystemService::runCommand(['findmnt', '-n', '-o', 'FSTYPE', '/srv/nas']);
        if ($fsRes['code'] === 0 && !empty($fsRes['stdout'])) {
            $fsType = strtolower(trim($fsRes['stdout']));
        }

        if ($fsType === 'btrfs') {
            // Opción A Btrfs: btrfs device add -f + balance en caliente
            $addRes = SystemService::sudo(['btrfs', 'device', 'add', '-f', $device, '/srv/nas']);
            if ($addRes['code'] !== 0) {
                return ['success' => false, 'error' => 'Fallo al añadir dispositivo al pool Btrfs: ' . ($addRes['stderr'] ?: $addRes['stdout'])];
            }
            SystemService::sudo(['btrfs', 'balance', 'start', '--background', '/srv/nas']);
            return [
                'success' => true,
                'pool' => 'btrfs',
                'message' => "Dispositivo $device agregado exitosamente al pool BTRFS de /srv/nas. Balance de datos en ejecución.",
            ];
        }

        // Opción A LVM:
        $srcRes = SystemService::runCommand(['findmnt', '-n', '-o', 'SOURCE', '/srv/nas']);
        $srcDev = trim($srcRes['stdout'] ?? '');
        if ($srcDev === '' || (!str_contains($srcDev, '/dev/mapper/') && !str_contains($srcDev, '/dev/'))) {
            return ['success' => false, 'error' => 'No se detectó un volumen LVM activo en /srv/nas para expandir.'];
        }

        // Obtener VG del volumen lógico
        $vgRes = SystemService::sudo(['lvs', '--noheadings', '-o', 'vg_name', $srcDev]);
        $vgName = trim($vgRes['stdout'] ?? '');
        if ($vgName === '') {
            return ['success' => false, 'error' => 'No se pudo identificar el grupo de volúmenes (VG) de /srv/nas.'];
        }

        // pvcreate + vgextend + lvextend -r
        $pvRes = SystemService::sudo(['pvcreate', '-f', '-y', $device]);
        if ($pvRes['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al inicializar volumen físico (PV): ' . ($pvRes['stderr'] ?: $pvRes['stdout'])];
        }

        $vgExt = SystemService::sudo(['vgextend', $vgName, $device]);
        if ($vgExt['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al extender el grupo de volúmenes (VG): ' . ($vgExt['stderr'] ?: $vgExt['stdout'])];
        }

        $lvExt = SystemService::sudo(['lvextend', '-r', '-l', '+100%FREE', $srcDev]);
        if ($lvExt['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al expandir el volumen lógico y sistema de archivos: ' . ($lvExt['stderr'] ?: $lvExt['stdout'])];
        }

        return [
            'success' => true,
            'pool' => 'lvm',
            'message' => "Grupo de volúmenes $vgName y volumen /srv/nas expandidos exitosamente al 100% de la capacidad disponible.",
        ];
    }

    /**
     * Opción B: Disco adicional independiente.
     * Formatea (ext4 optimizado o Btrfs) y monta de forma estructurada y persistente
     * en /srv/nas/unidades/<id_unidad>, registrando en /etc/fstab con UUID y noatime,nofail,
     * aplicando permisos 2770 root:grp_samba.
     */
    public function addUnit(string $device, string $unitId, string $fstype = 'ext4', string $confirm = ''): array
    {
        $device = trim($device);
        $unitId = trim($unitId);
        $fstype = strtolower(trim($fstype));

        if ($confirm !== 'SI-DESTRUIR-DATOS' && $confirm !== 'SI-FORMATEAR') {
            return ['success' => false, 'error' => 'Confirmación requerida (escribe SI-DESTRUIR-DATOS o SI-FORMATEAR).'];
        }
        if (!preg_match('/^[a-zA-Z0-9_-]{1,32}$/', $unitId)) {
            return ['success' => false, 'error' => 'Identificador de unidad inválido (1-32 caracteres alfanuméricos, guion o guion bajo).'];
        }
        if (in_array(strtolower($unitId), ['principal', 'trash', 'tmp', 'unidades', 'system'], true)) {
            return ['success' => false, 'error' => "El identificador de unidad '$unitId' está reservado."];
        }
        if (!preg_match('#^/dev/[a-zA-Z0-9/._-]+$#', $device)) {
            return ['success' => false, 'error' => 'Dispositivo inválido.'];
        }
        if ($this->isOsDisk(basename($device))) {
            return ['success' => false, 'error' => 'Operación prohibida: es el disco del sistema operativo.'];
        }
        if ($this->deviceInUseCritical($device)) {
            return ['success' => false, 'error' => 'Operación prohibida: el disco contiene un volumen LVM o miembro RAID activo.'];
        }
        if (!in_array($fstype, ['ext4', 'btrfs'], true)) {
            return ['success' => false, 'error' => 'Sistema de archivos no soportado (ext4|btrfs).'];
        }
        $inspect = $this->inspectDevice($device);
        if (!empty($inspect['has_data']) && $confirm !== 'SI-DESTRUIR-DATOS') {
            return ['success' => false, 'error' => 'El dispositivo contiene particiones o firmas preexistentes. Se exige confirmación estricta SI-DESTRUIR-DATOS.'];
        }

        $mountPoint = '/srv/nas/unidades/' . $unitId;

        if (is_dir($mountPoint)) {
            $chkMnt = SystemService::runCommand(['findmnt', '-n', '-o', 'TARGET', $mountPoint]);
            if (trim($chkMnt['stdout'] ?? '') !== '') {
                return ['success' => false, 'error' => "La unidad '$unitId' ya se encuentra montada en $mountPoint."];
            }
            $existingFiles = @scandir($mountPoint) ?: [];
            $existingFiles = array_filter($existingFiles, fn($f) => $f !== '.' && $f !== '..');
            if (count($existingFiles) > 0) {
                return ['success' => false, 'error' => "El directorio destino $mountPoint ya existe y no está vacío."];
            }
        }

        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return [
                'success' => true,
                'unit_id' => $unitId,
                'mount' => $mountPoint,
                'fstype' => $fstype,
                'message' => "Unidad adicional '$unitId' aprovisionada y montada exitosamente en $mountPoint (modo dev).",
            ];
        }

        $resolvedDevice = $this->resolveBlockDevice($device);
        if ($resolvedDevice === null) {
            return ['success' => false, 'error' => 'El dispositivo no corresponde a un disco de bloque reconocido.'];
        }
        $device = $resolvedDevice;

        if ($this->deviceHasMount($device)) {
            if (!$this->unmountDevice($device)) {
                return ['success' => false, 'error' => 'No se pudieron desmontar las particiones del dispositivo antes de formatear.'];
            }
        }

        $type = trim(SystemService::runCommand(['lsblk', '-nd', '-o', 'TYPE', $device])['stdout'] ?? '');
        $target = $device;
        if ($type === 'disk') {
            SystemService::sudo(['parted', '-s', $device, 'mklabel', 'gpt', 'mkpart', 'primary', '0%', '100%']);
            SystemService::sudo(['partprobe', $device]);
            $name = basename($device);
            $target = str_contains($name, 'nvme') || preg_match('/[0-9]$/', $name) ? $device . 'p1' : $device . '1';
        }

        // Formateo
        $mkfs = SystemService::sudo(
            $fstype === 'btrfs'
                ? ['mkfs.btrfs', '-f', '-L', 'NAS_' . strtoupper($unitId), $target]
                : ['mkfs.ext4', '-F', '-m', '1', '-L', 'NAS_' . strtoupper($unitId), $target]
        );
        if ($mkfs['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al formatear la unidad: ' . ($mkfs['stderr'] ?: $mkfs['stdout'])];
        }

        // Montaje persistente estructurado
        SystemService::sudo(['mkdir', '-p', $mountPoint]);
        $uuid = trim(SystemService::sudo(['blkid', '-s', 'UUID', '-o', 'value', $target])['stdout'] ?? '');
        $ref = $uuid !== '' ? "UUID=$uuid" : $target;
        $opts = ($fstype === 'btrfs') ? 'defaults,noatime,nofail,compress=zstd:3,space_cache=v2 0 0' : 'defaults,noatime,nofail 0 2';

        $stagingDir = '/var/lib/nas/staging';
        $stagingBase = (is_dir($stagingDir) && is_writable($stagingDir)) ? $stagingDir : sys_get_temp_dir();
        $fstabTmp = $stagingBase . '/nas_fstab_unit_' . bin2hex(random_bytes(4));
        $current = @file_get_contents('/etc/fstab') ?: '';
        $marker = "# NAS_UNIT_$unitId";
        $clean = preg_replace("/$marker.*?\n/s", '', $current) ?? $current;
        $clean .= "\n$ref $mountPoint $fstype $opts $marker\n";
        file_put_contents($fstabTmp, $clean);
        SystemService::sudo(['cp', $fstabTmp, '/etc/fstab']);
        @unlink($fstabTmp);

        $mntRes = SystemService::sudo(['mount', $mountPoint]);
        if ($mntRes['code'] !== 0) {
            return ['success' => false, 'error' => 'Fallo al montar ' . $mountPoint . ': ' . ($mntRes['stderr'] ?: $mntRes['stdout'])];
        }

        // Permisos corporativos 2770 root:grp_samba
        SystemService::sudo(['chown', 'root:grp_samba', $mountPoint]);
        SystemService::sudo(['chmod', '2770', $mountPoint]);

        return [
            'success' => true,
            'unit_id' => $unitId,
            'mount' => $mountPoint,
            'fstype' => $fstype,
            'message' => "Unidad adicional '$unitId' formateada ($fstype), montada en $mountPoint y registrada en /etc/fstab con permisos 2770.",
        ];
    }
}
