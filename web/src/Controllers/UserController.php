<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\AuditService;
use App\Services\UserService;

/**
 * Controlador API para gestión de usuarios Linux/Samba y grupos corporativos 'grp_*'.
 */
class UserController
{
    private UserService $user;

    public function __construct()
    {
        $this->user = new UserService();
    }

    public function users(Request $request): void
    {
        $users = $this->user->listUsers();
        Response::success($users);
    }

    public function groups(Request $request): void
    {
        $groups = $this->user->listGroups();
        Response::success($groups);
    }

    public function createUser(Request $request): void
    {
        $data = $request->getBody();
        $username = trim($data['username'] ?? '');
        $password = $data['password'] ?? '';
        $fullName = trim((string) ($data['full_name'] ?? ''));
        $groups = $data['groups'] ?? [];
        $isAdmin = (bool) ($data['is_admin'] ?? false);
        $canWeb = (bool) ($data['can_web'] ?? false);
        $sambaEnabled = (bool) ($data['samba_enabled'] ?? true);
        $isSuperadmin = (bool) ($data['is_superadmin'] ?? false);

        if (empty($username) || empty($password)) {
            Response::error('El usuario y la contraseña son obligatorios.');
            return;
        }

        // Un operador web no puede crear administradores ni superadministradores.
        if (!$this->isAdminCaller()) {
            $isAdmin = false;
            $isSuperadmin = false;
        }
        if ($isSuperadmin && !$this->isSuperCaller()) {
            Response::error('Solo un superadministrador puede crear otro superadministrador.', 403);
            return;
        }

        $res = $this->user->createUser($username, $password, $fullName, (array) $groups, $isAdmin, $canWeb, $sambaEnabled, $isSuperadmin);
        if (!$res['success']) {
            AuditService::log('user_create', $username, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al crear usuario.');
            return;
        }

        AuditService::log('user_create', $username, 'SUCCESS', ['groups' => (array) $groups, 'is_admin' => $isAdmin, 'can_web' => $canWeb, 'is_superadmin' => $isSuperadmin]);
        Response::success(null, $res['message'] ?? 'Usuario creado.');
    }

    public function deleteUser(Request $request, array $params = []): void
    {
        $username = $params['username'] ?? $request->get('username');
        if (empty($username)) {
            Response::error('Usuario no especificado.');
            return;
        }

        if (!$this->isAdminCaller() && $this->targetIsPrivileged($username)) {
            Response::error('No tienes permisos para eliminar cuentas administrativas.', 403);
            return;
        }

        $res = $this->user->deleteUser($username);
        if (!$res['success']) {
            AuditService::log('user_delete', $username, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al eliminar usuario.');
            return;
        }

        AuditService::log('user_delete', $username, 'SUCCESS');
        Response::success(null, $res['message'] ?? 'Usuario eliminado.');
    }

    public function createGroup(Request $request): void
    {
        $data = $request->getBody();
        $groupName = trim($data['name'] ?? '');

        if (empty($groupName)) {
            Response::error('El nombre del grupo es obligatorio.');
            return;
        }

        $res = $this->user->createGroup($groupName);
        if (!$res['success']) {
            AuditService::log('group_create', $groupName, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al crear grupo.');
            return;
        }

        AuditService::log('group_create', $groupName, 'SUCCESS');
        Response::success(null, $res['message'] ?? 'Grupo creado.');
    }

    public function deleteGroup(Request $request, array $params = []): void
    {
        $groupName = $params['name'] ?? $request->get('name');
        if (empty($groupName)) {
            Response::error('Nombre de grupo no especificado.');
            return;
        }

        $res = $this->user->deleteGroup($groupName);
        if (!$res['success']) {
            AuditService::log('group_delete', $groupName, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al eliminar grupo.');
            return;
        }

        AuditService::log('group_delete', $groupName, 'SUCCESS');
        Response::success(null, $res['message'] ?? 'Grupo eliminado.');
    }

    public function updateUser(Request $request): void
    {
        $data = $request->getBody();
        $username = trim($data['username'] ?? '');
        $password = isset($data['password']) ? (string) $data['password'] : '';
        $fullName = trim((string) ($data['full_name'] ?? ''));
        $groups = $data['groups'] ?? [];
        $isAdmin = (bool) ($data['is_admin'] ?? false);
        $canWeb = (bool) ($data['can_web'] ?? false);
        $sambaEnabled = (bool) ($data['samba_enabled'] ?? true);
        $isSuperadmin = (bool) ($data['is_superadmin'] ?? false);

        if (empty($username)) {
            Response::error('El nombre de usuario es obligatorio.');
            return;
        }

        // Un operador web no puede modificar ni otorgar privilegios administrativos.
        if (!$this->isAdminCaller()) {
            if ($this->targetIsPrivileged($username)) {
                Response::error('No tienes permisos para modificar cuentas administrativas.', 403);
                return;
            }
            $isAdmin = false;
            $isSuperadmin = false;
        }
        if ($isSuperadmin && !$this->isSuperCaller()) {
            Response::error('Solo un superadministrador puede otorgar el rol de superadministrador.', 403);
            return;
        }

        $res = $this->user->updateUser($username, $password !== '' ? $password : null, $fullName, (array) $groups, $isAdmin, $canWeb, $sambaEnabled, $isSuperadmin);
        if (!$res['success']) {
            AuditService::log('user_update', $username, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al actualizar usuario.');
            return;
        }

        AuditService::log('user_update', $username, 'SUCCESS', ['groups' => (array) $groups, 'is_admin' => $isAdmin, 'can_web' => $canWeb, 'is_superadmin' => $isSuperadmin]);
        Response::success(null, $res['message'] ?? 'Usuario actualizado.');
    }

    public function setPassword(Request $request): void
    {
        $data = $request->getBody();
        $username = trim($data['username'] ?? '');
        $password = (string) ($data['password'] ?? '');

        if (empty($username) || empty($password)) {
            Response::error('Usuario y contraseña son obligatorios.');
            return;
        }

        if (!$this->isAdminCaller() && $this->targetIsPrivileged($username)) {
            Response::error('No tienes permisos para cambiar la contraseña de cuentas administrativas.', 403);
            return;
        }

        $res = $this->user->setPassword($username, $password);
        if (!$res['success']) {
            AuditService::log('user_password', $username, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al cambiar la contraseña.');
            return;
        }

        AuditService::log('user_password', $username, 'SUCCESS');
        Response::success(null, $res['message'] ?? 'Contraseña actualizada.');
    }

    public function toggleUser(Request $request): void
    {
        $data = $request->getBody();
        $username = trim($data['username'] ?? '');
        $enabled = (bool) ($data['enabled'] ?? false);

        if (empty($username)) {
            Response::error('El nombre de usuario es obligatorio.');
            return;
        }

        if (!$this->isAdminCaller() && $this->targetIsPrivileged($username)) {
            Response::error('No tienes permisos para cambiar el estado de cuentas administrativas.', 403);
            return;
        }

        $res = $this->user->setEnabled($username, $enabled);
        if (!$res['success']) {
            AuditService::log('user_toggle', $username, 'FAILED', ['enabled' => $enabled, 'error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al cambiar el estado del usuario.');
            return;
        }

        AuditService::log('user_toggle', $username, 'SUCCESS', ['enabled' => $enabled]);
        Response::success(null, $res['message'] ?? 'Estado del usuario actualizado.');
    }

    public function userGroups(Request $request): void
    {
        $data = $request->getBody();
        $username = trim($data['username'] ?? '');
        $group = trim($data['group'] ?? '');
        $action = strtolower(trim($data['action'] ?? ''));

        if (empty($username) || empty($group) || !in_array($action, ['add', 'remove'], true)) {
            Response::error('Usuario, grupo y acción (add|remove) son obligatorios.');
            return;
        }

        $res = $action === 'add'
            ? $this->user->addUserToGroup($username, $group)
            : $this->user->removeUserFromGroup($username, $group);

        if (!$res['success']) {
            AuditService::log('user_group', $username, 'FAILED', ['group' => $group, 'action' => $action, 'error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al modificar la membresía.');
            return;
        }

        AuditService::log('user_group', $username, 'SUCCESS', ['group' => $group, 'action' => $action]);
        Response::success(null, $res['message'] ?? 'Membresía actualizada.');
    }

    public function rename(Request $request): void
    {
        $data = $request->getBody();
        $old = trim($data['old'] ?? '');
        $new = trim($data['new'] ?? '');

        if (empty($old) || empty($new)) {
            Response::error('Los nombres de grupo (actual y nuevo) son obligatorios.');
            return;
        }

        $res = $this->user->renameGroup($old, $new);
        if (!$res['success']) {
            AuditService::log('group_rename', $old, 'FAILED', ['new' => $new, 'error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al renombrar grupo.');
            return;
        }

        AuditService::log('group_rename', $old, 'SUCCESS', ['new' => $new]);
        Response::success(null, $res['message'] ?? 'Grupo renombrado.');
    }

    private function role(): string
    {
        return (string) ($_SESSION['nas_user']['role'] ?? 'operator');
    }

    private function isAdminCaller(): bool
    {
        return !empty($_SESSION['nas_user']['is_admin'])
            || in_array($this->role(), ['admin', 'superadmin'], true);
    }

    private function isSuperCaller(): bool
    {
        return !empty($_SESSION['nas_user']['is_superadmin'])
            || $this->role() === 'superadmin';
    }

    private function targetIsPrivileged(string $username): bool
    {
        $target = strtolower(trim($username));
        foreach ($this->user->listUsers() as $row) {
            if (strtolower((string) ($row['username'] ?? '')) === $target) {
                return !empty($row['is_admin']) || !empty($row['is_superadmin']);
            }
        }
        return false;
    }
}
