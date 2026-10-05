<?php

declare(strict_types=1);

namespace App\Services;

use RecursiveDirectoryIterator;
use RecursiveIteratorIterator;
use Throwable;
use ZipArchive;

/**
 * Servicio de Explorador de Archivos y Gestión de Almacenamiento (/srv/nas).
 * Proporciona navegación confinada por el árbol de recursos compartidos y respaldos,
 * subida de archivos (Drag-and-Drop) y descarga individual o comprimida (ZIP).
 */
class FileExplorerService
{
    private static ?string $customRootDir = null;

    public static function setRootDir(?string $dir): void
    {
        self::$customRootDir = $dir;
    }

    public static function getRootDir(): string
    {
        if (self::$customRootDir !== null) {
            return self::$customRootDir;
        }

        $prodRoot = '/srv/nas';
        if (is_dir($prodRoot)) {
            return realpath($prodRoot) ?: $prodRoot;
        }

        $localRoot = dirname(__DIR__, 2) . '/data/nas_root';
        if (!is_dir($localRoot)) {
            @mkdir($localRoot, 0770, true);
        }
        return realpath($localRoot) ?: $localRoot;
    }

    /**
     * Resuelve de forma estricta una subruta asegurando que resida dentro de la raíz permitida.
     */
    public function resolveSafePath(string $subpath): ?string
    {
        $rootDir = self::getRootDir();
        $cleanSub = trim(str_replace(['\\', '..'], ['/', ''], $subpath), '/');

        if (str_starts_with($cleanSub, '.trash') || $cleanSub === '.trash') {
            return null;
        }

        if ($cleanSub === '') {
            return $rootDir;
        }

        $candidate = $rootDir . '/' . $cleanSub;
        $realRoot = realpath($rootDir);
        $realCandidate = realpath($candidate);

        // Si el archivo/directorio aún no existe (ej. upload o mkdir), validamos el directorio padre
        if ($realCandidate === false) {
            $parent = dirname($candidate);
            $realParent = realpath($parent);
            if ($realParent !== false && $realRoot !== false && (str_starts_with($realParent, $realRoot) || $realParent === $realRoot)) {
                return $candidate;
            }
            return null;
        }

        if ($realRoot !== false && (str_starts_with($realCandidate, $realRoot) || $realCandidate === $realRoot)) {
            return $realCandidate;
        }

        return null;
    }

