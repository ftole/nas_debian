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

use App\Controllers\AuthController;
use App\Controllers\BackupController;
use App\Controllers\DashboardController;
use App\Controllers\DomainController;
use App\Controllers\FileExplorerController;
use App\Controllers\SambaController;
use App\Controllers\StorageController;
use App\Controllers\SystemController;
use App\Controllers\TerminalController;
use App\Controllers\UserController;
use App\Core\AuthMiddleware;
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

// Middleware de autenticación y protección de sesiones
if (!AuthMiddleware::check($request)) {
    return;
}

// Rutas de Autenticación (Login, Logout, Sesión)
$router->get('/login', [AuthController::class, 'showLogin']);
$router->post('/login', [AuthController::class, 'login']);
$router->post('/api/auth/login', [AuthController::class, 'login']);
$router->post('/logout', [AuthController::class, 'logout']);
$router->post('/api/auth/logout', [AuthController::class, 'logout']);
$router->get('/api/auth/me', [AuthController::class, 'me']);

// Rutas de Vistas (Directas y amigables)
$router->get('/', [DashboardController::class, 'index']);

$modules = [
    'dashboard', 'logs', 'storage', 'networking', 'services', 'terminal',
    'shares', 'backups', 'users', 'diagnostics', 'updates', 'applications', 'domain'
];
foreach ($modules as $mod) {
    $router->get('/' . $mod, function (Request $req) use ($mod): void {
        (new DashboardController())->index($req, ['view' => $mod]);
    });
}

// Rutas API: Métricas y Dashboard
$router->get('/api/metrics', [DashboardController::class, 'metrics']);

// Rutas API: Diagnóstico Integral del Sistema
$router->get('/api/diagnostics', [SystemController::class, 'diagnostics']);

// Rutas API: Recursos Compartidos (Samba)
$router->get('/api/shares', [SambaController::class, 'list']);
$router->get('/api/shares/list', [SambaController::class, 'list']);
$router->post('/api/shares', [SambaController::class, 'create']);
$router->post('/api/shares/update', [SambaController::class, 'update']);
$router->post('/api/shares/access', [SambaController::class, 'setAccess']);
$router->get('/api/shares/access', [SambaController::class, 'accessMap']);
$router->post('/api/shares/delete', [SambaController::class, 'delete']);
$router->delete('/api/shares/{name}', [SambaController::class, 'delete']);

// Rutas API: Central de Respaldos (Backups)
$router->get('/api/backups', [BackupController::class, 'list']);
$router->get('/api/backups/tasks', [BackupController::class, 'list']);
$router->post('/api/backups', [BackupController::class, 'create']);
$router->post('/api/backups/test', [BackupController::class, 'test']);
$router->post('/api/backups/delete', [BackupController::class, 'delete']);
$router->delete('/api/backups/{id}', [BackupController::class, 'delete']);
$router->post('/api/backups/{id}/run', [BackupController::class, 'run']);
$router->get('/api/backups/{id}/logs', [BackupController::class, 'logs']);
$router->get('/api/backups/{id}/status', [BackupController::class, 'status']);

// Rutas API: Almacenamiento y Discos
$router->get('/api/storage', [StorageController::class, 'overview']);
$router->get('/api/storage/overview', [StorageController::class, 'overview']);
$router->get('/api/storage/disks', [StorageController::class, 'overview']);
$router->post('/api/storage/scrub', [StorageController::class, 'scrubStart']);
$router->get('/api/storage/scrub', [StorageController::class, 'scrubStatus']);
$router->post('/api/storage/trim', [StorageController::class, 'trim']);
$router->post('/api/storage/format', [StorageController::class, 'format']);
$router->post('/api/storage/lvm', [StorageController::class, 'lvm']);
$router->post('/api/storage/subvolume', [StorageController::class, 'subvolume']);

// Rutas API: Usuarios y Grupos
$router->get('/api/users', [UserController::class, 'users']);
$router->get('/api/users/list', [UserController::class, 'users']);
$router->post('/api/users', [UserController::class, 'createUser']);
$router->post('/api/users/update', [UserController::class, 'updateUser']);
$router->post('/api/users/password', [UserController::class, 'setPassword']);
$router->post('/api/users/toggle', [UserController::class, 'toggleUser']);
$router->post('/api/users/groups', [UserController::class, 'userGroups']);
$router->post('/api/users/delete', [UserController::class, 'deleteUser']);
$router->delete('/api/users/{username}', [UserController::class, 'deleteUser']);
$router->get('/api/groups', [UserController::class, 'groups']);
$router->get('/api/groups/list', [UserController::class, 'groups']);
$router->post('/api/groups', [UserController::class, 'createGroup']);
$router->post('/api/groups/rename', [UserController::class, 'rename']);
$router->post('/api/groups/delete', [UserController::class, 'deleteGroup']);
$router->delete('/api/groups/{name}', [UserController::class, 'deleteGroup']);

// Rutas API: Sistema, Servicios y Logs
$router->get('/api/services', [SystemController::class, 'services']);
$router->get('/api/services/list', [SystemController::class, 'services']);
$router->post('/api/services/manage', [SystemController::class, 'manageService']);
$router->get('/api/logs', [SystemController::class, 'logs']);
$router->post('/api/system/reboot', [SystemController::class, 'reboot']);
$router->get('/api/system/updates', [SystemController::class, 'updates']);

// Rutas API: Consola Terminal Web Real
$router->post('/api/terminal/exec', [TerminalController::class, 'exec']);
$router->get('/api/terminal/history', [TerminalController::class, 'history']);
$router->post('/api/terminal/session', [TerminalController::class, 'session']);
$router->post('/api/terminal/send', [TerminalController::class, 'send']);
$router->get('/api/terminal/capture', [TerminalController::class, 'capture']);
$router->post('/api/terminal/resize', [TerminalController::class, 'resize']);
$router->post('/api/terminal/kill', [TerminalController::class, 'kill']);

// Rutas API: Explorador de Archivos y Almacenamiento (/srv/nas)
$router->get('/api/files', [FileExplorerController::class, 'list']);
$router->get('/api/files/list', [FileExplorerController::class, 'list']);
$router->post('/api/files/upload', [FileExplorerController::class, 'upload']);
$router->post('/api/files/mkdir', [FileExplorerController::class, 'mkdir']);
$router->post('/api/files/rename', [FileExplorerController::class, 'rename']);
$router->post('/api/files/delete', [FileExplorerController::class, 'delete']);
$router->get('/api/files/download', [FileExplorerController::class, 'download']);
$router->get('/api/files/content', [FileExplorerController::class, 'content']);
$router->post('/api/files/save', [FileExplorerController::class, 'save']);
$router->get('/api/files/raw', [FileExplorerController::class, 'raw']);

// Rutas API: Papelera de Reciclaje Confinada (/srv/nas/.trash)
$router->get('/api/files/trash', [FileExplorerController::class, 'trashList']);
$router->post('/api/files/trash/restore', [FileExplorerController::class, 'trashRestore']);
$router->post('/api/files/trash/delete', [FileExplorerController::class, 'trashDelete']);
$router->post('/api/files/trash/empty', [FileExplorerController::class, 'trashEmpty']);

// Rutas API: Integración con Active Directory (AD)
$router->get('/api/domain', [DomainController::class, 'status']);
$router->get('/api/domain/status', [DomainController::class, 'status']);
$router->post('/api/domain/discover', [DomainController::class, 'discover']);
$router->post('/api/domain/join', [DomainController::class, 'join']);
$router->post('/api/domain/leave', [DomainController::class, 'leave']);

// Despachar la petición entrante
$router->dispatch($request);
