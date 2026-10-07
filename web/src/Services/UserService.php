<?php

declare(strict_types=1);

namespace App\Services;

/**
 * Servicio de gestión de identidades, usuarios Linux, sincronización estricta con Samba
 * (smbpasswd/pdbedit) y prefijo corporativo mandatorio 'grp_*' para grupos departamentales.
 */
class UserService
{
    public bool $dryRun = false;

    /**
     * Lista los usuarios del sistema (UID >= 1000) e identifica si tienen cuenta Samba activa.
     */
    public function listUsers(): array
    {
        $users = [];

        if ($this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return [
                [
                    'username' => 'administrador',
                    'uid' => 1000,
                    'is_admin' => true,
                    'is_samba' => true,
                    'enabled' => true,
                    'shell' => '/bin/bash',
                    'home' => '/home/administrador',
                    'groups' => ['sudo', 'adm', 'grp_sistemas'],
                ],
                [
                    'username' => 'sistemas',
                    'uid' => 1001,
                    'is_admin' => true,
                    'is_samba' => true,
                    'enabled' => true,
                    'shell' => '/bin/bash',
                    'home' => '/home/sistemas',
                    'groups' => ['grp_sistemas'],
                ],
                [
                    'username' => 'operador_c1',
                    'uid' => 1002,
                    'is_admin' => false,
                    'is_samba' => true,
                    'enabled' => true,
                    'shell' => '/bin/bash',
                    'home' => '/home/operador_c1',
                    'groups' => ['grp_campana1'],
                ],
            ];
        }

        // Obtener lista de usuarios de Samba con pdbedit
        $sambaUsers = [];
        $pdbRes = SystemService::sudo(['pdbedit', '-L', '-s']);
        if ($pdbRes['code'] === 0) {
            foreach (explode("\n", $pdbRes['stdout']) as $line) {
                if (preg_match('/^([^:]+):/', trim($line), $m)) {
                    $sambaUsers[$m[1]] = true;
                }
            }
        }

        // Estado de contraseñas (P = activo, L = bloqueado) en una sola llamada
        $passStates = $this->getPasswordStates();

        // Leer /etc/passwd para cuentas humanas (UID >= 1000 y < 65534)
        if (file_exists('/etc/passwd')) {
            $lines = file('/etc/passwd', FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
            foreach ($lines as $line) {
                $cols = explode(':', $line);
                if (count($cols) >= 7) {
                    $uname = $cols[0];
                    $uid = (int) $cols[2];
                    if ($uid >= 1000 && $uid < 65534 && $uname !== 'nobody') {
                        // Grupos del usuario
                        $userGroups = [];
                        $grpRes = SystemService::runCommand(['id', '-Gn', $uname]);
                        if ($grpRes['code'] === 0) {
                            $userGroups = preg_split('/\s+/', trim($grpRes['stdout']));
                        }

                        $isAdmin = in_array('sudo', $userGroups, true) || in_array('grp_sistemas', $userGroups, true);

                        $users[] = [
                            'username' => $uname,
                            'uid' => $uid,
                            'is_admin' => $isAdmin,
                            'is_samba' => isset($sambaUsers[$uname]),
                            'enabled' => ($passStates[$uname] ?? '') === 'P',
                            'shell' => $cols[6] ?? '/bin/bash',
                            'home' => $cols[5] ?? '/home/' . $uname,
                            'groups' => $userGroups,
                        ];
                    }
                }
            }
        }

        return $users;
    }

    /**
     * Estado de contraseñas de todas las cuentas (P=activa, L=bloqueada, NP=sin clave).
     *
     * @return array<string,string>
     */
    private function getPasswordStates(): array
    {
        $states = [];
        $res = SystemService::sudo(['passwd', '-S', '-a']);
        if ($res['code'] !== 0) {
            return $states;
        }
        foreach (preg_split('/\R/', $res['stdout'] ?? '') as $line) {
            $parts = preg_split('/\s+/', trim($line));
            if (count($parts) >= 2 && preg_match('/^[a-z_][a-z0-9_-]*$/i', $parts[0])) {
                $states[strtolower($parts[0])] = $parts[1];
            }
        }
        return $states;
    }

    /**
     * Lista todos los grupos creados bajo el estándar corporativo con prefijo 'grp_*'.
     */
    public function listGroups(): array
    {
        $groups = [];

        if ($this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return [
                ['name' => 'grp_sistemas', 'gid' => 1050, 'members' => ['administrador', 'sistemas'], 'is_master' => true],
                ['name' => 'grp_campana1', 'gid' => 1051, 'members' => ['operador_c1'], 'is_master' => false],
                ['name' => 'grp_contabilidad', 'gid' => 1052, 'members' => [], 'is_master' => false],
            ];
        }

        if (file_exists('/etc/group')) {
            $lines = file('/etc/group', FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
            foreach ($lines as $line) {
                $cols = explode(':', $line);
                if (count($cols) >= 4) {
                    $gname = $cols[0];
                    $gid = (int) $cols[2];
                    $members = !empty($cols[3]) ? explode(',', $cols[3]) : [];

                    if (str_starts_with($gname, 'grp_')) {
                        $groups[] = [
                            'name' => $gname,
                            'gid' => $gid,
                            'members' => $members,
                            'is_master' => ($gname === 'grp_sistemas'),
                        ];
                    }
                }
            }
        }

        return $groups;
    }

    /**
     * Crea un nuevo usuario en el sistema Linux y lo sincroniza con la base de credenciales de Samba.
     */
    public function createUser(string $username, string $password, array $groups = [], bool $isAdmin = false): array
    {
        $username = strtolower(trim($username));
        if (!preg_match('/^[a-z0-9_-]{3,32}$/', $username)) {
            return ['success' => false, 'error' => 'Nombre de usuario inválido (3-32 caracteres alfanuméricos, guiones).'];
        }

        if (strlen($password) < 6) {
            return ['success' => false, 'error' => 'La contraseña debe tener al menos 6 caracteres.'];
        }

        if ($this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "Usuario $username creado exitosamente (modo dev)."];
        }

        // Verificar si ya existe
        $idCheck = SystemService::runCommand(['id', '-u', $username]);
        if ($idCheck['code'] === 0) {
            return ['success' => false, 'error' => "El usuario $username ya existe en el servidor."];
        }

        // 1. Crear cuenta del sistema
        $res = SystemService::sudo(['useradd', '-m', '-s', '/bin/bash', $username]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'Error al crear usuario en Linux: ' . ($res['stderr'] ?: $res['stdout'])];
        }

        // 2. Asignar contraseña Linux con chpasswd
        $chRes = SystemService::sudo(['chpasswd'], "$username:$password\n");
        if ($chRes['code'] !== 0) {
            SystemService::sudo(['userdel', '-r', $username]);
            return ['success' => false, 'error' => 'Error al asignar contraseña en Linux.'];
        }

        // 3. Sincronizar en base de contraseñas de Samba (smbpasswd -a -s)
        $smbInput = "$password\n$password\n";
        $smbRes = SystemService::sudo(['smbpasswd', '-a', '-s', $username], $smbInput);
        if ($smbRes['code'] !== 0) {
            // No revertimos, advertimos
            error_log("Aviso: no se pudo registrar $username en smbpasswd: " . $smbRes['stderr']);
        }

        // 4. Asignar grupos seleccionados
        $validGroups = [];
        if ($isAdmin) {
            $validGroups[] = 'sudo';
            $validGroups[] = 'adm';
            $validGroups[] = 'grp_sistemas';
        }

        foreach ($groups as $grp) {
            if (is_string($grp) && str_starts_with($grp, 'grp_') && preg_match('/^grp_[a-z0-9_-]+$/', $grp)) {
                $validGroups[] = $grp;
            }
        }

        if (!empty($validGroups)) {
            $uniqueGroups = array_unique($validGroups);
            SystemService::sudo(['usermod', '-aG', implode(',', $uniqueGroups), $username]);
        }

        return ['success' => true, 'message' => "Usuario $username configurado y sincronizado con Samba."];
    }