    /**
     * Lista los elementos contenidos en un subdirectorio con metadatos completos.
     */
    public function listDirectory(string $subpath = ''): array
    {
        $targetDir = $this->resolveSafePath($subpath);
        if ($targetDir === null || !is_dir($targetDir)) {
            return [
                'success' => false,
                'error' => 'Directorio no encontrado o ruta fuera de los límites autorizados.',
            ];
        }

        $rootDir = self::getRootDir();
        $relPath = str_replace('\\', '/', substr($targetDir, strlen($rootDir)));
        $relPath = trim($relPath, '/');

        $entries = @scandir($targetDir);
        if ($entries === false) {
            return [
                'success' => false,
                'error' => 'No se pudieron leer los contenidos del directorio.',
            ];
        }

        $items = [];
        foreach ($entries as $entry) {
            if ($entry === '.' || $entry === '..' || $entry === '.trash') {
                continue;
            }

            $fullPath = $targetDir . '/' . $entry;
            $isDir = is_dir($fullPath);
            $size = $isDir ? 0 : (@filesize($fullPath) ?: 0);
            $mtime = @filemtime($fullPath) ?: 0;
            $perms = @fileperms($fullPath);
            $permsStr = $perms !== false ? substr(sprintf('%o', $perms), -4) : '0660';

            $uid = @fileowner($fullPath);
            $gid = @filegroup($fullPath);
            $owner = 'sistemas';
            $group = 'grp_sistemas';
            if ($uid !== false && function_exists('posix_getpwuid')) {
                $pw = @posix_getpwuid($uid);
                if (is_array($pw) && !empty($pw['name'])) {
                    $owner = $pw['name'];
                }
            }
            if ($gid !== false && function_exists('posix_getgrgid')) {
                $gr = @posix_getgrgid($gid);
                if (is_array($gr) && !empty($gr['name'])) {
                    $group = $gr['name'];
                }
            }

            $dateFormatted = $mtime > 0 ? date('Y-m-d H:i:s', $mtime) : 'N/A';
            $itemRelPath = ($relPath !== '' ? $relPath . '/' : '') . $entry;

            $items[] = [
                'name' => $entry,
                'path' => $itemRelPath,
                'relative_path' => $itemRelPath,
                'is_dir' => $isDir,
                'type' => $this->detectFileType($entry, $isDir),
                'size_bytes' => $size,
                'size_formatted' => $this->formatBytes($size, $isDir),
                'mtime' => $dateFormatted,
                'modified_at' => $dateFormatted,
                'permissions' => $permsStr,
                'owner' => $owner,
                'group' => $group,
                'is_snapshot' => str_starts_with($entry, 'snapshot_'),
            ];
        }

        // Ordenar: primero carpetas alfabéticamente, luego archivos
        usort($items, function (array $a, array $b): int {
            if ($a['is_dir'] !== $b['is_dir']) {
                return $a['is_dir'] ? -1 : 1;
            }
            return strcasecmp($a['name'], $b['name']);
        });

        // Construir migas de pan (breadcrumbs)
        $breadcrumbs = [['name' => 'Raíz (/srv/nas)', 'path' => '']];
        if ($relPath !== '') {
            $parts = explode('/', $relPath);
            $accum = '';
            foreach ($parts as $p) {
                if ($p === '') continue;
                $accum = ($accum !== '' ? $accum . '/' : '') . $p;
                $breadcrumbs[] = [
                    'name' => $p,
                    'path' => $accum,
                ];
            }
        }

        $listPayload = [
            'success' => true,
            'current_path' => $relPath,
            'breadcrumbs' => $breadcrumbs,
            'items' => $items,
            'total_items' => count($items),
        ];
        $listPayload['data'] = [
            'current_path' => $relPath,
            'breadcrumbs' => $breadcrumbs,
            'items' => $items,
            'total_items' => count($items),
        ];

        return $listPayload;
    }

    /**
     * Procesa la subida de uno o varios archivos mediante Drag-and-Drop o formulario.
     */
    public function uploadFiles(string $targetSubpath, array $files): array
    {
        $targetDir = $this->resolveSafePath($targetSubpath);
        if ($targetDir === null || !is_dir($targetDir)) {
            return ['success' => false, 'error' => 'Directorio de destino inválido o inaccesible.'];
        }

        if (empty($files) || !isset($files['name'])) {
            return ['success' => false, 'error' => 'No se recibieron archivos para subir.'];
        }

        $names = is_array($files['name']) ? $files['name'] : [$files['name']];
        $tmpNames = is_array($files['tmp_name']) ? $files['tmp_name'] : [$files['tmp_name']];
        $errors = is_array($files['error']) ? $files['error'] : [$files['error']];
        $sizes = is_array($files['size']) ? $files['size'] : [$files['size']];

        $uploaded = [];
        $failed = [];

        for ($i = 0; $i < count($names); $i++) {
            $origName = $names[$i];
            $tmpPath = $tmpNames[$i];
            $err = $errors[$i];
            $size = $sizes[$i];

            if ($err !== UPLOAD_ERR_OK) {
                $failed[] = ['name' => $origName, 'error' => 'Error de subida de PHP (código ' . $err . ')'];
                continue;
            }

            // Saneamiento de nombre de archivo
            $cleanName = preg_replace('/[^\p{L}\p{N}._-]/u', '_', basename($origName));
            if ($cleanName === '' || $cleanName === '.' || $cleanName === '..') {
                $cleanName = 'archivo_subido_' . time();
            }

            $destination = $targetDir . '/' . $cleanName;

            // Mover archivo
            $moved = false;
            if (is_uploaded_file($tmpPath)) {
                $moved = @move_uploaded_file($tmpPath, $destination);
            } else {
                // Fallback para pruebas unitarias / CLI
                $moved = @copy($tmpPath, $destination);
            }

            if ($moved) {
                @chmod($destination, 0660);
                // Si existe el grupo corporativo grp_sistemas, asignarlo
                if (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                    @chgrp($destination, 'grp_sistemas');
                }

                $uploaded[] = [
                    'name' => $cleanName,
                    'size' => $size,
                ];

                AuditService::log('file_upload', ($targetSubpath !== '' ? $targetSubpath . '/' : '') . $cleanName, 'SUCCESS', [
                    'bytes' => $size,
                ]);
            } else {
                $failed[] = ['name' => $origName, 'error' => 'No se pudo guardar el archivo en el disco.'];
            }
        }

        $isOk = count($uploaded) > 0;
        return [
            'success' => $isOk,
            'status' => $isOk ? 'success' : 'error',
            'uploaded' => $uploaded,
            'failed' => $failed,
            'message' => sprintf('%d archivo(s) subido(s) con éxito.', count($uploaded)),
            'data' => [
                'uploaded' => $uploaded,
                'failed' => $failed,
            ],
        ];
    }

