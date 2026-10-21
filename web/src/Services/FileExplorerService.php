<?php

declare(strict_types=1);

namespace App\Services;

use RecursiveCallbackFilterIterator;
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

        if (
            str_starts_with($cleanSub, '.trash') || $cleanSub === '.trash'
            || str_starts_with($cleanSub, '.tmp_zip') || $cleanSub === '.tmp_zip'
        ) {
            return null;
        }

        if ($cleanSub === '') {
            return $rootDir;
        }

        $candidate = $rootDir . '/' . $cleanSub;
        $realRoot = realpath($rootDir);
        $realTrash = realpath(self::getTrashDir());
        $realTmpZip = realpath($rootDir . '/.tmp_zip');
        $realCandidate = realpath($candidate);

        // Bloqueo estricto del repositorio de papelera y directorio temporal de compresión ZIP
        if ($realTrash !== false && $realCandidate !== false) {
            if ($realCandidate === $realTrash || str_starts_with($realCandidate, $realTrash . DIRECTORY_SEPARATOR) || str_starts_with($realCandidate, $realTrash . '/')) {
                return null;
            }
        }
        if ($realTmpZip !== false && $realCandidate !== false) {
            if ($realCandidate === $realTmpZip || str_starts_with($realCandidate, $realTmpZip . DIRECTORY_SEPARATOR) || str_starts_with($realCandidate, $realTmpZip . '/')) {
                return null;
            }
        }

        // Si el archivo/directorio aún no existe (ej. upload o mkdir), validamos el directorio padre
        if ($realCandidate === false) {
            // Rechazar destinos que sean enlaces simbólicos, incluidos los colgantes: de lo
            // contrario operaciones de escritura seguirían el enlace y escaparían de la raíz.
            if (is_link($candidate)) {
                return null;
            }
            $parent = dirname($candidate);
            $realParent = realpath($parent);
            if ($realTrash !== false && $realParent !== false) {
                if ($realParent === $realTrash || str_starts_with($realParent, $realTrash . DIRECTORY_SEPARATOR) || str_starts_with($realParent, $realTrash . '/')) {
                    return null;
                }
            }
            if ($realTmpZip !== false && $realParent !== false) {
                if ($realParent === $realTmpZip || str_starts_with($realParent, $realTmpZip . DIRECTORY_SEPARATOR) || str_starts_with($realParent, $realTmpZip . '/')) {
                    return null;
                }
            }
            if ($realParent !== false && $realRoot !== false && ($realParent === $realRoot || str_starts_with($realParent, $realRoot . DIRECTORY_SEPARATOR))) {
                return $candidate;
            }
            return null;
        }

        if ($realRoot !== false && ($realCandidate === $realRoot || str_starts_with($realCandidate, $realRoot . DIRECTORY_SEPARATOR))) {
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
            if ($entry === '.' || $entry === '..' || $entry === '.trash' || $entry === '.tmp_zip') {
                continue;
            }

            $fullPath = $targetDir . '/' . $entry;
            $isDir = !is_link($fullPath) && is_dir($fullPath);
            $size = $isDir ? $this->calculateDirectorySize($fullPath) : (@filesize($fullPath) ?: 0);
            $mtime = @filemtime($fullPath) ?: 0;
            $perms = @fileperms($fullPath);
            $permsStr = $perms !== false ? substr(sprintf('%o', $perms), -4) : '0660';

            $uid = @fileowner($fullPath);
            $gid = @filegroup($fullPath);
            $owner = 'sistemas';
            $group = 'grp_samba';
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
            if (is_link($destination)) {
                $failed[] = ['name' => $origName, 'error' => 'Destino inválido: no se permite sobreescribir enlaces simbólicos.'];
                continue;
            }

            // Mover archivo
            $moved = false;
            if (is_uploaded_file($tmpPath)) {
                $moved = @move_uploaded_file($tmpPath, $destination);
            } else {
                // Fallback para pruebas unitarias / CLI
                $moved = @copy($tmpPath, $destination);
            }

            if ($moved) {
                $dirPerms = @fileperms($targetDir);
                $fileMode = ($dirPerms !== false && ($dirPerms & 0007) === 0007) ? 0666 : 0660;
                @chmod($destination, $fileMode);

                $targetGid = @filegroup($targetDir);
                if ($targetGid !== false && function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                    @chgrp($destination, $targetGid);
                } elseif (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                    @chgrp($destination, 'grp_samba');
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

        $parentPerms = @fileperms($parent);
        $dirMode = ($parentPerms !== false && ($parentPerms & 0007) === 0007) ? 02777 : 02770;
        if (!@mkdir($target, $dirMode, true)) {
            return ['success' => false, 'error' => 'No se pudo crear la carpeta en el sistema de archivos.'];
        }

        $parentGid = @filegroup($parent);
        if ($parentGid !== false && function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
            @chgrp($target, $parentGid);
        } elseif (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
            @chgrp($target, 'grp_samba');
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

        // La raíz del almacenamiento nunca puede ser el origen de un renombrado: su
        // directorio padre (/srv) queda fuera del confinamiento y el rename movería
        // todos los datos fuera de /srv/nas.
        $rootDir = self::getRootDir();
        if ($source === $rootDir) {
            return ['success' => false, 'error' => 'Operación no permitida: No se puede renombrar la raíz del almacenamiento.'];
        }

        $cleanName = preg_replace('/[^\p{L}\p{N}._-]/u', '_', trim($newName));
        if ($cleanName === '' || $cleanName === '.' || $cleanName === '..') {
            return ['success' => false, 'error' => 'Nuevo nombre inválido.'];
        }

        $parent = dirname($source);
        $destination = $parent . '/' . $cleanName;

        // Verificación canónica adicional: el directorio destino debe residir dentro de la raíz.
        $realParent = realpath($parent);
        $realRoot = realpath($rootDir);
        if ($realParent === false || $realRoot === false
            || ($realParent !== $realRoot && !str_starts_with($realParent, $realRoot . DIRECTORY_SEPARATOR))) {
            return ['success' => false, 'error' => 'Ruta de destino fuera de los límites autorizados.'];
        }

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
     * Mueve un archivo o carpeta hacia un directorio destino dentro del confinamiento seguro.
     */
    public function moveItem(string $subpath, string $targetDir): array
    {
        $source = $this->resolveSafePath($subpath);
        if ($source === null || !file_exists($source)) {
            return ['success' => false, 'error' => 'El elemento de origen no existe o la ruta es inválida.'];
        }

        $rootDir = self::getRootDir();
        if ($source === $rootDir) {
            return ['success' => false, 'error' => 'Operación no permitida: No se puede mover la raíz del almacenamiento.'];
        }

        $target = $this->resolveSafePath($targetDir);
        if ($target === null || !is_dir($target)) {
            return ['success' => false, 'error' => 'El directorio de destino no existe o es inválido.'];
        }

        $baseName = basename($source);
        $destination = rtrim($target, '/') . '/' . $baseName;

        if (file_exists($destination)) {
            return ['success' => false, 'error' => 'Ya existe un elemento con el mismo nombre en la carpeta de destino.'];
        }

        // Prevenir mover un directorio dentro de sí mismo
        if (is_dir($source) && str_starts_with($destination . '/', $source . '/')) {
            return ['success' => false, 'error' => 'No se puede mover un directorio dentro de sí mismo.'];
        }

        if (!@rename($source, $destination)) {
            return ['success' => false, 'error' => 'No se pudo mover el elemento en el sistema de archivos.'];
        }

        AuditService::log('file_move', $subpath, 'SUCCESS', ['destination' => $targetDir . '/' . $baseName]);

        return [
            'success' => true,
            'message' => 'Elemento movido exitosamente.',
            'destination' => $destination,
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
            @chmod($trashDir, 02770);
            if (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                @chgrp($trashDir, 'grp_samba');
            }
        } elseif (((@fileperms($trashDir) ?: 0) & 0770) !== 0770) {
            @chmod($trashDir, 02770);
        }
        return $trashDir;
    }

    /**
     * Retorna la ruta absoluta del directorio de trabajo temporal para compresiones ZIP (/srv/nas/.tmp_zip),
     * garantizando que los archivos ZIP masivos se generen en el volumen de almacenamiento NAS y no saturen /tmp.
     */
    public static function getTempZipDir(): string
    {
        $tmpZipDir = self::getRootDir() . '/.tmp_zip';
        if (!is_dir($tmpZipDir)) {
            @mkdir($tmpZipDir, 0770, true);
            @chmod($tmpZipDir, 02770);
            if (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                @chgrp($tmpZipDir, 'grp_samba');
            }
        } elseif (((@fileperms($tmpZipDir) ?: 0) & 0770) !== 0770) {
            @chmod($tmpZipDir, 02770);
        }
        if (is_dir($tmpZipDir) && is_writable($tmpZipDir)) {
            $now = time();
            foreach (glob($tmpZipDir . '/nas_export_*.zip') ?: [] as $oldZip) {
                $mtime = @filemtime($oldZip);
                if ($mtime !== false && ($now - $mtime) > 3600) {
                    @unlink($oldZip);
                }
            }
            return $tmpZipDir;
        }
        return sys_get_temp_dir();
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
            @rename($trashPath, $target);
            return ['success' => false, 'error' => 'No se pudieron registrar los metadatos en la base de datos: ' . $e->getMessage()];
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

        $isDir = !is_link($target) && is_dir($target);
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
                @chgrp($parentDir, 'grp_samba');
            }
        }

        // Si ya existe un elemento con el mismo nombre en el destino, agregar sufijo restaurado
        if (file_exists($destination)) {
            $isDirItem = (bool) ($item['is_dir'] ?? false);
            $ext = $isDirItem ? '' : pathinfo($destination, PATHINFO_EXTENSION);
            $base = $isDirItem ? $item['filename'] : pathinfo($destination, PATHINFO_FILENAME);
            $counter = 0;
            do {
                $suffix = '_restaurado_' . date('Ymd_His') . ($counter > 0 ? "_{$counter}" : '');
                $newName = $base . $suffix . ($ext !== '' ? '.' . $ext : '');
                $candidateDest = $parentDir . '/' . $newName;
                $counter++;
            } while (file_exists($candidateDest));

            $destination = $candidateDest;
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
     * Guarda el contenido de texto plano o código editado desde el previsualizador web.
     */
    public function saveFileContent(string $subpath, string $content): array
    {
        $target = $this->resolveSafePath($subpath);
        $rawTarget = self::getRootDir() . '/' . ltrim(str_replace('\\', '/', $subpath), '/');
        if ($target === null || is_dir($target) || is_link($target) || is_link($rawTarget)) {
            return ['success' => false, 'error' => 'Ruta de archivo inválida o no permitida.'];
        }

        $rootDir = self::getRootDir();
        if ($target === $rootDir) {
            return ['success' => false, 'error' => 'No se puede guardar sobre la raíz del almacenamiento.'];
        }

        $parentDir = dirname($target);
        if (!is_dir($parentDir)) {
            $grandParent = dirname($parentDir);
            $grandPerms = @fileperms($grandParent);
            $parentMode = ($grandPerms !== false && ($grandPerms & 0007) === 0007) ? 02777 : 02770;
            if (!@mkdir($parentDir, $parentMode, true)) {
                return ['success' => false, 'error' => 'No se pudo crear la carpeta contenedora.'];
            }
            $grandGid = @filegroup($grandParent);
            if ($grandGid !== false && function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                @chgrp($parentDir, $grandGid);
            } elseif (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
                @chgrp($parentDir, 'grp_samba');
            }
        }

        $written = @file_put_contents($target, $content);
        if ($written === false) {
            return ['success' => false, 'error' => 'No se pudo guardar el archivo. Verifique permisos de escritura.'];
        }

        $parentPerms = @fileperms($parentDir);
        $fileMode = ($parentPerms !== false && ($parentPerms & 0007) === 0007) ? 0666 : 0660;
        @chmod($target, $fileMode);
        $parentGid = @filegroup($parentDir);
        if ($parentGid !== false && function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
            @chgrp($target, $parentGid);
        } elseif (function_exists('chgrp') && DIRECTORY_SEPARATOR !== '\\') {
            @chgrp($target, 'grp_samba');
        }

        $normalizedContent = (string) preg_replace('/(\r\n|\n|\r)$/D', '', $content);
        $linesCount = $normalizedContent === '' ? (strlen($content) > 0 ? 1 : 0) : substr_count($normalizedContent, "\n") + 1;

        AuditService::log('file_save', $subpath, 'SUCCESS', [
            'bytes' => $written,
            'lines' => $linesCount,
        ]);

        return [
            'success' => true,
            'message' => 'Archivo guardado correctamente.',
            'bytes' => $written,
            'size_formatted' => $this->formatBytes($written, false),
            'lines_count' => $linesCount,
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
            'xlsx' => 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            'xls' => 'application/vnd.ms-excel',
            'docx' => 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
            'doc' => 'application/msword',
            'pptx' => 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
            'ppt' => 'application/vnd.ms-powerpoint',
            'ods' => 'application/vnd.oasis.opendocument.spreadsheet',
            'odt' => 'application/vnd.oasis.opendocument.text',
            'csv' => 'text/csv; charset=utf-8',
            'tsv' => 'text/tab-separated-values; charset=utf-8',
        ];
        $contentType = $mimes[$ext] ?? 'application/octet-stream';
        $size = filesize($target);

        if (getenv('APP_ENV') !== 'testing' && ob_get_level()) {
            ob_end_clean();
        }

        // Soporte completo para peticiones HTTP Byte-Range (reproducción y desplazamiento en video/audio)
        $rangeHeader = $_SERVER['HTTP_RANGE'] ?? '';
        if ($rangeHeader !== '' && $size !== false && $size > 0 && preg_match('/bytes=(\d*)-(\d*)/i', $rangeHeader, $matches)) {
            $start = $matches[1] !== '' ? (int) $matches[1] : 0;
            $end = $matches[2] !== '' ? (int) $matches[2] : $size - 1;

            if ($start > $end || $start >= $size) {
                if (!headers_sent()) {
                    http_response_code(416);
                    header("Content-Range: bytes */{$size}");
                }
                return;
            }

            $end = min($end, $size - 1);
            $length = $end - $start + 1;

            if (!headers_sent()) {
                http_response_code(206);
                header('Content-Type: ' . $contentType);
                if ($ext === 'svg') {
                    header('Content-Disposition: attachment; filename="' . rawurlencode(basename($target)) . '"');
                    header("Content-Security-Policy: default-src 'none'; sandbox");
                } else {
                    header('Content-Disposition: inline; filename="' . rawurlencode(basename($target)) . '"');
                }
                header('Accept-Ranges: bytes');
                header("Content-Range: bytes {$start}-{$end}/{$size}");
                header('Content-Length: ' . $length);
                header('Cache-Control: private, max-age=3600');
            }

            $handle = @fopen($target, 'rb');
            if ($handle !== false) {
                fseek($handle, $start);
                $remaining = $length;
                while ($remaining > 0 && !feof($handle)) {
                    $chunk = min(65536, $remaining);
                    $buffer = fread($handle, $chunk);
                    if ($buffer === false || $buffer === '') {
                        break;
                    }
                    echo $buffer;
                    $remaining -= strlen($buffer);
                    flush();
                }
                fclose($handle);
            }
            if (getenv('APP_ENV') !== 'testing') {
                exit;
            }
            return;
        }

        if (!headers_sent()) {
            header('Content-Type: ' . $contentType);
            if ($ext === 'svg') {
                header('Content-Disposition: attachment; filename="' . rawurlencode(basename($target)) . '"');
                header("Content-Security-Policy: default-src 'none'; sandbox");
            } else {
                header('Content-Disposition: inline; filename="' . rawurlencode(basename($target)) . '"');
            }
            header('Accept-Ranges: bytes');
            header('Cache-Control: private, max-age=3600');
            if ($size !== false) {
                header('Content-Length: ' . $size);
            }
        }

        $handle = @fopen($target, 'rb');
        if ($handle !== false) {
            while (!feof($handle)) {
                echo fread($handle, 65536);
                flush();
            }
            fclose($handle);
        }
        if (getenv('APP_ENV') !== 'testing') {
            exit;
        }
    }

    /**
     * Genera un archivo ZIP temporal de una carpeta y lo transmite al cliente.
     */
    public function downloadDirectoryZip(string $subpath, bool $prepareOnly = false, string $token = ''): void
    {
        $targetDir = $this->resolveSafePath($subpath);
        if ($targetDir === null || !is_dir($targetDir)) {
            if (!headers_sent()) {
                http_response_code(404);
            }
            echo 'Directorio no encontrado.';
            return;
        }

        $realTargetDir = realpath($targetDir);
        if ($realTargetDir === false) {
            if (!headers_sent()) {
                http_response_code(404);
            }
            echo 'Directorio no encontrado.';
            return;
        }

        $realBaseDir = realpath(self::getRootDir());
        if (trim($subpath, '/\\') === '' || ($realBaseDir !== false && $realTargetDir === $realBaseDir)) {
            if (!headers_sent()) {
                http_response_code(400);
            }
            echo 'La descarga comprimida de la raíz del almacenamiento (/srv/nas) no está permitida.';
            return;
        }

        if (!class_exists(ZipArchive::class)) {
            if (!headers_sent()) {
                http_response_code(500);
            }
            echo 'Error: La extensión php-zip no está habilitada en el servidor.';
            return;
        }

        @set_time_limit(0);
        if (getenv('APP_ENV') !== 'testing' && session_status() === PHP_SESSION_ACTIVE) {
            @session_write_close();
        }

        $folderName = basename($targetDir);
        $tmpZipDir = self::getTempZipDir();

        // Fase 2 (transmisión directa por navegador tras compresión previa con prepare=1)
        if ($token !== '') {
            if (preg_match('/^[a-f0-9]{32}$/', $token) !== 1) {
                if (!headers_sent()) {
                    http_response_code(400);
                }
                echo 'Token de descarga ZIP inválido.';
                return;
            }
            $preparedZip = $tmpZipDir . '/nas_export_' . $token . '.zip';
            if (!is_file($preparedZip) || is_link($preparedZip)) {
                if (!headers_sent()) {
                    http_response_code(404);
                }
                echo 'El archivo comprimido temporal ya fue descargado o expiró.';
                return;
            }
            register_shutdown_function(static function () use ($preparedZip): void {
                if (file_exists($preparedZip)) {
                    @unlink($preparedZip);
                }
            });
            $this->streamFileDownload($preparedZip, $folderName . '.zip', true);
            return;
        }

        $lockFile = sys_get_temp_dir() . '/nas_zip_download.lock';
        $lockFp = @fopen($lockFile, 'c+');
        if ($lockFp !== false && !flock($lockFp, LOCK_EX | LOCK_NB)) {
            if (!headers_sent()) {
                http_response_code(429);
            }
            echo 'Operación en curso: ya se está procesando una compresión ZIP en el servidor. Intente más tarde.';
            fclose($lockFp);
            return;
        }

        $exportToken = bin2hex(random_bytes(16));
        $zipFile = $tmpZipDir . '/nas_export_' . $exportToken . '.zip';
        $keepPreparedZip = false;
        $releaseLock = static function () use (&$lockFp): void {
            if (is_resource($lockFp)) {
                @flock($lockFp, LOCK_UN);
                @fclose($lockFp);
                $lockFp = null;
            }
        };
        register_shutdown_function(static function () use ($zipFile, &$keepPreparedZip, $releaseLock): void {
            if (!$keepPreparedZip && file_exists($zipFile)) {
                @unlink($zipFile);
            }
            $releaseLock();
        });

        $zip = new ZipArchive();
        if ($zip->open($zipFile, ZipArchive::CREATE | ZipArchive::OVERWRITE) !== true) {
            $releaseLock();
            if (!headers_sent()) {
                http_response_code(500);
            }
            echo 'Error al crear el archivo comprimido temporal.';
            return;
        }

        $dirIterator = new RecursiveDirectoryIterator($targetDir, RecursiveDirectoryIterator::SKIP_DOTS);
        $filterIterator = new RecursiveCallbackFilterIterator($dirIterator, static function ($current) {
            if ($current->isLink()) {
                return false;
            }
            $filename = $current->getFilename();
            if ($filename === '.trash' || $filename === '.tmp_zip') {
                return false;
            }
            return true;
        });
        $files = new RecursiveIteratorIterator(
            $filterIterator,
            RecursiveIteratorIterator::LEAVES_ONLY,
            RecursiveIteratorIterator::CATCH_GET_CHILD
        );

        $addedEntries = 0;
        $storeExts = [
            'zip' => true, 'rar' => true, '7z' => true, 'gz' => true, 'bz2' => true,
            'xz' => true, 'tgz' => true, 'iso' => true, 'img' => true, 'mp4' => true,
            'mkv' => true, 'avi' => true, 'mov' => true, 'webm' => true, 'mp3' => true,
            'flac' => true, 'jpg' => true, 'jpeg' => true, 'png' => true, 'webp' => true,
        ];

        foreach ($files as $file) {
            if ($file->isLink()) {
                continue;
            }
            if (!$file->isDir()) {
                $filePath = $file->getRealPath();
                if ($filePath === false) {
                    continue;
                }

                // Excluir elementos de la papelera de reciclaje (.trash) y del directorio temporal (.tmp_zip)
                if (
                    str_contains($filePath, DIRECTORY_SEPARATOR . '.trash' . DIRECTORY_SEPARATOR)
                    || str_ends_with($filePath, DIRECTORY_SEPARATOR . '.trash')
                    || str_contains($filePath, DIRECTORY_SEPARATOR . '.tmp_zip' . DIRECTORY_SEPARATOR)
                    || str_ends_with($filePath, DIRECTORY_SEPARATOR . '.tmp_zip')
                ) {
                    continue;
                }

                // Verificar canónicamente que el archivo resida dentro de $realTargetDir (bloquear symlinks externos)
                if (!str_starts_with($filePath, $realTargetDir . DIRECTORY_SEPARATOR) && !str_starts_with($filePath, $realTargetDir . '/')) {
                    continue;
                }

                // Nombre de entrada seguro para el archivo comprimido (prevención de Zip Slip).
                // Neutralizar separadores de Windows/Linux y descartar componentes de escape (..)
                $entryParts = [];
                $rawRelative = substr($filePath, strlen($realTargetDir) + 1);
                $normalized = str_replace('\\', '/', $rawRelative);
                $rawParts = explode('/', $normalized);
                foreach ($rawParts as $part) {
                    $part = trim($part);
                    if ($part === '' || $part === '.' || $part === '..') {
                        continue;
                    }
                    $part = str_replace(["\0", "\r", "\n", ':', '\\', '/'], '_', $part);
                    $entryParts[] = $part;
                }
                if ($entryParts === []) {
                    continue;
                }
                $relativePath = implode('/', $entryParts);

                if ($zip->addFile($filePath, $relativePath)) {
                    $ext = strtolower(pathinfo($filePath, PATHINFO_EXTENSION));
                    $fSize = (int) @$file->getSize();
                    if (isset($storeExts[$ext]) || $fSize > 52428800) {
                        @$zip->setCompressionName($relativePath, ZipArchive::CM_STORE);
                    }
                    $addedEntries++;
                }
            }
        }

        if ($addedEntries === 0) {
            $zip->addEmptyDir($folderName);
        }

        $zip->close();

        if (!file_exists($zipFile)) {
            $releaseLock();
            if (!headers_sent()) {
                http_response_code(500);
            }
            echo 'Error al generar archivo comprimido.';
            return;
        }

        if ($prepareOnly) {
            $keepPreparedZip = true;
            $releaseLock();
            $zipSize = (int) (@filesize($zipFile) ?: 0);
            if (!headers_sent()) {
                header('Content-Type: application/json; charset=utf-8');
            }
            echo json_encode([
                'success' => true,
                'token' => $exportToken,
                'filename' => $folderName . '.zip',
                'size_bytes' => $zipSize,
                'size_formatted' => $this->formatBytes($zipSize, false),
            ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
            return;
        }

        try {
            $this->streamFileDownload($zipFile, $folderName . '.zip', true);
        } finally {
            $releaseLock();
        }
    }

    /**
     * Descarga de archivo individual o carpeta comprimida.
     */
    public function downloadItem(string $subpath, bool $prepareOnly = false, string $token = ''): void
    {
        $target = $this->resolveSafePath($subpath);
        if ($target === null || !file_exists($target)) {
            if (!headers_sent()) {
                http_response_code(404);
            }
            echo 'Elemento no encontrado.';
            return;
        }

        if (is_dir($target)) {
            $this->downloadDirectoryZip($subpath, $prepareOnly, $token);
            return;
        }

        $this->streamFileDownload($target, basename($target), false);
    }

    private function streamFileDownload(string $filePath, string $downloadName, bool $unlinkAfter): void
    {
        @set_time_limit(0);
        if (getenv('APP_ENV') !== 'testing' && session_status() === PHP_SESSION_ACTIVE) {
            @session_write_close();
        }
        $size = filesize($filePath);
        if (getenv('APP_ENV') !== 'testing' && ob_get_level()) {
            ob_end_clean();
        }

        if (!headers_sent()) {
            header('Content-Description: File Transfer');
            header('Content-Type: application/octet-stream');
            header('Content-Disposition: attachment; filename="' . rawurlencode($downloadName) . '"');
            header('X-Accel-Buffering: no');
            header('Expires: 0');
            header('Cache-Control: must-revalidate');
            header('Pragma: public');
            if ($size !== false) {
                header('Content-Length: ' . $size);
            }
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
        if (getenv('APP_ENV') !== 'testing') {
            exit;
        }
    }

    private function deleteDirectoryRecursive(string $dir): bool
    {
        if (is_link($dir)) {
            return @unlink($dir);
        }
        $files = array_diff(scandir($dir) ?: [], ['.', '..']);
        foreach ($files as $file) {
            $path = $dir . '/' . $file;
            if (is_link($path)) {
                @unlink($path);
            } elseif (is_dir($path)) {
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
        $entries = 0;
        $maxEntries = 50000;
        $startTime = microtime(true);
        $maxSeconds = 0.25;
        $seenInodes = [];
        $trackInodes = DIRECTORY_SEPARATOR !== '\\';

        try {
            $dirIterator = new RecursiveDirectoryIterator($dir, RecursiveDirectoryIterator::SKIP_DOTS);
            $filterIterator = new RecursiveCallbackFilterIterator($dirIterator, static function ($current) {
                if ($current->isLink()) {
                    return false;
                }
                $filename = $current->getFilename();
                if ($filename === '.trash' || $filename === '.tmp_zip') {
                    return false;
                }
                return true;
            });
            $files = new RecursiveIteratorIterator(
                $filterIterator,
                RecursiveIteratorIterator::LEAVES_ONLY,
                RecursiveIteratorIterator::CATCH_GET_CHILD
            );
            foreach ($files as $file) {
                if ($file->isFile()) {
                    if ($trackInodes) {
                        $inode = (int) @$file->getInode();
                        if ($inode > 0) {
                            if (isset($seenInodes[$inode])) {
                                $entries++;
                                if ($entries >= $maxEntries || (microtime(true) - $startTime) >= $maxSeconds) {
                                    break;
                                }
                                continue;
                            }
                            $seenInodes[$inode] = true;
                        }
                    }
                    $size += (int) @$file->getSize();
                }
                $entries++;
                if ($entries >= $maxEntries || (microtime(true) - $startTime) >= $maxSeconds) {
                    break;
                }
            }
        } catch (Throwable) {
            // Ignorar errores de acceso
        }
        return $size;
    }

    public function detectFileType(string $filename, bool $isDir): string
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

    private function formatBytes(int $bytes, bool $isDir = false): string
    {
        unset($isDir);
        if ($bytes >= 1099511627776) {
            return round($bytes / 1099511627776, 2) . ' TB';
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
