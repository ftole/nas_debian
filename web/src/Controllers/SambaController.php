<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\AuditService;
use App\Services\SambaService;
use App\Services\UserService;

/**
 * Controlador API para recursos compartidos Samba.
 */
class SambaController
{
    private SambaService $samba;

    public function __construct()
    {
        $this->samba = new SambaService();
    }

    public function list(Request $request): void
    {
        $shares = $this->samba->listShares();
        Response::success($shares);
    }

    public function create(Request $request): void
    {
        $data = $request->getBody();
        $name = trim($data['name'] ?? '');
        if (empty($name)) {
            Response::error('El nombre del recurso compartido es obligatorio.');
            return;
        }

        $res = $this->samba->createShare($data);
        if (!$res['success']) {
            AuditService::log('share_create', $name, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al crear recurso compartido.');
            return;
        }

        AuditService::log('share_create', $name, 'SUCCESS', [
            'scheme' => $data['scheme'] ?? 1,
            'comment' => $data['comment'] ?? '',
            'hidden' => !empty($data['hidden']),
        ]);
        Response::success(null, $res['message'] ?? 'Recurso creado.');
    }

    public function delete(Request $request, array $params = []): void
    {
        $name = $params['name'] ?? $request->get('name');
        if (empty($name)) {
            Response::error('Nombre de recurso no especificado.');
            return;
        }

        $deleteFiles = (bool) $request->get('delete_files', false);
        $res = $this->samba->deleteShare($name, $deleteFiles);

        if (!$res['success']) {
            AuditService::log('share_delete', $name, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al eliminar recurso compartido.');
            return;
        }

        AuditService::log('share_delete', $name, 'SUCCESS', ['delete_files' => $deleteFiles]);
        Response::success(null, $res['message'] ?? 'Recurso eliminado.');
    }

    public function update(Request $request): void
    {
        $data = $request->getBody();
        $name = trim($data['name'] ?? '');
        if ($name === '') {
            Response::error('Nombre de recurso no especificado.');
            return;
        }

        $res = $this->samba->updateShare($name, $data);
        if (!$res['success']) {
            AuditService::log('share_update', $name, 'FAILED', ['error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al actualizar el recurso compartido.');
            return;
        }

        AuditService::log('share_update', $name, 'SUCCESS', ['scheme' => $data['scheme'] ?? null]);
        Response::success(null, $res['message'] ?? 'Recurso actualizado.');
    }

    public function setAccess(Request $request): void
    {
        $data = $request->getBody();
        $share = trim($data['share'] ?? '');
        $kind = strtolower(trim($data['kind'] ?? ''));
        $target = trim($data['name'] ?? '');
        $level = strtolower(trim($data['level'] ?? ''));

        if ($share === '' || $target === '' || !in_array($kind, ['group', 'user'], true)) {
            Response::error('Parámetros incompletos (share, kind, name, level).');
            return;
        }

        $res = $this->samba->setAccess($share, $kind, $target, $level);
        if (!$res['success']) {
            AuditService::log('share_access', $share, 'FAILED', ['kind' => $kind, 'target' => $target, 'level' => $level, 'error' => $res['error'] ?? '']);
            Response::error($res['error'] ?? 'Error al actualizar el acceso.');
            return;
        }

        AuditService::log('share_access', $share, 'SUCCESS', ['kind' => $kind, 'target' => $target, 'level' => $level]);
        Response::success(null, $res['message'] ?? 'Acceso actualizado.');
    }

    /**
     * Repara las ACL POSIX de uno o todos los recursos para alinearlas con smb.conf.
     */
    public function repairAccess(Request $request): void
    {
        $data = $request->getBody();
        $name = trim((string) ($data['share'] ?? ''));

        if ($name !== '') {
            $res = $this->samba->syncShareAcls($name);
            if (!$res['success']) {
                AuditService::log('share_acl_repair', $name, 'FAILED', ['error' => $res['error'] ?? '']);
                Response::error($res['error'] ?? 'No se pudieron reparar las ACL.');
                return;
            }
            AuditService::log('share_acl_repair', $name, 'SUCCESS');
            Response::success(null, $res['message'] ?? 'ACL reparadas.');
            return;
        }

        $fixed = 0;
        foreach ($this->samba->listShares() as $share) {
            $res = $this->samba->syncShareAcls($share['name']);
            if (!empty($res['success'])) {
                $fixed++;
            }
        }
        AuditService::log('share_acl_repair', '*', 'SUCCESS', ['shares' => $fixed]);
        Response::success(['repaired' => $fixed], "ACL sincronizadas en $fixed recurso(s).");
    }

    /**
     * Matriz de acceso: recursos × grupos y recursos × usuarios (nivel efectivo).
     */
    public function accessMap(Request $request): void
    {
        $userService = new UserService();
        $groups = $userService->listGroups();
        $users = $userService->listUsers();
        $map = $this->samba->getAccessMap();
        $shares = array_keys($map);

        $groupMatrix = [];
        foreach ($groups as $g) {
            if (!empty($g['is_special'])) {
                continue;
            }
            $gname = $g['name'];
            $row = ['name' => $gname];
            foreach ($shares as $share) {
                $entry = $map[$share];
                if (in_array($gname, $entry['write_groups'], true)) {
                    $row[$share] = 'write';
                } elseif (in_array($gname, $entry['read_groups'], true)) {
                    $row[$share] = 'read';
                } else {
                    $row[$share] = 'none';
                }
            }
            $groupMatrix[] = $row;
        }

        $userMatrix = [];
        foreach ($users as $u) {
            $uname = $u['username'];
            $ugroups = $u['groups'] ?? [];
            $row = ['name' => $uname, 'is_admin' => !empty($u['is_admin'])];
            foreach ($shares as $share) {
                $entry = $map[$share];
                $write = in_array($uname, $entry['write_users'], true)
                    || count(array_intersect($ugroups, $entry['write_groups'])) > 0;
                $read = $write
                    || in_array($uname, $entry['read_users'], true)
                    || count(array_intersect($ugroups, $entry['read_groups'])) > 0;
                $row[$share] = $write ? 'write' : ($read ? 'read' : 'none');
            }
            $userMatrix[] = $row;
        }

        Response::success([
            'shares' => $shares,
            'groups' => $groupMatrix,
            'users' => $userMatrix,
        ]);
    }
}