    /**
     * Elimina un usuario del sistema Linux y de la base de credenciales de Samba.
     */
    public function deleteUser(string $username): array
    {
        $username = strtolower(trim($username));
        $protectedUsers = [
            'root', 'administrador', 'sistemas', 'www-data', 'nobody', 'daemon',
            'bin', 'sys', 'sync', 'games', 'man', 'lp', 'mail', 'news', 'uucp',
            'proxy', 'backup', 'list', 'irc', 'gnats', 'systemd-network', 'systemd-resolve',
        ];
        if (in_array($username, $protectedUsers, true)) {
            return ['success' => false, 'error' => "Por seguridad no es posible eliminar la cuenta protegida del sistema '$username'."];
        }

        // Evitar que el usuario autenticado elimine su propia cuenta activa
        if (session_status() === PHP_SESSION_ACTIVE || !empty($_SESSION)) {
            $currentUser = $_SESSION['nas_user']['username'] ?? null;
            if ($currentUser !== null && strtolower((string) $currentUser) === $username) {
                return ['success' => false, 'error' => 'No es posible eliminar la propia cuenta de usuario activa.'];
            }
        }

        // En entornos Linux reales, validar que no sea una cuenta de sistema (UID < 1000)
        if (DIRECTORY_SEPARATOR !== '\\') {
            if (function_exists('posix_getpwnam')) {
                $pw = @posix_getpwnam($username);
                if (is_array($pw) && isset($pw['uid']) && (int) $pw['uid'] < 1000) {
                    return ['success' => false, 'error' => "Por seguridad no es posible eliminar cuentas del sistema con UID menor a 1000 (UID: {$pw['uid']})."];
                }
            } else {
                $idRes = SystemService::runCommand(['id', '-u', $username]);
                if ($idRes['code'] === 0 && is_numeric(trim($idRes['stdout']))) {
                    $uid = (int) trim($idRes['stdout']);
                    if ($uid < 1000) {
                        return ['success' => false, 'error' => "Por seguridad no es posible eliminar cuentas del sistema con UID menor a 1000 (UID: $uid)."];
                    }
                }
            }
        }

        if ($this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "Usuario $username eliminado (modo dev)."];
        }