    /**
     * Crea un nuevo directorio dentro de la ruta especificada.
     */
    public function createDirectory(string $parentSubpath, string $dirName): array
    {
        $cleanDirName = preg_replace('/[^\p{L}\p{N}._-]/u', '_', trim($dirName));
        if ($cleanDirName === '' || $cleanDirName === '.' || $cleanDirName === '..') {
            return ['success' => false, 'error' => 'Nombre de carpeta inválido.'];
        }

        $parent = $this->resolveSafePath($parentSubpath);
        if ($parent === null || !is_dir($parent)) {
            return ['success' => false, 'error' => 'Directorio padre no encontrado o inaccesible.'];
        }

        $target = $parent . '/' . $cleanDirName;
        if (file_exists($target)) {
            return ['success' => false, 'error' => 'Ya existe un archivo o carpeta con ese nombre.'];
        }

        if (!@mkdir($target, 02770, true)) {
            return ['success' => false, 'error' => 'No se pudo crear la carpeta en el sistema de archivos.'];
        }

        if (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
            @chgrp($target, 'grp_sistemas');
        }

        AuditService::log('dir_create', ($parentSubpath !== '' ? $parentSubpath . '/' : '') . $cleanDirName, 'SUCCESS');

        return [
            'success' => true,
            'message' => 'Carpeta creada correctamente.',
            'name' => $cleanDirName,
        ];
    }

    /**
     * Renombra un archivo o carpeta dentro del confinamiento seguro.
     */
    public function renameItem(string $subpath, string $newName): array
    {
        $source = $this->resolveSafePath($subpath);
        if ($source === null || !file_exists($source)) {
            return ['success' => false, 'error' => 'El elemento de origen no existe o la ruta es inválida.'];
        }

        $cleanName = preg_replace('/[^\p{L}\p{N}._-]/u', '_', trim($newName));
        if ($cleanName === '' || $cleanName === '.' || $cleanName === '..') {
            return ['success' => false, 'error' => 'Nuevo nombre inválido.'];
        }

        $parent = dirname($source);
        $destination = $parent . '/' . $cleanName;

        if (file_exists($destination)) {
            return ['success' => false, 'error' => 'Ya existe un elemento con el nuevo nombre.'];
        }

        if (!@rename($source, $destination)) {
            return ['success' => false, 'error' => 'No se pudo renombrar el elemento en el sistema de archivos.'];
        }

        AuditService::log('file_rename', $subpath, 'SUCCESS', ['new_name' => $cleanName]);

        return [
            'success' => true,
            'message' => 'Elemento renombrado exitosamente.',
            'new_name' => $cleanName,
        ];
    }

    /**
     * Retorna la ruta absoluta del repositorio de papelera confinada (/srv/nas/.trash).
     */
    public static function getTrashDir(): string
    {
        $trashDir = self::getRootDir() . '/.trash';
        if (!is_dir($trashDir)) {
            @mkdir($trashDir, 0770, true);
            if (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                @chgrp($trashDir, 'grp_sistemas');
            }
        }
        return $trashDir;
    }

