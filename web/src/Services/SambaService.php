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
                    'valid_users' => ['@grp_sistemas'],
                    'write_list' => ['@grp_sistemas'],
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
                    'valid_users' => ['@grp_sistemas'],
                    'write_list' => ['@grp_sistemas'],
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
            } elseif ($readOnly) {
                $scheme = 3;
                $schemeName = 'Solo Lectura Estricta';
            } elseif (!empty($writeList) && count($writeList) < count($validUsers)) {
                $scheme = 2;
                $schemeName = 'Solo Lectura General + Escritura Exclusiva';
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
        $name = trim($data['name'] ?? '');
        $comment = trim($data['comment'] ?? '');
        $scheme = (int) ($data['scheme'] ?? 1);
        $groups = $data['groups'] ?? [];
        $writeGroup = trim($data['write_group'] ?? '');
        $isGuest = ($scheme === 4) || !empty($data['guest_ok']);
        $isHidden = !empty($data['hidden']);

        // Nombre de recurso válido
        if (!preg_match('/^[A-Za-z0-9_-]{1,60}$/', $name)) {
            return ['success' => false, 'error' => 'El nombre del recurso debe tener entre 1 y 60 caracteres alfanuméricos.'];
        }

        if ($isHidden && !str_ends_with($name, '$')) {
            $name .= '$';
        }

        // Ruta estándar dentro del almacenamiento administrado /srv/nas
        $subfolder = rtrim($name, '$');
        $path = '/srv/nas/' . $subfolder;

        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Recurso [$name] creado correctamente (modo dev)."];
        }

        // 1. Preparar directorio en disco
        SystemService::sudo(['mkdir', '-p', $path]);
        SystemService::sudo(['chown', 'root:grp_sistemas', $path]);
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
                $shareProps['read only'] = 'no';
                $shareProps['guest ok'] = 'no';
                $wgClean = ltrim($writeGroup, '@');
                $wEntry = '@' . $wgClean;
                $allUsers = array_unique(array_merge($cleanGroups, [$wEntry]));
                $shareProps['valid users'] = implode(' ', $allUsers);
                $shareProps['write list'] = $wEntry;

                // Configurar ACLs POSIX para que los nuevos archivos hereden lectura al resto
                if (!empty($wgClean)) {
                    SystemService::sudo(['setfacl', '-R', '-m', "g:$wgClean:rwx", $path]);
                    SystemService::sudo(['setfacl', '-R', '-d', '-m', "g:$wgClean:rwx", $path]);
                }
                foreach ($cleanGroups as $rg) {
                    $rgClean = ltrim($rg, '@');
                    if ($rgClean !== $wgClean) {
                        SystemService::sudo(['setfacl', '-R', '-m', "g:$rgClean:r-x", $path]);
                        SystemService::sudo(['setfacl', '-R', '-d', '-m', "g:$rgClean:r-x", $path]);
                    }
                }
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

        // 3. Recargar servicio Samba
        SystemService::sudo(['systemctl', 'reload', 'smbd']);

        return ['success' => true, 'message' => "Recurso compartido [$name] creado y activo en Samba."];
    }

    /**
     * Elimina una sección de recurso de smb.conf y recarga Samba.
     */
    public function deleteShare(string $name, bool $deleteFiles = false): array
    {
        $lower = strtolower($name);
        if (in_array($lower, ['global', 'printers', 'print$'], true)) {
            return ['success' => false, 'error' => "La sección [$name] está protegida por el sistema."];
        }

        if (DIRECTORY_SEPARATOR === '\\') {
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
            $buffer .= "[$sectionName]\n";
            foreach ($props as $key => $val) {
                $buffer .= "   $key = $val\n";
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
            $tmpFile = tempnam(sys_get_temp_dir(), 'smbconf_');
            if ($tmpFile === false) {
                return ['success' => false, 'error' => 'No se pudo crear archivo temporal para smb.conf.'];
            }

            file_put_contents($tmpFile, $buffer);

            // Validar con testparm -s
            $tpRes = SystemService::runCommand(['testparm', '-s', $tmpFile]);
            if ($tpRes['code'] !== 0) {
                @unlink($tmpFile);
                return [
                    'success' => false,
                    'error' => 'La configuración generada tiene errores de sintaxis en testparm: ' . ($tpRes['stderr'] ?: $tpRes['stdout']),
                ];
            }

            // Copiar a /etc/samba/smb.conf con sudo
            $cpRes = SystemService::sudo(['cp', $tmpFile, $this->confPath]);
            @unlink($tmpFile);

            if ($cpRes['code'] !== 0) {
                return ['success' => false, 'error' => 'Error al escribir en ' . $this->confPath . ': ' . $cpRes['stderr']];
            }

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
        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Recurso [$name] actualizado (modo dev)."];
        }

        $sections = $this->parseSmbConf();
        if (!isset($sections[$name])) {
            return ['success' => false, 'error' => "El recurso [$name] no existe en smb.conf."];
        }

        $props = $sections[$name];
        $scheme = (int) ($data['scheme'] ?? 0);
        $comment = trim((string) ($data['comment'] ?? ''));
        $groups = $this->cleanGroupTokens((array) ($data['groups'] ?? []));
        $writeGroup = ltrim(trim((string) ($data['write_group'] ?? '')), '@');
        $writeEntry = $writeGroup !== '' ? '@' . $writeGroup : '';
        $isHidden = !empty($data['hidden']);

        // Separar tokens de grupo (@grupo) de los usuarios explícitos (suelto)
        $valid = $this->splitTokens((string) ($props['valid users'] ?? ''));
        $write = $this->splitTokens((string) ($props['write list'] ?? ''));
        $validUsers = array_values(array_filter($valid, fn($t) => !str_starts_with($t, '@')));
        $writeUsers = array_values(array_filter($write, fn($t) => !str_starts_with($t, '@')));

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
            $props['read only'] = 'no';
            $props['guest ok'] = 'no';
            unset($props['public'], $props['guest only']);
            $groupsWithWriter = $groups;
            if ($writeEntry !== '') {
                $groupsWithWriter[] = $writeEntry;
            }
            $props['valid users'] = $this->joinTokens(array_merge($groupsWithWriter, $validUsers));
            $props['write list'] = $this->joinTokens(array_merge($writeEntry !== '' ? [$writeEntry] : [], $writeUsers));
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
        if (DIRECTORY_SEPARATOR === '\\') {
            return ['success' => true, 'message' => "Acceso actualizado en [$share] (modo dev)."];
        }
        if (!in_array($level, ['none', 'read', 'write'], true)) {
            return ['success' => false, 'error' => 'Nivel de acceso inválido.'];
        }

        $sections = $this->parseSmbConf();
        if (!isset($sections[$share])) {
            return ['success' => false, 'error' => "El recurso [$share] no existe en smb.conf."];
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
        SystemService::sudo(['systemctl', 'reload', 'smbd']);

        return ['success' => true, 'message' => "Acceso de [$target] en [$share] fijado a '$level'."];
    }

    /**
     * Devuelve el mapa de acceso por recurso: grupos/usuarios de lectura y escritura.
     *
     * @return array<string,array{read_groups:array,write_groups:array,read_users:array,write_users:array}>
     */
    public function getAccessMap(): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return [];
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
}
