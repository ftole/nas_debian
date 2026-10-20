<?php

declare(strict_types=1);

namespace App\Services;

/**
 * Servicio de administración de Samba 4, gestión de /etc/samba/smb.conf,
 * recursos compartidos visibles y ocultos ($), y 4 esquemas de permisos granulares.
 */
class SambaService
{
    private string $confPath;

    public function __construct(string $confPath = '/etc/samba/smb.conf')
    {
        $this->confPath = $confPath;
    }

    /**
     * Lista los recursos compartidos definidos en smb.conf excluyendo secciones internas.
     */
    public function listShares(): array
    {
        if (DIRECTORY_SEPARATOR === '\\' || !file_exists($this->confPath)) {
            return [
                [
                    'name' => 'SISTEMAS',
                    'path' => '/srv/nas/SISTEMAS',
                    'comment' => 'Recurso maestro del departamento de Sistemas',
                    'scheme' => 1,
                    'scheme_name' => 'Lectura y Escritura por Grupo',
                    'hidden' => false,
                    'read_only' => false,
                    'guest_ok' => false,
                    'valid_users' => ['@grp_samba'],
                    'write_list' => ['@grp_samba'],
                ],
                [
                    'name' => 'BACKUPS_WINDOWS$',
                    'path' => '/srv/nas/BACKUPS_HISTORICOS',
                    'comment' => 'Repositorio administrativo de copias de Windows',
                    'scheme' => 1,
                    'scheme_name' => 'Lectura y Escritura por Grupo',
                    'hidden' => true,
                    'read_only' => false,
                    'guest_ok' => false,
                    'valid_users' => ['@grp_samba'],
                    'write_list' => ['@grp_samba'],
                ],
                [
                    'name' => 'PUBLICO',
                    'path' => '/srv/nas/PUBLICO',
                    'comment' => 'Carpeta de intercambio general para invitados',
                    'scheme' => 4,
                    'scheme_name' => 'Acceso Público / Invitados',
                    'hidden' => false,
                    'read_only' => false,
                    'guest_ok' => true,
                    'valid_users' => [],
                    'write_list' => [],
                ],
            ];
        }

        $sections = $this->parseSmbConf();
        $shares = [];

        foreach ($sections as $name => $props) {
            $lower = strtolower($name);
            if (in_array($lower, ['global', 'printers', 'print$'], true)) {
                continue;
            }

            $path = $props['path'] ?? '';
            $comment = $props['comment'] ?? '';
            $readOnly = (strtolower($props['read only'] ?? 'yes') === 'yes');
            $guestOk = (strtolower($props['guest ok'] ?? ($props['public'] ?? 'no')) === 'yes');

            $validUsersRaw = $props['valid users'] ?? '';
            $writeListRaw = $props['write list'] ?? '';

            $validUsers = !empty($validUsersRaw) ? preg_split('/\s+/', trim($validUsersRaw)) : [];
            $writeList = !empty($writeListRaw) ? preg_split('/\s+/', trim($writeListRaw)) : [];

            // Identificar esquema de permisos
            $scheme = 1;
            $schemeName = 'Lectura y Escritura por Grupo';

            if ($guestOk) {
                $scheme = 4;
                $schemeName = 'Acceso Público / Invitados';
                $readOnly = false;
            } elseif (!empty($writeList)) {
                if (!empty($validUsers) && count($writeList) < count($validUsers)) {
                    $scheme = 2;
                    $schemeName = 'Solo Lectura General + Escritura Exclusiva';
                    $readOnly = false;
                } else {
                    $scheme = 1;
                    $schemeName = 'Lectura y Escritura por Grupo';
                    $readOnly = false;
                }
            } elseif ($readOnly) {
                $scheme = 3;
                $schemeName = 'Solo Lectura Estricta';
                $readOnly = true;
            } else {
                $scheme = 1;
                $schemeName = 'Lectura y Escritura por Grupo';
                $readOnly = false;
            }

            $shares[] = [
                'name' => $name,
                'path' => $path,
                'comment' => $comment,
                'scheme' => $scheme,
                'scheme_name' => $schemeName,
                'hidden' => str_ends_with($name, '$'),
                'read_only' => $readOnly,
                'guest_ok' => $guestOk,
                'valid_users' => $validUsers,
                'write_list' => $writeList,
            ];
        }

        return $shares;
    }