    /**
     * Mueve un archivo o carpeta a la papelera segura (/srv/nas/.trash) y registra sus metadatos.
     */
    public function moveToTrash(string $subpath, string $username = 'sistemas'): array
    {
        $target = $this->resolveSafePath($subpath);
        $rootDir = self::getRootDir();

        if ($target === null || !file_exists($target)) {
            return ['success' => false, 'error' => 'El elemento no existe o la ruta es inválida.'];
        }

        if ($target === $rootDir) {
            return ['success' => false, 'error' => 'Operación no permitida: No se puede eliminar la raíz del almacenamiento.'];
        }

        $trashDir = self::getTrashDir();
        $isDir = is_dir($target);
        $size = $isDir ? $this->calculateDirectorySize($target) : (@filesize($target) ?: 0);
        $cleanBase = preg_replace('/[^\p{L}\p{N}._-]/u', '_', basename($target));
        if ($cleanBase === '' || $cleanBase === '.' || $cleanBase === '..') {
            $cleanBase = 'item_' . time();
        }
        $trashName = uniqid('trash_', true) . '_' . $cleanBase;
        $trashPath = $trashDir . '/' . $trashName;

        if (!@rename($target, $trashPath)) {
            return ['success' => false, 'error' => 'No se pudo mover el elemento a la papelera.'];
        }

        try {
            $insertId = DatabaseService::insert('trash_items', [
                'original_path' => $subpath,
                'trash_name' => $trashName,
                'filename' => basename($target),
                'is_dir' => $isDir ? 1 : 0,
                'size_bytes' => $size,
                'deleted_by' => $username,
                'deleted_at' => date('Y-m-d H:i:s'),
            ]);
        } catch (Throwable $e) {
            error_log('Error registrando metadatos en trash_items: ' . $e->getMessage());
            $insertId = 0;
        }

        AuditService::log('trash_move', $subpath, 'SUCCESS', [
            'id' => $insertId,
            'trash_name' => $trashName,
            'size' => $size,
            'by' => $username,
        ]);

        return [
            'success' => true,
            'message' => $isDir ? 'Carpeta movida a la papelera.' : 'Archivo movido a la papelera.',
            'trash_id' => $insertId,
            'trash_name' => $trashName,
        ];
    }

    /**
     * Elimina un archivo o directorio. Por defecto lo envía a la papelera con opción de eliminación permanente.
     */
    public function deleteItem(string $subpath, bool $permanent = false, string $username = 'sistemas'): array
    {
        if (!$permanent) {
            return $this->moveToTrash($subpath, $username);
        }

        return $this->deleteItemPermanently($subpath);
    }

    /**
     * Elimina de forma definitiva e irreversible un archivo o directorio.
     */
    public function deleteItemPermanently(string $subpath): array
    {
        $target = $this->resolveSafePath($subpath);
        $rootDir = self::getRootDir();

        if ($target === null || !file_exists($target)) {
            return ['success' => false, 'error' => 'El elemento no existe o la ruta es inválida.'];
        }

        if ($target === $rootDir) {
            return ['success' => false, 'error' => 'Operación no permitida: No se puede eliminar la raíz del almacenamiento.'];
        }

        $isDir = is_dir($target);
        $deleted = $isDir ? $this->deleteDirectoryRecursive($target) : @unlink($target);

        if (!$deleted) {
            return ['success' => false, 'error' => 'No se pudo eliminar el elemento (verifique permisos).'];
        }

        AuditService::log($isDir ? 'dir_delete' : 'file_delete', $subpath, 'SUCCESS');

        return [
            'success' => true,
            'message' => $isDir ? 'Carpeta eliminada definitivamente con éxito.' : 'Archivo eliminado definitivamente con éxito.',
        ];
    }

