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
            if ($entry === '.' || $entry === '..') {
                continue;
            }

            $fullPath = $targetDir . '/' . $entry;
            $isDir = is_dir($fullPath);
            $size = $isDir ? 0 : (@filesize($fullPath) ?: 0);
            $mtime = @filemtime($fullPath) ?: 0;
            $perms = @fileperms($fullPath);
            $permsStr = $perms !== false ? substr(sprintf('%o', $perms), -4) : '0660';

            $itemRelPath = ($relPath !== '' ? $relPath . '/' : '') . $entry;

            $items[] = [
                'name' => $entry,
                'path' => $itemRelPath,
                'is_dir' => $isDir,
                'type' => $this->detectFileType($entry, $isDir),
                'size_bytes' => $size,
                'size_formatted' => $this->formatBytes($size, $isDir),
                'mtime' => $mtime > 0 ? date('Y-m-d H:i:s', $mtime) : 'N/A',
                'permissions' => $permsStr,
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

        return [
            'success' => true,
            'current_path' => $relPath,
            'breadcrumbs' => $breadcrumbs,
            'items' => $items,
            'total_items' => count($items),
        ];
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

        return [
            'success' => count($uploaded) > 0,
            'uploaded' => $uploaded,
            'failed' => $failed,
            'message' => sprintf('%d archivo(s) subido(s) con éxito.', count($uploaded)),
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
     * Elimina de forma segura un archivo o directorio.
     */
    public function deleteItem(string $subpath): array
    {
        $target = $this->resolveSafePath($subpath);
        $rootDir = self::getRootDir();

        if ($target === null || !file_exists($target)) {
            return ['success' => false, 'error' => 'El elemento no existe o la ruta es inválida.'];
        }

        // Prohibir terminantemente eliminar la raíz del NAS
        if ($target === $rootDir) {
            return ['success' => false, 'error' => 'Operación no permitida: No se puede eliminar la raíz del almacenamiento.'];
        }

        $isDir = is_dir($target);
        $deleted = false;

        if ($isDir) {
            $deleted = $this->deleteDirectoryRecursive($target);
        } else {
            $deleted = @unlink($target);
        }

        if (!$deleted) {
            return ['success' => false, 'error' => 'No se pudo eliminar el elemento (verifique permisos).'];
        }

        AuditService::log($isDir ? 'dir_delete' : 'file_delete', $subpath, 'SUCCESS');

        return [
            'success' => true,
            'message' => $isDir ? 'Carpeta eliminada con éxito.' : 'Archivo eliminado con éxito.',
        ];
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
