<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
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
        $groups = $data['groups'] ?? [];
        $isAdmin = (bool) ($data['is_admin'] ?? false);

        if (empty($username) || empty($password)) {
            Response::error('El usuario y la contraseña son obligatorios.');
            return;
        }

        $res = $this->user->createUser($username, $password, (array) $groups, $isAdmin);
        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al crear usuario.');
            return;
        }

        Response::success(null, $res['message'] ?? 'Usuario creado.');
    }

    public function deleteUser(Request $request, array $params = []): void
    {
        $username = $params['username'] ?? $request->get('username');
        if (empty($username)) {
            Response::error('Usuario no especificado.');
            return;
        }

        $res = $this->user->deleteUser($username);
        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al eliminar usuario.');
            return;
        }

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
            Response::error($res['error'] ?? 'Error al crear grupo.');
            return;
        }

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
            Response::error($res['error'] ?? 'Error al eliminar grupo.');
            return;
        }

        Response::success(null, $res['message'] ?? 'Grupo eliminado.');
    }
}