    /**
     * Lista los elementos en la papelera de reciclaje.
     */
    public function listTrash(): array
    {
        $trashDir = self::getTrashDir();
        $items = [];
        try {
            $rows = DatabaseService::query('SELECT * FROM trash_items ORDER BY deleted_at DESC');
            foreach ($rows as $row) {
                $fullPath = $trashDir . '/' . $row['trash_name'];
                if (!file_exists($fullPath)) {
                    DatabaseService::execute('DELETE FROM trash_items WHERE id = ?', [$row['id']]);
                    continue;
                }

                $isDir = (bool) ($row['is_dir'] ?? 0);
                $size = (int) ($row['size_bytes'] ?? 0);
                $items[] = [
                    'id' => (int) $row['id'],
                    'name' => $row['filename'],
                    'filename' => $row['filename'],
                    'original_path' => $row['original_path'],
                    'trash_name' => $row['trash_name'],
                    'is_dir' => $isDir,
                    'type' => $this->detectFileType($row['filename'], $isDir),
                    'size_bytes' => $size,
                    'size_formatted' => $this->formatBytes($size, $isDir),
                    'deleted_by' => $row['deleted_by'],
                    'deleted_at' => $row['deleted_at'],
                ];
            }
        } catch (Throwable $e) {
            error_log('Error listando papelera: ' . $e->getMessage());
        }

        return [
            'success' => true,
            'items' => $items,
            'total' => count($items),
            'count' => count($items),
        ];
    }

    /**
     * Restaura un elemento desde la papelera a su ubicación original.
     */
    public function restoreTrashItem(int $id): array
    {
        try {
            $rows = DatabaseService::query('SELECT * FROM trash_items WHERE id = ?', [$id]);
        } catch (Throwable $e) {
            return ['success' => false, 'error' => 'Error al consultar la papelera: ' . $e->getMessage()];
        }

        if (empty($rows)) {
            return ['success' => false, 'error' => 'Elemento no encontrado en la papelera.'];
        }

        $item = $rows[0];
        $trashDir = self::getTrashDir();
        $source = $trashDir . '/' . $item['trash_name'];

        if (!file_exists($source)) {
            DatabaseService::execute('DELETE FROM trash_items WHERE id = ?', [$id]);
            return ['success' => false, 'error' => 'El archivo físico no existe en la papelera.'];
        }

        $rootDir = self::getRootDir();
        $destRel = trim(str_replace(['\\', '..'], ['/', ''], $item['original_path']), '/');
        $destination = $rootDir . ($destRel !== '' ? '/' . $destRel : '/' . $item['filename']);

        // Recrear directorios intermedios si fueron eliminados
        $parentDir = dirname($destination);
        if (!is_dir($parentDir)) {
            if (!@mkdir($parentDir, 02770, true)) {
                return ['success' => false, 'error' => 'No se pudo recrear la carpeta de destino original.'];
            }
            if (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                @chgrp($parentDir, 'grp_sistemas');
            }
        }

        // Si ya existe un elemento con el mismo nombre en el destino, agregar sufijo restaurado
        if (file_exists($destination)) {
            $ext = pathinfo($destination, PATHINFO_EXTENSION);
            $base = pathinfo($destination, PATHINFO_FILENAME);
            $suffix = '_restaurado_' . date('Ymd_His');
            $newName = $base . $suffix . ($ext !== '' ? '.' . $ext : '');
            $destination = $parentDir . '/' . $newName;
            $destRel = ($destRel !== basename($destRel) ? dirname($destRel) . '/' : '') . $newName;
        }

        if (!@rename($source, $destination)) {
            return ['success' => false, 'error' => 'No se pudo restaurar el archivo a su ubicación.'];
        }

        DatabaseService::execute('DELETE FROM trash_items WHERE id = ?', [$id]);
        AuditService::log('trash_restore', $item['original_path'], 'SUCCESS', [
            'id' => $id,
            'restored_to' => $destRel,
        ]);

        return [
            'success' => true,
            'message' => 'Elemento restaurado con éxito.',
            'restored_path' => $destRel,
        ];
    }

    /**
     * Elimina definitivamente un elemento específico de la papelera.
     */
    public function deleteTrashItem(int $id): array
    {
        try {
            $rows = DatabaseService::query('SELECT * FROM trash_items WHERE id = ?', [$id]);
        } catch (Throwable $e) {
            return ['success' => false, 'error' => 'Error al consultar papelera: ' . $e->getMessage()];
        }

        if (empty($rows)) {
            return ['success' => false, 'error' => 'Elemento no encontrado en la papelera.'];
        }

        $item = $rows[0];
        $trashDir = self::getTrashDir();
        $source = $trashDir . '/' . $item['trash_name'];

        if (file_exists($source)) {
            if (is_dir($source)) {
                $this->deleteDirectoryRecursive($source);
            } else {
                @unlink($source);
            }
        }

        DatabaseService::execute('DELETE FROM trash_items WHERE id = ?', [$id]);
        AuditService::log('trash_delete', $item['original_path'], 'SUCCESS', ['id' => $id]);

        return [
            'success' => true,
            'message' => 'Elemento eliminado definitivamente de la papelera.',
        ];
    }

