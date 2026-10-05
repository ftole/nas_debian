<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\FileExplorerService;

/**
 * Controlador del Explorador de Archivos Web.
 * Enruta la navegación de recursos, subida por drag-and-drop, descargas y operaciones.
 */
class FileExplorerController
{
    private FileExplorerService $fileService;

    public function __construct(?FileExplorerService $service = null)
    {
        $this->fileService = $service ?? new FileExplorerService();
    }

    private function resolveSubpath(string $root, string $path): string
    {
        $clean = trim(str_replace(['\\', '..'], ['/', ''], $path), '/');
        if ($root === 'backups') {
            if ($clean === '') {
                return 'BACKUPS_HISTORICOS';
            }
            if (!str_starts_with($clean, 'BACKUPS_HISTORICOS')) {
                return 'BACKUPS_HISTORICOS/' . $clean;
            }
        }
        return $clean;
    }

    public function list(Request $request): void
    {
        $root = (string) ($request->getQuery()['root'] ?? 'nas');
        $rawPath = (string) ($request->getQuery()['path'] ?? '');
        $path = $this->resolveSubpath($root, $rawPath);
        $result = $this->fileService->listDirectory($path);

        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'No se pudo listar el directorio.', 404);
            return;
        }

        Response::json($result);
    }

    public function upload(Request $request): void
    {
        $root = (string) ($_POST['root'] ?? 'nas');
        $rawPath = (string) ($_POST['path'] ?? '');
        $path = $this->resolveSubpath($root, $rawPath);
        $files = $_FILES['files'] ?? $_FILES['file'] ?? [];

        $result = $this->fileService->uploadFiles($path, $files);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo en la subida de archivos.', 400, $result);
            return;
        }

        Response::json($result);
    }

    public function mkdir(Request $request): void
    {
        $body = $request->getBody();
        $root = (string) ($body['root'] ?? 'nas');
        $rawPath = (string) ($body['path'] ?? '');
        $path = $this->resolveSubpath($root, $rawPath);
        $name = (string) ($body['name'] ?? '');

        if ($name === '') {
            Response::error('El nombre de la carpeta es obligatorio.', 400);
            return;
        }

        $result = $this->fileService->createDirectory($path, $name);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al crear la carpeta.', 400);
            return;
        }

        Response::json($result);
    }

    public function rename(Request $request): void
    {
        $body = $request->getBody();
        $root = (string) ($body['root'] ?? 'nas');
        $rawPath = (string) ($body['old_path'] ?? $body['path'] ?? '');
        $path = $this->resolveSubpath($root, $rawPath);
        $newName = (string) ($body['new_name'] ?? '');

        if ($path === '' || $newName === '') {
            Response::error('Ruta y nuevo nombre son obligatorios.', 400);
            return;
        }

        $result = $this->fileService->renameItem($path, $newName);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al renombrar el elemento.', 400);
            return;
        }

        Response::json($result);
    }

    public function delete(Request $request): void
    {
        $body = $request->getBody();
        $root = (string) ($body['root'] ?? 'nas');
        $rawPath = (string) ($body['path'] ?? '');
        $path = $this->resolveSubpath($root, $rawPath);

        if ($path === '') {
            Response::error('Debe especificar la ruta del archivo o carpeta a eliminar.', 400);
            return;
        }

        $result = $this->fileService->deleteItem($path);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al eliminar el elemento.', 400);
            return;
        }

        Response::json($result);
    }

    public function download(Request $request): void
    {
        $root = (string) ($request->getQuery()['root'] ?? 'nas');
        $rawPath = (string) ($request->getQuery()['path'] ?? '');
        $path = $this->resolveSubpath($root, $rawPath);

        $this->fileService->downloadItem($path);
    }
}
