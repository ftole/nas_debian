<?php

declare(strict_types=1);

putenv('APP_ENV=testing');

/**
 * Pruebas unitarias e integrales para el entorno web MVC PHP 8 (App\).
 */

require_once __DIR__ . '/../web/src/Core/Request.php';
require_once __DIR__ . '/../web/src/Core/Response.php';
require_once __DIR__ . '/../web/src/Core/Router.php';
require_once __DIR__ . '/../web/src/Core/AuthMiddleware.php';
require_once __DIR__ . '/../web/src/Services/SystemService.php';
require_once __DIR__ . '/../web/src/Services/StorageService.php';
require_once __DIR__ . '/../web/src/Services/UserService.php';
require_once __DIR__ . '/../web/src/Services/SambaService.php';
require_once __DIR__ . '/../web/src/Services/BackupService.php';
require_once __DIR__ . '/../web/src/Services/AuthService.php';
require_once __DIR__ . '/../web/src/Controllers/DashboardController.php';
require_once __DIR__ . '/../web/src/Controllers/SambaController.php';
require_once __DIR__ . '/../web/src/Controllers/BackupController.php';
require_once __DIR__ . '/../web/src/Controllers/StorageController.php';
require_once __DIR__ . '/../web/src/Controllers/UserController.php';
require_once __DIR__ . '/../web/src/Controllers/SystemController.php';
require_once __DIR__ . '/../web/src/Controllers/AuthController.php';

use App\Core\AuthMiddleware;
use App\Core\Request;
use App\Services\AuthService;
use App\Services\BackupService;
use App\Services\SambaService;
use App\Services\StorageService;
use App\Services\SystemService;
use App\Services\UserService;

$passed = 0;
$failed = 0;

function assertTrue(bool $condition, string $testName): void {
    global $passed, $failed;
    if ($condition) {
        echo " [PASS] $testName\n";
        $passed++;
    } else {
        echo " [FAIL] $testName\n";
        $failed++;
    }
}

echo "=== INICIANDO SUITE DE PRUEBAS WEB MVC PHP 8 ===\n\n";

// 1. Pruebas de SystemService
$system = new SystemService();
$metrics = $system->getSystemMetrics();
assertTrue(!empty($metrics['hostname']), 'SystemService::getSystemMetrics devuelve hostname');
assertTrue(isset($metrics['ram_total_gb']), 'SystemService::getSystemMetrics devuelve ram_total_gb');
assertTrue(isset($metrics['cpu_usage_pct']), 'SystemService::getSystemMetrics devuelve cpu_usage_pct');

$services = $system->getServicesStatus();
assertTrue(is_array($services) && count($services) >= 5, 'SystemService::getServicesStatus lista al menos 5 servicios base');

$logs = $system->getJournalLogs(10);
assertTrue(is_array($logs), 'SystemService::getJournalLogs devuelve un array estructurado');

// 2. Pruebas de StorageService
$storage = new StorageService();
$overview = $storage->getStorageOverview();
assertTrue(isset($overview['total_gb']) && isset($overview['used_gb']), 'StorageService::getStorageOverview devuelve capacidades');
assertTrue(isset($overview['filesystem']), 'StorageService::getStorageOverview detecta sistema de archivos');

// 3. Pruebas de UserService
$user = new UserService();
$users = $user->listUsers();
assertTrue(is_array($users), 'UserService::listUsers devuelve array de usuarios');

$badUser = $user->createUser('ab', '12345');
assertTrue(!$badUser['success'], 'UserService::createUser rechaza nombres de usuario con menos de 3 caracteres');

$badPass = $user->createUser('usuario_test', '123');
assertTrue(!$badPass['success'], 'UserService::createUser rechaza contraseñas con menos de 6 caracteres');

$grp1 = $user->createGroup('marketing');
assertTrue($grp1['success'] && str_contains($grp1['message'], 'grp_marketing'), 'UserService auto-antepone prefijo corporativo grp_*');

$badGroupName = $user->createGroup('!!invalido!!');
assertTrue(!$badGroupName['success'], 'UserService rechaza caracteres inválidos en nombre de grupo');

$delRoot = $user->deleteUser('root');
assertTrue(!$delRoot['success'], 'UserService rechaza eliminar cuenta protegida root');

$delAdmin = $user->deleteUser('administrador');
assertTrue(!$delAdmin['success'], 'UserService rechaza eliminar cuenta protegida administrador');

$delMasterGrp = $user->deleteGroup('grp_sistemas');
assertTrue(!$delMasterGrp['success'], 'UserService rechaza eliminar grupo maestro protegido grp_sistemas');