    /**
     * Vacía por completo la papelera de reciclaje.
     */
    public function emptyTrash(): array
    {
        $trashDir = self::getTrashDir();
        $count = 0;

        try {
            $rows = DatabaseService::query('SELECT id, trash_name, original_path FROM trash_items');
            foreach ($rows as $row) {
                $source = $trashDir . '/' . $row['trash_name'];
                if (file_exists($source)) {
                    if (is_dir($source)) {
                        $this->deleteDirectoryRecursive($source);
                    } else {
                        @unlink($source);
                    }
                }
                $count++;
            }
            DatabaseService::execute('DELETE FROM trash_items');
        } catch (Throwable $e) {
            error_log('Error vaciando trash_items: ' . $e->getMessage());
        }

        // Limpiar cualquier residuo huérfano en el directorio .trash
        if (is_dir($trashDir)) {
            $entries = @scandir($trashDir) ?: [];
            foreach ($entries as $e) {
                if ($e === '.' || $e === '..') continue;
                $f = $trashDir . '/' . $e;
                if (is_dir($f)) {
                    $this->deleteDirectoryRecursive($f);
                } else {
                    @unlink($f);
                }
            }
        }

        AuditService::log('trash_empty', '.trash', 'SUCCESS', ['count' => $count]);

        return [
            'success' => true,
            'message' => "Papelera vaciada con éxito ({$count} elementos eliminados).",
            'deleted_count' => $count,
        ];
    }

    /**
     * Retorna el número de elementos contenidos en la papelera.
     */
    public function getTrashCount(): int
    {
        try {
            $res = DatabaseService::query('SELECT COUNT(*) as cnt FROM trash_items');
            return (int) ($res[0]['cnt'] ?? 0);
        } catch (Throwable) {
            return 0;
        }
    }

    /**
     * Obtiene el contenido de texto plano o código para previsualización segura en el visor.
     */
    public function getFileContent(string $subpath, int $maxBytes = 2097152): array
    {
        $target = $this->resolveSafePath($subpath);
        if ($target === null || !file_exists($target) || is_dir($target)) {
            return ['success' => false, 'error' => 'Archivo no encontrado o es un directorio.'];
        }

        $size = filesize($target) ?: 0;
        if ($size > $maxBytes) {
            return [
                'success' => false,
                'error' => sprintf('El archivo supera el límite de previsualización (%s). Utiliza la opción de descarga.', $this->formatBytes($maxBytes, false)),
                'size_bytes' => $size,
                'size_formatted' => $this->formatBytes($size, false),
            ];
        }

        $content = @file_get_contents($target);
        if ($content === false) {
            return ['success' => false, 'error' => 'No se pudo leer el contenido del archivo.'];
        }

        // Detección de caracteres binarios o nulos
        $isBinary = str_contains(substr($content, 0, 8192), "\0");
        if ($isBinary) {
            return [
                'success' => false,
                'error' => 'El archivo es de formato binario y no puede previsualizarse como texto plano.',
                'is_binary' => true,
                'size_bytes' => $size,
                'size_formatted' => $this->formatBytes($size, false),
            ];
        }

        $isUtf8 = false;
        if (function_exists('mb_check_encoding')) {
            $isUtf8 = mb_check_encoding($content, 'UTF-8');
        } else {
            $isUtf8 = @preg_match('//u', $content) === 1;
        }

        if (!$isUtf8) {
            if (function_exists('mb_convert_encoding')) {
                $content = mb_convert_encoding($content, 'UTF-8', 'ISO-8859-1, Windows-1252, ASCII');
            } elseif (function_exists('iconv')) {
                $converted = @iconv('ISO-8859-1', 'UTF-8//IGNORE', $content);
                if ($converted !== false) {
                    $content = $converted;
                }
            }
        }

        $normalizedContent = (string) preg_replace('/(\r\n|\n|\r)$/D', '', $content);
        $linesCount = $normalizedContent === '' ? (strlen($content) > 0 ? 1 : 0) : substr_count($normalizedContent, "\n") + 1;
        $ext = strtolower(pathinfo($target, PATHINFO_EXTENSION));

        return [
            'success' => true,
            'name' => basename($target),
            'path' => $subpath,
            'size_bytes' => $size,
            'size_formatted' => $this->formatBytes($size, false),
            'lines_count' => $linesCount,
            'content' => $content,
            'extension' => $ext,
            'is_binary' => false,
        ];
    }

