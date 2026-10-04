<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\SambaService;

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
        if (empty($data['name'])) {
            Response::error('El nombre del recurso compartido es obligatorio.');
            return;
        }

        $res = $this->samba->createShare($data);
        if (!$res['success']) {
            Response::error($res['error'] ?? 'Error al crear recurso compartido.');
            return;
        }

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
            Response::error($res['error'] ?? 'Error al eliminar recurso compartido.');
            return;
        }

        Response::success(null, $res['message'] ?? 'Recurso eliminado.');
    }
}