    /**
     * Crea un nuevo recurso compartido aplicando el esquema de permisos y configurando
     * las ACLs POSIX en disco con herencia por defecto.
     */
    public function createShare(array $data): array
    {
        $name = str_replace(["\r", "\n"], '', trim((string) ($data['name'] ?? '')));
        $comment = $this->sanitizeComment((string) ($data['comment'] ?? ''));
        $scheme = (int) ($data['scheme'] ?? 1);
        $groups = (array) ($data['groups'] ?? []);
        $writeGroup = str_replace(["\r", "\n"], '', trim((string) ($data['write_group'] ?? '')));
        $isGuest = ($scheme === 4) || !empty($data['guest_ok']);
        $isHidden = !empty($data['hidden']);

        // Nombre de recurso válido
        if (!preg_match('/^[A-Za-z0-9_-]{1,60}\$?$/', $name)) {
            return ['success' => false, 'error' => 'El nombre del recurso debe tener entre 1 y 60 caracteres alfanuméricos.'];
        }

        $reserved = ['global', 'homes', 'printers', 'print$', 'ipc$', 'netlogon', 'sysvol'];
        $lower = strtolower(rtrim($name, '$'));
        if (in_array(strtolower($name), $reserved, true) || in_array($lower, $reserved, true)) {
            return ['success' => false, 'error' => "El nombre de recurso [$name] está reservado por el sistema."];
        }

        if ($isHidden && !str_ends_with($name, '$')) {
            $name .= '$';
        }

        // Ruta estándar dentro del almacenamiento administrado (/srv/nas o /srv/nas/unidades/<id>)
        $subfolder = rtrim($name, '$');
        $baseDir = '/srv/nas';
        $unit = trim((string) ($data['unit'] ?? ($data['unit_id'] ?? '')));
        if ($unit !== '' && $unit !== 'principal' && $unit !== '/srv/nas') {
            $cleanUnit = preg_replace('#^(/srv/nas/unidades/|unidades/)#', '', $unit);
            $cleanUnit = trim((string) $cleanUnit, '/');
            if (!preg_match('/^[a-zA-Z0-9_-]{1,32}$/', $cleanUnit)) {
                return ['success' => false, 'error' => 'Identificador de unidad no válido.'];
            }
            $baseDir = '/srv/nas/unidades/' . $cleanUnit;
        }

        if (!empty($data['path']) && (str_starts_with((string) $data['path'], '/srv/nas/') || str_starts_with((string) $data['path'], '/srv/nas'))) {
            $pathCandidate = rtrim((string) $data['path'], '/');
            if (!str_contains($pathCandidate, '..')) {
                $path = $pathCandidate;
            }
        }
        if (!isset($path)) {
            $path = $baseDir . '/' . $subfolder;
        }

        $serverRole = SystemService::getServerRole();
        if ($serverRole === 'BACKUP') {
            $isExplicitlyHidden = $isHidden || str_ends_with($name, '$');
            $isBrowseable = isset($data['browseable']) && ($data['browseable'] === 'yes' || $data['browseable'] === true || $data['browseable'] === 'true');
            if ($isGuest || $scheme === 4 || !$isExplicitlyHidden || (isset($data['hidden']) && !$data['hidden']) || $isBrowseable) {
                return ['success' => false, 'error' => 'En el rol Central de Backup solo se permiten recursos de respaldo ocultos ($) sin acceso público.'];
            }
            $isHidden = true;
            if (!str_ends_with($name, '$')) {
                $name .= '$';
            }
            foreach ($groups as $g) {
                $gClean = ltrim(trim($g), '@');
                if ($gClean !== '' && !in_array($gClean, ['grp_samba', 'grp_backups'], true)) {
                    return ['success' => false, 'error' => 'En el rol Central de Backup los recursos solo pueden ser accesibles por grp_samba o grp_backups.'];
                }
            }
            if ($writeGroup !== '') {
                $wClean = ltrim(trim($writeGroup), '@');
                if ($wClean !== '' && !in_array($wClean, ['grp_samba', 'grp_backups'], true)) {
                    return ['success' => false, 'error' => 'En el rol Central de Backup los recursos solo pueden ser accesibles por grp_samba o grp_backups.'];
                }
            }
        }

        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "Recurso [$name] creado correctamente (modo dev).", 'path' => $path];
        }

        // 1. Preparar directorio en disco
        SystemService::sudo(['mkdir', '-p', $path]);
        SystemService::sudo(['chown', 'root:grp_samba', $path]);
        SystemService::sudo(['chmod', '2770', $path]);

        // Construir directivas Samba según esquema
        $shareProps = [
            'comment' => $comment ?: "Recurso $name",
            'path' => $path,
            'browseable' => $isHidden ? 'no' : 'yes',
            'create mask' => '0770',
            'directory mask' => '0770',
            'force create mode' => '0770',
            'force directory mode' => '0770',
            'vfs objects' => 'acl_xattr streams_xattr full_audit',
        ];

        // Limpiar formato de grupos para Samba (@nombre)
        $cleanGroups = [];
        foreach ($groups as $g) {
            $gClean = ltrim(trim($g), '@');
            if (preg_match('/^[a-z0-9_-]+$/i', $gClean)) {
                $cleanGroups[] = '@' . $gClean;
            }
        }
        // Los administradores (grp_samba) conservan acceso a todos los recursos
        if (!in_array('@grp_samba', $cleanGroups, true)) {
            $cleanGroups[] = '@grp_samba';
        }

        switch ($scheme) {
            case 1: // Lectura y Escritura por Grupo
                $shareProps['read only'] = 'no';
                $shareProps['guest ok'] = 'no';
                if (!empty($cleanGroups)) {
                    $shareProps['valid users'] = implode(' ', $cleanGroups);
                    $shareProps['write list'] = implode(' ', $cleanGroups);
                }
                break;

            case 2: // Solo Lectura General + Escritura Exclusiva
                $shareProps['read only'] = 'yes';
                $shareProps['guest ok'] = 'no';
                $wgClean = ltrim($writeGroup, '@');
                $wEntry = '@' . $wgClean;
                $allUsers = array_unique(array_merge($cleanGroups, [$wEntry]));
                $shareProps['valid users'] = implode(' ', $allUsers);
                $shareProps['write list'] = trim($wEntry . ' @grp_samba');
                break;

            case 3: // Solo Lectura Estricta
                $shareProps['read only'] = 'yes';
                $shareProps['guest ok'] = 'no';
                if (!empty($cleanGroups)) {
                    $shareProps['valid users'] = implode(' ', $cleanGroups);
                }
                break;

            case 4: // Acceso Público / Invitados
                $shareProps['read only'] = 'no';
                $shareProps['guest ok'] = 'yes';
                $shareProps['public'] = 'yes';
                $shareProps['guest only'] = 'yes';
                $shareProps['create mask'] = '0777';
                $shareProps['directory mask'] = '0777';
                $shareProps['force create mode'] = '0777';
                $shareProps['force directory mode'] = '0777';
                SystemService::sudo(['chmod', '2777', $path]);
                break;
        }

        // 2. Modificar smb.conf de forma atómica y verificar con testparm
        $updateRes = $this->updateShareInConf($name, $shareProps);
        if (!$updateRes['success']) {
            return $updateRes;
        }

        // 3. Recargar servicio Samba y sincronizar ACL POSIX con el esquema definido.
        SystemService::sudo(['systemctl', 'reload', 'smbd']);
        $this->syncShareAcls($name);

        return ['success' => true, 'message' => "Recurso compartido [$name] creado y activo en Samba.", 'path' => $path];
    }

    /**
     * Elimina una sección de recurso de smb.conf y recarga Samba.
     */
    public function deleteShare(string $name, bool $deleteFiles = false): array
    {
        $reserved = ['global', 'homes', 'printers', 'print$', 'ipc$', 'netlogon', 'sysvol'];
        $lower = strtolower($name);
        $lowerClean = strtolower(rtrim($name, '$'));
        if (in_array($lower, $reserved, true) || in_array($lowerClean, $reserved, true)) {
            return ['success' => false, 'error' => "La sección [$name] está protegida por el sistema."];
        }

        if (DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "Recurso [$name] eliminado (modo dev)."];
        }

        $sections = $this->parseSmbConf();
        if (!isset($sections[$name])) {
            return ['success' => false, 'error' => "El recurso [$name] no existe en smb.conf."];
        }

        $sharePath = $sections[$name]['path'] ?? '';
        unset($sections[$name]);

        $saveRes = $this->saveSectionsToConf($sections);
        if (!$saveRes['success']) {
            return $saveRes;
        }

        SystemService::sudo(['systemctl', 'reload', 'smbd']);

        if ($deleteFiles && !empty($sharePath) && str_starts_with($sharePath, '/srv/nas/') && $sharePath !== '/srv/nas') {
            SystemService::sudo(['rm', '-rf', $sharePath]);
        }

        return ['success' => true, 'message' => "Recurso [$name] eliminado correctamente."];
    }

    /**
     * Parser nativo para archivos de configuración INI estilo smb.conf.
     */
    private function parseSmbConf(): array
    {
        if (!file_exists($this->confPath)) {
            return [];
        }

        $lines = file($this->confPath, FILE_IGNORE_NEW_LINES);
        $sections = [];
        $currentSection = null;

        foreach ($lines as $line) {
            $trimmed = trim($line);
            if (empty($trimmed) || str_starts_with($trimmed, '#') || str_starts_with($trimmed, ';')) {
                continue;
            }

            if (preg_match('/^\[([^\]]+)\]$/', $trimmed, $m)) {
                $currentSection = trim($m[1]);
                $sections[$currentSection] = [];
                continue;
            }

            if ($currentSection !== null && str_contains($trimmed, '=')) {
                [$key, $val] = explode('=', $trimmed, 2);
                $sections[$currentSection][trim($key)] = trim($val);
            }
        }

        return $sections;
    }

    /**
     * Agrega o reemplaza un recurso en smb.conf de forma atómica y valida con testparm.
     */
    private function updateShareInConf(string $name, array $props): array
    {
        $sections = $this->parseSmbConf();
        $sections[$name] = $props;
        return $this->saveSectionsToConf($sections);
    }

    /**
     * Agrega, actualiza o elimina directivas en la sección [global] de smb.conf de forma
     * insensible a mayúsculas/minúsculas para evitar duplicados y conflictos de sintaxis.
     */
    public function updateGlobalConfig(array $setProps, array $removeKeys = []): array
    {
        $sections = $this->parseSmbConf();
        if (!isset($sections['global'])) {
            $sections['global'] = [];
        }

        $cleanRemove = array_map(fn($k) => strtolower(trim((string) $k)), $removeKeys);

        // Eliminar claves solicitadas (comparación insensible a mayúsculas/minúsculas)
        foreach (array_keys($sections['global']) as $existingKey) {
            if (in_array(strtolower(trim((string) $existingKey)), $cleanRemove, true)) {
                unset($sections['global'][$existingKey]);
            }
        }

        // Establecer o actualizar claves asegurando no duplicar por variaciones de formato/caja
        foreach ($setProps as $k => $v) {
            $kLower = strtolower(trim((string) $k));
            foreach (array_keys($sections['global']) as $existingKey) {
                if (strtolower(trim((string) $existingKey)) === $kLower) {
                    unset($sections['global'][$existingKey]);
                }
            }
            $sections['global'][$k] = $v;
        }

        return $this->saveSectionsToConf($sections);
    }

    /**
     * Escribe las secciones a un archivo temporal, comprueba sintaxis con testparm y
     * promueve atómicamente a /etc/samba/smb.conf.
     */
    private function saveSectionsToConf(array $sections): array
    {
        $buffer = "# ==============================================================================\n";
        $buffer .= "# Servidor NAS Debian 13 - Configuración Samba (smb.conf)\n";
        $buffer .= "# Generado automáticamente por el Panel Web NAS\n";
        $buffer .= "# ==============================================================================\n\n";

        foreach ($sections as $sectionName => $props) {
            // smb.conf es sensible a saltos de linea: ningun nombre de seccion, clave
            // o valor puede contener CR/LF para evitar inyeccion de directivas arbitrarias.
            $cleanSection = str_replace(["\r", "\n"], '', (string) $sectionName);
            $buffer .= "[$cleanSection]\n";
            foreach ($props as $key => $val) {
                $cleanKey = str_replace(["\r", "\n"], '', (string) $key);
                $cleanVal = str_replace(["\r", "\n"], '', (string) $val);
                $buffer .= "   $cleanKey = $cleanVal\n";
            }
            $buffer .= "\n";
        }

        $lockFile = '/var/lock/nas_smbconf.lock';
        if (DIRECTORY_SEPARATOR === '\\' || !is_dir('/var/lock') || !is_writable('/var/lock')) {
            $lockFile = sys_get_temp_dir() . '/nas_smbconf.lock';
        }

        $lockFp = @fopen($lockFile, 'c+');
        if ($lockFp !== false) {
            flock($lockFp, LOCK_EX);
        }

        try {
            $stagingDir = '/var/lib/nas/staging';
            $tmpDir = (is_dir($stagingDir) && is_writable($stagingDir)) ? $stagingDir : sys_get_temp_dir();
            $tmpFile = tempnam($tmpDir, 'smbconf_');
            if ($tmpFile === false) {
                return ['success' => false, 'error' => 'No se pudo crear archivo temporal para smb.conf.'];
            }

            file_put_contents($tmpFile, $buffer);

            // Validar con testparm -s si la herramienta está disponible en el sistema
            $hasTestparm = false;
            $pathDirs = explode(PATH_SEPARATOR, (string)(getenv('PATH') ?: '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'));
            foreach ($pathDirs as $dir) {
                $binPath = rtrim($dir, '/\\') . DIRECTORY_SEPARATOR . 'testparm';
                if (@is_file($binPath) && @is_executable($binPath)) {
                    $hasTestparm = true;
                    break;
                }
            }

            if ($hasTestparm) {
                $tpRes = SystemService::runCommand(['testparm', '-s', $tmpFile]);
                if ($tpRes['code'] !== 0) {
                    @unlink($tmpFile);
                    return [
                        'success' => false,
                        'error' => 'La configuración generada tiene errores de sintaxis en testparm: ' . ($tpRes['stderr'] ?: $tpRes['stdout']),
                    ];
                }
            }

            // Si la ruta destino tiene permisos de escritura directos (ej. pruebas o tmp),
            // usar copy directamente; de lo contrario invocar sudo cp.
            if (is_writable($this->confPath) || (!file_exists($this->confPath) && is_writable(dirname($this->confPath)))) {
                if (!@copy($tmpFile, $this->confPath)) {
                    $cpRes = SystemService::sudo(['cp', $tmpFile, $this->confPath]);
                    if ($cpRes['code'] !== 0) {
                        @unlink($tmpFile);
                        return ['success' => false, 'error' => 'Error al escribir en ' . $this->confPath . ': ' . $cpRes['stderr']];
                    }
                }
            } else {
                $cpRes = SystemService::sudo(['cp', $tmpFile, $this->confPath]);
                if ($cpRes['code'] !== 0) {
                    @unlink($tmpFile);
                    return ['success' => false, 'error' => 'Error al escribir en ' . $this->confPath . ': ' . $cpRes['stderr']];
                }
            }
            @unlink($tmpFile);

            return ['success' => true];
        } finally {
            if ($lockFp !== false) {
                flock($lockFp, LOCK_UN);
                fclose($lockFp);
            }
        }
    }

    /**
     * Edita un recurso existente (esquema, grupos autorizados, grupo de escritura,
     * visibilidad y comentario) preservando las concesiones explícitas por usuario.
     */
    public function updateShare(string $name, array $data): array
    {
        $reserved = ['global', 'homes', 'printers', 'print$', 'ipc$', 'netlogon', 'sysvol'];
        $lower = strtolower(rtrim($name, '$'));
        if (in_array(strtolower($name), $reserved, true) || in_array($lower, $reserved, true)) {
            return ['success' => false, 'error' => "La sección [$name] está protegida por el sistema y no puede modificarse como recurso compartido."];
        }

        $serverRole = SystemService::getServerRole();
        if ($serverRole === 'BACKUP') {
            $schemeVal = (int) ($data['scheme'] ?? 0);
            $guestOkVal = !empty($data['guest_ok']) || !empty($data['guest-ok']);
            $isBrowseable = isset($data['browseable']) ? ($data['browseable'] === 'yes' || $data['browseable'] === true || $data['browseable'] === 'true') : null;
            if ($schemeVal === 4 || $guestOkVal || !str_ends_with($name, '$') || (isset($data['hidden']) && !$data['hidden']) || ($isBrowseable === true)) {
                return ['success' => false, 'error' => 'En el rol Central de Backup solo se permiten recursos de respaldo ocultos ($) sin acceso público.'];
            }
            if (array_key_exists('groups', $data)) {
                foreach ((array) $data['groups'] as $g) {
                    $gClean = ltrim(trim($g), '@');
                    if ($gClean !== '' && !in_array($gClean, ['grp_samba', 'grp_backups'], true)) {
                        return ['success' => false, 'error' => 'En el rol Central de Backup los recursos solo pueden ser accesibles por grp_samba o grp_backups.'];
                    }
                }
            }
            if (array_key_exists('valid_users', $data)) {
                foreach ((array) $data['valid_users'] as $g) {
                    $gClean = ltrim(trim($g), '@');
                    if (str_starts_with(trim($g), '@') && !in_array($gClean, ['grp_samba', 'grp_backups'], true)) {
                        return ['success' => false, 'error' => 'En el rol Central de Backup los recursos solo pueden ser accesibles por grp_samba o grp_backups.'];
                    }
                }
            }
            if (array_key_exists('write_list', $data)) {
                foreach ((array) $data['write_list'] as $g) {
                    $gClean = ltrim(trim($g), '@');
                    if (str_starts_with(trim($g), '@') && !in_array($gClean, ['grp_samba', 'grp_backups'], true)) {
                        return ['success' => false, 'error' => 'En el rol Central de Backup los recursos solo pueden ser accesibles por grp_samba o grp_backups.'];
                    }
                }
            }
            if (!empty($data['write_group'])) {
                $wClean = ltrim(trim($data['write_group']), '@');
                if ($wClean !== '' && !in_array($wClean, ['grp_samba', 'grp_backups'], true)) {
                    return ['success' => false, 'error' => 'En el rol Central de Backup los recursos solo pueden ser accesibles por grp_samba o grp_backups.'];
                }
            }
            $data['hidden'] = true;
        }

        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Recurso [$name] actualizado (modo dev)."];
        }

        $sections = $this->parseSmbConf();
        if (!isset($sections[$name])) {
            return ['success' => false, 'error' => "El recurso [$name] no existe en smb.conf."];
        }

        $props = $sections[$name];
        $scheme = (int) ($data['scheme'] ?? 0);
        $comment = $this->sanitizeComment((string) ($data['comment'] ?? ''));

        // Separar tokens de grupo (@grupo) de los usuarios explícitos (suelto)
        $valid = $this->splitTokens((string) ($props['valid users'] ?? ''));
        $write = $this->splitTokens((string) ($props['write list'] ?? ''));
        $validUsers = array_values(array_filter($valid, fn($t) => !str_starts_with($t, '@')));
        $writeUsers = array_values(array_filter($write, fn($t) => !str_starts_with($t, '@')));

        if (array_key_exists('groups', $data)) {
            $groups = $this->cleanGroupTokens((array) $data['groups']);
        } else {
            // Preservar grupos existentes si no se suministran
            $groups = array_values(array_filter($valid, fn($t) => str_starts_with($t, '@')));
        }
        // Los administradores (grp_samba) conservan acceso a todos los recursos
        if (!in_array('@grp_samba', $groups, true)) {
            $groups[] = '@grp_samba';
        }
        $writeGroup = str_replace(["\r", "\n"], '', ltrim(trim((string) ($data['write_group'] ?? '')), '@'));
        $writeEntry = $writeGroup !== '' ? '@' . $writeGroup : '';
        $isHidden = !empty($data['hidden']);

        if ($comment !== '') {
            $props['comment'] = $comment;
        }
        $props['browseable'] = $isHidden ? 'no' : 'yes';

        if ($scheme === 4) {
            $props['read only'] = 'no';
            $props['guest ok'] = 'yes';
            $props['public'] = 'yes';
            $props['guest only'] = 'yes';
            $props['create mask'] = '0777';
            $props['directory mask'] = '0777';
            unset($props['valid users'], $props['write list']);
        } elseif ($scheme === 3) {
            $props['read only'] = 'yes';
            $props['guest ok'] = 'no';
            unset($props['public'], $props['guest only']);
            $props['valid users'] = $this->joinTokens(array_merge($groups, $validUsers));
            unset($props['write list']);
        } elseif ($scheme === 2) {
            $props['read only'] = 'yes';
            $props['guest ok'] = 'no';
            unset($props['public'], $props['guest only']);
            $groupsWithWriter = $groups;
            if ($writeEntry !== '') {
                $groupsWithWriter[] = $writeEntry;
            }
            $props['valid users'] = $this->joinTokens(array_merge($groupsWithWriter, $validUsers));
            $props['write list'] = $this->joinTokens(array_merge($writeEntry !== '' ? [$writeEntry] : [], $writeUsers, ['@grp_samba']));
        } else {
            $props['read only'] = 'no';
            $props['guest ok'] = 'no';
            unset($props['public'], $props['guest only']);
            $props['valid users'] = $this->joinTokens(array_merge($groups, $validUsers));
            $props['write list'] = $this->joinTokens(array_merge($groups, $writeUsers));
        }

        foreach (['valid users', 'write list'] as $k) {
            if (isset($props[$k]) && $props[$k] === '') {
                unset($props[$k]);
            }
        }

        $sections[$name] = $props;
        $res = $this->saveSectionsToConf($sections);
        if (!$res['success']) {
            return $res;
        }
        SystemService::sudo(['systemctl', 'reload', 'smbd']);
        $this->syncShareAcls($name);

        return ['success' => true, 'message' => "Recurso [$name] actualizado correctamente."];
    }

    /**
     * Concede, cambia o revoca el acceso de un grupo o usuario a un recurso.
     *
     * @param string $share  Nombre del recurso.
     * @param string $kind   'group' | 'user'.
     * @param string $target Nombre del grupo (grp_*) o usuario.
     * @param string $level  'none' | 'read' | 'write'.
     */
    public function setAccess(string $share, string $kind, string $target, string $level): array
    {
        $level = match (strtolower(trim($level))) {
            'ro' => 'read',
            'rw' => 'write',
            default => strtolower(trim($level)),
        };
        if (!in_array($level, ['none', 'read', 'write'], true)) {
            return ['success' => false, 'error' => 'Nivel de acceso inválido (use read/write/none o ro/rw).'];
        }

        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Acceso de [$target] en [$share] fijado a '$level' (modo dev)."];
        }

        $sections = $this->parseSmbConf();
        if (!isset($sections[$share])) {
            return ['success' => false, 'error' => "El recurso [$share] no existe en smb.conf."];
        }

        $serverRole = SystemService::getServerRole();
        if ($serverRole === 'BACKUP' && $kind === 'group' && $level !== 'none') {
            $gClean = ltrim(trim($target), '@');
            if ($gClean !== '' && !in_array($gClean, ['grp_samba', 'grp_backups'], true)) {
                return ['success' => false, 'error' => 'En el rol Central de Backup los recursos solo pueden ser accesibles por grp_samba o grp_backups.'];
            }
        }
        if ($serverRole === 'BACKUP' && $kind === 'user' && $level !== 'none') {
            $userClean = trim($target);
            if (DIRECTORY_SEPARATOR !== '\\' && function_exists('posix_getpwnam') && getenv('APP_ENV') !== 'testing') {
                $grpRes = SystemService::runCommand(['id', '-Gn', $userClean]);
                if ($grpRes['code'] === 0) {
                    $userGroups = preg_split('/\s+/', trim($grpRes['stdout']));
                    if (!in_array('grp_samba', $userGroups, true) && !in_array('grp_backups', $userGroups, true)) {
                        return ['success' => false, 'error' => 'En el rol Central de Backup los usuarios solo pueden acceder si pertenecen a grp_samba o grp_backups.'];
                    }
                }
            }
        }

        $target = trim($target);
        if ($kind === 'group') {
            $target = ltrim($target, '@');
            if (!preg_match('/^[a-z0-9_-]+$/i', $target)) {
                return ['success' => false, 'error' => 'Nombre de grupo inválido.'];
            }
            $token = '@' . $target;
        } elseif ($kind === 'user') {
            if (!preg_match('/^[a-z0-9_-]{2,32}$/i', $target)) {
                return ['success' => false, 'error' => 'Nombre de usuario inválido.'];
            }
            $token = $target;
        } else {
            return ['success' => false, 'error' => 'Tipo de destino inválido (group|user).'];
        }

        $props = $sections[$share];
        $valid = $this->splitTokens((string) ($props['valid users'] ?? ''));
        $write = $this->splitTokens((string) ($props['write list'] ?? ''));
        $valid = array_values(array_filter($valid, fn($t) => $t !== $token));
        $write = array_values(array_filter($write, fn($t) => $t !== $token));

        if ($level === 'read') {
            $valid[] = $token;
        } elseif ($level === 'write') {
            $valid[] = $token;
            $write[] = $token;
        }

        $props['valid users'] = $this->joinTokens($valid);
        $props['write list'] = $this->joinTokens($write);
        foreach (['valid users', 'write list'] as $k) {
            if ($props[$k] === '') {
                unset($props[$k]);
            }
        }

        $sections[$share] = $props;
        $res = $this->saveSectionsToConf($sections);
        if (!$res['success']) {
            return $res;
        }

        if ($level === 'none') {
            $path = trim((string) ($props['path'] ?? ''));
            if ($path !== '' && str_starts_with($path, '/srv/nas')) {
                $aclEntry = str_starts_with($token, '@') ? ('g:' . ltrim($token, '@')) : ('u:' . $token);
                SystemService::sudo(['setfacl', '-R', '-x', $aclEntry, $path]);
                SystemService::sudo(['setfacl', '-R', '-d', '-x', $aclEntry, $path]);
            }
        }

        SystemService::sudo(['systemctl', 'reload', 'smbd']);
        $this->syncShareAcls($share);

        return ['success' => true, 'message' => "Acceso de [$target] en [$share] fijado a '$level'."];
    }

    /**
     * Audita las ACL POSIX del directorio de un recurso comparándolas contra la configuración
     * de smb.conf para detectar discrepancias reales antes de ejecutar operaciones recursivas.
     *
     * @return array{success: bool, share: string, path: string, in_sync: bool, discrepancies: array<string>, expected_count?: int, error?: string}
     */
    public function auditShareAcls(string $name): array
    {
        $sections = $this->parseSmbConf();
        if (!isset($sections[$name])) {
            return [
                'success' => false,
                'share' => $name,
                'path' => '',
                'in_sync' => false,
                'discrepancies' => ["El recurso [$name] no existe en smb.conf."],
                'error' => "El recurso [$name] no existe en smb.conf.",
            ];
        }

        $props = $sections[$name];
        $path = trim((string) ($props['path'] ?? ''));
        if ($path === '' || !str_starts_with($path, '/srv/nas')) {
            return [
                'success' => false,
                'share' => $name,
                'path' => $path,
                'in_sync' => false,
                'discrepancies' => ['El recurso no apunta a una ruta administrada en /srv/nas.'],
                'error' => 'El recurso no apunta a una ruta administrada en /srv/nas.',
            ];
        }

        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                'success' => true,
                'share' => $name,
                'path' => $path,
                'in_sync' => true,
                'discrepancies' => [],
                'expected_count' => 0,
            ];
        }

        if (!is_dir($path)) {
            return [
                'success' => true,
                'share' => $name,
                'path' => $path,
                'in_sync' => false,
                'discrepancies' => ["El directorio $path no existe en el sistema de archivos."],
                'expected_count' => 0,
            ];
        }

        $guestOk = (strtolower((string) ($props['guest ok'] ?? 'no')) === 'yes');
        $discrepancies = [];

        // Obtener ACL actuales con getfacl
        $cmd = SystemService::runCommand(['getfacl', '-p', '-E', $path]);
        if ($cmd['code'] !== 0) {
            return [
                'success' => false,
                'share' => $name,
                'path' => $path,
                'in_sync' => false,
                'discrepancies' => ['Error al ejecutar getfacl en ' . $path],
                'error' => 'Error al leer ACL con getfacl',
            ];
        }

        $output = (string) $cmd['stdout'];
        $currentOwner = '';
        $currentGroup = '';
        $currentAcl = [];
        $currentDef = [];

        foreach (explode("\n", $output) as $line) {
            $line = trim($line);
            if (str_starts_with($line, '# owner:')) {
                $currentOwner = trim(substr($line, 8));
            } elseif (str_starts_with($line, '# group:')) {
                $currentGroup = trim(substr($line, 8));
            } elseif (str_starts_with($line, 'default:user:')) {
                $p = explode(':', $line);
                if (count($p) >= 4 && $p[2] !== '') {
                    $currentDef['u:' . $p[2]] = $p[3];
                }
            } elseif (str_starts_with($line, 'default:group:')) {
                $p = explode(':', $line);
                if (count($p) >= 4 && $p[2] !== '') {
                    $currentDef['g:' . $p[2]] = $p[3];
                }
            } elseif (str_starts_with($line, 'user:')) {
                $p = explode(':', $line);
                if (count($p) >= 3 && $p[1] !== '') {
                    $currentAcl['u:' . $p[1]] = $p[2];
                }
            } elseif (str_starts_with($line, 'group:')) {
                $p = explode(':', $line);
                if (count($p) >= 3 && $p[1] !== '') {
                    $currentAcl['g:' . $p[1]] = $p[2];
                }
            }
        }

        if ($guestOk) {
            foreach (['u:www-data' => 'rwx', 'u:nobody' => 'rwx'] as $k => $expectedPerm) {
                if (!isset($currentAcl[$k])) {
                    $discrepancies[] = "Falta entrada ACL de acceso para $k (esperado: $expectedPerm)";
                } elseif ($currentAcl[$k] !== $expectedPerm) {
                    $discrepancies[] = "Permisos desalineados en ACL de acceso para $k (actual: {$currentAcl[$k]}, esperado: $expectedPerm)";
                }
            }
        } else {
            if ($currentOwner !== 'root') {
                $discrepancies[] = "Propietario del directorio incorrecto (actual: $currentOwner, esperado: root)";
            }
            if ($currentGroup !== 'grp_samba') {
                $discrepancies[] = "Grupo del directorio incorrecto (actual: $currentGroup, esperado: grp_samba)";
            }

            $valid = $this->splitTokens((string) ($props['valid users'] ?? ''));
            $write = $this->splitTokens((string) ($props['write list'] ?? ''));

            $levels = [];
            foreach ($valid as $t) {
                $levels[$t] = 'read';
            }
            foreach ($write as $t) {
                $levels[$t] = 'write';
            }
            $levels['@grp_samba'] = 'write';
            $levels['u:www-data'] = 'write';

            $expectedKeys = [];
            foreach ($levels as $token => $level) {
                $perm = ($level === 'write') ? 'rwx' : 'r-x';
                $key = str_starts_with($token, '@') ? ('g:' . ltrim($token, '@')) : (str_starts_with($token, 'u:') ? $token : ('u:' . $token));
                $expectedKeys[$key] = $perm;

                if (!isset($currentAcl[$key])) {
                    $discrepancies[] = "Falta entrada ACL de acceso para $key (esperado: $perm)";
                } elseif ($currentAcl[$key] !== $perm) {
                    $discrepancies[] = "Permisos desalineados en ACL de acceso para $key (actual: {$currentAcl[$key]}, esperado: $perm)";
                }

                if (!isset($currentDef[$key])) {
                    $discrepancies[] = "Falta entrada ACL por defecto para $key (esperado: $perm)";
                } elseif ($currentDef[$key] !== $perm) {
                    $discrepancies[] = "Permisos desalineados en ACL por defecto para $key (actual: {$currentDef[$key]}, esperado: $perm)";
                }
            }

            foreach ($currentAcl as $k => $perm) {
                if (!isset($expectedKeys[$k])) {
                    $discrepancies[] = "Entrada residual en ACL de acceso: $k ($perm)";
                }
            }
        }

        return [
            'success' => true,
            'share' => $name,
            'path' => $path,
            'in_sync' => empty($discrepancies),
            'discrepancies' => $discrepancies,
            'expected_count' => count($guestOk ? ['u:www-data', 'u:nobody'] : ($expectedKeys ?? [])),
        ];
    }

    /**
     * Audita las ACL de uno o todos los recursos compartidos.
     *
     * @return array{shares: array<string, array>, total: int, out_of_sync_count: int, in_sync_count: int}
     */
    public function auditAcls(?string $shareName = null): array
    {
        $sharesToAudit = [];
        if (!empty($shareName)) {
            $sharesToAudit = [$shareName];
        } else {
            foreach ($this->listShares() as $s) {
                $sharesToAudit[] = $s['name'];
            }
        }

        $results = [];
        $outOfSync = 0;
        $inSync = 0;

        foreach ($sharesToAudit as $sn) {
            $res = $this->auditShareAcls($sn);
            $results[$sn] = $res;
            if (!empty($res['in_sync'])) {
                $inSync++;
            } else {
                $outOfSync++;
            }
        }

        return [
            'shares' => $results,
            'total' => count($results),
            'out_of_sync_count' => $outOfSync,
            'in_sync_count' => $inSync,
        ];
    }

    /**
     * Recalcula las ACL POSIX del directorio de un recurso para que coincidan con
     * 'valid users' / 'write list' de smb.conf. Corrige desajustes donde Samba
     * autoriza a un grupo pero el sistema de archivos lo deniega (y viceversa).
     *
     * Si $force es false, audita previamente las ACL y omite la reaplicación recursiva
     * (setfacl -R) si el directorio ya se encuentra sincronizado, previniendo I/O innecesario.
     */
    public function syncShareAcls(string $name, bool $force = false): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "ACL de [$name] sincronizadas (modo dev)."];
        }

        // Si no se fuerza la reaplicación, auditar primero para prevenir sobrecargas de I/O innecesarias
        if (!$force) {
            $audit = $this->auditShareAcls($name);
            if (!empty($audit['in_sync'])) {
                return [
                    'success' => true,
                    'message' => "ACL de [$name] ya están sincronizadas con smb.conf. Se omitió la reaplicación recursiva.",
                    'skipped' => true,
                    'path' => $audit['path'] ?? ('/srv/nas/' . $name),
                ];
            }
        }

        $sections = $this->parseSmbConf();
        if (!isset($sections[$name])) {
            return ['success' => false, 'error' => "El recurso [$name] no existe en smb.conf."];
        }

        $props = $sections[$name];
        $path = trim((string) ($props['path'] ?? ''));
        if ($path === '' || !str_starts_with($path, '/srv/nas')) {
            return ['success' => false, 'error' => 'El recurso no apunta a una ruta administrada en /srv/nas.'];
        }

        SystemService::sudo(['mkdir', '-p', $path]);

        $guestOk = (strtolower((string) ($props['guest ok'] ?? 'no')) === 'yes');
        if ($guestOk) {
            SystemService::sudo(['chmod', '2777', $path]);
            SystemService::sudo(['setfacl', '-m', 'u:www-data:rwx,u:nobody:rwx', $path]);
            SystemService::sudo(['setfacl', '-d', '-m', 'u:www-data:rwx,u:nobody:rwx,g::rwx,o::rwx', $path]);
            return ['success' => true, 'message' => "Recurso público [$name]: permisos abiertos aplicados."];
        }

        // Base: propietario root, grupo administrador con acceso total.
        SystemService::sudo(['chown', 'root:grp_samba', $path]);
        SystemService::sudo(['chmod', '2770', $path]);

        $valid = $this->splitTokens((string) ($props['valid users'] ?? ''));
        $write = $this->splitTokens((string) ($props['write list'] ?? ''));

        // Combinar niveles: la escritura prevalece sobre la lectura.
        $levels = [];
        foreach ($valid as $t) {
            $levels[$t] = 'read';
        }
        foreach ($write as $t) {
            $levels[$t] = 'write';
        }
        // Los administradores (grp_samba) siempre conservan acceso total.
        $levels['@grp_samba'] = 'write';
        // El panel web accede por Samba; se concede a www-data para tareas internas.
        $levels['u:www-data'] = 'write';

        $aclEntries = [];
        foreach ($levels as $token => $level) {
            $perm = ($level === 'write') ? 'rwx' : 'r-x';
            if (str_starts_with($token, '@')) {
                $aclEntries[] = 'g:' . ltrim($token, '@') . ':' . $perm;
            } elseif (str_starts_with($token, 'u:')) {
                $aclEntries[] = $token . ':' . $perm;
            } else {
                $aclEntries[] = 'u:' . $token . ':' . $perm;
            }
        }

        $applied = count($aclEntries);
        if ($applied > 0) {
            $aclSpec = implode(',', $aclEntries);
            SystemService::sudo(['setfacl', '-R', '-m', $aclSpec, $path]);
            SystemService::sudo(['setfacl', '-R', '-d', '-m', $aclSpec, $path]);
        }

        // Permitir la travesía de /srv/nas a cualquier cuenta autenticada.
        SystemService::sudo(['chmod', 'o+x', '/srv/nas']);

        return [
            'success' => true,
            'message' => "ACL de [$name] sincronizadas ($applied entradas) en $path.",
        ];
    }

    /**
     * Devuelve el mapa de acceso por recurso: grupos/usuarios de lectura y escritura.
     *
     * @return array<string,array{read_groups:array,write_groups:array,read_users:array,write_users:array}>
     */
    public function getAccessMap(): array
    {
        if (DIRECTORY_SEPARATOR === '\\' || !file_exists($this->confPath)) {
            $mockShares = $this->listShares();
            $map = [];
            foreach ($mockShares as $s) {
                $writeGroups = array_values(array_map(fn($t) => ltrim($t, '@'), array_filter($s['write_list'] ?? [], fn($t) => str_starts_with($t, '@'))));
                $writeUsers = array_values(array_filter($s['write_list'] ?? [], fn($t) => !str_starts_with($t, '@')));
                $readGroups = array_values(array_map(fn($t) => ltrim($t, '@'), array_filter($s['valid_users'] ?? [], fn($t) => str_starts_with($t, '@') && !in_array($t, $s['write_list'] ?? [], true))));
                $readUsers = array_values(array_filter($s['valid_users'] ?? [], fn($t) => !str_starts_with($t, '@') && !in_array($t, $s['write_list'] ?? [], true)));
                $map[$s['name']] = [
                    'read_groups' => $readGroups,
                    'write_groups' => $writeGroups,
                    'read_users' => $readUsers,
                    'write_users' => $writeUsers,
                    'guest_ok' => !empty($s['guest_ok']),
                ];
            }
            return $map;
        }

        $sections = $this->parseSmbConf();
        $map = [];
        foreach ($sections as $name => $props) {
            if (in_array(strtolower($name), ['global', 'printers', 'print$'], true)) {
                continue;
            }
            $valid = $this->splitTokens((string) ($props['valid users'] ?? ''));
            $write = $this->splitTokens((string) ($props['write list'] ?? ''));

            $writeGroups = array_values(array_map(fn($t) => ltrim($t, '@'), array_filter($write, fn($t) => str_starts_with($t, '@'))));
            $writeUsers = array_values(array_filter($write, fn($t) => !str_starts_with($t, '@')));
            $readGroups = array_values(array_map(fn($t) => ltrim($t, '@'), array_filter($valid, fn($t) => str_starts_with($t, '@') && !in_array($t, $write, true))));
            $readUsers = array_values(array_filter($valid, fn($t) => !str_starts_with($t, '@') && !in_array($t, $write, true)));

            $map[$name] = [
                'read_groups' => $readGroups,
                'write_groups' => $writeGroups,
                'read_users' => $readUsers,
                'write_users' => $writeUsers,
                'guest_ok' => (strtolower((string) ($props['guest ok'] ?? 'no')) === 'yes'),
            ];
        }
        return $map;
    }

    private function splitTokens(string $value): array
    {
        $value = trim($value);
        return $value === '' ? [] : preg_split('/\s+/', $value);
    }

    private function joinTokens(array $tokens): string
    {
        return implode(' ', array_values(array_unique(array_filter($tokens, fn($t) => $t !== ''))));
    }

    private function cleanGroupTokens(array $groups): array
    {
        $out = [];
        foreach ($groups as $g) {
            $g = ltrim(trim((string) $g), '@');
            if (preg_match('/^[a-z0-9_-]+$/i', $g)) {
                $out[] = '@' . $g;
            }
        }
        return array_values(array_unique($out));
    }

    /**
     * Sanitiza el comentario de un recurso para evitar inyección en archivos INI (smb.conf).
     * Neutraliza caracteres de control, delimitadores de comentario (; y #), inicio de sección ([)
     * y barras invertidas finales que actúan como continuación de línea en Samba.
     */
    private function sanitizeComment(string $comment): string
    {
        $comment = preg_replace('/[\x00-\x1F\x7F;#[\]]+/', ' ', $comment) ?? '';
        $comment = rtrim(trim($comment), '\\');
        return trim($comment);
    }

    /**
     * Reemplaza de forma segura un bloque de sección en el contenido de smb.conf,
     * utilizando preg_replace_callback para preservar intactos los caracteres '$' y '\'
     * en recursos compartidos ocultos (terminados en '$').
     */
    public function replaceSectionInContent(string $content, string $name, string $replacement): string
    {
        $escapedName = preg_quote($name, '#');
        $pattern = '#(\[' . $escapedName . '\](?:(?!\[).)*)#s';
        if (preg_match($pattern, $content)) {
            return (string) preg_replace_callback($pattern, fn() => $replacement, $content);
        }
        return rtrim($content) . "\n\n" . $replacement . "\n";
    }

    /**
     * Escapa caracteres especiales de reemplazo regex ('$' y '\') para bloques de recursos Samba.
     */
    public static function escapeReplacementBlock(string $block): string
    {
        return addcslashes($block, '\\$');
    }
}