    /**
     * Transmite de forma segura el archivo en bruto (raw) para renderizado multimedia o embebido (PDF/Imágenes).
     */
    public function streamRawFile(string $subpath): void
    {
        $target = $this->resolveSafePath($subpath);
        if ($target === null || !file_exists($target) || is_dir($target)) {
            http_response_code(404);
            echo 'Archivo no encontrado.';
            return;
        }

        $ext = strtolower(pathinfo($target, PATHINFO_EXTENSION));
        $mimes = [
            'png' => 'image/png',
            'jpg' => 'image/jpeg',
            'jpeg' => 'image/jpeg',
            'gif' => 'image/gif',
            'svg' => 'image/svg+xml',
            'webp' => 'image/webp',
            'bmp' => 'image/bmp',
            'ico' => 'image/x-icon',
            'pdf' => 'application/pdf',
            'mp4' => 'video/mp4',
            'webm' => 'video/webm',
            'mp3' => 'audio/mpeg',
            'wav' => 'audio/wav',
            'ogg' => 'audio/ogg',
            'txt' => 'text/plain; charset=utf-8',
            'json' => 'application/json',
            'md' => 'text/markdown; charset=utf-8',
        ];
        $contentType = $mimes[$ext] ?? 'application/octet-stream';
        $size = filesize($target);

        if (ob_get_level()) {
            ob_end_clean();
        }

        header('Content-Type: ' . $contentType);
        header('Content-Disposition: inline; filename="' . rawurlencode(basename($target)) . '"');
        header('Accept-Ranges: bytes');
        header('Cache-Control: private, max-age=3600');
        if ($size !== false) {
            header('Content-Length: ' . $size);
        }

        $handle = @fopen($target, 'rb');
        if ($handle !== false) {
            while (!feof($handle)) {
                echo fread($handle, 65536);
                flush();
            }
            fclose($handle);
        }
        exit;
    }

    /**
     * Genera un archivo ZIP temporal de una carpeta y lo transmite al cliente.
     */
    public function downloadDirectoryZip(string $subpath): void
    {
        $targetDir = $this->resolveSafePath($subpath);
        if ($targetDir === null || !is_dir($targetDir)) {
            http_response_code(404);
            echo 'Directorio no encontrado.';
            return;
        }

        if (!class_exists(ZipArchive::class)) {
            http_response_code(500);
            echo 'Error: La extensión php-zip no está habilitada en el servidor.';
            return;
        }

        $folderName = basename($targetDir);
        $zipFile = sys_get_temp_dir() . '/nas_export_' . uniqid() . '.zip';

        $zip = new ZipArchive();
        if ($zip->open($zipFile, ZipArchive::CREATE | ZipArchive::OVERWRITE) !== true) {
            http_response_code(500);
            echo 'Error al crear el archivo comprimido temporal.';
            return;
        }

        $files = new RecursiveIteratorIterator(
            new RecursiveDirectoryIterator($targetDir, RecursiveDirectoryIterator::SKIP_DOTS),
            RecursiveIteratorIterator::LEAVES_ONLY
        );

        $totalSize = 0;
        $maxZipSize = 500 * 1024 * 1024; // Límite de seguridad: 500 MB
        $addedEntries = 0;

        foreach ($files as $file) {
            if (!$file->isDir()) {
                $filePath = $file->getRealPath();
                $relativePath = substr($filePath, strlen($targetDir) + 1);
                $totalSize += $file->getSize();

                if ($totalSize > $maxZipSize) {
                    $zip->close();
                    @unlink($zipFile);
                    http_response_code(413);
                    echo 'El tamaño de la carpeta supera el límite web permitido (500 MB). Utilice la red compartida Samba para transferencias masivas.';
                    return;
                }

                $zip->addFile($filePath, $relativePath);
                $addedEntries++;
            }
        }

        if ($addedEntries === 0) {
            $zip->addEmptyDir($folderName);
        }

        $zip->close();

        if (!file_exists($zipFile)) {
            http_response_code(500);
            echo 'Error al generar archivo comprimido.';
            return;
        }

        $this->streamFileDownload($zipFile, $folderName . '.zip', true);
    }

