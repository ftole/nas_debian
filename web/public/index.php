<?php

declare(strict_types=1);

/**
 * Front Controller del Panel Web NAS Debian 13.
 * Enruta peticiones a los controladores MVC correspondientes.
 */

// Autocarga PSR-4 (Nativo o via Composer)
$composerAutoload = dirname(__DIR__) . '/vendor/autoload.php';
if (file_exists($composerAutoload)) {
    require_once $composerAutoload;
} else {
    spl_autoload_register(function (string $class): void {
        $prefix = 'App\\';
        $baseDir = dirname(__DIR__) . '/src/';
        $len = strlen($prefix);
        if (strncmp($prefix, $class, $len) !== 0) {
            return;
        }
        $relativeClass = substr($class, $len);
        $file = $baseDir . str_replace('\\', '/', $relativeClass) . '.php';
        if (file_exists($file)) {
            require_once $file;
        }
    });
}

use App\Controllers\BackupController;
use App\Controllers\DashboardController;
use App\Controllers\SambaController;
use App\Controllers\StorageController;
use App\Controllers\SystemController;
use App\Controllers\UserController;
use App\Core\Request;
use App\Core\Response;
use App\Core\Router;

// Manejo global seguro de excepciones
set_exception_handler(function (\Throwable $e): void {
    error_log("Excepción no capturada en Panel Web: " . $e->getMessage() . "\n" . $e->getTraceAsString());
    $isJson = isset($_SERVER['HTTP_ACCEPT']) && str_contains($_SERVER['HTTP_ACCEPT'], 'application/json');
    if ($isJson || (isset($_SERVER['REQUEST_URI']) && str_starts_with($_SERVER['REQUEST_URI'], '/api'))) {
        Response::error('Ocurrió un error interno en el servidor.', 500);
    } else {
        http_response_code(500);
        echo "<h1>500 - Error Interno del Servidor</h1>";
        echo "<p>Ha ocurrido un problema al procesar la solicitud. Revisa los registros del sistema.</p>";
    }
});

$request = new Request();
$router = new Router();

// Rutas de Vistas
$router->get('/', [DashboardController::class, 'index']);

// Rutas API: Métricas y Dashboard
$router->get('/api/metrics', [DashboardController::class, 'metrics']);

// Rutas API: Recursos Compartidos (Samba)
$router->get('/api/shares', [SambaController::class, 'list']);
$router->post('/api/shares', [SambaController::class, 'create']);
$router->post('/api/shares/delete', [SambaController::class, 'delete']);
$router->delete('/api/shares/{name}', [SambaController::class, 'delete']);

// Rutas API: Central de Respaldos (Backups)
$router->get('/api/backups', [BackupController::class, 'list']);
$router->post('/api/backups', [BackupController::class, 'create']);
$router->post('/api/backups/delete', [BackupController::class, 'delete']);
$router->delete('/api/backups/{id}', [BackupController::class, 'delete']);
$router->post('/api/backups/{id}/run', [BackupController::class, 'run']);
$router->get('/api/backups/{id}/logs', [BackupController::class, 'logs']);

// Rutas API: Almacenamiento y Discos
$router->get('/api/storage', [StorageController::class, 'overview']);
$router->post('/api/storage/scrub', [StorageController::class, 'scrubStart']);
$router->get('/api/storage/scrub', [StorageController::class, 'scrubStatus']);
$router->post('/api/storage/trim', [StorageController::class, 'trim']);

// Rutas API: Usuarios y Grupos
$router->get('/api/users', [UserController::class, 'users']);
$router->post('/api/users', [UserController::class, 'createUser']);
$router->post('/api/users/delete', [UserController::class, 'deleteUser']);
$router->delete('/api/users/{username}', [UserController::class, 'deleteUser']);
$router->get('/api/groups', [UserController::class, 'groups']);
$router->post('/api/groups', [UserController::class, 'createGroup']);
$router->post('/api/groups/delete', [UserController::class, 'deleteGroup']);
$router->delete('/api/groups/{name}', [UserController::class, 'deleteGroup']);

// Rutas API: Sistema, Servicios y Logs
$router->get('/api/services', [SystemController::class, 'services']);
$router->post('/api/services/manage', [SystemController::class, 'manageService']);
$router->get('/api/logs', [SystemController::class, 'logs']);
$router->post('/api/system/reboot', [SystemController::class, 'reboot']);
$router->get('/api/system/updates', [SystemController::class, 'updates']);

// Despachar la petición entrante
$router->dispatch($request);
