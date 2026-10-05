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
    }
}