    /**
     * Descarga de archivo individual o carpeta comprimida.
     */
    public function downloadItem(string $subpath): void
    {
        $target = $this->resolveSafePath($subpath);
        if ($target === null || !file_exists($target)) {
            http_response_code(404);
            echo 'Elemento no encontrado.';
            return;
        }

        if (is_dir($target)) {
            $this->downloadDirectoryZip($subpath);
            return;
        }

        $this->streamFileDownload($target, basename($target), false);
    }

    private function streamFileDownload(string $filePath, string $downloadName, bool $unlinkAfter): void
    {
        $size = filesize($filePath);
        if (ob_get_level()) {
            ob_end_clean();
        }

        header('Content-Description: File Transfer');
        header('Content-Type: application/octet-stream');
        header('Content-Disposition: attachment; filename="' . rawurlencode($downloadName) . '"');
        header('Expires: 0');
        header('Cache-Control: must-revalidate');
        header('Pragma: public');
        if ($size !== false) {
            header('Content-Length: ' . $size);
        }

        $handle = @fopen($filePath, 'rb');
        if ($handle !== false) {
            while (!feof($handle)) {
                echo fread($handle, 65536);
                flush();
            }
            fclose($handle);
        }

        if ($unlinkAfter) {
            @unlink($filePath);
        }
        exit;
    }

    private function deleteDirectoryRecursive(string $dir): bool
    {
        $files = array_diff(scandir($dir) ?: [], ['.', '..']);
        foreach ($files as $file) {
            $path = $dir . '/' . $file;
            if (is_dir($path)) {
                $this->deleteDirectoryRecursive($path);
            } else {
                @unlink($path);
            }
        }
        return @rmdir($dir);
    }

    private function calculateDirectorySize(string $dir): int
    {
        $size = 0;
        try {
            $files = new RecursiveIteratorIterator(
                new RecursiveDirectoryIterator($dir, RecursiveDirectoryIterator::SKIP_DOTS),
                RecursiveIteratorIterator::LEAVES_ONLY
            );
            foreach ($files as $file) {
                $size += $file->getSize();
            }
        } catch (Throwable) {
            // Ignorar errores de acceso
        }
        return $size;
    }

    private function detectFileType(string $filename, bool $isDir): string
    {
        if ($isDir) {
            return 'folder';
        }

        $ext = strtolower(pathinfo($filename, PATHINFO_EXTENSION));
        return match ($ext) {
            'zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar' => 'archive',
            'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods' => 'document',
            'png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'ico' => 'image',
            'mp3', 'wav', 'ogg', 'flac', 'm4a' => 'audio',
            'mp4', 'mkv', 'avi', 'mov', 'webm' => 'video',
            'sh', 'py', 'php', 'js', 'json', 'yml', 'yaml', 'xml', 'sql', 'conf', 'ini', 'txt', 'md' => 'code',
            default => 'file',
        };
    }

    private function formatBytes(int $bytes, bool $isDir): string
    {
        if ($isDir) {
            return '--';
        }

        if ($bytes >= 1073741824) {
            return round($bytes / 1073741824, 2) . ' GB';
        }
        if ($bytes >= 1048576) {
            return round($bytes / 1048576, 1) . ' MB';
        }
        if ($bytes >= 1024) {
            return round($bytes / 1024, 1) . ' KB';
        }
        return $bytes . ' B';
    }
}
