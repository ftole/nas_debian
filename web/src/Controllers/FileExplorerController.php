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
        $root = (string) $request->getQuery('root', 'nas');
        $rawPath = (string) $request->getQuery('path', '');
        $path = $this->resolveSubpath($root, $rawPath);
        $result = $this->fileService->listDirectory($path);

        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'No se pudo listar el directorio.', 404);
            return;
        }

        $items = $result['items'] ?? [];
        $breadcrumbs = $result['breadcrumbs'] ?? [];
        $currentPath = $result['current_path'] ?? $path;
        $totalItems = $result['total_items'] ?? count($items);

        $payload = [
            'success' => true,
            'current_path' => $currentPath,
            'breadcrumbs' => $breadcrumbs,
            'items' => $items,
            'total_items' => $totalItems,
            'data' => [
                'current_path' => $currentPath,
                'breadcrumbs' => $breadcrumbs,
                'items' => $items,
                'total_items' => $totalItems,
            ],
        ];

        Response::json($payload);
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

        $payload = array_merge($result, [
            'status' => 'success',
            'data' => $result,
        ]);
        Response::json($payload);
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
        $permanent = (bool) ($body['permanent'] ?? false);
        $user = (string) ($_SESSION['nas_user']['username'] ?? 'sistemas');

        if ($path === '') {
            Response::error('Debe especificar la ruta del archivo o carpeta a eliminar.', 400);
            return;
        }

        $result = $this->fileService->deleteItem($path, $permanent, $user);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al eliminar el elemento.', 400);
            return;
        }

        Response::json($result);
    }

    public function trashList(Request $request): void
    {
        $result = $this->fileService->listTrash();
        $count = $this->fileService->getTrashCount();
        $payload = [
            'success' => true,
            'items' => $result['items'] ?? [],
            'total' => $count,
            'count' => $count,
            'data' => [
                'items' => $result['items'] ?? [],
                'total' => $count,
                'count' => $count,
            ],
        ];
        Response::json($payload);
    }

    public function trashRestore(Request $request): void
    {
        $body = $request->getBody();
        $id = (int) ($body['id'] ?? 0);
        if ($id <= 0) {
            Response::error('Identificador de elemento de papelera no válido.', 400);
            return;
        }

        $result = $this->fileService->restoreTrashItem($id);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al restaurar el elemento.', 400);
            return;
        }

        Response::json($result);
    }

    public function trashDelete(Request $request): void
    {
        $body = $request->getBody();
        $id = (int) ($body['id'] ?? 0);
        if ($id <= 0) {
            Response::error('Identificador de elemento de papelera no válido.', 400);
            return;
        }

        $result = $this->fileService->deleteTrashItem($id);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al eliminar de la papelera.', 400);
            return;
        }

        Response::json($result);
    }

    public function trashEmpty(Request $request): void
    {
        $result = $this->fileService->emptyTrash();
        Response::json($result);
    }

    public function content(Request $request): void
    {
        $root = (string) $request->getQuery('root', 'nas');
        $rawPath = (string) $request->getQuery('path', '');
        $path = $this->resolveSubpath($root, $rawPath);

        if ($path === '') {
            Response::error('Ruta de archivo no especificada.', 400);
            return;
        }

        $result = $this->fileService->getFileContent($path);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'No se pudo obtener el contenido del archivo.', 400, $result);
            return;
        }

        $payload = array_merge($result, [
            'data' => $result,
        ]);
        Response::json($payload);
    }

    public function save(Request $request): void
    {
        $body = $request->getBody();
        $root = (string) ($body['root'] ?? 'nas');
        $rawPath = (string) ($body['path'] ?? '');
        $path = $this->resolveSubpath($root, $rawPath);
        $content = (string) ($body['content'] ?? '');

        if ($path === '') {
            Response::error('Ruta de archivo no especificada.', 400);
            return;
        }

        $result = $this->fileService->saveFileContent($path, $content);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Error al guardar el archivo.', 400);
            return;
        }

        Response::json($result);
    }

    public function raw(Request $request): void
    {
        $root = (string) $request->getQuery('root', 'nas');
        $rawPath = (string) $request->getQuery('path', '');
        $path = $this->resolveSubpath($root, $rawPath);

        if ($path === '') {
            http_response_code(400);
            echo 'Ruta no especificada.';
            return;
        }

        $this->fileService->streamRawFile($path);
    }

    public function download(Request $request): void
    {
        $root = (string) $request->getQuery('root', 'nas');
        $rawPath = (string) $request->getQuery('path', '');
        $path = $this->resolveSubpath($root, $rawPath);

        $this->fileService->downloadItem($path);
    }
}