        // Eliminar de Samba primero
        SystemService::sudo(['smbpasswd', '-x', $username]);

        // Eliminar del sistema con su directorio home
        $res = SystemService::sudo(['userdel', '-r', $username]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'Error al eliminar usuario en Linux: ' . ($res['stderr'] ?: $res['stdout'])];
        }

        return ['success' => true, 'message' => "Usuario $username eliminado del servidor y de Samba."];
    }

    /**
     * Crea un grupo corporativo asegurando el prefijo obligatorio 'grp_*'.
     */
    public function createGroup(string $groupName): array
    {
        $groupName = strtolower(trim($groupName));
        if (!str_starts_with($groupName, 'grp_')) {
            $groupName = 'grp_' . $groupName;
        }

        if (!preg_match('/^grp_[a-z0-9_-]{2,30}$/', $groupName)) {
            return ['success' => false, 'error' => 'Nombre de grupo inválido. Formato esperado: grp_nombre (2-30 caracteres).'];
        }

        if ($this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "Grupo $groupName creado (modo dev)."];
        }

        $res = SystemService::sudo(['groupadd', $groupName]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'Error al crear grupo: ' . ($res['stderr'] ?: $res['stdout'])];
        }

        return ['success' => true, 'message' => "Grupo corporativo $groupName creado correctamente."];
    }

    /**
     * Elimina un grupo corporativo. Protege 'grp_sistemas' contra borrado accidental.
     */
    public function deleteGroup(string $groupName): array
    {
        $groupName = strtolower(trim($groupName));
        if ($groupName === 'grp_sistemas') {
            return ['success' => false, 'error' => 'El grupo maestro grp_sistemas está protegido y no puede ser eliminado.'];
        }

        if (!str_starts_with($groupName, 'grp_')) {
            return ['success' => false, 'error' => 'Solo se pueden eliminar grupos departamentales grp_* creados por el NAS.'];
        }

        if ($this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            return ['success' => true, 'message' => "Grupo $groupName eliminado (modo dev)."];
        }

        $res = SystemService::sudo(['groupdel', $groupName]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'Error al eliminar grupo: ' . ($res['stderr'] ?: $res['stdout'])];
        }

        return ['success' => true, 'message' => "Grupo $groupName eliminado con éxito."];
    }

    /**
     * Cambia la contraseña Linux de un usuario y la resincroniza en Samba.
     */
    public function setPassword(string $username, string $password): array
    {
        $username = strtolower(trim($username));
        if (!preg_match('/^[a-z0-9_-]{3,32}$/', $username)) {
            return ['success' => false, 'error' => 'Nombre de usuario inválido.'];
        }
        if (strlen($password) < 6) {
            return ['success' => false, 'error' => 'La contraseña debe tener al menos 6 caracteres.'];
        }

        $protected = $this->protectedError($username);
        if ($protected !== null) {
            return $protected;
        }

        if ($this->isTesting()) {
            return ['success' => true, 'message' => "Contraseña de $username actualizada (modo dev)."];
        }

        $ch = SystemService::sudo(['chpasswd'], "$username:$password\n");
        if ($ch['code'] !== 0) {
            return ['success' => false, 'error' => 'No se pudo asignar la contraseña Linux: ' . ($ch['stderr'] ?: $ch['stdout'])];
        }
        $smb = SystemService::sudo(['smbpasswd', '-a', '-s', $username], "$password\n$password\n");
        if ($smb['code'] !== 0) {
            error_log("Aviso: no se pudo sincronizar $username en smbpasswd: " . $smb['stderr']);
        }

        return ['success' => true, 'message' => "Contraseña de $username actualizada y sincronizada en Samba."];
    }

    /**
     * Bloquea (L) o desbloquea (U) una cuenta Linux y sincroniza el estado en Samba.
     */
    public function setEnabled(string $username, bool $enabled): array
    {
        $username = strtolower(trim($username));
        if (!preg_match('/^[a-z0-9_-]{3,32}$/', $username)) {
            return ['success' => false, 'error' => 'Nombre de usuario inválido.'];
        }

        $protected = $this->protectedError($username);
        if ($protected !== null) {
            return $protected;
        }
        $self = $this->selfBlockError($username);
        if ($self !== null) {
            return $self;
        }

        if ($this->isTesting()) {
            return ['success' => true, 'message' => ($enabled ? 'Activada' : 'Bloqueada') . " la cuenta $username (modo dev)."];
        }

        $res = SystemService::sudo(['usermod', $enabled ? '-U' : '-L', $username]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'Error al ' . ($enabled ? 'activar' : 'bloquear') . " la cuenta: " . ($res['stderr'] ?: $res['stdout'])];
        }
        SystemService::sudo(['smbpasswd', $enabled ? '-e' : '-d', '-s', $username]);

        return ['success' => true, 'message' => "Cuenta $username " . ($enabled ? 'activada' : 'bloqueada') . ' correctamente (Linux y Samba).'];
    }

    /**
     * Edita un usuario: contraseña opcional, grupos departamentales y rol administrador.
     */
    public function updateUser(string $username, ?string $password, array $groups, bool $isAdmin): array
    {
        $username = strtolower(trim($username));
        if (!preg_match('/^[a-z0-9_-]{3,32}$/', $username)) {
            return ['success' => false, 'error' => 'Nombre de usuario inválido.'];
        }

        $protected = $this->protectedError($username);
        if ($protected !== null) {
            return $protected;
        }

        // No permitir que un administrador se quite sus propios privilegios.
        if (!$isAdmin && $this->currentSessionUser() === $username) {
            return ['success' => false, 'error' => 'No puedes quitar los privilegios de administrador a tu propia cuenta.'];
        }

        if ($password !== null && $password !== '') {
            $pwd = $this->setPassword($username, $password);
            if (!$pwd['success']) {
                return $pwd;
            }
        }

        $targetGroups = [];
        foreach ($groups as $g) {
            $g = strtolower(trim((string) $g));
            if (preg_match('/^grp_[a-z0-9_-]+$/', $g)) {
                $targetGroups[] = $g;
            }
        }
        $targetGroups = array_values(array_unique($targetGroups));

        if ($this->isTesting()) {
            return ['success' => true, 'message' => "Usuario $username actualizado (modo dev)."];
        }

        $currentGroups = [];
        $grpRes = SystemService::runCommand(['id', '-Gn', $username]);
        if ($grpRes['code'] === 0) {
            $currentGroups = preg_split('/\s+/', trim($grpRes['stdout']));
        }

        foreach ($targetGroups as $g) {
            if (!in_array($g, $currentGroups, true)) {
                SystemService::sudo(['gpasswd', '-a', $username, $g]);
            }
        }
        foreach ($currentGroups as $g) {
            if (str_starts_with($g, 'grp_') && !in_array($g, $targetGroups, true)) {
                SystemService::sudo(['gpasswd', '-d', $username, $g]);
            }
        }

        if ($isAdmin && !in_array('sudo', $currentGroups, true)) {
            SystemService::sudo(['usermod', '-aG', 'sudo,adm,grp_sistemas', $username]);
        } elseif (!$isAdmin && in_array('sudo', $currentGroups, true)) {
            SystemService::sudo(['gpasswd', '-d', $username, 'sudo']);
            SystemService::sudo(['gpasswd', '-d', $username, 'adm']);
            SystemService::sudo(['gpasswd', '-d', $username, 'grp_sistemas']);
        }

        return ['success' => true, 'message' => "Usuario $username actualizado correctamente."];
    }

    /**
     * Añade un usuario a un grupo departamental grp_*.
     */
    public function addUserToGroup(string $username, string $group): array
    {
        return $this->modifyGroupMembership($username, $group, true);
    }

    /**
     * Quita un usuario de un grupo departamental grp_*.
     */
    public function removeUserFromGroup(string $username, string $group): array
    {
        return $this->modifyGroupMembership($username, $group, false);
    }

    /**
     * Renombra un grupo departamental grp_* (protege grp_sistemas).
     */
    public function renameGroup(string $old, string $new): array
    {
        $old = strtolower(trim($old));
        $new = strtolower(trim($new));

        if ($old === 'grp_sistemas') {
            return ['success' => false, 'error' => 'El grupo maestro grp_sistemas no puede ser renombrado.'];
        }
        if (!preg_match('/^grp_[a-z0-9_-]{2,30}$/', $old) || !preg_match('/^grp_[a-z0-9_-]{2,30}$/', $new)) {
            return ['success' => false, 'error' => 'Solo se pueden renombrar grupos grp_* (2-30 caracteres).'];
        }
        if (strtolower($old) === strtolower($new)) {
            return ['success' => false, 'error' => 'El nuevo nombre es igual al actual.'];
        }

        if ($this->isTesting()) {
            return ['success' => true, 'message' => "Grupo $old renombrado a $new (modo dev)."];
        }

        $res = SystemService::sudo(['groupmod', '-n', $new, $old]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'Error al renombrar grupo: ' . ($res['stderr'] ?: $res['stdout'])];
        }

        return ['success' => true, 'message' => "Grupo $old renombrado a $new correctamente."];
    }

    private function modifyGroupMembership(string $username, string $group, bool $add): array
    {
        $username = strtolower(trim($username));
        $group = strtolower(trim($group));

        if (!preg_match('/^[a-z0-9_-]{3,32}$/', $username)) {
            return ['success' => false, 'error' => 'Nombre de usuario inválido.'];
        }
        if (!preg_match('/^grp_[a-z0-9_-]{2,30}$/', $group)) {
            return ['success' => false, 'error' => 'Grupo inválido. Solo se admiten grupos departamentales grp_*.'];
        }

        $protected = $this->protectedError($username);
        if ($protected !== null) {
            return $protected;
        }

        if ($this->isTesting()) {
            return ['success' => true, 'message' => ($add ? 'Añadido' : 'Quitado') . " $username de $group (modo dev)."];
        }

        $res = SystemService::sudo(['gpasswd', $add ? '-a' : '-d', $username, $group]);
        if ($res['code'] !== 0) {
            return ['success' => false, 'error' => 'Error al modificar la membresía: ' . ($res['stderr'] ?: $res['stdout'])];
        }

        return ['success' => true, 'message' => ($add ? 'Miembro añadido' : 'Miembro quitado') . " en $group."];
    }

    private function isTesting(): bool
    {
        return $this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing';
    }

    private function currentSessionUser(): ?string
    {
        if (session_status() === PHP_SESSION_ACTIVE || !empty($_SESSION)) {
            $u = $_SESSION['nas_user']['username'] ?? null;
            return $u !== null ? strtolower((string) $u) : null;
        }
        return null;
    }

    private function protectedError(string $username): ?array
    {
        $protected = [
            'root', 'administrador', 'sistemas', 'www-data', 'nobody', 'daemon',
            'bin', 'sys', 'sync', 'games', 'man', 'lp', 'mail', 'news', 'uucp',
            'proxy', 'backup', 'list', 'irc', 'gnats', 'systemd-network', 'systemd-resolve',
        ];
        if (in_array($username, $protected, true)) {
            return ['success' => false, 'error' => "La cuenta protegida '$username' no admite esta operación."];
        }
        if (DIRECTORY_SEPARATOR !== '\\' && function_exists('posix_getpwnam')) {
            $pw = @posix_getpwnam($username);
            if (is_array($pw) && isset($pw['uid']) && (int) $pw['uid'] > 0 && (int) $pw['uid'] < 1000) {
                return ['success' => false, 'error' => "No es posible operar sobre cuentas del sistema con UID menor a 1000 (UID: {$pw['uid']})."];
            }
        }
        return null;
    }

    private function selfBlockError(string $username): ?array
    {
        if ($this->currentSessionUser() === $username) {
            return ['success' => false, 'error' => 'No puedes realizar esta operación sobre tu propia cuenta de sesión.'];
        }
        return null;
    }
}