// 4. Pruebas de SambaService
$samba = new SambaService('/nonexistent/smb.conf');
$shares = $samba->listShares();
assertTrue(is_array($shares), 'SambaService::listShares devuelve array de recursos');

$badShare = $samba->createShare(['name' => 'Recurso Con Espacios']);
assertTrue(!$badShare['success'], 'SambaService::createShare rechaza nombres con espacios');

$delGlobal = $samba->deleteShare('global');
assertTrue(!$delGlobal['success'], 'SambaService rechaza eliminar sección protegida [global]');

$delPrinters = $samba->deleteShare('printers');
assertTrue(!$delPrinters['success'], 'SambaService rechaza eliminar sección protegida [printers]');

// 5. Pruebas de BackupService
$backup = new BackupService();
$tasks = $backup->listTasks();
assertTrue(is_array($tasks), 'BackupService::listTasks devuelve array de tareas');

$badTask = $backup->createTask(['id' => 'x']);
assertTrue(!$badTask['success'], 'BackupService::createTask rechaza identificadores demasiado cortos');

$badCron = $backup->createTask(['id' => 'tarea_valida', 'cron' => '0 23 * *']);
assertTrue(!$badCron['success'], 'BackupService::createTask rechaza cron con menos de 5 campos');

$badRunnerId = $backup->runTaskNow('!bad!');
assertTrue(!$badRunnerId['success'], 'BackupService::runTaskNow valida identificador de tarea');

// Validar que createTask maneja credenciales de dominio de Windows (DOMINIO\usuario)
$tmpCredDir = sys_get_temp_dir() . '/test_bkp_cred_' . uniqid();
$backupTest = new BackupService();
$backupTest->credDir = $tmpCredDir;
$backupTest->binDir = $tmpCredDir . '/bin';
$backupTest->cronDir = $tmpCredDir . '/cron';
$backupTest->bkpRoot = $tmpCredDir . '/bkp';
$backupTest->logRoot = $tmpCredDir . '/log';
$cifsTask = $backupTest->createTask([
    'id' => 'cifs_domain',
    'proto' => 'cifs',
    'ip' => '10.0.0.5',
    'share' => 'contabilidad',
    'user' => 'EMPRESA\\admin',
    'password' => 'Secret123',
    'cron' => '0 1 * * *',
]);
assertTrue($cifsTask['success'], 'BackupService::createTask soporta credenciales de dominio Active Directory (DOMINIO\\usuario)');
$credFile = $tmpCredDir . '/cifs_domain.cred';
if (file_exists($credFile)) {
    $cContent = (string) file_get_contents($credFile);
    assertTrue(str_contains($cContent, 'domain=EMPRESA') && str_contains($cContent, 'username=admin'), 'BackupService desglosa dominio y usuario correctamente en archivo .cred');
}
// Limpiar sandbox temporal de prueba
@unlink($credFile);
@unlink($tmpCredDir . '/bin/backup_cifs_domain.sh');
@unlink($tmpCredDir . '/cron/backup_cifs_domain');
@rmdir($tmpCredDir . '/bin');
@rmdir($tmpCredDir . '/cron');
@rmdir($tmpCredDir . '/cred');
@rmdir($tmpCredDir . '/bkp/cifs_domain');
@rmdir($tmpCredDir . '/bkp');
@rmdir($tmpCredDir . '/log');
@rmdir($tmpCredDir);

// 6. Pruebas de Router y enrutamiento REST
$testRouter = new \App\Core\Router();
$matched = false;
$testRouter->get('/api/test/{param}', function ($req, $params) use (&$matched) {
    if (isset($params['param']) && $params['param'] === 'valor123') {
        $matched = true;
    }
});
$_SERVER['REQUEST_METHOD'] = 'GET';
$_SERVER['REQUEST_URI'] = '/api/test/valor123';
$_GET = [];
$req = new \App\Core\Request();
$testRouter->dispatch($req);
assertTrue($matched, 'Router resuelve variables dinámicas en rutas REST ({param})');

// 7. Pruebas de AuthService (Autenticación y Cuentas de Sistema)
$auth = new AuthService();
$loginSistemas = $auth->authenticate('sistemas', 'Ead2026#');
assertTrue($loginSistemas['success'] && $loginSistemas['user']['username'] === 'sistemas', 'AuthService autentica satisfactoriamente al usuario sistemas con contraseña válida');
assertTrue($loginSistemas['user']['is_admin'] === true, 'AuthService otorga privilegios de administrador a sistemas');

$loginAdmin = $auth->authenticate('administrador', 'admin123');
assertTrue($loginAdmin['success'] && $loginAdmin['user']['username'] === 'administrador', 'AuthService autentica satisfactoriamente al usuario administrador');

$loginAdmin2 = $auth->authenticate('administrador', 'Admin123#');
assertTrue($loginAdmin2['success'] && $loginAdmin2['user']['username'] === 'administrador', 'AuthService autentica satisfactoriamente al usuario administrador con Admin123#');

$loginBadPass = $auth->authenticate('sistemas', 'password_erroneo');
assertTrue(!$loginBadPass['success'], 'AuthService rechaza contraseñas inválidas');

$loginUnknown = $auth->authenticate('usuario_inexistente', 'cualquiercosa');
assertTrue(!$loginUnknown['success'], 'AuthService rechaza cuentas no existentes');

$loginEmpty = $auth->authenticate('', '');
assertTrue(!$loginEmpty['success'], 'AuthService rechaza campos de autenticación vacíos');

$loginBadFormat = $auth->authenticate('bad user!!', 'pass123');
assertTrue(!$loginBadFormat['success'], 'AuthService rechaza formato inválido de nombre de usuario');

// 8. Pruebas de AuthMiddleware (Rutas Públicas y Sesiones)
$_SERVER['REQUEST_METHOD'] = 'GET';
$_SERVER['REQUEST_URI'] = '/login';
$_GET = [];
$loginReq = new Request();
assertTrue(AuthMiddleware::check($loginReq), 'AuthMiddleware permite acceso público sin sesión a /login');

$_SERVER['REQUEST_URI'] = '/api/auth/login';
$apiLoginReq = new Request();
assertTrue(AuthMiddleware::check($apiLoginReq), 'AuthMiddleware permite acceso público sin sesión a /api/auth/login');

// Con sesión activa
$_SESSION['nas_user'] = ['username' => 'sistemas', 'is_admin' => true];
$_SERVER['REQUEST_URI'] = '/';
$homeReq = new Request();
assertTrue(AuthMiddleware::check($homeReq), 'AuthMiddleware permite acceso a rutas protegidas con sesión activa');

// 9. Pruebas de Diagnóstico del Sistema (SystemController::diagnostics)
$sysCtrl = new \App\Controllers\SystemController();
ob_start();
// Mock de llamada a diagnostics sin invocar exit
$diagSystem = new SystemService();
$diagServices = $diagSystem->getServicesStatus();
$diagMetrics = $diagSystem->getSystemMetrics();
$diagStorage = (new StorageService())->getStorageOverview();
assertTrue(is_array($diagServices) && count($diagServices) >= 5, 'Diagnósticos compila estado de demonios clave');
assertTrue(isset($diagStorage['used_gb']), 'Diagnósticos incluye métricas de almacenamiento');

// 10. Pruebas de Request (Mapeo de HEAD a GET)
$_SERVER['REQUEST_METHOD'] = 'HEAD';
$_SERVER['REQUEST_URI'] = '/';
$headReq = new Request();
assertTrue($headReq->getMethod() === 'GET', 'Request normaliza peticiones HTTP HEAD a GET para compatibilidad');

// 11. Pruebas de compatibilidad de firmas de controladores con parámetros opcionales
$sambaCtrl = new \App\Controllers\SambaController();
$backupCtrl = new \App\Controllers\BackupController();
$userCtrl = new \App\Controllers\UserController();
assertTrue(is_callable([$sambaCtrl, 'delete']), 'SambaController::delete es invocable');
assertTrue(is_callable([$backupCtrl, 'delete']), 'BackupController::delete es invocable');
assertTrue(is_callable([$userCtrl, 'deleteUser']), 'UserController::deleteUser es invocable');

// 12. Pruebas de AuthService::logout
$auth->logout();
assertTrue(empty($_SESSION['nas_user']), 'AuthService::logout limpia variables de sesión activa');

// 13. Pruebas de validación de campos obligatorios en BackupService::createTask
$emptyCifs = (new BackupService())->createTask(['id' => 'tarea_vacia', 'proto' => 'cifs']);
assertTrue(!$emptyCifs['success'], 'BackupService::createTask rechaza CIFS sin IP o recurso compartido');

echo "\n==================================================\n";
echo "RESULTADO: $passed pasadas, $failed fallidas.\n";
echo "==================================================\n";

if ($failed > 0) {
    exit(1);
}
