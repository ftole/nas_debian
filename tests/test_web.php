<?php

declare(strict_types=1);

putenv('APP_ENV=testing');
putenv('NAS_TEST_ADMIN_PASS=Admin123#');
putenv('NAS_TEST_SISTEMAS_PASS=Ead2026#');
putenv('NAS_TEST_ADMIN_TOKENS=admin123,Admin123#,Ead2026#');

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
require_once __DIR__ . '/../web/src/Services/AuditService.php';
require_once __DIR__ . '/../web/src/Controllers/DashboardController.php';
require_once __DIR__ . '/../web/src/Controllers/SambaController.php';
require_once __DIR__ . '/../web/src/Controllers/BackupController.php';
require_once __DIR__ . '/../web/src/Controllers/StorageController.php';
require_once __DIR__ . '/../web/src/Controllers/UserController.php';
require_once __DIR__ . '/../web/src/Controllers/SystemController.php';
require_once __DIR__ . '/../web/src/Controllers/AuthController.php';
require_once __DIR__ . '/../web/src/Services/DatabaseService.php';
require_once __DIR__ . '/../web/src/Services/TerminalService.php';
require_once __DIR__ . '/../web/src/Services/FileExplorerService.php';
require_once __DIR__ . '/../web/src/Services/DomainService.php';
require_once __DIR__ . '/../web/src/Controllers/TerminalController.php';
require_once __DIR__ . '/../web/src/Controllers/FileExplorerController.php';
require_once __DIR__ . '/../web/src/Controllers/DomainController.php';

use App\Core\AuthMiddleware;
use App\Core\Request;
use App\Core\Response;
use App\Core\Router;
use App\Services\AuditService;
use App\Services\AuthService;
use App\Services\BackupService;
use App\Services\DatabaseService;
use App\Services\DomainService;
use App\Services\FileExplorerService;
use App\Services\SambaService;
use App\Services\StorageService;
use App\Services\SystemService;
use App\Services\TerminalService;
use App\Services\UserService;
use App\Controllers\AuthController;
use App\Controllers\SystemController;
use App\Controllers\TerminalController;

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

$delMasterGrp = $user->deleteGroup('grp_samba');
assertTrue(!$delMasterGrp['success'], 'UserService rechaza eliminar grupo maestro protegido grp_samba');

$delWebGrp = $user->deleteGroup('grp_web');
assertTrue(!$delWebGrp['success'], 'UserService rechaza eliminar el grupo especial grp_web');
$renWebGrp = $user->renameGroup('grp_web', 'grp_x');
assertTrue(!$renWebGrp['success'], 'UserService rechaza renombrar el grupo especial grp_web');
$grpListAll = $user->listGroups();
$webEntry = null;
foreach ($grpListAll as $gl) {
    if (($gl['name'] ?? '') === 'grp_web') {
        $webEntry = $gl;
    }
}
assertTrue($webEntry !== null && ($webEntry['is_special'] ?? false) === true, 'UserService::listGroups marca grp_web como especial');

// 3.1 Gestor real de usuarios (edición, estado, contraseña, miembros y renombrado)
$firstUser = $users[0] ?? [];
assertTrue(isset($firstUser['enabled'], $firstUser['shell'], $firstUser['home']),
    'UserService::listUsers expone estado (enabled), shell y home');
assertTrue(isset($firstUser['full_name'], $firstUser['can_web'], $firstUser['samba_enabled']),
    'UserService::listUsers expone full_name, can_web y samba_enabled');
assertTrue(isset($users[0]['is_admin']) && isset($users[0]['is_samba']),
    'UserService::listUsers mantiene campos is_admin e is_samba');

$setPwdOk = $user->setPassword('usuario_test', 'claveNueva123');
assertTrue($setPwdOk['success'], 'UserService::setPassword actualiza contraseña (modo test)');
$setPwdShort = $user->setPassword('usuario_test', '123');
assertTrue(!$setPwdShort['success'], 'UserService::setPassword rechaza contraseñas cortas');
$setPwdRoot = $user->setPassword('root', 'claveNueva123');
assertTrue(!$setPwdRoot['success'], 'UserService::setPassword rechaza cuenta protegida root');
$setPwdAdmin = $user->setPassword('administrador', 'claveNueva123');
assertTrue(!$setPwdAdmin['success'], 'UserService::setPassword rechaza cuenta protegida administrador');

$enableOk = $user->setEnabled('usuario_test', false);
assertTrue($enableOk['success'], 'UserService::setEnabled bloquea cuenta (modo test)');
$enableBack = $user->setEnabled('usuario_test', true);
assertTrue($enableBack['success'], 'UserService::setEnabled desbloquea cuenta (modo test)');
$enableRoot = $user->setEnabled('root', false);
assertTrue(!$enableRoot['success'], 'UserService::setEnabled rechaza bloquear cuenta protegida');

$updOk = $user->updateUser('usuario_test', 'claveNueva123', 'Operador de Prueba', ['grp_marketing'], true, false, true);
assertTrue($updOk['success'], 'UserService::updateUser actualiza contraseña/grupos/admin (modo test)');
$updBadUser = $user->updateUser('u', 'claveNueva123', '', [], false, false, true);
assertTrue(!$updBadUser['success'], 'UserService::updateUser rechaza usuario inválido');
$updProtected = $user->updateUser('sistemas', null, '', [], false, false, true);
assertTrue(!$updProtected['success'], 'UserService::updateUser rechaza cuenta protegida sistemas');
$updWeb = $user->updateUser('usuario_test', null, '', ['grp_marketing'], false, true, true);
assertTrue($updWeb['success'], 'UserService::updateUser admite acceso web (can_web) en modo test');

$addMember = $user->addUserToGroup('usuario_test', 'grp_marketing');
assertTrue($addMember['success'], 'UserService::addUserToGroup añade miembro a grp_* (modo test)');
$addBadGroup = $user->addUserToGroup('usuario_test', 'marketing');
assertTrue(!$addBadGroup['success'], 'UserService::addUserToGroup rechaza grupos sin prefijo grp_');
$rmMember = $user->removeUserFromGroup('usuario_test', 'grp_marketing');
assertTrue($rmMember['success'], 'UserService::removeUserFromGroup quita miembro de grp_* (modo test)');
$rmProtected = $user->removeUserFromGroup('root', 'grp_marketing');
assertTrue(!$rmProtected['success'], 'UserService::removeUserFromGroup rechaza cuentas protegidas');

$renameOk = $user->renameGroup('grp_marketing', 'grp_finanzas');
assertTrue($renameOk['success'], 'UserService::renameGroup renombra grupo grp_* (modo test)');
$renameMaster = $user->renameGroup('grp_samba', 'grp_x');
assertTrue(!$renameMaster['success'], 'UserService::renameGroup protege grp_samba');
$renameSame = $user->renameGroup('grp_finanzas', 'grp_finanzas');
assertTrue(!$renameSame['success'], 'UserService::renameGroup rechaza renombrar con el mismo nombre');
$renameInvalid = $user->renameGroup('grp_finanzas', 'finanzas');
assertTrue(!$renameInvalid['success'], 'UserService::renameGroup exige prefijo grp_* en el nuevo nombre');

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

// 4.1 Edición de recursos y matriz de acceso (SambaService)
$badLevel = $samba->setAccess('X', 'group', 'grp_x', 'invalid');
assertTrue(!$badLevel['success'], 'SambaService::setAccess rechaza nivel inválido');
$badKind = $samba->setAccess('X', 'otro', 'grp_x', 'read');
assertTrue(!$badKind['success'], 'SambaService::setAccess rechaza tipo inválido (group|user)');
$badGroup = $samba->setAccess('X', 'group', '!!bad!!', 'read');
assertTrue(!$badGroup['success'], 'SambaService::setAccess rechaza nombre de grupo inválido');
$updMissing = $samba->updateShare('NO_EXISTE', ['scheme' => 1]);
assertTrue(!$updMissing['success'], 'SambaService::updateShare rechaza recurso inexistente');
assertTrue(is_array($samba->getAccessMap()), 'SambaService::getAccessMap devuelve array');

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
assertTrue(($loginSistemas['user']['can_web'] ?? false) === true, 'AuthService expone la capacidad can_web en la sesión');

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

// 14. Pruebas de Request::getClientIp y trusted proxies
$_SERVER['REMOTE_ADDR'] = '192.168.1.150';
unset($_SERVER['HTTP_X_FORWARDED_FOR'], $_SERVER['HTTP_CLIENT_IP']);
$ipReq1 = new Request();
assertTrue($ipReq1->getClientIp() === '192.168.1.150', 'Request::getClientIp resuelve REMOTE_ADDR');

$_SERVER['HTTP_X_FORWARDED_FOR'] = '10.20.30.40, 192.168.1.150';
$ipReq2 = new Request();
assertTrue($ipReq2->getClientIp() === '192.168.1.150', 'Request::getClientIp ignora HTTP_X_FORWARDED_FOR de cliente no confiable');

$_SERVER['REMOTE_ADDR'] = '127.0.0.1'; // Proxy local de confianza
$_SERVER['HTTP_X_FORWARDED_FOR'] = '10.20.30.40, 192.168.1.150';
$ipReq3 = new Request();
assertTrue($ipReq3->getClientIp() === '10.20.30.40', 'Request::getClientIp prioriza primera IP de HTTP_X_FORWARDED_FOR desde proxy de confianza');

unset($_SERVER['HTTP_X_FORWARDED_FOR']);
$_SERVER['HTTP_CLIENT_IP'] = '172.16.5.99';
$ipReq4 = new Request();
assertTrue($ipReq4->getClientIp() === '172.16.5.99', 'Request::getClientIp resuelve HTTP_CLIENT_IP desde proxy de confianza');

// 15. Pruebas de AuditService::log
$_SESSION['nas_user'] = ['username' => 'test_admin', 'is_admin' => true];
$tempLog = tempnam(sys_get_temp_dir(), 'nas_admin_test_');
$origLogPath = AuditService::getLogPath();
AuditService::setLogPath($tempLog);
AuditService::log('share_create', 'VENTAS', 'SUCCESS', ['scheme' => 1]);
AuditService::log('login_failure', 'unknown', 'FAILED', ['reason' => 'bad_pass']);
$writtenLog = file_get_contents($tempLog);
AuditService::setLogPath($origLogPath);
@unlink($tempLog);

assertTrue(str_contains($writtenLog, '[test_admin] [share_create] [VENTAS] [SUCCESS] {"scheme":1}'), 'AuditService escribe formato estructurado con JSON');
assertTrue(str_contains($writtenLog, '[test_admin] [login_failure] [unknown] [FAILED] {"reason":"bad_pass"}'), 'AuditService registra fallos y detalles');


// 16. Pruebas de SystemService::parseSambaAuditLog y filtros con fixtures herméticos
$origSambaPath = SystemService::$sambaAuditPath;
$origAdminPath = SystemService::$adminAuditPath;
$origBackupPath = SystemService::$backupAuditPath;

$tempSamba = tempnam(sys_get_temp_dir(), 'nas_samba_test_');
$tempAdmin = tempnam(sys_get_temp_dir(), 'nas_admin_test_');
$tempBackup = tempnam(sys_get_temp_dir(), 'nas_bkp_test_');

file_put_contents($tempSamba, implode("\n", [
    '2026-10-05T10:15:32-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|openat|ok|r|Balance_General_2026.xlsx',
    '2026-10-05T10:16:05-06:00 srv-nas smbd_audit[3412]: administrador|10.10.1.251|adm-pc|SISTEMAS|openat|ok|w|Presupuesto_Anual.xlsx',
    '2026-10-05T10:17:12-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|renameat|ok|borrador_acta.docx|acta_final.docx',
    '2026-10-05T10:18:40-06:00 srv-nas smbd_audit[3412]: administrador|10.10.1.251|adm-pc|SISTEMAS|unlinkat|ok|archivo_temporal.tmp',
    '2026-10-05T10:19:00-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|mkdirat|ok|Reportes_Q3',
    '2026-10-05T10:19:30-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|rmdir|ok|Carpeta_Vieja',
    '2026-10-05T10:20:15-06:00 srv-nas smbd_audit[3412]: sistemas|10.10.1.250|sis-frank|SISTEMAS|connect|ok|SISTEMAS',
]) . "\n");

file_put_contents($tempAdmin, implode("\n", [
    '[2026-10-05 10:14:00] [10.10.1.250] [sistemas] [login_success] [sistemas] [SUCCESS] {"role":"Administrador"}',
    '[2026-10-05 10:15:00] [10.10.1.250] [sistemas] [share_create] [PUBLICO] [SUCCESS] {"scheme":4,"comment":"Acceso general"}',
    '[2026-10-05 10:20:00] [10.10.1.250] [sistemas] [user_create] [operador1] [SUCCESS] {"groups":["grp_operaciones"]}',
    '[2026-10-05 10:25:00] [10.10.1.250] [sistemas] [backup_create] [win_contabilidad] [SUCCESS] {"proto":"cifs","cron":"0 23 * * *"}',
    '[2026-10-05 10:30:00] [10.10.1.250] [sistemas] [service_manage] [smbd] [SUCCESS] {"action":"restart"}',
]) . "\n");

file_put_contents($tempBackup, implode("\n", [
    '[2026-10-05 23:00:01] [win_contabilidad] [LOCK_ACQUIRED] [info] Bloqueo de ejecucion exclusivo adquirido',
    '[2026-10-05 23:00:02] [win_contabilidad] [SPACE_CHECK] [info] Espacio verificado: 45000 MB libres (25% en uso)',
    '[2026-10-05 23:00:04] [win_contabilidad] [MOUNT_SUCCESS] [info] Recurso CIFS //10.10.1.50/Contabilidad montado exitosamente',
    '[2026-10-05 23:00:15] [win_contabilidad] [RSYNC_COMPLETED] [info] Sincronizacion rsync finalizada correctamente',
    '[2026-10-05 23:00:16] [win_contabilidad] [SNAPSHOT_PROMOTED] [notice] Snapshot promovido: snapshot_2026-10-05_230000',
    '[2026-10-05 23:00:17] [win_contabilidad] [ROTATION_PRUNED] [info] Rotado snapshot antiguo: snapshot_2026-09-01_230000',
    '[2026-10-05 23:00:18] [win_contabilidad] [BACKUP_COMPLETED] [notice] Respaldo CIFS win_contabilidad finalizado con exito',
]) . "\n");

SystemService::$sambaAuditPath = $tempSamba;
SystemService::$adminAuditPath = $tempAdmin;
SystemService::$backupAuditPath = $tempBackup;

$sambaLogs = $system->parseSambaAuditLog(50);
assertTrue(is_array($sambaLogs) && count($sambaLogs) > 0, 'SystemService::parseSambaAuditLog retorna eventos estructurados');
assertTrue(isset($sambaLogs[0]['source']) && $sambaLogs[0]['source'] === 'samba_audit', 'parseSambaAuditLog asigna source=samba_audit');
assertTrue(isset($sambaLogs[0]['action_label']) && isset($sambaLogs[0]['badge']), 'parseSambaAuditLog genera etiquetas amigables y badges');
$rmdirMatches = array_values(array_filter($sambaLogs, fn($l) => $l['action'] === 'rmdir'));
assertTrue(count($rmdirMatches) >= 1 && $rmdirMatches[0]['action_label'] === 'Eliminación de carpeta', 'parseSambaAuditLog parsea correctamente rmdir');

$sambaFiltered = $system->parseSambaAuditLog(50, 'Balance');
assertTrue(is_array($sambaFiltered) && count($sambaFiltered) >= 1, 'parseSambaAuditLog soporta filtrado por subcadena q');
$allMatchSamba = true;
foreach ($sambaFiltered as $sl) {
    if (!str_contains(strtolower(json_encode($sl)), 'balance')) {
        $allMatchSamba = false;
        break;
    }
}
assertTrue($allMatchSamba, 'Todos los resultados filtrados de Samba contienen el término buscado');

// 17. Pruebas de SystemService::parseAdminAuditLog y filtros
$adminLogs = $system->parseAdminAuditLog(50);
assertTrue(is_array($adminLogs) && count($adminLogs) > 0, 'SystemService::parseAdminAuditLog retorna eventos de administración');
assertTrue(isset($adminLogs[0]['source']) && $adminLogs[0]['source'] === 'admin', 'parseAdminAuditLog asigna source=admin');
assertTrue(isset($adminLogs[0]['action_label']) && isset($adminLogs[0]['status']), 'parseAdminAuditLog genera status y etiquetas');

$adminFiltered = $system->parseAdminAuditLog(50, 'PUBLICO');
assertTrue(is_array($adminFiltered) && count($adminFiltered) >= 1, 'parseAdminAuditLog soporta filtrado por query');

// 18. Pruebas de SystemService::parseBackupAuditLog y filtros
$backupLogs = $system->parseBackupAuditLog(50);
assertTrue(is_array($backupLogs) && count($backupLogs) > 0, 'SystemService::parseBackupAuditLog retorna bitácora unificada de respaldos');
assertTrue(isset($backupLogs[0]['source']) && $backupLogs[0]['source'] === 'backup', 'parseBackupAuditLog asigna source=backup');
assertTrue(isset($backupLogs[0]['event_label']) && isset($backupLogs[0]['severity']), 'parseBackupAuditLog clasifica severidad y evento');

$backupFiltered = $system->parseBackupAuditLog(50, 'CIFS');
assertTrue(is_array($backupFiltered) && count($backupFiltered) >= 1, 'parseBackupAuditLog soporta filtrado por término');

// 19. Pruebas de SystemService::getLogs (orígenes individuales y consolidación cronológica)
$allLogs = $system->getLogs('all', 10);
assertTrue(is_array($allLogs) && count($allLogs) <= 10, 'getLogs(all) consolida y respeta el límite solicitado');

$sambaOnly = $system->getLogs('samba_audit', 10);
$allSamba = true;
foreach ($sambaOnly as $l) {
    if ($l['source'] !== 'samba_audit') {
        $allSamba = false;
        break;
    }
}
assertTrue($allSamba && count($sambaOnly) > 0, 'getLogs(samba_audit) retorna únicamente registros Samba');

$adminOnly = $system->getLogs('admin', 10);
$allAdmin = true;
foreach ($adminOnly as $l) {
    if ($l['source'] !== 'admin') {
        $allAdmin = false;
        break;
    }
}
assertTrue($allAdmin && count($adminOnly) > 0, 'getLogs(admin) retorna únicamente registros administrativos');

$backupOnly = $system->getLogs('backup', 10);
$allBackup = true;
foreach ($backupOnly as $l) {
    if ($l['source'] !== 'backup') {
        $allBackup = false;
        break;
    }
}
assertTrue($allBackup && count($backupOnly) > 0, 'getLogs(backup) retorna únicamente registros de respaldos');

$systemOnly = $system->getLogs('system', 10);
$allSystem = true;
foreach ($systemOnly as $l) {
    if ($l['source'] !== 'system') {
        $allSystem = false;
        break;
    }
}
assertTrue($allSystem, 'getLogs(system) retorna únicamente registros del sistema');

// Verificación del orden cronológico descendente en consolidación
$isSortedDesc = true;
for ($i = 0; $i < count($allLogs) - 1; $i++) {
    if (strcmp($allLogs[$i]['timestamp'] ?? '', $allLogs[$i + 1]['timestamp'] ?? '') < 0) {
        $isSortedDesc = false;
        break;
    }
}
assertTrue($isSortedDesc, 'getLogs(all) ordena los registros consolidados en orden cronológico descendente');

// 20. Pruebas de DatabaseService (SQLite nativo)
$tempDbPath = sys_get_temp_dir() . '/test_nas_' . uniqid() . '.sqlite';
DatabaseService::setDbPath($tempDbPath);
$pdo = DatabaseService::getConnection();
assertTrue($pdo instanceof \PDO, 'DatabaseService::getConnection retorna instancia activa de PDO');

$tables = DatabaseService::query("SELECT name FROM sqlite_master WHERE type='table'");
$tableNames = array_column($tables, 'name');
assertTrue(in_array('audit_logs', $tableNames), 'DatabaseService crea tabla audit_logs');
assertTrue(in_array('terminal_history', $tableNames), 'DatabaseService crea tabla terminal_history');
assertTrue(in_array('system_settings', $tableNames), 'DatabaseService crea tabla system_settings');
assertTrue(in_array('domain_config', $tableNames), 'DatabaseService crea tabla domain_config');
assertTrue(in_array('trash_items', $tableNames), 'DatabaseService crea tabla trash_items');

$insId = DatabaseService::insert('system_settings', ['key' => 'test_k', 'value' => 'test_v']);
assertTrue($insId > 0, 'DatabaseService::insert inserta registro y retorna ID');
$setting = DatabaseService::query("SELECT value FROM system_settings WHERE key = ?", ['test_k']);
assertTrue(($setting[0]['value'] ?? '') === 'test_v', 'DatabaseService::query recupera valor insertado');
DatabaseService::setDbPath(null);
@unlink($tempDbPath);

// 21. Pruebas de TerminalService
$terminal = new TerminalService();
$termRes = $terminal->execute('echo "ANTIGRAVITY_TEST"', '/srv/nas');
assertTrue(isset($termRes['exit_code']) && $termRes['exit_code'] === 0, 'TerminalService ejecuta comando bash con código 0');
assertTrue(str_contains($termRes['output'], 'ANTIGRAVITY_TEST'), 'TerminalService retorna salida estándar del comando');
assertTrue(!empty($termRes['cwd']), 'TerminalService retorna directorio de trabajo actual');
assertTrue(isset($termRes['data']) && is_array($termRes['data']), 'TerminalService retorna estructura anidada data');

// Comando cd
$cdRes = $terminal->execute('cd /tmp', '/srv/nas');
assertTrue($cdRes['exit_code'] === 0, 'TerminalService soporta navegación con cd');
assertTrue(str_contains($cdRes['cwd'], 'tmp'), 'TerminalService actualiza directorio de trabajo en cd');

// Comando cd a ruta inexistente
$badCd = $terminal->execute('cd /ruta_inexistente_9999', '/srv/nas');
assertTrue($badCd['exit_code'] === 1 && str_contains($badCd['output'], 'bash: cd'), 'TerminalService maneja cd inexistente sin error fatal');

// Comando cd con comillas y rutas complejas
$quotedCd = $terminal->execute('cd "/tmp"', '/srv/nas');
assertTrue($quotedCd['exit_code'] === 0 && str_contains($quotedCd['cwd'], 'tmp'), 'TerminalService soporta cd con comillas dobles');

$singleQuotedCd = $terminal->execute("cd '/tmp'", '/srv/nas');
assertTrue($singleQuotedCd['exit_code'] === 0 && str_contains($singleQuotedCd['cwd'], 'tmp'), 'TerminalService soporta cd con comillas simples');

$tildeCd = $terminal->execute('cd ~', '/tmp');
assertTrue($tildeCd['exit_code'] === 0 && str_contains($tildeCd['cwd'], 'nas'), 'TerminalService soporta expansion de tilde (~)');

$dashCd = $terminal->execute('cd -', '/srv/nas');
assertTrue($dashCd['exit_code'] === 0 && str_contains($dashCd['cwd'], 'tmp'), 'TerminalService soporta retorno con cd -');

// Comando exit/logout nativo amigable
$exitRes = $terminal->execute('exit', '/srv/nas');
assertTrue($exitRes['success'] && str_contains($exitRes['output'], 'consola web de administración integrada'), 'TerminalService maneja exit con mensaje amigable');

// Detección de comandos interactivos incompatibles (su, sudo -i, sudo bash, nano, top, watch, tail -f)
$suWarn = $terminal->execute('su -', '/srv/nas');
assertTrue($suWarn['success'] && str_contains($suWarn['output'], 'Aviso de terminal interactiva'), 'TerminalService detecta su - y sugiere alternativas sin error fatal');

$sudoBashWarn = $terminal->execute('sudo bash', '/srv/nas');
assertTrue($sudoBashWarn['success'] && str_contains($sudoBashWarn['output'], 'Aviso de terminal interactiva'), 'TerminalService detecta sudo bash y previene bloqueo');

$watchWarn = $terminal->execute('watch df -h', '/srv/nas');
assertTrue($watchWarn['success'] && str_contains($watchWarn['output'], 'watch'), 'TerminalService detecta comando watch y evita bucle infinito');

$tailfWarn = $terminal->execute('tail -f /var/log/syslog', '/srv/nas');
assertTrue($tailfWarn['success'] && str_contains($tailfWarn['output'], 'tail -f'), 'TerminalService detecta tail -f y evita bloqueo indefinido');

$nanoWarn = $terminal->execute('nano /etc/samba/smb.conf', '/srv/nas');
assertTrue($nanoWarn['success'] && str_contains($nanoWarn['output'], 'editor interactivo'), 'TerminalService detecta nano y previene cuelgues');

$topWarn = $terminal->execute('top', '/srv/nas');
assertTrue($topWarn['success'] && str_contains($topWarn['output'], 'monitor interactivo'), 'TerminalService detecta top y sugiere ps o dashboard');

// Comando help nativo
$helpRes = $terminal->execute('help', '/srv/nas');
assertTrue($helpRes['success'] && str_contains($helpRes['output'], 'Comandos rápidos del sistema'), 'TerminalService provee guía de comandos con help');

// Comando ll con función integrada
$llRes = $terminal->execute('ll', '/srv/nas');
assertTrue($llRes['success'] && $llRes['exit_code'] === 0, 'TerminalService soporta alias/función ll de listar archivos');

// Historial en base de datos
$history = $terminal->getHistory(5);
assertTrue(is_array($history), 'TerminalService::getHistory retorna historial estructurado');

// 22. Pruebas de FileExplorerService
$fileExp = new FileExplorerService();

// Verificación de protección Jail Traversal y aislamiento de .trash
$safePath = $fileExp->resolveSafePath('../../../etc/passwd');
assertTrue($safePath === null || !str_contains($safePath, 'etc/passwd'), 'FileExplorerService::resolveSafePath bloquea escape de directorio (Path Traversal)');

$trashSafe = $fileExp->resolveSafePath('.trash');
assertTrue($trashSafe === null, 'FileExplorerService::resolveSafePath bloquea acceso directo a .trash');

$trashSubSafe = $fileExp->resolveSafePath('.trash/secreto.txt');
assertTrue($trashSubSafe === null, 'FileExplorerService::resolveSafePath bloquea acceso a subrutas dentro de .trash');

// Directorio y BD temporal para pruebas completas de explorador y papelera
$tempExpDir = sys_get_temp_dir() . '/nas_test_explorer_' . uniqid();
@mkdir($tempExpDir, 0770, true);
file_put_contents($tempExpDir . '/documento.txt', 'Contenido confidencial');

FileExplorerService::setRootDir($tempExpDir);
$tempExpDb = sys_get_temp_dir() . '/nas_test_exp_db_' . uniqid() . '.sqlite';
DatabaseService::setDbPath($tempExpDb);

// Aislamiento: listDirectory debe excluir .trash
@mkdir($tempExpDir . '/.trash', 0770, true);
file_put_contents($tempExpDir . '/.trash/oculto.txt', 'no ver');

$listRes = $fileExp->listDirectory('');
assertTrue($listRes['success'], 'FileExplorerService::listDirectory retorna éxito en directorio válido');
assertTrue(is_array($listRes['items']) && count($listRes['items']) >= 1, 'FileExplorerService lista archivos existentes');
assertTrue(($listRes['items'][0]['name'] ?? '') === 'documento.txt', 'FileExplorerService detecta nombre de archivo');
assertTrue(isset($listRes['data']) && is_array($listRes['data']['items']), 'FileExplorerService retorna bloque anidado data');
assertTrue(isset($listRes['items'][0]['relative_path']) && isset($listRes['items'][0]['modified_at']), 'FileExplorerService provee relative_path y modified_at');
assertTrue(isset($listRes['items'][0]['owner']) && isset($listRes['items'][0]['group']), 'FileExplorerService provee propietario y grupo POSIX');

$hasTrashInList = false;
foreach ($listRes['items'] as $it) {
    if ($it['name'] === '.trash') {
        $hasTrashInList = true;
        break;
    }
}
assertTrue(!$hasTrashInList, 'FileExplorerService::listDirectory excluye carpeta reservada .trash');

// Crear subcarpeta
$mkdirRes = $fileExp->createDirectory('', 'Subcarpeta_Test');
assertTrue($mkdirRes['success'] && is_dir($tempExpDir . '/Subcarpeta_Test'), 'FileExplorerService::createDirectory crea subdirectorio');

// Renombrar archivo
$renameRes = $fileExp->renameItem('documento.txt', 'documento_renombrado.txt');
assertTrue($renameRes['success'] && file_exists($tempExpDir . '/documento_renombrado.txt'), 'FileExplorerService::renameItem renombra elemento');

// Mover a papelera y consultar
file_put_contents($tempExpDir . '/archivo_papelera.txt', "Linea 1\nLinea 2\nLinea 3");
$trashMoveRes = $fileExp->moveToTrash('archivo_papelera.txt', 'operador_test');
assertTrue($trashMoveRes['success'] && !file_exists($tempExpDir . '/archivo_papelera.txt'), 'FileExplorerService::moveToTrash mueve archivo a la papelera');
assertTrue($fileExp->getTrashCount() >= 1, 'FileExplorerService::getTrashCount detecta elementos en papelera');

$trashList = $fileExp->listTrash();
assertTrue($trashList['success'] && count($trashList['items']) >= 1, 'FileExplorerService::listTrash retorna listado de elementos');
$foundItem = null;
foreach ($trashList['items'] as $ti) {
    if ($ti['filename'] === 'archivo_papelera.txt') {
        $foundItem = $ti;
        break;
    }
}
assertTrue($foundItem !== null && $foundItem['deleted_by'] === 'operador_test', 'listTrash registra metadatos de usuario y nombre original');

// Restaurar desde papelera
$restoreRes = $fileExp->restoreTrashItem($foundItem['id']);
assertTrue($restoreRes['success'], 'FileExplorerService::restoreTrashItem restaura archivo desde papelera');
assertTrue(file_exists($tempExpDir . '/archivo_papelera.txt'), 'Archivo restaurado existe nuevamente en la ruta original');
assertTrue(file_get_contents($tempExpDir . '/archivo_papelera.txt') === "Linea 1\nLinea 2\nLinea 3", 'Contenido de archivo restaurado es íntegro');

// Eliminar definitivamente de papelera
$fileExp->moveToTrash('archivo_papelera.txt', 'operador_test');
$trashListAfter = $fileExp->listTrash();
$itemToDelete = $trashListAfter['items'][0];
$delTrashRes = $fileExp->deleteTrashItem($itemToDelete['id']);
assertTrue($delTrashRes['success'], 'FileExplorerService::deleteTrashItem elimina definitivamente de papelera');
assertTrue($fileExp->getTrashCount() === 0, 'Contador de papelera se reduce tras eliminación');

// Vaciar papelera con múltiples elementos
file_put_contents($tempExpDir . '/item1.txt', 'dato1');
file_put_contents($tempExpDir . '/item2.txt', 'dato2');
$fileExp->moveToTrash('item1.txt', 'admin');
$fileExp->moveToTrash('item2.txt', 'admin');
assertTrue($fileExp->getTrashCount() === 2, 'Papelera contiene dos elementos previo a vaciado');
$emptyRes = $fileExp->emptyTrash();
assertTrue($emptyRes['success'] && $emptyRes['deleted_count'] >= 2, 'FileExplorerService::emptyTrash vacía papelera por completo');
assertTrue($fileExp->getTrashCount() === 0, 'getTrashCount retorna 0 tras vaciar papelera');

// Eliminación permanente directa con deleteItem(..., permanent: true)
file_put_contents($tempExpDir . '/archivo_directo.txt', 'inmediato');
$delDirect = $fileExp->deleteItem('archivo_directo.txt', true);
assertTrue($delDirect['success'] && !file_exists($tempExpDir . '/archivo_directo.txt'), 'FileExplorerService::deleteItem con permanent=true elimina sin enviar a papelera');
assertTrue($fileExp->getTrashCount() === 0, 'Eliminación permanente no altera la papelera');

// Previsualizador de archivos: getFileContent
file_put_contents($tempExpDir . '/codigo.php', "<?php\n// Test preview\necho 'Hola';\n");
$contentRes = $fileExp->getFileContent('codigo.php');
assertTrue($contentRes['success'], 'FileExplorerService::getFileContent lee archivo de texto');
assertTrue($contentRes['lines_count'] === 3 && $contentRes['extension'] === 'php', 'getFileContent calcula líneas y extensión');
assertTrue(str_contains($contentRes['content'], "echo 'Hola';"), 'getFileContent devuelve el contenido exacto');

// Detección de binarios
file_put_contents($tempExpDir . '/binario.dat', "\x7F\x45\x4C\x46\x00\x00\x00");
$binRes = $fileExp->getFileContent('binario.dat');
assertTrue(!$binRes['success'] && ($binRes['is_binary'] ?? false) === true, 'getFileContent detecta archivo binario y rechaza previsualización plana');

// Límite de tamaño en previsualización
$limitRes = $fileExp->getFileContent('codigo.php', 5);
assertTrue(!$limitRes['success'] && str_contains($limitRes['error'], 'límite de previsualización'), 'getFileContent rechaza archivos que exceden tamaño máximo');

// Limpieza de temporales de FileExplorer
@rmdir($tempExpDir . '/Subcarpeta_Test');
@unlink($tempExpDir . '/documento_renombrado.txt');
@unlink($tempExpDir . '/codigo.php');
@unlink($tempExpDir . '/binario.dat');
@unlink($tempExpDir . '/.trash/oculto.txt');
@rmdir($tempExpDir . '/.trash');
@rmdir($tempExpDir);
FileExplorerService::setRootDir(null);
DatabaseService::setDbPath(null);
@unlink($tempExpDb);

// 23. Pruebas de DomainService
$domain = new DomainService();
$domainStatus = $domain->getStatus();
assertTrue(isset($domainStatus['joined']), 'DomainService::getStatus retorna estado joined');
assertTrue(isset($domainStatus['workgroup']), 'DomainService::getStatus retorna grupo de trabajo');

$invalidDisc = $domain->discover('!!dominio_invalido!!');
assertTrue(!$invalidDisc['success'], 'DomainService::discover rechaza nombres de dominio con formato inválido');

// 24. Pruebas de Certificación Fase 1 y 2

// 24.1 AuthService: Suprimir contraseñas mock en producción
putenv('APP_ENV=production');
$authProd = new AuthService();
$authProd->dryRun = false;
$prodRes = $authProd->authenticate('administrador', 'Admin123#');
assertTrue(!$prodRes['success'], 'AuthService suprime contraseñas mock en producción');
putenv('APP_ENV=testing');

// 24.2 AuthService: Rate limiting con SQLite tras 5 intentos fallidos
$rateAuth = new AuthService();
$testRateIp = '198.51.100.42';
$rateAuth->clearFailedAttempts($testRateIp);
assertTrue(!$rateAuth->isRateLimited($testRateIp), 'AuthService: IP sin intentos no está bloqueada');
for ($i = 0; $i < 5; $i++) {
    $rateAuth->recordFailedAttempt($testRateIp, 'admin_prueba');
}
assertTrue($rateAuth->isRateLimited($testRateIp), 'AuthService bloquea IP tras 5 intentos fallidos');
$rateAuth->clearFailedAttempts($testRateIp);
assertTrue(!$rateAuth->isRateLimited($testRateIp), 'AuthService desbloquea IP tras limpieza de intentos');

// 24.3 AuthMiddleware: Generación y Verificación de Token CSRF
$csrfToken = AuthMiddleware::getCsrfToken();
assertTrue(is_string($csrfToken) && strlen($csrfToken) === 64, 'AuthMiddleware genera token CSRF de 64 caracteres hex');
assertTrue(AuthMiddleware::verifyCsrfToken($csrfToken), 'AuthMiddleware valida token CSRF idéntico');
assertTrue(!AuthMiddleware::verifyCsrfToken('invalido_token_123'), 'AuthMiddleware rechaza token CSRF inválido');

// 24.4 AuthMiddleware: Bloqueo de peticiones mutantes sin CSRF y admisión con CSRF
$_SESSION['nas_user'] = ['username' => 'sistemas', 'is_admin' => true];
$_SERVER['REQUEST_METHOD'] = 'POST';
$_SERVER['REQUEST_URI'] = '/api/shares';
unset($_SERVER['HTTP_X_CSRF_TOKEN'], $_SERVER['HTTP_X_XSRF_TOKEN']);
$_POST = [];
$mutantNoCsrf = new Request();
ob_start();
$resNoCsrf = AuthMiddleware::check($mutantNoCsrf);
ob_end_clean();
assertTrue(!$resNoCsrf, 'AuthMiddleware bloquea petición mutante POST sin token CSRF');

$_SERVER['HTTP_X_CSRF_TOKEN'] = $csrfToken;
$mutantWithCsrf = new Request();
assertTrue(AuthMiddleware::check($mutantWithCsrf), 'AuthMiddleware autoriza petición mutante POST con token CSRF en cabecera');
unset($_SERVER['HTTP_X_CSRF_TOKEN']);

// 24.5 BackupService & BackupController: Blindaje contra Path Traversal
$bkpService = new BackupService();
$ptDelete = $bkpService->deleteTask('../../etc/cron.d/malicioso');
assertTrue(!$ptDelete['success'], 'BackupService::deleteTask rechaza identificador con Path Traversal');
$ptDeleteOk = $bkpService->deleteTask('tarea_valida_99');
assertTrue($ptDeleteOk['success'], 'BackupService::deleteTask acepta identificador alfanumérico válido');

$bkpCtrl = new \App\Controllers\BackupController();
$ptReq = new Request();
ob_start();
$bkpCtrl->delete($ptReq, ['id' => '../../../etc/shadow']);
$bkpDelOut = ob_get_clean();
assertTrue(str_contains($bkpDelOut, 'Identificador de tarea inválido'), 'BackupController::delete valida formato y rechaza traversal');

// 24.6 UserService: Prohibir eliminación de cuentas protegidas y auto-eliminación
$uServ = new UserService();
$delSistemas = $uServ->deleteUser('sistemas');
assertTrue(!$delSistemas['success'], 'UserService rechaza eliminar cuenta de sistema protegida sistemas');
$delWwwData = $uServ->deleteUser('www-data');
assertTrue(!$delWwwData['success'], 'UserService rechaza eliminar cuenta de sistema protegida www-data');

$_SESSION['nas_user'] = ['username' => 'operador_demo', 'is_admin' => true];
$delSelf = $uServ->deleteUser('operador_demo');
assertTrue(!$delSelf['success'], 'UserService rechaza auto-eliminación de la cuenta de usuario activa');
unset($_SESSION['nas_user']);

// 24.7 FileExplorerService: Mitigación de Stored XSS en SVG y transmisión segura
$fService = new FileExplorerService();
$tempSvgDir = sys_get_temp_dir() . '/nas_svg_test_' . uniqid();
@mkdir($tempSvgDir, 0777, true);
file_put_contents($tempSvgDir . '/vector.svg', '<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>');
FileExplorerService::setRootDir($tempSvgDir);
assertTrue($fService->detectFileType('vector.svg', false) === 'image', 'FileExplorerService detecta archivos SVG como imagen');

ob_start();
$fService->streamRawFile('vector.svg');
$rawSvgOut = ob_get_clean();
assertTrue(str_contains($rawSvgOut, '<svg'), 'FileExplorerService::streamRawFile transmite contenido SVG sin abortar proceso en testing');

// 24.8 FileExplorerService: Confinamiento canónico en descarga ZIP (exclusión de symlinks externos)
$zipSubDir = $tempSvgDir . '/carpeta_export';
@mkdir($zipSubDir, 0777, true);
file_put_contents($zipSubDir . '/seguro.txt', 'datos_seguros_123');
$outsideSecret = sys_get_temp_dir() . '/outside_leak_' . uniqid() . '.txt';
file_put_contents($outsideSecret, 'secreto_fuera_de_raiz');
@symlink($outsideSecret, $zipSubDir . '/symlink_leak.txt');

ob_start();
$fService->downloadDirectoryZip('carpeta_export');
$zipStreamOut = ob_get_clean();
assertTrue(!str_contains($zipStreamOut, 'secreto_fuera_de_raiz'), 'FileExplorerService::downloadDirectoryZip confina archivos y excluye symlinks externos');

@unlink($zipSubDir . '/symlink_leak.txt');
@unlink($zipSubDir . '/seguro.txt');
@rmdir($zipSubDir);
@rmdir($tempSvgDir . '/.tmp_zip');
@unlink($tempSvgDir . '/vector.svg');
@rmdir($tempSvgDir);
@unlink($outsideSecret);
FileExplorerService::setRootDir(null);

// 24.8.1 FileExplorerService: Soporte y transmisión de documentos de ofimática (Opción 3)
$tempOfficeDir = sys_get_temp_dir() . '/nas_office_test_' . uniqid();
@mkdir($tempOfficeDir, 0777, true);
file_put_contents($tempOfficeDir . '/balance.xlsx', "PK\x03\x04mock_excel_bytes");
file_put_contents($tempOfficeDir . '/informe.docx', "PK\x03\x04mock_word_bytes");
file_put_contents($tempOfficeDir . '/slides.pptx', "PK\x03\x04mock_pptx_bytes");
FileExplorerService::setRootDir($tempOfficeDir);

assertTrue($fService->detectFileType('balance.xlsx', false) === 'document', 'FileExplorerService detecta archivos XLSX como document');
assertTrue($fService->detectFileType('informe.docx', false) === 'document', 'FileExplorerService detecta archivos DOCX como document');
assertTrue($fService->detectFileType('slides.pptx', false) === 'document', 'FileExplorerService detecta archivos PPTX como document');

ob_start();
$fService->streamRawFile('balance.xlsx');
$rawXlsxOut = ob_get_clean();
assertTrue(str_contains($rawXlsxOut, 'mock_excel_bytes'), 'FileExplorerService::streamRawFile transmite contenido binario XLSX');

ob_start();
$fService->streamRawFile('informe.docx');
$rawDocxOut = ob_get_clean();
assertTrue(str_contains($rawDocxOut, 'mock_word_bytes'), 'FileExplorerService::streamRawFile transmite contenido binario DOCX');

@unlink($tempOfficeDir . '/balance.xlsx');
@unlink($tempOfficeDir . '/informe.docx');
@unlink($tempOfficeDir . '/slides.pptx');
@rmdir($tempOfficeDir);
FileExplorerService::setRootDir(null);

// 24.8.2 Verificación de librerías locales de ofimática 100% offline
$vendorDir = __DIR__ . '/../web/public/js/vendor';
assertTrue(file_exists($vendorDir . '/xlsx.full.min.js') && filesize($vendorDir . '/xlsx.full.min.js') > 100000, 'Librería local SheetJS xlsx.full.min.js disponible para visualización offline');
assertTrue(file_exists($vendorDir . '/jszip.min.js') && filesize($vendorDir . '/jszip.min.js') > 50000, 'Librería local JSZip jszip.min.js disponible para visualización offline');
assertTrue(file_exists($vendorDir . '/docx-preview.min.js') && filesize($vendorDir . '/docx-preview.min.js') > 40000, 'Librería local docx-preview docx-preview.min.js disponible para visualización offline');

// 24.9 AuthController: Rate limiting con HTTP 429 tras 5 intentos fallidos
$authCtrl = new AuthController();
$rateIp = '198.51.100.88';
$rateAuth = new AuthService();
$rateAuth->clearFailedAttempts($rateIp);
for ($i = 0; $i < 5; $i++) {
    $rateAuth->recordFailedAttempt($rateIp, 'intruso');
}
$_SERVER['REMOTE_ADDR'] = $rateIp;
$_SERVER['REQUEST_METHOD'] = 'POST';
$_SERVER['CONTENT_TYPE'] = 'application/json';
$_SERVER['HTTP_ACCEPT'] = 'application/json';
$_POST = [];
$rateReq = new Request();
ob_start();
$authCtrl->login($rateReq);
$rateRespOut = ob_get_clean();
$rateRespJson = json_decode($rateRespOut, true);
assertTrue(isset($rateRespJson['success']) && $rateRespJson['success'] === false && str_contains($rateRespJson['error'] ?? '', 'Demasiados intentos'), 'AuthController::login responde 429 por rate limiting tras 5 intentos');
$rateAuth->clearFailedAttempts($rateIp);

// 24.10 Router & Logout: Requiere estrictamente POST y rechaza GET con 404
$routerTest = new Router();
$routerTest->post('/logout', [AuthController::class, 'logout']);
$_SERVER['REQUEST_METHOD'] = 'GET';
$_SERVER['REQUEST_URI'] = '/logout';
$_SERVER['HTTP_ACCEPT'] = 'application/json';
$getLogoutReq = new Request();
ob_start();
$routerTest->dispatch($getLogoutReq);
$getLogoutOut = ob_get_clean();
$getLogoutJson = json_decode($getLogoutOut, true);
assertTrue(isset($getLogoutJson['success']) && $getLogoutJson['success'] === false && str_contains($getLogoutJson['error'] ?? '', 'Ruta no encontrada'), 'Router rechaza método GET para /logout (HTTP 404)');

// 24.11 SystemService: Consulta de versión e información de commit
$sysServ = new SystemService();
$updStatus = $sysServ->checkUpdates();
assertTrue(isset($updStatus['version']) && isset($updStatus['installed_commit']), 'SystemService::checkUpdates reporta campos de versión y commit');

// 25. Roles, superadministrador, grupos reservados, ACL y clasificación de backups
$authRoles = new AuthService();
$loginAdminRole = $authRoles->authenticate('administrador', 'Admin123#');
assertTrue(($loginAdminRole['user']['role'] ?? '') === 'superadmin' && !empty($loginAdminRole['user']['is_superadmin']),
    'AuthService otorga rol superadmin a la cuenta administrador (modo test)');
$loginSistRole = $authRoles->authenticate('sistemas', 'Ead2026#');
assertTrue(($loginSistRole['user']['role'] ?? '') === 'admin' && empty($loginSistRole['user']['is_superadmin']),
    'AuthService otorga rol admin a la cuenta sistemas (modo test)');

$createSuper = $user->createUser('nuevo_super', 'clave12345', 'Nuevo Super', [], false, true, true, true);
assertTrue($createSuper['success'], 'UserService::createUser acepta el flag is_superadmin (modo test)');

$reservedGroup = $user->createGroup('grp_superadmin');
assertTrue(!$reservedGroup['success'], 'UserService rechaza crear el grupo reservado grp_superadmin');
$renameToReserved = $user->renameGroup('grp_marketing', 'grp_superadmin');
assertTrue(!$renameToReserved['success'], 'UserService rechaza renombrar un grupo a grp_superadmin');
$addToSuper = $user->addUserToGroup('usuario_test', 'grp_superadmin');
assertTrue(!$addToSuper['success'], 'UserService rechaza añadir miembros al grupo especial grp_superadmin');
$delSuperGroup = $user->deleteGroup('grp_superadmin');
assertTrue(!$delSuperGroup['success'], 'UserService rechaza eliminar el grupo especial grp_superadmin');

$sambaAcl = new SambaService(sys_get_temp_dir() . '/nas_smb_inexistente_' . bin2hex(random_bytes(3)) . '.conf');
$syncMissing = $sambaAcl->syncShareAcls('NO_EXISTE');
assertTrue(!$syncMissing['success'], 'SambaService::syncShareAcls rechaza recursos inexistentes');

$bkClass = new BackupService();
$logFalloPrevio = "[2026-10-08 07:00:00] [t] [MOUNT_FAILED] [err] error previo\n"
    . "[2026-10-08 10:00:00] [t] [LOCK_ACQUIRED] [info] lock\n"
    . "[2026-10-08 10:00:05] [t] [BACKUP_COMPLETED] [notice] ok\n";
assertTrue($bkClass->classifyLatestRun($logFalloPrevio) === 'ok',
    'BackupService::classifyLatestRun ignora fallos de ejecuciones anteriores');
$logFalloFinal = "[2026-10-08 10:00:00] [t] [BACKUP_COMPLETED] [notice] ok\n"
    . "[2026-10-08 11:00:00] [t] [LOCK_ACQUIRED] [info] lock\n"
    . "[2026-10-08 11:00:05] [t] [RSYNC_FAILED] [err] fallo\n";
assertTrue($bkClass->classifyLatestRun($logFalloFinal) === 'error',
    'BackupService::classifyLatestRun reporta error solo si falla la última ejecución');

$disksInfo = $storage->getDisks();
$disksOk = is_array($disksInfo) && (empty($disksInfo) || (isset($disksInfo[0]['protected']) && isset($disksInfo[0]['in_use'])));
assertTrue($disksOk, 'StorageService::getDisks expone los campos protected e in_use');
$fmtBad = $storage->formatAndMount('/dev/sdz', 'ext4', 'NO');
assertTrue(!$fmtBad['success'], 'StorageService::formatAndMount exige la confirmación SI-FORMATEAR');
$unmountDev = $storage->unmountDevice('/dev/sdb');
assertTrue($unmountDev === true, 'StorageService::unmountDevice ejecuta desmontaje pasando ruta de dispositivo');
$unmountMount = $storage->unmountDevice('/mnt/secundario');
assertTrue($unmountMount === true, 'StorageService::unmountDevice ejecuta desmontaje pasando ruta de punto de montaje');
$unmountEmpty = $storage->unmountDevice('');
assertTrue($unmountEmpty === false, 'StorageService::unmountDevice rechaza rutas vacías');
$hasMountEmpty = $storage->deviceHasMount('');
assertTrue($hasMountEmpty === false, 'StorageService::deviceHasMount retorna false para rutas vacías');

// 26. Pruebas de Roles de Servidor, Restricciones de Endpoints y Rate Limiting
putenv('NAS_SERVER_ROLE=ARCHIVOS');
assertTrue(SystemService::getServerRole() === 'ARCHIVOS', 'SystemService::getServerRole retorna ARCHIVOS según variable');
assertTrue(SystemService::getServerRoleLabel() === 'ARCHIVOS', 'SystemService::getServerRoleLabel retorna etiqueta ARCHIVOS');

putenv('NAS_SERVER_ROLE=BACKUP');
assertTrue(SystemService::getServerRole() === 'BACKUP', 'SystemService::getServerRole retorna BACKUP');
assertTrue(SystemService::getServerRoleLabel() === 'BACKUP', 'SystemService::getServerRoleLabel retorna etiqueta BACKUP');

putenv('NAS_SERVER_ROLE=ARCHIVOS_BACKUP');
assertTrue(SystemService::getServerRole() === 'ARCHIVOS_BACKUP', 'SystemService::getServerRole retorna ARCHIVOS_BACKUP');
assertTrue(SystemService::getServerRoleLabel() === 'ARCHIVOS & BACKUP', 'SystemService::getServerRoleLabel retorna etiqueta ARCHIVOS & BACKUP');

// AuthMiddleware y Server Role
putenv('NAS_SERVER_ROLE=ARCHIVOS');
assertTrue(!AuthMiddleware::isServerRoleAllowed('/backups'), 'AuthMiddleware bloquea /backups en rol ARCHIVOS');
assertTrue(!AuthMiddleware::isServerRoleAllowed('/api/backups/task1'), 'AuthMiddleware bloquea /api/backups en rol ARCHIVOS');
assertTrue(AuthMiddleware::isServerRoleAllowed('/shares'), 'AuthMiddleware permite /shares en rol ARCHIVOS');

putenv('NAS_SERVER_ROLE=ARCHIVOS_BACKUP');
assertTrue(AuthMiddleware::isServerRoleAllowed('/backups'), 'AuthMiddleware permite /backups en rol ARCHIVOS_BACKUP');
assertTrue(AuthMiddleware::isServerRoleAllowed('/shares'), 'AuthMiddleware permite /shares en rol ARCHIVOS_BACKUP');

// SambaController y restricción en rol BACKUP
putenv('NAS_SERVER_ROLE=BACKUP');
$_SESSION['nas_user'] = ['username' => 'sistemas', 'is_admin' => true, 'role' => 'admin'];
$sambaCtrl = new \App\Controllers\SambaController();
unset($_SERVER['CONTENT_TYPE']);
$_SERVER['REQUEST_METHOD'] = 'POST';
$_POST = ['name' => 'PUBLICO', 'scheme' => 4];
$reqPublic = new Request();
ob_start();
$sambaCtrl->create($reqPublic);
$respPublic = ob_get_clean();
$jsonPub = json_decode($respPublic, true);
assertTrue(isset($jsonPub['error']) && str_contains($jsonPub['error'], 'ocultos'), 'SambaController rechaza recursos públicos en rol BACKUP');

$_POST = ['name' => 'BACKUP_DEV', 'scheme' => 1, 'hidden' => true];
$reqHidden = new Request();
ob_start();
$sambaCtrl->create($reqHidden);
$respHidden = ob_get_clean();
$jsonHidden = json_decode($respHidden, true);
assertTrue(isset($jsonHidden['success']) && $jsonHidden['success'] === true, 'SambaController acepta recursos ocultos en rol BACKUP');

// Rate limiting SQLite
$testIp = '127.0.0.99';
$testKey = 'rate_crit:' . $testIp . ':/api/storage/format';
AuthService::clearRateLimits($testKey);
$limitOk = true;
for ($i = 0; $i < 10; $i++) {
    if (!AuthService::checkRateLimit($testKey, 10, 60)) {
        $limitOk = false;
        break;
    }
}
assertTrue($limitOk, 'AuthService::checkRateLimit permite hasta el umbral de solicitudes');
$limitBlocked = !AuthService::checkRateLimit($testKey, 10, 60);
assertTrue($limitBlocked, 'AuthService::checkRateLimit bloquea la solicitud que excede el umbral');

AuthService::clearRateLimits($testKey);
$limitReset = AuthService::checkRateLimit($testKey, 10, 60);
assertTrue($limitReset, 'AuthService::clearRateLimits restablece la ventana de rate limiting');

// 27. Pruebas de Request::getIp, AuthMiddleware rate limit crítico y validación de LVM
$_SERVER['REMOTE_ADDR'] = '192.168.10.25';
$reqAlias = new Request();
assertTrue($reqAlias->getIp() === '192.168.10.25', 'Request::getIp retorna la IP resuelta correctamente');
assertTrue($reqAlias->getIp() === $reqAlias->getClientIp(), 'Request::getIp es idéntico a Request::getClientIp');

// Comprobación de que AuthMiddleware procesa endpoints críticos con rate limiter sin excepciones
$_SERVER['REQUEST_METHOD'] = 'POST';
$_SERVER['REQUEST_URI'] = '/api/storage/lvm';
$_SERVER['HTTP_ACCEPT'] = 'application/json';
$reqCritLvm = new Request();
try {
    $refMethod = new ReflectionMethod(AuthMiddleware::class, 'checkCriticalRateLimit');
    $refMethod->setAccessible(true);
    $resCrit = $refMethod->invoke(null, $reqCritLvm);
    assertTrue($resCrit === true, 'AuthMiddleware::checkCriticalRateLimit autoriza petición LVM dentro de umbral');
} catch (\Throwable $e) {
    assertTrue(false, 'AuthMiddleware::checkCriticalRateLimit no debe lanzar excepciones: ' . $e->getMessage());
}

// Pruebas de validación de StorageService::lvmCreate
$badConfirmLvm = $storage->lvmCreate('/dev/sdb', 'vg_nas', 'lv_data', '100%FREE', 'ext4', 'NO');
assertTrue(!$badConfirmLvm['success'] && str_contains($badConfirmLvm['error'], 'Confirmación requerida'), 'StorageService::lvmCreate exige SI-FORMATEAR');

$badSizeLvm = $storage->lvmCreate('/dev/sdb', 'vg_nas', 'lv_data', 'tamano_invalido', 'ext4', 'SI-FORMATEAR');
assertTrue(!$badSizeLvm['success'] && str_contains($badSizeLvm['error'], 'inválido'), 'StorageService::lvmCreate rechaza tamaño inválido');

$goodSizeFree = $storage->lvmCreate('/dev/sdb', 'vg_nas', 'lv_data', '100%FREE', 'ext4', 'SI-FORMATEAR');
assertTrue($goodSizeFree['success'], 'StorageService::lvmCreate acepta 100%FREE (modo dev/test)');

$goodSizeVg = $storage->lvmCreate('/dev/sdb', 'vg_nas', 'lv_data', '50%VG', 'ext4', 'SI-FORMATEAR');
assertTrue($goodSizeVg['success'], 'StorageService::lvmCreate acepta 50%VG (modo dev/test)');

$goodSizeFixed = $storage->lvmCreate('/dev/sdb', 'vg_nas', 'lv_data', '200G', 'ext4', 'SI-FORMATEAR');
assertTrue($goodSizeFixed['success'], 'StorageService::lvmCreate acepta tamaño fijo 200G (modo dev/test)');

// Verificación de integridad en web/public/js/app.js (Cero variable shadowing en diálogos del navegador)
$appJsContent = file_get_contents(__DIR__ . '/../web/public/js/app.js');
assertTrue(!preg_match('/\b(const|let|var)\s+(confirm|alert|prompt)\b/', $appJsContent), 'app.js no declara variables locales confirm/alert/prompt que rompan APIs globales');
assertTrue(!preg_match('/(?<!\.)\b(confirm|prompt|alert)\s*\(/', $appJsContent), 'app.js invoca confirm/prompt/alert exclusivamente mediante window.');
assertTrue(str_contains($appJsContent, 'confirmText'), 'submitStorageManage utiliza confirmText para evitar enmascarar confirm');

// Verificación de resolución de usuario y contexto en modo CLI para auditoría
AuditService::setCurrentUser('operador_test');
assertTrue(AuditService::getCurrentUser() === 'operador_test', 'AuditService respeta override explícito de usuario');
AuditService::setCurrentUser(null);

$savedUser = $_SESSION['nas_user'] ?? null;
unset($_SESSION['nas_user']);
putenv('SUDO_USER=sistemas_sudo');
assertTrue(AuditService::getCurrentUser() === 'sistemas_sudo', 'AuditService resuelve SUDO_USER en entorno CLI');
putenv('SUDO_USER');
if ($savedUser !== null) {
    $_SESSION['nas_user'] = $savedUser;
}

// Pruebas de remediaciones de seguridad, almacenamiento y servicios (Fases 1, 2 y 3)

// 1. SambaService: rechazo de nombres reservados en creación y actualización
$sambaSec = new SambaService('/nonexistent/smb.conf');
foreach (['global', 'printers', 'print$'] as $reservedName) {
    $resCreate = $sambaSec->createShare(['name' => $reservedName, 'path' => '/srv/nas/' . $reservedName]);
    assertTrue(!$resCreate['success'] && (str_contains($resCreate['error'], 'reservado') || str_contains($resCreate['error'], 'protegida')), "SambaService::createShare rechaza nombre reservado '$reservedName'");

    $resUpdate = $sambaSec->updateShare($reservedName, ['comment' => 'test']);
    assertTrue(!$resUpdate['success'] && (str_contains($resUpdate['error'], 'reservado') || str_contains($resUpdate['error'], 'protegida')), "SambaService::updateShare rechaza seccion reservada '$reservedName'");
}

// 2. SambaService::setAccess normaliza 'ro' y 'rw' a 'read' y 'write'
$resRo = $sambaSec->setAccess('RECURSO_INEXISTENTE', 'group', 'grp_ventas', 'ro');
assertTrue($resRo['error'] !== 'Nivel de acceso inválido (use read/write/none o ro/rw).' && str_contains($resRo['error'], 'no existe'), "SambaService::setAccess acepta y normaliza 'ro'");
$resRw = $sambaSec->setAccess('RECURSO_INEXISTENTE', 'group', 'grp_ventas', 'rw');
assertTrue($resRw['error'] !== 'Nivel de acceso inválido (use read/write/none o ro/rw).' && str_contains($resRw['error'], 'no existe'), "SambaService::setAccess acepta y normaliza 'rw'");

// 3. StorageService: listado y creacion de subvolumenes Btrfs
$btrfsList = $storage->btrfsListSubvolumes('/srv/nas');
assertTrue(is_array($btrfsList) && count($btrfsList) > 0 && isset($btrfsList[0]['name']), 'StorageService::btrfsListSubvolumes retorna listado de subvolumenes');
$btrfsBad = $storage->btrfsCreateSubvolume('nombre invalido con espacios');
assertTrue(!$btrfsBad['success'] && str_contains($btrfsBad['error'], 'inválido'), 'StorageService::btrfsCreateSubvolume valida nombre');
$btrfsGood = $storage->btrfsCreateSubvolume('subvol_qa_test');
assertTrue($btrfsGood['success'] && str_contains($btrfsGood['path'], 'subvol_qa_test'), 'StorageService::btrfsCreateSubvolume crea subvolumen exitosamente');

// 4. SystemService: bloqueo de accion 'stop' para servicios criticos web (nginx, php-fpm)
$sysManage = new SystemService();
$stopNginx = $sysManage->manageService('nginx', 'stop');
assertTrue(!$stopNginx['success'] && str_contains($stopNginx['error'], 'no está permitida'), 'SystemService::manageService bloquea accion stop para nginx');
$stopPhp = $sysManage->manageService('php8.4-fpm', 'stop');
assertTrue(!$stopPhp['success'] && str_contains($stopPhp['error'], 'no está permitida'), 'SystemService::manageService bloquea accion stop para php-fpm');

// 5. UserController: bloqueo de elevacion o modificacion de cuentas administrativas por operadores
$_SERVER['REQUEST_METHOD'] = 'POST';
$_POST = [
    'username' => 'sistemas',
    'group' => 'grp_samba',
    'action' => 'add',
];
$userCtrlQa = new \App\Controllers\UserController();
$savedUserSession = $_SESSION['nas_user'] ?? null;
$_SESSION['nas_user'] = ['username' => 'operador_no_admin', 'role' => 'operator', 'is_admin' => false];
ob_start();
$reqPriv = new Request();
$userCtrlQa->userGroups($reqPriv);
$outPriv = ob_get_clean();
$jsonPriv = json_decode($outPriv, true);
assertTrue(isset($jsonPriv['success']) && $jsonPriv['success'] === false && str_contains($jsonPriv['error'] ?? '', 'No tienes permisos para modificar grupos'), 'UserController::userGroups bloquea intentos de alterar cuentas administrativas por operadores');
if ($savedUserSession !== null) {
    $_SESSION['nas_user'] = $savedUserSession;
} else {
    unset($_SESSION['nas_user']);
}
$_POST = [];

// 6. FileExplorerService: bloqueo de descarga ZIP de la raiz completa (/srv/nas)
$fExpQa = new FileExplorerService();
ob_start();
$fExpQa->downloadDirectoryZip('');
$zipRootOut = ob_get_clean();
assertTrue(str_contains($zipRootOut, 'no está permitida'), 'FileExplorerService::downloadDirectoryZip responde con bloqueo de descarga al intentar comprimir la raiz /srv/nas');

// 7. StorageService::runTrim y startBtrfsScrub en modo testing devuelven success
$trimRes = $storage->runTrim('/srv/nas');
assertTrue($trimRes['success'] === true && str_contains($trimRes['message'], 'modo dev/test'), 'StorageService::runTrim retorna éxito simulado en modo test');
$scrubRes = $storage->startBtrfsScrub('/srv/nas');
assertTrue($scrubRes['success'] === true && str_contains($scrubRes['message'], 'modo dev/test'), 'StorageService::startBtrfsScrub retorna éxito simulado en modo test');

// 8. BackupService: no travesía destructiva por symlinks en removeDirectoryRecursive
$tmpBase = sys_get_temp_dir() . '/nas_test_bkp_symlink_' . uniqid();
$targetDir = $tmpBase . '/target_dir';
$bkpDir = $tmpBase . '/bkp_dir';
@mkdir($targetDir, 0777, true);
@mkdir($bkpDir, 0777, true);
file_put_contents($targetDir . '/important.txt', 'DATOS_CRITICOS');
@symlink($targetDir, $bkpDir . '/link_to_target');

$bkpRefl = new \ReflectionClass(BackupService::class);
$rmMethod = $bkpRefl->getMethod('removeDirectoryRecursive');
$rmMethod->setAccessible(true);
$bkpInst = new BackupService();
$rmMethod->invoke($bkpInst, $bkpDir);

assertTrue(!file_exists($bkpDir), 'BackupService::removeDirectoryRecursive elimina la carpeta de backup');
assertTrue(file_exists($targetDir . '/important.txt'), 'BackupService::removeDirectoryRecursive no destruye archivos fuera del enlace simbolico');
@unlink($targetDir . '/important.txt');
@rmdir($targetDir);
@rmdir($tmpBase);

// 9. SystemService::tailFileLines lectura eficiente en bloques y filtrado
$tmpLargeLog = tempnam(sys_get_temp_dir(), 'nas_tail_test_');
$logData = [];
for ($i = 1; $i <= 300; $i++) {
    $logData[] = "[2026-10-14 12:00:$i] [task_$i] [EVENT_$i] Mensaje de prueba linea $i " . ($i === 250 ? 'MARCADOR_ESPECIAL' : '');
}
file_put_contents($tmpLargeLog, implode("\n", $logData) . "\n");

$sysRefl = new \ReflectionClass(SystemService::class);
$tailMethod = $sysRefl->getMethod('tailFileLines');
$tailMethod->setAccessible(true);
$sysInst = new SystemService();

$memBefore = memory_get_usage();
$tailedLines = $tailMethod->invoke($sysInst, $tmpLargeLog, 20, null, 1024);
$memAfter = memory_get_usage();
$memDelta = $memAfter - $memBefore;

assertTrue(is_array($tailedLines) && count($tailedLines) === 20, 'tailFileLines retorna exactamente el límite de líneas solicitadas');
assertTrue(str_contains($tailedLines[0], 'linea 300'), 'tailFileLines retorna las líneas en orden cronológico inverso (la más reciente primero)');
assertTrue(str_contains($tailedLines[19], 'linea 281'), 'tailFileLines preserva continuidad de las últimas líneas');
assertTrue($memDelta < 262144, 'tailFileLines opera con consumo de memoria acotado (< 256 KB RAM)');

$filteredTailed = $tailMethod->invoke($sysInst, $tmpLargeLog, 10, 'MARCADOR_ESPECIAL', 1024);
assertTrue(count($filteredTailed) === 1 && str_contains($filteredTailed[0], 'MARCADOR_ESPECIAL'), 'tailFileLines filtra correctamente por query');

@unlink($tmpLargeLog);

// 10. BackupService::readTaskLog lectura de últimos 64 KB con fseek
$tmpTaskLog = tempnam(sys_get_temp_dir(), 'nas_tasklog_test_');
$chunk100k = str_repeat("LINEA_RELLENO_BACKUP_HISTORICO_1234567890\n", 2500); // ~100 KB
$tailContent = "[2026-10-14 23:00:00] [INICIANDO BACKUP] Tarea iniciada\n[2026-10-14 23:00:15] BACKUP FINALIZADO CON ÉXITO\n";
file_put_contents($tmpTaskLog, $chunk100k . $tailContent);

$bkpSvc = new BackupService();
$readChunk = $bkpSvc->readTaskLog($tmpTaskLog, 4096);
assertTrue(strlen($readChunk) <= 4096, 'readTaskLog respeta el tamaño máximo en bytes especificado');
assertTrue(str_contains($readChunk, 'BACKUP FINALIZADO CON ÉXITO'), 'readTaskLog incluye el segmento final del archivo');
assertTrue($bkpSvc->classifyLatestRun($readChunk) === 'ok', 'classifyLatestRun clasifica como ok a partir de readTaskLog');

@unlink($tmpTaskLog);

// 11. SambaService::updateGlobalConfig y DomainService::syncSambaAdConfig
$tmpSmbConf = tempnam(sys_get_temp_dir(), 'nas_smb_ad_');
file_put_contents($tmpSmbConf, "[global]\n   workgroup = WORKGROUP\n   server role = standalone server\n   security = user\n");

$sambaAd = new SambaService($tmpSmbConf);
$updGlobalRes = $sambaAd->updateGlobalConfig(['netbios name' => 'SRV-TEST']);
assertTrue($updGlobalRes['success'], 'SambaService::updateGlobalConfig actualiza directivas en [global]');

$origDomainSmbPath = DomainService::$smbConfPath;
DomainService::$smbConfPath = $tmpSmbConf;
$domainSvc = new DomainService();

// Unión al dominio
$joinRes = $domainSvc->syncSambaAdConfig(true, 'corp.example.com');
assertTrue($joinRes['success'], 'DomainService::syncSambaAdConfig aplica configuración de dominio');
$joinedConf = file_get_contents($tmpSmbConf);
assertTrue(str_contains($joinedConf, 'security = ads'), 'syncSambaAdConfig establece security = ads');
assertTrue(str_contains($joinedConf, 'realm = CORP.EXAMPLE.COM'), 'syncSambaAdConfig establece realm en mayúsculas');
assertTrue(str_contains($joinedConf, 'workgroup = CORP'), 'syncSambaAdConfig establece workgroup con el NetBIOS del dominio');
assertTrue(!str_contains($joinedConf, 'server role'), 'syncSambaAdConfig elimina server role para evitar conflicto con ads');

// Desvinculación del dominio
$leaveRes = $domainSvc->syncSambaAdConfig(false);
assertTrue($leaveRes['success'], 'DomainService::syncSambaAdConfig restaura configuración standalone');
$leavedConf = file_get_contents($tmpSmbConf);
assertTrue(str_contains($leavedConf, 'security = user'), 'syncSambaAdConfig restaura security = user al desvincularse');
assertTrue(str_contains($leavedConf, 'server role = standalone server'), 'syncSambaAdConfig restaura server role = standalone server');
assertTrue(!str_contains($leavedConf, 'realm ='), 'syncSambaAdConfig elimina la directiva realm al desvincularse');
assertTrue(str_contains($leavedConf, 'workgroup = TEAM-JOFRATO'), 'syncSambaAdConfig restaura workgroup standalone por defecto');

DomainService::$smbConfPath = $origDomainSmbPath;
@unlink($tmpSmbConf);

// 12. SambaService::updateGlobalConfig manejo insensible a mayúsculas y minúsculas
$tmpMixedSmbConf = tempnam(sys_get_temp_dir(), 'nas_smb_case_');
file_put_contents($tmpMixedSmbConf, "[global]\n   WorkGroup = MI-EMPRESA\n   Server Role = standalone server\n   Security = user\n");
$sambaMixed = new SambaService($tmpMixedSmbConf);
$resMixed = $sambaMixed->updateGlobalConfig(['security' => 'ads', 'realm' => 'EMPRESA.LOCAL', 'workgroup' => 'EMPRESA'], ['server role']);
assertTrue($resMixed['success'], 'updateGlobalConfig procesa smb.conf con mayúsculas y minúsculas');
$mixedOut = file_get_contents($tmpMixedSmbConf);
assertTrue(!preg_match('/server role\s*=\s*standalone server/i', $mixedOut), 'updateGlobalConfig elimina Server Role insensible a mayúsculas');
assertTrue(str_contains($mixedOut, 'security = ads'), 'updateGlobalConfig establece security = ads');
assertTrue(!str_contains($mixedOut, 'Security = user'), 'updateGlobalConfig reemplaza Security = user sin duplicar directiva');
@unlink($tmpMixedSmbConf);

// 13. SystemService::parseAdminAuditLog y parseSambaAuditLog búsqueda por etiquetas traducidas
$tmpAdminAudit = tempnam(sys_get_temp_dir(), 'nas_admin_test_');
file_put_contents($tmpAdminAudit, "[2026-10-14 10:00:00] [10.10.1.250] [sistemas] [login_success] [sistemas] [SUCCESS] {}\n[2026-10-14 10:01:00] [10.10.1.250] [sistemas] [user_create] [operador1] [SUCCESS] {}\n");
$origAdminAuditPath = SystemService::$adminAuditPath;
SystemService::$adminAuditPath = $tmpAdminAudit;
$adminFound = $sysInst->parseAdminAuditLog(10, 'Inicio de sesión');
assertTrue(count($adminFound) === 1 && $adminFound[0]['action'] === 'login_success', 'parseAdminAuditLog localiza eventos por etiqueta traducida Inicio de sesión');
$adminFound2 = $sysInst->parseAdminAuditLog(10, 'Crear usuario');
assertTrue(count($adminFound2) === 1 && $adminFound2[0]['action'] === 'user_create', 'parseAdminAuditLog localiza eventos por etiqueta Crear usuario');
SystemService::$adminAuditPath = $origAdminAuditPath;
@unlink($tmpAdminAudit);

$tmpSambaAudit = tempnam(sys_get_temp_dir(), 'nas_samba_test_');
file_put_contents($tmpSambaAudit, "2026-10-14T12:00:00-06:00 srv-nas smbd_audit[123]: sistemas|10.10.1.250|sis-pc|SISTEMAS|openat|ok|w|balance.xlsx\n");
$origSambaAuditPath = SystemService::$sambaAuditPath;
SystemService::$sambaAuditPath = $tmpSambaAudit;
$sambaFound = $sysInst->parseSambaAuditLog(10, 'Escritura');
assertTrue(count($sambaFound) === 1 && $sambaFound[0]['action'] === 'openat', 'parseSambaAuditLog localiza eventos por etiqueta Modificación / Escritura');
SystemService::$sambaAuditPath = $origSambaAuditPath;
@unlink($tmpSambaAudit);

// 14. SystemService::tailFileLines acotación de memoria ante línea masiva sin newlines
$tmpHugeLineFile = tempnam(sys_get_temp_dir(), 'nas_huge_line_');
$hugeFp = fopen($tmpHugeLineFile, 'wb');
for ($k = 0; $k < 10; $k++) {
    fwrite($hugeFp, str_repeat("A", 65536)); // ~650 KB sin newlines
}
fclose($hugeFp);
$memStart = memory_get_usage();
$hugeTailed = $tailMethod->invoke($sysInst, $tmpHugeLineFile, 5, null, 65536);
$memUsed = memory_get_usage() - $memStart;
assertTrue($memUsed < 300000, 'tailFileLines no desborda memoria RAM (< 300 KB) ante líneas masivas corruptas sin newlines');
@unlink($tmpHugeLineFile);

// 15. Pruebas de UserService y UserController para gestión avanzada de miembros y grupos
$savedUserSession = $_SESSION['nas_user'] ?? null;

// UserService::addGroupMember y removeGroupMember (modo test)
$userSvcTest = new UserService();
$addMemRes = $userSvcTest->addGroupMember('grp_contabilidad', 'usuario_test');
assertTrue($addMemRes['success'], 'UserService::addGroupMember añade miembro a grupo departamental');

$remMemRes = $userSvcTest->removeGroupMember('grp_contabilidad', 'usuario_test');
assertTrue($remMemRes['success'], 'UserService::removeGroupMember remueve miembro de grupo departamental');

// UserService::assignUserGroups (modo test)
$assignRes = $userSvcTest->assignUserGroups('usuario_test', ['grp_ventas', 'grp_marketing']);
assertTrue($assignRes['success'], 'UserService::assignUserGroups asigna lista de grupos departamentales');

// RBAC: Superadministrador puede gestionar miembros de grp_superadmin
$_SESSION['nas_user'] = ['username' => 'superadmin', 'role' => 'superadmin', 'is_superadmin' => true, 'is_admin' => true];
$superAddRes = $userSvcTest->addGroupMember('grp_superadmin', 'otro_admin');
assertTrue($superAddRes['success'], 'Superadministrador puede añadir miembros a grp_superadmin');

// RBAC: Superadministrador no puede removerse a sí mismo de grp_superadmin
$superSelfRem = $userSvcTest->removeGroupMember('grp_superadmin', 'superadmin');
$selfRemErr = $superSelfRem['error'] ?? $superSelfRem['message'] ?? '';
assertTrue(!$superSelfRem['success'] && str_contains($selfRemErr, 'propia cuenta'), 'Superadministrador no puede removerse a sí mismo de grp_superadmin');

// RBAC: Administrador estándar no puede tocar grp_superadmin
$_SESSION['nas_user'] = ['username' => 'admin_estandar', 'role' => 'admin', 'is_admin' => true, 'is_superadmin' => false];
$adminTouchSuper = $userSvcTest->addGroupMember('grp_superadmin', 'otro_user');
$adminTouchErr = $adminTouchSuper['error'] ?? $adminTouchSuper['message'] ?? '';
assertTrue(!$adminTouchSuper['success'] && str_contains($adminTouchErr, 'superadministrador'), 'Administrador estándar no puede gestionar miembros de grp_superadmin');

// RBAC: Operador web es bloqueado de gestionar miembros
$_SESSION['nas_user'] = ['username' => 'operador_web', 'role' => 'operator', 'is_admin' => false];
$operTouchGroup = $userSvcTest->addGroupMember('grp_contabilidad', 'otro_user');
$operTouchErr = $operTouchGroup['error'] ?? $operTouchGroup['message'] ?? '';
assertTrue(!$operTouchGroup['success'] && str_contains($operTouchErr, 'privilegios'), 'Operador web no puede gestionar miembros de grupos');

// UserController::assignGroups validación de permisos y superadmin
$userCtrlTest = new \App\Controllers\UserController();
$_POST = ['username' => 'usuario_test', 'groups' => ['grp_contabilidad']];
$_SERVER['REQUEST_METHOD'] = 'POST';

// Con sesión operador -> 403 / success false
ob_start();
$userCtrlTest->assignGroups(new Request());
$outAssignOper = ob_get_clean();
$jsonAssignOper = json_decode($outAssignOper, true);
assertTrue(isset($jsonAssignOper['success']) && $jsonAssignOper['success'] === false && str_contains($jsonAssignOper['error'] ?? '', 'No tienes permisos'), 'UserController::assignGroups bloquea operadores');

// Con sesión admin intentando alterar un superadmin -> 403 / success false
$_SESSION['nas_user'] = ['username' => 'admin_estandar', 'role' => 'admin', 'is_admin' => true, 'is_superadmin' => false];
$_POST = ['username' => 'administrador', 'groups' => ['grp_samba']];
ob_start();
$userCtrlTest->assignGroups(new Request());
$outAssignSuper = ob_get_clean();
$jsonAssignSuper = json_decode($outAssignSuper, true);
assertTrue(isset($jsonAssignSuper['success']) && $jsonAssignSuper['success'] === false && str_contains($jsonAssignSuper['error'] ?? '', 'superadministrador'), 'UserController::assignGroups impide a administradores estándar alterar superadmin');

// updateUser sobre superadmin por admin estándar
$_POST = ['username' => 'administrador', 'full_name' => 'Intento Modificar', 'is_admin' => true];
ob_start();
$userCtrlTest->updateUser(new Request());
$outUpdSuper = ob_get_clean();
$jsonUpdSuper = json_decode($outUpdSuper, true);
assertTrue(isset($jsonUpdSuper['success']) && $jsonUpdSuper['success'] === false && str_contains($jsonUpdSuper['error'] ?? '', 'superadministrador'), 'UserController::updateUser impide a administrador estándar alterar superadmin');

// setPassword sobre superadmin por admin estándar
$_POST = ['username' => 'administrador', 'password' => 'HackPass123!'];
ob_start();
$userCtrlTest->setPassword(new Request());
$outPassSuper = ob_get_clean();
$jsonPassSuper = json_decode($outPassSuper, true);
assertTrue(isset($jsonPassSuper['success']) && $jsonPassSuper['success'] === false && str_contains($jsonPassSuper['error'] ?? '', 'superadministrador'), 'UserController::setPassword impide a administrador estándar cambiar clave de superadmin');

// toggleUser sobre superadmin por admin estándar
$_POST = ['username' => 'administrador', 'enabled' => false];
ob_start();
$userCtrlTest->toggleUser(new Request());
$outTglSuper = ob_get_clean();
$jsonTglSuper = json_decode($outTglSuper, true);
assertTrue(isset($jsonTglSuper['success']) && $jsonTglSuper['success'] === false && str_contains($jsonTglSuper['error'] ?? '', 'superadministrador'), 'UserController::toggleUser impide a administrador estándar alterar estado de superadmin');

// deleteUser sobre superadmin
$_GET = ['username' => 'administrador'];
ob_start();
$userCtrlTest->deleteUser(new Request());
$outDelSuper = ob_get_clean();
$jsonDelSuper = json_decode($outDelSuper, true);
assertTrue(isset($jsonDelSuper['success']) && $jsonDelSuper['success'] === false && str_contains($jsonDelSuper['error'] ?? '', 'superadministrador'), 'UserController::deleteUser impide eliminar cuenta de superadministrador');
$_GET = [];

if ($savedUserSession !== null) {
    $_SESSION['nas_user'] = $savedUserSession;
} else {
    unset($_SESSION['nas_user']);
}
$_POST = [];

// 26. Pruebas de permisos dinámicos en FileExplorerService y default ACLs en recursos públicos
$testStorageRoot = sys_get_temp_dir() . '/nas_storage_root_' . bin2hex(random_bytes(3));
@mkdir($testStorageRoot, 0777, true);
FileExplorerService::setRootDir($testStorageRoot);

$testExpDir = $testStorageRoot . '/carpeta_publica';
@mkdir($testExpDir, 0777, true);
@chmod($testExpDir, 0777);

$tmpUploadSrc = tempnam(sys_get_temp_dir(), 'nas_up_src_');
file_put_contents($tmpUploadSrc, 'CONTENIDO_PRUEBA_SUBIDA_WEB');

$fakeFiles = [
    'name' => ['documento_publico.txt'],
    'type' => ['text/plain'],
    'tmp_name' => [$tmpUploadSrc],
    'error' => [UPLOAD_ERR_OK],
    'size' => [filesize($tmpUploadSrc)],
];

$fileExpInst = new FileExplorerService();
$upResPub = $fileExpInst->uploadFiles('carpeta_publica', $fakeFiles);
assertTrue($upResPub['success'], 'FileExplorerService::uploadFiles sube archivo a subdirectorio');

$destFile = $testExpDir . '/documento_publico.txt';
if (file_exists($destFile)) {
    $fMode = @fileperms($destFile) & 0777;
    assertTrue(($fMode & 0666) === 0666, 'FileExplorerService asigna permisos abiertos 0666 en directorios públicos');
    @unlink($destFile);
}

@unlink($tmpUploadSrc);
@rmdir($testExpDir);
@rmdir($testStorageRoot);
FileExplorerService::setRootDir(null);

// SambaService default ACLs en guest share
$tmpGuestConf = tempnam(sys_get_temp_dir(), 'nas_guest_smb_');
file_put_contents($tmpGuestConf, "[global]\n   workgroup = TEST\n[PUBLICO]\n   path = /srv/nas/PUBLICO\n   guest ok = yes\n   read only = no\n");
$sambaGuest = new SambaService($tmpGuestConf);
$syncGuestRes = $sambaGuest->syncShareAcls('PUBLICO');
assertTrue($syncGuestRes['success'], 'SambaService::syncShareAcls procesa correctamente recursos públicos guest ok');
@unlink($tmpGuestConf);

// Pruebas de detección de esquemas en SambaService (Hallazgo 1)
$tmpSchemeConf = tempnam(sys_get_temp_dir(), 'nas_scheme_smb_');
file_put_contents($tmpSchemeConf, <<<CONF
[global]
   workgroup = TEAM-JOFRATO
[VENTAS]
   path = /srv/nas/VENTAS
   read only = yes
   valid users = @grp_ventas @grp_samba
   write list = @grp_ventas @grp_samba
[CONTABILIDAD]
   path = /srv/nas/CONTABILIDAD
   read only = yes
   valid users = @grp_contabilidad @grp_auditores
   write list = @grp_contabilidad
[HISTORICO]
   path = /srv/nas/HISTORICO
   read only = yes
   valid users = @grp_consulta
CONF
);
$sambaScheme = new SambaService($tmpSchemeConf);
$schemeShares = $sambaScheme->listShares();
$sharesByName = [];
foreach ($schemeShares as $s) {
    $sharesByName[$s['name']] = $s;
}

assertTrue(isset($sharesByName['VENTAS']), 'SambaService::listShares incluye recurso VENTAS');
assertTrue(($sharesByName['VENTAS']['scheme'] ?? 0) === 1, 'Recurso con write_list coincidente se clasifica como Esquema 1');
assertTrue(($sharesByName['VENTAS']['read_only'] ?? true) === false, 'Recurso Esquema 1 tiene read_only = false');

assertTrue(isset($sharesByName['CONTABILIDAD']), 'SambaService::listShares incluye recurso CONTABILIDAD');
assertTrue(($sharesByName['CONTABILIDAD']['scheme'] ?? 0) === 2, 'Recurso con write_list menor a valid_users se clasifica como Esquema 2');
assertTrue(($sharesByName['CONTABILIDAD']['read_only'] ?? true) === false, 'Recurso Esquema 2 tiene read_only = false');

assertTrue(isset($sharesByName['HISTORICO']), 'SambaService::listShares incluye recurso HISTORICO');
assertTrue(($sharesByName['HISTORICO']['scheme'] ?? 0) === 3, 'Recurso con read only y sin write_list se clasifica como Esquema 3');
assertTrue(($sharesByName['HISTORICO']['read_only'] ?? false) === true, 'Recurso Esquema 3 tiene read_only = true');
@unlink($tmpSchemeConf);

// Pruebas de filtrado de DOM según rol y filesystem en layout.php (Hallazgo 2)
ob_start();
$metrics = ['hostname' => 'SRV-TEST', 'uptime' => '1d', 'kernel' => '6.12', 'cpu_model' => 'x86_64', 'cpu_usage_pct' => 10, 'ram_total_gb' => 16, 'ram_used_gb' => 4, 'ram_usage_pct' => 25];
$storage = ['filesystem' => 'ext4', 'total_gb' => 1000, 'used_gb' => 200, 'usage_percent' => 20, 'device_type' => 'Disco'];
$services = [];
$activeView = 'dashboard';
putenv('NAS_SERVER_ROLE=ARCHIVOS');
include dirname(__DIR__) . '/web/templates/layout.php';
$htmlArchivos = ob_get_clean();

assertTrue(strpos($htmlArchivos, 'id="view-backups"') === false, 'layout.php en rol ARCHIVOS no renderiza view-backups en el DOM');
assertTrue(strpos($htmlArchivos, 'modal-new-backup') === false, 'layout.php en rol ARCHIVOS no renderiza modal-new-backup en el DOM');
assertTrue(strpos($htmlArchivos, 'Iniciar auditoría criptográfica BTRFS scrub') === false, 'layout.php en rol ARCHIVOS y ext4 no renderiza botón de acción rápida de BTRFS scrub');
assertTrue(strpos($htmlArchivos, 'value="backups"') === false, 'layout.php en rol ARCHIVOS no renderiza opción backups en explorador');

ob_start();
$storage = ['filesystem' => 'btrfs', 'total_gb' => 1000, 'used_gb' => 200, 'usage_percent' => 20, 'device_type' => 'Disco'];
putenv('NAS_SERVER_ROLE=ARCHIVOS_BACKUP');
include dirname(__DIR__) . '/web/templates/layout.php';
$htmlBackup = ob_get_clean();

assertTrue(strpos($htmlBackup, 'id="view-backups"') !== false, 'layout.php en rol ARCHIVOS_BACKUP renderiza view-backups');
assertTrue(strpos($htmlBackup, 'Iniciar auditoría criptográfica BTRFS scrub') !== false, 'layout.php en rol ARCHIVOS_BACKUP con btrfs renderiza botón de acción rápida de BTRFS scrub');
assertTrue(strpos($htmlBackup, 'value="backups"') !== false, 'layout.php en rol ARCHIVOS_BACKUP renderiza opción backups en explorador');

// Pruebas específicas de rol BACKUP puro (Central de Respaldos)
ob_start();
putenv('NAS_SERVER_ROLE=BACKUP');
include dirname(__DIR__) . '/web/templates/layout.php';
$htmlPureBackup = ob_get_clean();

assertTrue(strpos($htmlPureBackup, 'id="view-backups"') !== false, 'layout.php en rol BACKUP renderiza view-backups en el DOM');
assertTrue(strpos($htmlPureBackup, 'modal-new-backup') !== false, 'layout.php en rol BACKUP renderiza modal-new-backup en el DOM');
assertTrue(strpos($htmlPureBackup, 'Iniciar auditoría criptográfica BTRFS scrub') !== false, 'layout.php en rol BACKUP con btrfs renderiza botón de acción rápida de BTRFS scrub');
assertTrue(strpos($htmlPureBackup, 'value="backups"') !== false, 'layout.php en rol BACKUP renderiza opción backups en explorador');

// Pruebas de restricciones de seguridad SambaService en rol BACKUP
$tmpBkpConf = tempnam(sys_get_temp_dir(), 'nas_bkp_smb_');
file_put_contents($tmpBkpConf, "[global]\n   workgroup = TEAM-JOFRATO\n[TEST$]\n   path = /srv/nas/TEST\n");
$sambaBkpService = new SambaService($tmpBkpConf);

$rejVisible = $sambaBkpService->createShare(['name' => 'VISIBLE_SHARE', 'scheme' => 1, 'groups' => ['grp_samba']]);
assertTrue(!$rejVisible['success'], 'SambaService::createShare en rol BACKUP rechaza recurso visible sin sufijo $');

$rejVisibleFalse = $sambaBkpService->createShare(['name' => 'NO_DOLLAR', 'scheme' => 1, 'groups' => ['grp_samba'], 'hidden' => false]);
assertTrue(!$rejVisibleFalse['success'], 'SambaService::createShare en rol BACKUP rechaza recurso explícito con hidden=false');

$okHiddenNorm = $sambaBkpService->createShare(['name' => 'AUTO_DOLLAR', 'scheme' => 1, 'groups' => ['grp_samba'], 'hidden' => true]);
assertTrue($okHiddenNorm['success'], 'SambaService::createShare en rol BACKUP normaliza a sufijo $ cuando hidden=true');

$rejPublic = $sambaBkpService->createShare(['name' => 'PUBLIC_TEST$', 'scheme' => 4]);
assertTrue(!$rejPublic['success'], 'SambaService::createShare en rol BACKUP rechaza recurso público (scheme 4)');

$rejBrowseable = $sambaBkpService->createShare(['name' => 'BROWSE_TEST$', 'scheme' => 1, 'browseable' => 'yes']);
assertTrue(!$rejBrowseable['success'], 'SambaService::createShare en rol BACKUP rechaza browseable=yes');

$rejGroup = $sambaBkpService->createShare(['name' => 'GRP_TEST$', 'scheme' => 1, 'groups' => ['grp_contabilidad']]);
assertTrue(!$rejGroup['success'], 'SambaService::createShare en rol BACKUP rechaza grupo no autorizado grp_contabilidad');

$rejAccess = $sambaBkpService->setAccess('TEST$', 'group', 'grp_contabilidad', 'read');
assertTrue(!$rejAccess['success'], 'SambaService::setAccess en rol BACKUP rechaza asociar grupos ajenos a grp_samba/grp_backups');

$okBkpShare = $sambaBkpService->createShare(['name' => 'BACKUPS_WIN$', 'scheme' => 1, 'groups' => ['grp_backups']]);
assertTrue($okBkpShare['success'], 'SambaService::createShare en rol BACKUP acepta recurso oculto con grp_backups');

$rejUpdatePublic = $sambaBkpService->updateShare('TEST$', ['scheme' => 4]);
assertTrue(!$rejUpdatePublic['success'], 'SambaService::updateShare en rol BACKUP rechaza scheme=4');

$rejUpdateBrowse = $sambaBkpService->updateShare('TEST$', ['browseable' => 'yes']);
assertTrue(!$rejUpdateBrowse['success'], 'SambaService::updateShare en rol BACKUP rechaza browseable=yes');

$rejBrowseBool = $sambaBkpService->createShare(['name' => 'BROWSE_BOOL$', 'scheme' => 1, 'browseable' => true]);
assertTrue(!$rejBrowseBool['success'], 'SambaService::createShare en rol BACKUP rechaza browseable=true (booleano)');

$rejUpdateBrowseBool = $sambaBkpService->updateShare('TEST$', ['browseable' => true]);
assertTrue(!$rejUpdateBrowseBool['success'], 'SambaService::updateShare en rol BACKUP rechaza browseable=true (booleano)');

$rejUpdateBadGrp = $sambaBkpService->updateShare('TEST$', ['groups' => ['grp_marketing']]);
assertTrue(!$rejUpdateBadGrp['success'], 'SambaService::updateShare en rol BACKUP rechaza grupos ajenos en updateShare');

// Protección del grupo especial grp_backups
$userSvc = new UserService();
$rejDelGrp = $userSvc->deleteGroup('grp_backups');
assertTrue(!$rejDelGrp['success'], 'UserService::deleteGroup protege grp_backups contra eliminación accidental');

$rejRenBkp = $userSvc->renameGroup('grp_backups', 'grp_x');
assertTrue(!$rejRenBkp['success'], 'UserService::renameGroup protege grp_backups contra renombrado');

$rejRenToBkp = $userSvc->renameGroup('grp_marketing', 'grp_backups');
assertTrue(!$rejRenToBkp['success'], 'UserService::renameGroup impide renombrar hacia grp_backups');

$groupsList = $userSvc->listGroups();
$hasBkpSpecial = false;
foreach ($groupsList as $grp) {
    if ($grp['name'] === 'grp_backups' && !empty($grp['is_special'])) {
        $hasBkpSpecial = true;
        break;
    }
}
assertTrue($hasBkpSpecial, 'UserService::listGroups clasifica grp_backups como grupo especial de sistema');

// SambaController::update y HTTP 403
$_SESSION['nas_user'] = ['username' => 'sistemas', 'is_admin' => true, 'role' => 'admin'];
$sambaCtrlBkp = new \App\Controllers\SambaController();
unset($_SERVER['CONTENT_TYPE']);
$_SERVER['REQUEST_METHOD'] = 'POST';
$_POST = ['name' => 'BKP_E2E$', 'scheme' => 1, 'groups' => ['grp_contabilidad']];
$reqUpdBad = new Request();
ob_start();
$sambaCtrlBkp->update($reqUpdBad);
$respUpdBad = ob_get_clean();
$jsonUpdBad = json_decode($respUpdBad, true);
assertTrue(isset($jsonUpdBad['error']) && Response::getStatusCode() === 403, 'SambaController::update rechaza grupos no autorizados con HTTP 403');

@unlink($tmpBkpConf);

// Restaurar rol por defecto
putenv('NAS_SERVER_ROLE=ARCHIVOS_BACKUP');

// Restaurar rutas originales y limpiar temporales
SystemService::$sambaAuditPath = $origSambaPath;
SystemService::$adminAuditPath = $origAdminPath;
SystemService::$backupAuditPath = $origBackupPath;
@unlink($tempSamba);
@unlink($tempAdmin);
// ==============================================================================
// 27. Pruebas de Mitigación de Vulnerabilidades Pentest (vuln-0001 a vuln-0014)
// ==============================================================================

// vuln-0001: SambaService sanitiza saltos de línea en comentarios y directivas
$tmpSambaTestConf = sys_get_temp_dir() . '/smb_test_' . bin2hex(random_bytes(3)) . '.conf';
$sambaSvcTest = new SambaService($tmpSambaTestConf);
$resUpdConf = $sambaSvcTest->updateGlobalConfig([
    'server string' => "NAS Principal\nroot preexec = /bin/sh -c 'id > /tmp/pwn'",
]);
assertTrue($resUpdConf['success'], 'SambaService::updateGlobalConfig procesa directivas');
$confContent = (string) @file_get_contents($tmpSambaTestConf);
assertTrue(!str_contains($confContent, "\nroot preexec"),
    'SambaService sanitiza saltos de línea en directivas previniendo inyección en smb.conf (vuln-0001)');
@unlink($tmpSambaTestConf);

// vuln-0002: BackupService sanitiza campos en tareas
$bkSvcTest = new BackupService();
$resBadTask = $bkSvcTest->createTask([
    'id' => 'test; rm -rf /',
    'source_type' => 'windows',
    'source_ip' => '10.10.1.50',
    'source_share' => 'share',
    'username' => 'user',
    'password' => 'pass',
    'schedule' => '0 2 * * *',
]);
assertTrue(!$resBadTask['success'], 'BackupService::createTask rechaza ID con inyección de comandos (vuln-0002)');

// vuln-0012: FileExplorerService rechaza escapes y enlaces simbólicos
$feSvc = new FileExplorerService();
$resSafeTraversal = $feSvc->resolveSafePath('../etc/shadow');
assertTrue($resSafeTraversal === null, 'FileExplorerService::resolveSafePath bloquea path traversal (vuln-0012)');
$resSafeRoot = $feSvc->resolveSafePath('/etc/passwd');
assertTrue($resSafeRoot === null, 'FileExplorerService::resolveSafePath bloquea rutas fuera de /srv/nas (vuln-0012)');

// vuln-0008/0009: TerminalService bloquea comandos destructivos
$termSvc = new TerminalService();
$cmdBlock1 = $termSvc->execute('mkfs.ext4 /dev/sdb1');
assertTrue($cmdBlock1['exit_code'] === 126, 'TerminalService bloquea mkfs sobre dispositivos de bloque (vuln-0009)');
$cmdBlock2 = $termSvc->execute('dd if=/dev/zero of=/dev/sda bs=1M');
assertTrue($cmdBlock2['exit_code'] === 126, 'TerminalService bloquea dd sobre dispositivos de bloque (vuln-0009)');
$cmdBlock3 = $termSvc->execute('rm -rf /');
assertTrue($cmdBlock3['exit_code'] === 126, 'TerminalService bloquea rm recursivo en la raíz / (vuln-0009)');
$cmdSafe = $termSvc->execute('ls -la /srv/nas');
assertTrue($cmdSafe['exit_code'] === 0, 'TerminalService permite comandos seguros no destructivos');

// vuln-0013: StorageService valida dispositivos de bloque
$storeTest = new StorageService();
$badFmtDev = $storeTest->formatAndMount('/dev/../etc/shadow', 'ext4', 'SI-FORMATEAR');
assertTrue(!$badFmtDev['success'], 'StorageService::formatAndMount rechaza rutas que no sean dispositivos válidos (vuln-0013)');
if (DIRECTORY_SEPARATOR !== '\\') {
    $charFmtNull = $storeTest->formatAndMount('/dev/null', 'ext4', 'SI-FORMATEAR');
    assertTrue(!$charFmtNull['success'] && str_contains($charFmtNull['error'], 'no corresponde a un disco'),
        'StorageService::formatAndMount rechaza dispositivo de caracteres /dev/null (vuln-0013)');
    $charFmtZero = $storeTest->formatAndMount('/dev/zero', 'ext4', 'SI-FORMATEAR');
    assertTrue(!$charFmtZero['success'] && str_contains($charFmtZero['error'], 'no corresponde a un disco'),
        'StorageService::formatAndMount rechaza dispositivo de caracteres /dev/zero (vuln-0013)');
}

// vuln-0014: StorageService detecta volúmenes LVM activos y miembros RAID
$tmpMdstat = sys_get_temp_dir() . '/mock_mdstat_' . bin2hex(random_bytes(3));
file_put_contents($tmpMdstat, "Personalities : [raid1]\nmd0 : active raid1 sdb1[1] sda1[0]\n      104320 blocks [2/2] [UU]\n");
StorageService::$mdstatPath = $tmpMdstat;
$isRaidInUse = $storeTest->deviceInUseCritical('/dev/sda1');
$isCleanInUse = $storeTest->deviceInUseCritical('/dev/sdz1');
StorageService::$mdstatPath = '/proc/mdstat';
@unlink($tmpMdstat);
assertTrue($isRaidInUse, 'StorageService::deviceInUseCritical detecta miembro de RAID activo en mdstat (vuln-0014)');
assertTrue(!$isCleanInUse, 'StorageService::deviceInUseCritical no marca disco limpio fuera de RAID (vuln-0014)');

StorageService::$mockPvs = "  /dev/sdb2  vg_nas lvm2 a-- <200.00g <50.00g\n";
$isLvmInUse = $storeTest->deviceInUseCritical('/dev/sdb2');
$isLvmClean = $storeTest->deviceInUseCritical('/dev/sdd1');
StorageService::$mockPvs = null;
assertTrue($isLvmInUse, 'StorageService::deviceInUseCritical detecta volumen fisico LVM activo (vuln-0014)');
assertTrue(!$isLvmClean, 'StorageService::deviceInUseCritical no marca disco limpio fuera de LVM (vuln-0014)');

// vuln-0010: TerminalService aísla historial por usuario
DatabaseService::query('DELETE FROM terminal_history WHERE username IN (:u1, :u2)', ['u1' => 'alice', 'u2' => 'bob']);
DatabaseService::insert('terminal_history', ['command' => 'echo ALICE_SECRET', 'username' => 'alice', 'cwd' => '/srv/nas', 'exit_code' => 0]);
DatabaseService::insert('terminal_history', ['command' => 'echo BOB_PUBLIC', 'username' => 'bob', 'cwd' => '/srv/nas', 'exit_code' => 0]);
$_SESSION['nas_user'] = ['username' => 'bob', 'role' => 'admin', 'is_admin' => true];
$bobHistory = $termSvc->getHistory(10);
$bobCommands = array_column($bobHistory, 'command');
assertTrue(in_array('echo BOB_PUBLIC', $bobCommands, true), 'TerminalService::getHistory incluye comandos del usuario autenticado (vuln-0010)');
assertTrue(!in_array('echo ALICE_SECRET', $bobCommands, true), 'TerminalService::getHistory NO expone comandos de otros usuarios (vuln-0010)');
DatabaseService::query('DELETE FROM terminal_history WHERE username IN (:u1, :u2)', ['u1' => 'alice', 'u2' => 'bob']);

// vuln-0005: SystemController restringe manageService ante rol operador
$_SESSION['nas_user'] = ['username' => 'operador1', 'role' => 'operator', 'is_admin' => false];
$sysCtrl = new SystemController();
$_SERVER['CONTENT_TYPE'] = 'application/json';
$_SERVER['HTTP_ACCEPT'] = 'application/json';
$reqOperatorManage = new Request();
ob_start();
$sysCtrl->manageService($reqOperatorManage);
$rawOpResp = ob_get_clean();
$opResp = json_decode($rawOpResp, true);
assertTrue(is_array($opResp) && empty($opResp['success']) && str_contains($opResp['error'] ?? '', 'administrador'),
    'SystemController::manageService responde 403 / error ante llamadas de operador (vuln-0005)');

// vuln-0008: TerminalController restringe terminal ante rol operador
$termCtrl = new TerminalController($termSvc);
ob_start();
$termCtrl->exec($reqOperatorManage);
$rawOpTerm = ob_get_clean();
$opTermResp = json_decode($rawOpTerm, true);
assertTrue(is_array($opTermResp) && empty($opTermResp['success']) && str_contains($opTermResp['error'] ?? '', 'administrador'),
    'TerminalController::exec responde 403 / error ante llamadas de operador (vuln-0008)');

// vuln-0007: AuthMiddleware::verifySameOrigin valida puertos y orígenes no permitidos
$_SERVER['HTTP_HOST'] = '10.10.1.222:8443';
$_SERVER['HTTP_ORIGIN'] = 'https://10.10.1.222:8443';
$reqSameOriginPort = new Request();
assertTrue(AuthMiddleware::verifySameOrigin($reqSameOriginPort), 'verifySameOrigin acepta origen y puerto coincidentes');
$_SERVER['HTTP_ORIGIN'] = 'http://10.10.1.222:9000';
$reqDiffPort = new Request();
assertTrue(!AuthMiddleware::verifySameOrigin($reqDiffPort), 'verifySameOrigin rechaza origen con puerto diferente (vuln-0007)');
$_SERVER['HTTP_ORIGIN'] = 'null';
$reqNullOrigin = new Request();
assertTrue(!AuthMiddleware::verifySameOrigin($reqNullOrigin), 'verifySameOrigin rechaza Origin: null (vuln-0007)');
unset($_SERVER['HTTP_ORIGIN'], $_SERVER['HTTP_HOST']);

// ==============================================================================
// 28. Validaciones Adicionales de Remediación Técnica (Audit nas-debian_d219)
// ==============================================================================

// SSRF & BackupService: Bloqueo de orígenes de red peligrosos y formatos bracketed/opciones (vuln-0017)
$bkSvcAudit = new BackupService();
$resSsrfLoopback = $bkSvcAudit->createTask([
    'id' => 'bkp_ssrf_loop',
    'source_type' => 'windows',
    'source_ip' => '127.0.0.1',
    'source_share' => 'share',
    'username' => 'user',
    'password' => 'pass',
    'schedule' => '0 2 * * *',
]);
assertTrue(!$resSsrfLoopback['success'], 'BackupService::createTask bloquea origen loopback 127.0.0.1 (vuln-0017)');

$resSsrfLinkLocal = $bkSvcAudit->createTask([
    'id' => 'bkp_ssrf_ll',
    'source_type' => 'windows',
    'source_ip' => '169.254.10.20',
    'source_share' => 'share',
    'username' => 'user',
    'password' => 'pass',
    'schedule' => '0 2 * * *',
]);
assertTrue(!$resSsrfLinkLocal['success'], 'BackupService::createTask bloquea origen link-local 169.254.10.20 (vuln-0017)');

assertTrue($bkSvcAudit->validateOriginHost('[::1]') !== null, 'BackupService::validateOriginHost bloquea IPv6 en corchetes [::1]');
assertTrue($bkSvcAudit->validateOriginHost('[127.0.0.1]') !== null, 'BackupService::validateOriginHost bloquea IPv4 en corchetes [127.0.0.1]');
assertTrue($bkSvcAudit->validateOriginHost('[::ffff:127.0.0.1]') !== null, 'BackupService::validateOriginHost bloquea IPv4 mapeada en IPv6 [::ffff:127.0.0.1]');
assertTrue($bkSvcAudit->validateOriginHost('localhost6') !== null, 'BackupService::validateOriginHost bloquea alias localhost6');
assertTrue($bkSvcAudit->validateOriginHost('-oProxyCommand=calc') !== null, 'BackupService::validateOriginHost rechaza hosts con guion inicial');
assertTrue($bkSvcAudit->validateOriginHost('10.10.1.50') === null, 'BackupService::validateOriginHost autoriza IP de red local valida');

// Gap CIFS: Sanitización y rechazo de contraseñas con \r o \n
$resCifsNl = $bkSvcAudit->createTask([
    'id' => 'bkp_cifs_nl',
    'source_type' => 'windows',
    'source_ip' => '10.10.1.50',
    'source_share' => 'share',
    'username' => 'user',
    'password' => "secret\nusername=injected",
    'schedule' => '0 2 * * *',
]);
assertTrue(!$resCifsNl['success'] && str_contains($resCifsNl['error'] ?? '', 'saltos de línea'),
    'BackupService::createTask rechaza contraseñas CIFS con saltos de línea');

// vuln-0014: SSH usuario con guion inicial
$resSshDash = $bkSvcAudit->createTask([
    'id' => 'bkp_ssh_dash',
    'source_type' => 'linux',
    'source_ip' => '10.10.1.50',
    'source_path' => '/var/data',
    'username' => '-eviluser',
    'password' => 'secret',
    'schedule' => '0 2 * * *',
]);
assertTrue(!$resSshDash['success'] && (str_contains($resSshDash['error'] ?? '', 'guión') || str_contains($resSshDash['error'] ?? '', 'guion')),
    'BackupService::createTask rechaza usuario SSH con guion inicial (vuln-0014)');

// vuln-0015: Confinar origen de respaldo local dentro de /srv/nas y prevenir recursión
$resLocalEsc = $bkSvcAudit->createTask([
    'id' => 'bkp_local_esc',
    'source_type' => 'local',
    'source_path' => '/srv/nas/../../etc',
    'schedule' => '0 2 * * *',
]);
assertTrue(!$resLocalEsc['success'], 'BackupService::createTask confina origen local dentro de /srv/nas (vuln-0015)');
assertTrue(!$bkSvcAudit->isAllowedLocalPath('/srv/nas/BACKUPS_HISTORICOS'), 'BackupService::isAllowedLocalPath bloquea usar el repositorio de backups como origen (previene recursion)');

// vuln-0013: AuthService::clearFailedAttempts retrocompatible y con username
$authSvcTest = new AuthService();
$authSvcTest->recordFailedAttempt('198.51.100.99', 'testuser');
$authSvcTest->clearFailedAttempts('198.51.100.99', 'testuser');
$isBlocked = $authSvcTest->isRateLimited('198.51.100.99');
assertTrue(!$isBlocked, 'AuthService::clearFailedAttempts purga intentos con IP y username (vuln-0013)');
$authSvcTest->recordFailedAttempt('198.51.100.99', 'testuser2');
$authSvcTest->clearFailedAttempts('198.51.100.99');
assertTrue(!$authSvcTest->isRateLimited('198.51.100.99'), 'AuthService::clearFailedAttempts funciona con un solo argumento (retrocompatibilidad)');

// vuln-0012: SambaController restringe Esquema 4 (guest ok = yes) a administradores
$sambaCtrlAudit = new \App\Controllers\SambaController();
$_SESSION['nas_user'] = ['username' => 'operador2', 'role' => 'operator', 'is_admin' => false];
$_SERVER['REQUEST_METHOD'] = 'POST';
unset($_SERVER['CONTENT_TYPE']);
$_POST = [
    'name' => 'PUBLICO_GUEST_TEST',
    'path' => '/srv/nas/PUBLICO_GUEST_TEST',
    'scheme' => 4,
];
$reqGuestShare = new Request();
ob_start();
$sambaCtrlAudit->create($reqGuestShare);
$rawGuestResp = ob_get_clean();
$guestResp = json_decode($rawGuestResp, true);
assertTrue(is_array($guestResp) && empty($guestResp['success']) && str_contains($guestResp['error'] ?? '', 'administrador'),
    'SambaController::create restringe Esquema 4 (invitados) ante rol operador con 403 (vuln-0012)');

$_POST = [
    'name' => 'PUBLICO_GUEST_DASH',
    'path' => '/srv/nas/PUBLICO_GUEST_DASH',
    'guest-ok' => true,
];
$reqGuestDash = new Request();
ob_start();
$sambaCtrlAudit->create($reqGuestDash);
$rawGuestDash = ob_get_clean();
$guestDashResp = json_decode($rawGuestDash, true);
assertTrue(is_array($guestDashResp) && empty($guestDashResp['success']) && str_contains($guestDashResp['error'] ?? '', 'administrador'),
    'SambaController::create restringe guest-ok ante rol operador con 403');

// vuln-0010: StorageService detecta particiones LVM en disco padre
StorageService::$mockPvs = "  /dev/sdb2  vg_nas lvm2 a-- <200.00g <50.00g\n";
$storeTestAudit = new StorageService();
assertTrue($storeTestAudit->deviceInUseCritical('/dev/sdb'), 'StorageService::deviceInUseCritical detecta disco padre /dev/sdb con particion LVM /dev/sdb2');
StorageService::$mockPvs = null;

// vuln-0007 / vuln-0008: TerminalService cliente y normalización destructiva
$termSvcAudit = new TerminalService();
$termSvcAudit->setClientId('sess-test-999');
assertTrue($termSvcAudit->getClientId() === 'sess-test-999', 'TerminalService soporta asignación de client_id (vuln-0007)');

$cmdBlockedByComment = $termSvcAudit->execute('mkfs.ext4 /dev/sdb1 # comentario de prueba');
assertTrue($cmdBlockedByComment['exit_code'] === 126, 'TerminalService detecta comando destructivo con comentario shell (vuln-0008)');

$cmdBlockedByQuotes = $termSvcAudit->execute('m"k"fs.ext4 /dev/sdb1');
assertTrue($cmdBlockedByQuotes['exit_code'] === 126, 'TerminalService detecta comando destructivo ofuscado con comillas (vuln-0008)');

$cmdBlockedByNull = $termSvcAudit->execute("mkfs.ext4\0 /dev/sdb1");
assertTrue($cmdBlockedByNull['exit_code'] === 126, 'TerminalService detecta comando destructivo con caracteres nulos (vuln-0008)');

$cmdBlockedChained = $termSvcAudit->execute('rm -rf /; echo done');
assertTrue($cmdBlockedChained['exit_code'] === 126, 'TerminalService bloquea rm en raiz encadenado con punto y coma');

$cmdBlockedSubshell = $termSvcAudit->execute('echo $(rm -rf /)');
assertTrue($cmdBlockedSubshell['exit_code'] === 126, 'TerminalService bloquea rm en raiz dentro de subshell $()');

$cmdBlockedPipe = $termSvcAudit->execute('rm -rf /| cat');
assertTrue($cmdBlockedPipe['exit_code'] === 126, 'TerminalService bloquea rm en raiz con pipe |');

// vuln-0001 / vuln-0002: SheetJS versión >= 0.20.2
$vendorXlsxPath = __DIR__ . '/../web/public/js/vendor/xlsx.full.min.js';
$vendorXlsxData = (string) @file_get_contents($vendorXlsxPath);
assertTrue(str_contains($vendorXlsxData, '.version="0.20.') || str_contains($vendorXlsxData, '.version="0.21.'),
    'Librería local SheetJS xlsx.full.min.js actualizada contra vulnerabilidades conocidas (versión >= 0.20.2)');

// ==============================================================================
// 27. Pruebas de regresión: Remediación de 11 vulnerabilidades (nas-debian_d219)
// ==============================================================================

// 1) vuln-0002: BackupService fail-closed ante direcciones no canónicas o inválidas (SSRF)
$bkpRemed = new BackupService();
assertTrue($bkpRemed->validateOriginHost('0x7f000001') !== null, 'BackupService::validateOriginHost bloquea hex IPv4 0x7f000001 (vuln-0002)');
assertTrue($bkpRemed->validateOriginHost('0X7F000001') !== null, 'BackupService::validateOriginHost bloquea hex IPv4 en mayúsculas 0X7F000001');
assertTrue($bkpRemed->validateOriginHost('0x7f.0.0.1') !== null, 'BackupService::validateOriginHost bloquea dotted-hex 0x7f.0.0.1');
assertTrue($bkpRemed->validateOriginHost('127.0.0.1.') !== null, 'BackupService::validateOriginHost bloquea IP con punto final 127.0.0.1.');
assertTrue($bkpRemed->validateOriginHost('0xA9FEA9FE') !== null, 'BackupService::validateOriginHost bloquea hex link-local 0xA9FEA9FE');

// 2) Operador restringido en scripts de respaldo y subdirectorio local
$_SESSION['nas_user'] = ['username' => 'operador_bkp', 'role' => 'operator', 'is_admin' => false];
$opScriptTask = $bkpRemed->createTask([
    'id' => 'task_op_script',
    'proto' => 'local',
    'cron' => '0 2 * * *',
    'path' => '/srv/nas/SISTEMAS',
    'script' => 'echo pwn',
]);
assertTrue(!$opScriptTask['success'] && str_contains($opScriptTask['error'] ?? '', 'operador no puede configurar scripts'), 'BackupService::createTask bloquea scripts personalizados para rol operador');

$subfolderTrav = $bkpRemed->createTask([
    'id' => 'task_trav',
    'proto' => 'local',
    'cron' => '0 2 * * *',
    'path' => '/srv/nas/SISTEMAS',
    'local_subfolder' => '../escaped',
]);
assertTrue(!$subfolderTrav['success'] && str_contains($subfolderTrav['error'] ?? '', 'secuencias prohibidas'), 'BackupService::createTask valida local_subfolder contra path traversal');
unset($_SESSION['nas_user']);

// 3) vuln-0004: AuditService::log neutraliza saltos de línea y caracteres de control (Log Forgery)
$logTmp = sys_get_temp_dir() . '/audit_test_' . uniqid() . '.log';
AuditService::setLogPath($logTmp);
AuditService::log("action\r\ninjected", "user_target\n[2026-01-01 00:00:00] [9.9.9.9] [root] [user_create] [FORGED] [SUCCESS] {}", 'FAILED', ['err' => 'test']);
$logLines = file($logTmp, FILE_IGNORE_NEW_LINES | FILE_SKIP_EMPTY_LINES);
assertTrue(count($logLines) === 1, 'AuditService::log neutraliza saltos de línea y produce exactamente una sola línea por registro (vuln-0004)');
assertTrue(str_contains($logLines[0] ?? '', 'user_target [2026-01-01 00:00:00]'), 'AuditService::log reemplaza saltos de línea con espacios sin romper la estructura');
@unlink($logTmp);

// 4) AuthService limitación de tasa por usuario además de IP
$authRemed = new AuthService();
$targetUser = 'test_user_limit_' . uniqid();
for ($i = 0; $i < 5; $i++) {
    $authRemed->recordFailedAttempt('198.51.100.' . $i, $targetUser);
}
assertTrue($authRemed->isRateLimited('192.0.2.1', $targetUser), 'AuthService::isRateLimited detecta límite alcanzado por nombre de usuario');
$authRemed->clearFailedAttempts('192.0.2.1', $targetUser);
assertTrue(!$authRemed->isRateLimited('192.0.2.1', $targetUser), 'AuthService::clearFailedAttempts limpia intentos por usuario e IP');

// 5) vuln-0005: SystemService redacta credenciales en bitácoras de journalctl
$sysSvcRemed = new SystemService();
$reflSys = new \ReflectionClass($sysSvcRemed);
$methodRedact = $reflSys->getMethod('redactJournalSecrets');
$methodRedact->setAccessible(true);
$rawMsg = "sistemas : PWD=/home/sistemas ; USER=root ; COMMAND=/usr/bin/sudo -u www-data -- sh -c 'printf \\'root:Secret123#\\\\n\\' | sudo -n /usr/local/sbin/nas-chpasswd; echo rc=0'";
$cleanMsg = $methodRedact->invoke($sysSvcRemed, $rawMsg);
assertTrue(!str_contains($cleanMsg, 'Secret123#'), 'SystemService::redactJournalSecrets oculta contraseña canalizada a nas-chpasswd (vuln-0005)');
assertTrue(str_contains($cleanMsg, '[REDACTADO]'), 'SystemService::redactJournalSecrets sustituye credencial con [REDACTADO]');

$rawArg = "COMMAND=/usr/bin/nas-cli user create jdoe --password=AdminPass456# --admin";
$cleanArg = $methodRedact->invoke($sysSvcRemed, $rawArg);
assertTrue(!str_contains($cleanArg, 'AdminPass456#'), 'SystemService::redactJournalSecrets oculta argumento --password');
assertTrue(str_contains($cleanArg, '--password=[REDACTADO]'), 'SystemService::redactJournalSecrets sustituye argumento con [REDACTADO]');

// SystemController::logs bloquea operador en source=system o source=all
$sysCtrlRemed = new SystemController();
$_SESSION['nas_user'] = ['username' => 'operador_log', 'role' => 'operator', 'is_admin' => false];
$reqSysLogs = new Request();
ob_start();
$sysCtrlRemed->logs($reqSysLogs);
$outLogs = ob_get_clean();
$jsonLogs = json_decode($outLogs, true);
assertTrue(isset($jsonLogs['success']) && $jsonLogs['success'] === false, 'SystemController::logs bloquea consulta de source=all a operadores con 403');
unset($_SESSION['nas_user']);

// 6) vuln-0007: FileExplorerService::renameItem prohíbe renombrar la raíz del almacenamiento
$feRemed = new FileExplorerService();
$renameRootRes = $feRemed->renameItem('.', 'nas_renamed_escape');
assertTrue(!$renameRootRes['success'] && str_contains($renameRootRes['error'] ?? '', 'No se puede renombrar la raíz'), 'FileExplorerService::renameItem prohíbe renombrar la raíz del almacenamiento (vuln-0007)');
$renameSlashRes = $feRemed->renameItem('/', 'nas_renamed_escape');
assertTrue(!$renameSlashRes['success'] && str_contains($renameSlashRes['error'] ?? '', 'No se puede renombrar la raíz'), 'FileExplorerService::renameItem prohíbe renombrar /');

// FileExplorerService::downloadDirectoryZip prohíbe comprimir la raíz del almacenamiento
ob_start();
$feRemed->downloadDirectoryZip('');
$zipOut = ob_get_clean();
assertTrue(str_contains($zipOut, 'no está permitida'), 'FileExplorerService::downloadDirectoryZip prohíbe comprimir la raíz /srv/nas antes de chequeo ZipArchive');

// 7) vuln-0008: StorageService::btrfsSubvolume protege disco del SO y miembros LVM/RAID
$storageRemed = new StorageService();
$subvolBadConfirm = $storageRemed->btrfsSubvolume('/dev/sda', 'subvol_test', 'NO');
assertTrue(!$subvolBadConfirm['success'] && str_contains($subvolBadConfirm['error'] ?? '', 'Confirmación requerida'), 'StorageService::btrfsSubvolume exige SI-FORMATEAR');

// 8) SambaService rechaza secciones reservadas ampliadas
$sambaRemed = new SambaService();
$resHomes = $sambaRemed->createShare(['name' => 'homes', 'scheme' => 1]);
assertTrue(!$resHomes['success'] && str_contains($resHomes['error'] ?? '', 'reservado'), 'SambaService::createShare rechaza recurso reservado homes');
$resIpc = $sambaRemed->createShare(['name' => 'ipc$', 'scheme' => 1]);
assertTrue(!$resIpc['success'] && str_contains($resIpc['error'] ?? '', 'reservado'), 'SambaService::createShare rechaza recurso reservado ipc$');
$resNetlogon = $sambaRemed->createShare(['name' => 'netlogon', 'scheme' => 1]);
assertTrue(!$resNetlogon['success'] && str_contains($resNetlogon['error'] ?? '', 'reservado'), 'SambaService::createShare rechaza recurso reservado netlogon');
$resSysvol = $sambaRemed->createShare(['name' => 'sysvol', 'scheme' => 1]);
assertTrue(!$resSysvol['success'] && str_contains($resSysvol['error'] ?? '', 'reservado'), 'SambaService::createShare rechaza recurso reservado sysvol');

$delReserved = $sambaRemed->deleteShare('netlogon');
assertTrue(!$delReserved['success'] && str_contains($delReserved['error'] ?? '', 'protegida'), 'SambaService::deleteShare rechaza eliminar recurso reservado netlogon');

$updReserved = $sambaRemed->updateShare('sysvol', []);
assertTrue(!$updReserved['success'] && str_contains($updReserved['error'] ?? '', 'protegida'), 'SambaService::updateShare rechaza actualizar recurso reservado sysvol');

// SambaService sanitiza comentarios de recursos contra inyección INI
$reflSamba = new \ReflectionClass($sambaRemed);
$methodComment = $reflSamba->getMethod('sanitizeComment');
$methodComment->setAccessible(true);
$dirtyComment = "Comentario de prueba; directiva=invalida # [nueva_seccion]\r\npath=/srv/evil\\";
$sanitizedComment = $methodComment->invoke($sambaRemed, $dirtyComment);
assertTrue(!str_contains($sanitizedComment, ';'), 'SambaService::sanitizeComment elimina punto y coma (;)');
assertTrue(!str_contains($sanitizedComment, '#'), 'SambaService::sanitizeComment elimina numeral (#)');
assertTrue(!str_contains($sanitizedComment, '['), 'SambaService::sanitizeComment elimina corchetes ([)');
assertTrue(!str_contains($sanitizedComment, "\n"), 'SambaService::sanitizeComment elimina saltos de línea');
assertTrue(!str_ends_with($sanitizedComment, '\\'), 'SambaService::sanitizeComment elimina barra invertida final');

// 9) vuln-0011: TerminalService bloquea evasión por variables, sustitución y decodificadores
$termRemed = new TerminalService();
$cmdIfs = $termRemed->execute('rm -rf${IFS}/');
assertTrue($cmdIfs['exit_code'] === 126, 'TerminalService bloquea rm -rf${IFS}/ mediante normalización de \$IFS (vuln-0011)');

$cmdVarDev = $termRemed->execute('z9=/dev; sudo parted -s ${z9}/nvme0n1 print');
assertTrue($cmdVarDev['exit_code'] === 126, 'TerminalService bloquea reconstrucción de /dev mediante variables combinadas con parted/sudo (vuln-0011)');

$cmdBase64Sudo = $termRemed->execute('cc9=$(echo bWtmcy5leHQ0 | base64 -d); sudo $cc9 -n /dev/sda');
assertTrue($cmdBase64Sudo['exit_code'] === 126, 'TerminalService bloquea sustitución de comando combinada con sudo o herramienta protegida (vuln-0011)');

// 10) AuthMiddleware: Sec-Fetch-Site y SameSite=Strict
$_SERVER['REQUEST_METHOD'] = 'POST';
$_SERVER['HTTP_SEC_FETCH_SITE'] = 'cross-site';
$reqSecFetch = new Request();
ob_start();
$secCheck = AuthMiddleware::check($reqSecFetch);
ob_end_clean();
assertTrue(!$secCheck, 'AuthMiddleware bloquea peticiones mutantes con Sec-Fetch-Site cross-site');
unset($_SERVER['HTTP_SEC_FETCH_SITE']);
$_SERVER['REQUEST_METHOD'] = 'GET';

// 11) AuthService::verifyCredentials y mitigación de timing attack con eliminación de auth_verify_cache
$authRemed = new AuthService();
$_SESSION['auth_verify_cache'] = 'dummy_cached_token';
$verifyFake = $authRemed->verifyCredentials('non_existent_user_123', 'wrong_pass');
assertTrue(!$verifyFake, 'AuthService::verifyCredentials rechaza usuario inexistente');
assertTrue(!isset($_SESSION['auth_verify_cache']), 'AuthService::verifyCredentials purga $_SESSION[auth_verify_cache]');

// 12) AuthService: rate limiting por clave login_user:<username>
$testLimUser = 'rate_test_user_' . bin2hex(random_bytes(4));
for ($i = 0; $i < AuthService::MAX_FAILED_ATTEMPTS; $i++) {
    $authRemed->recordFailedAttempt('192.0.2.1', $testLimUser);
}
assertTrue($authRemed->isRateLimited('198.51.100.99', $testLimUser), 'AuthService::isRateLimited bloquea por clave login_user tras superar intentos');
$authRemed->clearFailedAttempts('192.0.2.1', $testLimUser);
assertTrue(!$authRemed->isRateLimited('198.51.100.99', $testLimUser), 'AuthService::clearFailedAttempts limpia rate limit de login_user');

// 13) BackupService: isAllowedRemoteHost bloquea las IPs del propio servidor NAS
$bkpRemed = new BackupService();
$localIps = $bkpRemed->getLocalServerIps();
assertTrue(count($localIps) >= 1, 'BackupService::getLocalServerIps detecta IPs locales del NAS');
foreach ($localIps as $lip) {
    assertTrue(!$bkpRemed->isAllowedRemoteHost($lip), "BackupService::isAllowedRemoteHost bloquea IP propia del servidor NAS ($lip)");
}

// 14) BackupService: operador restringido estrictamente a lista preautorizada
$unauthHost = '198.51.100.222';
assertTrue(!$bkpRemed->isAllowedRemoteHost($unauthHost, 'operator'), 'BackupService::isAllowedRemoteHost restringe operador ante hosts no autorizados');
$authHostTest = '198.51.100.' . random_int(100, 200);
$bkpRemed->addAllowedBackupHost($authHostTest);
assertTrue($bkpRemed->isAllowedRemoteHost($authHostTest, 'operator'), 'BackupService autoriza host previamente registrado para operador');

// 15) BackupService: validateLocalSubfolder y updateTask
$_SESSION['nas_user']['role'] = 'operator';
$_SESSION['nas_user']['groups'] = ['grp_marketing'];
$bkpRemed->sambaService = new SambaService('/nonexistent/smb.conf');
$subfolderErr = $bkpRemed->validateLocalSubfolder('BACKUPS_WINDOWS$/sub', 'operator');
assertTrue($subfolderErr !== null && str_contains($subfolderErr, 'Acceso denegado'), 'BackupService::validateLocalSubfolder bloquea subcarpeta en share sin permisos para operador');
$subfolderShareErr = $bkpRemed->validateLocalSubfolder('SISTEMAS/sub', 'operator');
assertTrue($subfolderShareErr !== null && str_contains($subfolderShareErr, 'Acceso denegado'), 'BackupService::validateLocalSubfolder bloquea share de Samba sin membresia de grupo para operador');

$updScriptBad = $bkpRemed->updateTask('task_test', ['post_script' => 'echo pwn']);
assertTrue(!$updScriptBad['success'] && str_contains($updScriptBad['error'] ?? '', 'scripts personalizados'), 'BackupService::updateTask prohíbe a operador modificar scripts');
unset($_SESSION['nas_user']);

// 16) SambaService::replaceSectionInContent preserva intactos $ y \ en shares ocultos
$origSmb = "[global]\n   workgroup = WORKGROUP\n\n[BACKUPS_WINDOWS$]\n   path = /srv/nas/BACKUPS\n   read only = no\n";
$newBlock = "[BACKUPS_WINDOWS$]\n   path = /srv/nas/BACKUPS_MOD\n   comment = Share con barra \\ y simbolo $\n   read only = yes\n";
$replacedSmb = $sambaRemed->replaceSectionInContent($origSmb, 'BACKUPS_WINDOWS$', $newBlock);
assertTrue(str_contains($replacedSmb, '[BACKUPS_WINDOWS$]'), 'SambaService::replaceSectionInContent preserva corchetes y signo $ en share oculto');
assertTrue(str_contains($replacedSmb, 'simbolo $'), 'SambaService::replaceSectionInContent preserva caracter $ en directivas');
assertTrue(str_contains($replacedSmb, 'barra \\'), 'SambaService::replaceSectionInContent preserva barra invertida \\');

// 17) TerminalService: bloquea variable como comando, decodificadores con pipe y eval $()
$cmdVarCmd = $termRemed->execute('c=fdisk; $c /dev/sda');
assertTrue($cmdVarCmd['exit_code'] === 126, 'TerminalService bloquea ejecución donde el comando es una variable ($c /dev/sda)');

$cmdEvalSub = $termRemed->execute('eval $(echo cm0gLXJmIC8= | base64 -d)');
assertTrue($cmdEvalSub['exit_code'] === 126, 'TerminalService bloquea combinación de eval con decodificador base64 y sustitución');

$cmdPipeSh = $termRemed->execute('echo bWtmcy5leHQ0IC9kZXYvc2Rh | base64 -d | sh');
assertTrue($cmdPipeSh['exit_code'] === 126, 'TerminalService bloquea decodificador base64 canalizado a intérprete sh');

// 18) UserService::updateUser revoca privilegios a admin creado con adm/grp_samba sin requerir sudo
$userTestRemed = new UserService();
$updDemote = $userTestRemed->updateUser('admin_demo', null, 'Usuario Demo', ['grp_general'], false, true, true, false);
assertTrue($updDemote['success'], 'UserService::updateUser permite revocar privilegios administrativos');

// ==============================================================================
// 28. Pruebas de Tamaño de Carpetas, Compresión ZIP (.tmp_zip) y Origen Remoto en Backups
// ==============================================================================

$tempSizeRoot = sys_get_temp_dir() . '/nas_size_root_' . uniqid();
@mkdir($tempSizeRoot, 0777, true);
FileExplorerService::setRootDir($tempSizeRoot);

$feSizeTest = new FileExplorerService();
$sizeFolderName = 'CARPETA_TAMANO_' . uniqid();
$feSizeTest->createDirectory('', $sizeFolderName);
$sizeFolderAbs = FileExplorerService::getRootDir() . '/' . $sizeFolderName;
file_put_contents($sizeFolderAbs . '/archivo_2kb.bin', str_repeat('A', 2048));
file_put_contents($sizeFolderAbs . '/archivo_1kb.bin', str_repeat('B', 1024));

$listRootForSize = $feSizeTest->listDirectory('');
$foundFolderItem = null;
foreach ($listRootForSize['items'] ?? [] as $it) {
    if (($it['name'] ?? '') === $sizeFolderName) {
        $foundFolderItem = $it;
        break;
    }
}
assertTrue($foundFolderItem !== null, 'FileExplorerService::listDirectory encuentra la carpeta creada');
assertTrue(($foundFolderItem['size_bytes'] ?? 0) === 3072, 'FileExplorerService::listDirectory calcula size_bytes exacto (3072 B) para carpetas');
assertTrue(($foundFolderItem['size_formatted'] ?? '--') === '3 KB', 'FileExplorerService::listDirectory formatea size_formatted (3 KB) para carpetas en lugar de --');

if (DIRECTORY_SEPARATOR === '/' && @link($sizeFolderAbs . '/archivo_2kb.bin', $sizeFolderAbs . '/archivo_2kb_hardlink.bin')) {
    clearstatcache();
    $listHardlinkSize = $feSizeTest->listDirectory('');
    $hlFolderItem = null;
    foreach ($listHardlinkSize['items'] ?? [] as $it) {
        if (($it['name'] ?? '') === $sizeFolderName) {
            $hlFolderItem = $it;
            break;
        }
    }
    assertTrue(($hlFolderItem['size_bytes'] ?? 0) === 3072, 'FileExplorerService::listDirectory deduplica hardlinks por inodo dentro de la misma carpeta');
    @unlink($sizeFolderAbs . '/archivo_2kb_hardlink.bin');
}

// Verificar que .tmp_zip se crea dentro de la raíz NAS con permisos 02770 (sin pérdida por umask), está oculto en listDirectory y bloqueado en resolveSafePath
$tmpZipDir = FileExplorerService::getTempZipDir();
assertTrue(is_dir($tmpZipDir) && str_ends_with(str_replace('\\', '/', $tmpZipDir), '/.tmp_zip'), 'FileExplorerService::getTempZipDir utiliza /.tmp_zip en el volumen de almacenamiento');
if (DIRECTORY_SEPARATOR === '/') {
    assertTrue(((@fileperms($tmpZipDir) ?: 0) & 0770) === 0770, 'FileExplorerService::getTempZipDir aplica permisos 02770 con escritura de grupo pese a umask');
}
assertTrue($feSizeTest->resolveSafePath('.tmp_zip') === null, 'FileExplorerService::resolveSafePath bloquea acceso directo a .tmp_zip');
assertTrue($feSizeTest->resolveSafePath('.tmp_zip/test.zip') === null, 'FileExplorerService::resolveSafePath bloquea acceso a subrutas dentro de .tmp_zip');

$listAfterTmpZip = $feSizeTest->listDirectory('');
$tmpZipVisible = false;
foreach ($listAfterTmpZip['items'] ?? [] as $it) {
    if (($it['name'] ?? '') === '.tmp_zip') {
        $tmpZipVisible = true;
        break;
    }
}
assertTrue(!$tmpZipVisible, 'FileExplorerService::listDirectory oculta el directorio interno .tmp_zip');

// Verificar compresión y descarga ZIP directa (1 paso) y limpieza posterior en .tmp_zip
ob_start();
$feSizeTest->downloadDirectoryZip($sizeFolderName);
$zipBinaryStream = ob_get_clean();
assertTrue(strlen($zipBinaryStream) > 50 && str_starts_with($zipBinaryStream, "PK\x03\x04"), 'FileExplorerService::downloadDirectoryZip transmite un archivo ZIP válido (cabecera PK)');
$leftoverZips = glob($tmpZipDir . '/nas_export_*.zip') ?: [];
assertTrue($leftoverZips === [], 'FileExplorerService::downloadDirectoryZip elimina el archivo ZIP temporal de .tmp_zip tras finalizar la transmisión');

// Verificar flujo de compresión y descarga en 2 pasos (prepare=true + token) para archivos masivos sin límite de RAM en el navegador
ob_start();
$feSizeTest->downloadDirectoryZip($sizeFolderName, true);
$prepJsonRaw = ob_get_clean();
$prepRes = json_decode((string) $prepJsonRaw, true) ?: [];
$prepToken = (string) ($prepRes['token'] ?? '');
assertTrue(($prepRes['success'] ?? false) === true && strlen($prepToken) === 32, 'FileExplorerService::downloadDirectoryZip(prepareOnly=true) genera token hexadecimal de 32 caracteres');
assertTrue(($prepRes['filename'] ?? '') === $sizeFolderName . '.zip', 'FileExplorerService::downloadDirectoryZip(prepareOnly=true) devuelve el nombre del archivo ZIP');
$preparedZips = glob($tmpZipDir . '/nas_export_' . $prepToken . '.zip') ?: [];
assertTrue(count($preparedZips) === 1, 'FileExplorerService::downloadDirectoryZip(prepareOnly=true) persiste el archivo temporal en .tmp_zip hasta su descarga');

ob_start();
$feSizeTest->downloadDirectoryZip($sizeFolderName, false, $prepToken);
$tokenBinaryStream = ob_get_clean();
assertTrue(strlen($tokenBinaryStream) > 50 && str_starts_with($tokenBinaryStream, "PK\x03\x04"), 'FileExplorerService::downloadDirectoryZip(token) transmite el archivo ZIP preparado');
$leftoverAfterToken = glob($tmpZipDir . '/nas_export_*.zip') ?: [];
assertTrue($leftoverAfterToken === [], 'FileExplorerService::downloadDirectoryZip(token) elimina el archivo ZIP temporal de .tmp_zip tras transmitirlo');

$feSizeTest->deleteItem($sizeFolderName, true);
@rmdir($tmpZipDir);
@rmdir($tempSizeRoot);
FileExplorerService::setRootDir(null);

// Verificar extracción de origen remoto en BackupService::listTasks() con comillas simples (escapeshellarg) y dobles
$bkpOriginDir = sys_get_temp_dir() . '/nas_bkp_runners_' . uniqid();
@mkdir($bkpOriginDir, 0770, true);
$bkpOriginSvc = new BackupService();
$bkpOriginSvc->binDir = $bkpOriginDir;
$bkpOriginSvc->cronDir = $bkpOriginDir;
$bkpOriginSvc->logRoot = $bkpOriginDir;

// Runner 1: CIFS con comillas simples (generado por escapeshellarg en BackupService::buildCifsRunner)
file_put_contents($bkpOriginDir . '/backup_cifs_single.sh', "#!/usr/bin/env bash\nTASK_ID='cifs_single'\nSRC_IP='10.10.1.150'\nSRC_SHARE='Facturacion_2026'\nRETENTION=14\n");
// Runner 2: SSH con comillas simples y puerto personalizado
file_put_contents($bkpOriginDir . '/backup_ssh_single.sh', "#!/usr/bin/env bash\nTASK_ID='ssh_single'\nSRC_IP='10.10.1.160'\nSRC_PORT=2222\nSRC_USER='operador_lnx'\nSRC_PATH='/var/backups/postgres'\nRETENTION=21\n");
// Runner 3: CIFS con comillas dobles (creado desde TUI Bash)
file_put_contents($bkpOriginDir . '/backup_cifs_double.sh', "#!/usr/bin/env bash\nTASK_ID=\"cifs_double\"\nSRC_IP=\"10.10.1.170\"\nSRC_SHARE=\"RecursosHumanos\"\nRETENTION=30\n");
// Runner 4: Local con comillas simples
file_put_contents($bkpOriginDir . '/backup_local_single.sh', "#!/usr/bin/env bash\nTASK_ID='local_single'\nSRC_PATH='/srv/nas/SISTEMAS'\nRETENTION=7\n");

$parsedTasks = $bkpOriginSvc->listTasks();
$taskMap = [];
foreach ($parsedTasks as $pt) {
    $taskMap[$pt['id']] = $pt;
}

assertTrue(($taskMap['cifs_single']['source'] ?? '') === '//10.10.1.150/Facturacion_2026', 'BackupService::listTasks extrae origen CIFS con comillas simples (//10.10.1.150/Facturacion_2026)');
assertTrue(($taskMap['cifs_single']['destination'] ?? '') === '//10.10.1.150/Facturacion_2026', 'BackupService::listTasks incluye clave destination para compatibilidad con CLI');
assertTrue(($taskMap['ssh_single']['source'] ?? '') === 'operador_lnx@10.10.1.160:2222:/var/backups/postgres', 'BackupService::listTasks extrae origen SSH con comillas simples y puerto personalizado');
assertTrue(($taskMap['cifs_double']['source'] ?? '') === '//10.10.1.170/RecursosHumanos', 'BackupService::listTasks extrae origen CIFS con comillas dobles (retrocompatibilidad TUI)');
assertTrue(($taskMap['local_single']['source'] ?? '') === '/srv/nas/SISTEMAS', 'BackupService::listTasks extrae origen Local con comillas simples');

foreach (glob($bkpOriginDir . '/*') ?: [] as $f) {
    @unlink($f);
}
@rmdir($bkpOriginDir);

// ==============================================================================
// 29. Confinamiento de Terminal a Superadmin y Humanización Firewall UFW
// ==============================================================================

// TerminalController exige privilegios estrictos de superadministrador
$_SESSION['nas_user'] = [
    'username' => 'admin_standar',
    'is_admin' => true,
    'is_superadmin' => false,
    'role' => 'admin',
];
$termCtrlStrict = new TerminalController($termSvc);
ob_start();
$termCtrlStrict->exec($reqOperatorManage);
$rawAdminTerm = ob_get_clean();
$adminTermResp = json_decode($rawAdminTerm, true);
assertTrue(is_array($adminTermResp) && empty($adminTermResp['success']) && str_contains($adminTermResp['error'] ?? '', 'superadministrador'),
    'TerminalController::exec rechaza administrador normal con 403 (solo superadmin)');

// TerminalController rechaza todos sus métodos ante administradores normales (solo superadmin)
foreach (['session', 'send', 'capture', 'resize', 'kill', 'history'] as $termMethod) {
    ob_start();
    $termCtrlStrict->$termMethod($reqOperatorManage);
    $rawMethodOut = ob_get_clean();
    $methodResp = json_decode($rawMethodOut, true);
    assertTrue(is_array($methodResp) && empty($methodResp['success']) && str_contains($methodResp['error'] ?? '', 'superadministrador'),
        "TerminalController::{$termMethod} rechaza administrador estándar con 403 (solo superadmin)");
}

// Superadministrador tiene acceso concedido
$_SESSION['nas_user'] = [
    'username' => 'super_root',
    'is_admin' => true,
    'is_superadmin' => true,
    'role' => 'superadmin',
];
ob_start();
$termCtrlStrict->exec($reqOperatorManage);
$rawSuperTerm = ob_get_clean();
$superTermResp = json_decode($rawSuperTerm, true);
assertTrue(is_array($superTermResp) && !empty($superTermResp['success']),
    'TerminalController::exec autoriza a superadministrador');

// AuthMiddleware::isSuperAdmin reconoce superadmin y rechaza admin u operador
assertTrue(AuthMiddleware::isSuperAdmin(), 'AuthMiddleware::isSuperAdmin reconoce superadmin');
$_SESSION['nas_user']['is_superadmin'] = false;
$_SESSION['nas_user']['role'] = 'admin';
assertTrue(!AuthMiddleware::isSuperAdmin(), 'AuthMiddleware::isSuperAdmin rechaza admin regular');

// AuthMiddleware::check bloquea rutas de terminal a usuarios no superadmin
$_SERVER['REQUEST_URI'] = '/api/terminal/session';
$_SERVER['REQUEST_METHOD'] = 'POST';
$_SERVER['HTTP_ACCEPT'] = 'application/json';
$_POST = ['csrf_token' => AuthMiddleware::getCsrfToken()];
$reqTermBlocked = new Request();
ob_start();
$resMwBlocked = AuthMiddleware::check($reqTermBlocked);
$rawMwOut = ob_get_clean();
$mwJson = json_decode($rawMwOut, true);
assertTrue(!$resMwBlocked && is_array($mwJson) && empty($mwJson['success']) && str_contains($mwJson['error'] ?? '', 'superadministrador'),
    'AuthMiddleware::check bloquea /api/terminal/session ante admin regular con 403');

// AuthMiddleware::check bloquea vista GET /terminal ante admin regular
$_SERVER['REQUEST_URI'] = '/terminal';
$_SERVER['REQUEST_METHOD'] = 'GET';
unset($_SERVER['HTTP_ACCEPT']);
$reqViewBlocked = new Request();
ob_start();
$resViewBlocked = AuthMiddleware::check($reqViewBlocked);
$rawViewOut = ob_get_clean();
assertTrue(!$resViewBlocked && str_contains($rawViewOut, '403') && str_contains($rawViewOut, 'superadministradores'),
    'AuthMiddleware::check bloquea GET /terminal ante admin regular emitiendo 403');

// En layout.php, la terminal está completamente excluida del DOM para administradores normales
$_SESSION['nas_user'] = [
    'username' => 'admin_standar',
    'is_admin' => true,
    'is_superadmin' => false,
    'role' => 'admin',
];
ob_start();
$metrics = (new SystemService())->getSystemMetrics();
$storage = (new StorageService())->getStorageOverview();
$services = (new SystemService())->getServicesStatus();
$activeView = 'dashboard';
require dirname(__DIR__) . '/web/templates/layout.php';
$htmlAdmin = ob_get_clean();
assertTrue(!str_contains($htmlAdmin, 'id="nav-terminal"') && !str_contains($htmlAdmin, 'id="view-terminal"') && !str_contains($htmlAdmin, "switchView('terminal')"),
    'layout.php no renderiza #nav-terminal ni #view-terminal ni botón de cabecera a administradores estándar');

// En layout.php, la terminal se renderiza para superadministradores
$_SESSION['nas_user'] = [
    'username' => 'sistemas',
    'is_admin' => true,
    'is_superadmin' => true,
    'role' => 'superadmin',
];
ob_start();
require dirname(__DIR__) . '/web/templates/layout.php';
$htmlSuper = ob_get_clean();
assertTrue(str_contains($htmlSuper, 'id="nav-terminal"') && str_contains($htmlSuper, 'id="view-terminal"'),
    'layout.php renderiza #nav-terminal y #view-terminal para superadministradores');

// Humanización de logs UFW para MNDP MikroTik (UDP 5678 IPv6 Multicast con corchetes RFC 3986)
$ufwRawMndp = '[UFW BLOCK] IN=eth0 OUT= MAC=01:00:5e:00:00:01 SRC=fe80:0000:0000:0000:f61e:57ff:fe0c:31ac DST=ff02:0000:0000:0000:0000:0000:0000:0001 LEN=120 PROTO=UDP SPT=5678 DPT=5678';
$reflection = new ReflectionClass(SystemService::class);
$humanizeMethod = $reflection->getMethod('humanizeSystemLog');
$sysSvcInst = new SystemService();
$humanLog = $humanizeMethod->invoke($sysSvcInst, 'kernel', $ufwRawMndp);
assertTrue(str_contains($humanLog['human'], 'MikroTik MNDP') &&
    str_contains($humanLog['human'], '[fe80::f61e:57ff:fe0c:31ac]:5678') &&
    str_contains($humanLog['human'], '[ff02::1]:5678') &&
    str_contains($humanLog['human'], '[IPv6 Multicast]'),
    'SystemService::humanizeSystemLog traduce y enriquece log UFW MNDP con IPv6 comprimida y corchetes RFC 3986');

// Humanización de logs UFW para SSDP (UDP 1900 IPv4 Multicast)
$ufwRawSsdp = '[UFW BLOCK] IN=eth0 OUT= SRC=10.10.1.50 DST=239.255.255.250 PROTO=UDP SPT=1900 DPT=1900';
$humanLogSsdp = $humanizeMethod->invoke($sysSvcInst, 'kernel', $ufwRawSsdp);
assertTrue(str_contains($humanLogSsdp['human'], 'SSDP') &&
    str_contains($humanLogSsdp['human'], '[IPv4 Multicast]') &&
    !str_contains($humanLogSsdp['human'], '[IPv6 Multicast]'),
    'SystemService::humanizeSystemLog clasifica correctamente multidifusión IPv4 como [IPv4 Multicast]');

// Humanización de cadena textual exacta reportada por el usuario
$ufwUserLiteral = '[UFW] Bloqueado UDP de fe80:0000:0000:0000:f61e:57ff:fe0c:31ac:5678 → ff02:0000:0000:0000:0000:0000:0001:5678';
$humanLogLiteral = $humanizeMethod->invoke($sysSvcInst, 'system', $ufwUserLiteral);
assertTrue(str_contains($humanLogLiteral['human'], 'MikroTik MNDP') &&
    str_contains($humanLogLiteral['human'], '[fe80::f61e:57ff:fe0c:31ac]:5678') &&
    str_contains($humanLogLiteral['human'], '[ff02::1]:5678'),
    'SystemService::humanizeSystemLog traduce correctamente el formato legacy reportado por el usuario');

// Búsqueda semántica en getLogs reconoce el contenido humanizado
$sysMockClass = new class extends SystemService {
    public function getJournalLogs(int $limit = 80, ?string $unit = null): array
    {
        return [
            [
                'timestamp' => '2026-10-19 18:00:00',
                'unit' => 'kernel',
                'message' => '[UFW BLOCK] IN=eth0 OUT= MAC=01:00:5e:00:00:01 SRC=fe80:0000:0000:0000:f61e:57ff:fe0c:31ac DST=ff02:0000:0000:0000:0000:0000:0000:0001 LEN=120 PROTO=UDP SPT=5678 DPT=5678',
            ],
        ];
    }
};
$searchMndp = $sysMockClass->getLogs('system', 10, 'MikroTik');
assertTrue(count($searchMndp) === 1 && str_contains($searchMndp[0]['human'], 'MikroTik MNDP'),
    'SystemService::getLogs permite filtrar registros por términos semánticos humanizados ("MikroTik")');

// Pruebas de captura universal de IP y soporte anti-spam en AuditService
$tempAuditLog = tempnam(sys_get_temp_dir(), 'audit_ip_test_');
AuditService::setLogPath($tempAuditLog);
AuditService::log('login_test', 'test_target', 'SUCCESS', ['ip' => '192.168.1.100']);
$contentAudit = (string) file_get_contents($tempAuditLog);
@unlink($tempAuditLog);
assertTrue(str_contains($contentAudit, '[192.168.1.100]'),
    'AuditService::log respeta captura universal de IP provista explícitamente');

// Pruebas de captura universal de IP en SystemService::parseBackupAuditLog
$tempBackupLog = tempnam(sys_get_temp_dir(), 'backup_ip_test_');
file_put_contents($tempBackupLog, implode("\n", [
    '[2026-10-19 20:00:00] [win_ventas] [10.10.1.50] [MOUNT_SUCCESS] [ok] Recurso montado exitosamente',
    '[2026-10-19 20:00:05] [lnx_prod] [RSYNC_COMPLETED] [notice] Sincronizacion rsync finalizada de root@10.10.1.55:/var/www',
]));
SystemService::$backupAuditPath = $tempBackupLog;
$parsedBkp = $sysSvcInst->parseBackupAuditLog(10);
@unlink($tempBackupLog);
assertTrue(count($parsedBkp) === 2 && $parsedBkp[0]['ip'] === '10.10.1.55' && $parsedBkp[1]['ip'] === '10.10.1.50',
    'SystemService::parseBackupAuditLog captura universalmente la IP en formatos de 5 y 4 corchetes');

// Pruebas de agrupación anti-spam (xN) en SystemService::groupConsecutiveLogs
$rawFloodLogs = [
    [
        'source' => 'system',
        'unit' => 'kernel',
        'user' => 'system',
        'ip' => '10.10.1.200',
        'action' => 'journal_log',
        'target' => 'kernel',
        'status' => 'BLOCK',
        'human' => '[Firewall UFW] Tráfico bloqueado',
        'message' => 'UFW BLOCK',
    ],
    [
        'source' => 'system',
        'unit' => 'kernel',
        'user' => 'system',
        'ip' => '10.10.1.200',
        'action' => 'journal_log',
        'target' => 'kernel',
        'status' => 'BLOCK',
        'human' => '[Firewall UFW] Tráfico bloqueado',
        'message' => 'UFW BLOCK',
    ],
    [
        'source' => 'system',
        'unit' => 'kernel',
        'user' => 'system',
        'ip' => '10.10.1.200',
        'action' => 'journal_log',
        'target' => 'kernel',
        'status' => 'BLOCK',
        'human' => '[Firewall UFW] Tráfico bloqueado',
        'message' => 'UFW BLOCK',
    ],
    [
        'source' => 'system',
        'unit' => 'systemd',
        'user' => 'system',
        'ip' => '127.0.0.1',
        'action' => 'journal_log',
        'target' => 'systemd',
        'status' => 'OK',
        'human' => '[Servicio] smbd iniciado con éxito',
        'message' => 'Started smbd.service',
    ],
];
$groupedResults = $sysSvcInst->groupConsecutiveLogs($rawFloodLogs);
assertTrue(count($groupedResults) === 2,
    'SystemService::groupConsecutiveLogs agrupa eventos consecutivos idénticos');
assertTrue(($groupedResults[0]['count'] ?? 0) === 3 && ($groupedResults[0]['count_badge'] ?? '') === 'x3' && str_contains($groupedResults[0]['human'], '(x3)'),
    'SystemService::groupConsecutiveLogs asigna contador x3 y etiqueta anti-spam');

// Pruebas de diferenciación cromática y humanización en SystemService::humanizeSystemLog
$sshSuccess = $humanizeMethod->invoke($sysSvcInst, 'sshd', 'Accepted password for operador from 192.168.1.50 port 54321 ssh2');
assertTrue($sshSuccess['badge'] === 'ok' && $sshSuccess['status'] === 'OK' && $sshSuccess['ip'] === '192.168.1.50' && str_contains($sshSuccess['human'], 'Acceso concedido a operador'),
    'SystemService::humanizeSystemLog humaniza y diferencia cromáticamente accesos SSH exitosos');

$sshFailed = $humanizeMethod->invoke($sysSvcInst, 'sshd', 'Failed password for invalid user hacker from 203.0.113.5 port 43210 ssh2');
assertTrue($sshFailed['badge'] === 'err' && $sshFailed['status'] === 'FAILED' && $sshFailed['ip'] === '203.0.113.5' && str_contains($sshFailed['human'], 'Intento de acceso fallido para hacker'),
    'SystemService::humanizeSystemLog humaniza y alerta en rojo/err intentos SSH fallidos');

$svcStarted = $sysSvcInst->humanizeSystemLog('systemd', 'Started Samba SMB Daemon.');
assertTrue($svcStarted['badge'] === 'ok' && $svcStarted['status'] === 'OK' && str_contains($svcStarted['human'], 'Samba SMB Daemon iniciado con éxito'),
    'SystemService::humanizeSystemLog traduce y colorea el arranque exitoso de servicios systemd');

$ntpSync = $sysSvcInst->humanizeSystemLog('systemd-timesyncd', 'Initial clock synchronization to Mon 2026-10-19 20:00:00 UTC.');
assertTrue($ntpSync['badge'] === 'ok' && $ntpSync['action_label'] === 'NTP' && str_contains($ntpSync['human'], 'Reloj del sistema sincronizado'),
    'SystemService::humanizeSystemLog clasifica y humaniza sincronizaciones NTP');

// Pruebas de resiliencia en parseBackupAuditLog ante corchetes anidados en el mensaje (4 corchetes)
$tempResilienceLog = tempnam(sys_get_temp_dir(), 'backup_resilience_test_');
file_put_contents($tempResilienceLog, implode("\n", [
    '[2026-10-19 20:00:00] [lnx_prod] [RSYNC_COMPLETED] [notice] [DRY-RUN] Sincronizacion rsync de prueba',
    '[2026-10-19 20:00:01] [win_app] [srv-db.corp] [BACKUP_COMPLETED] [ok] Respaldo de servidor completado',
]));
SystemService::$backupAuditPath = $tempResilienceLog;
$parsedResilience = $sysSvcInst->parseBackupAuditLog(10);
@unlink($tempResilienceLog);
assertTrue(count($parsedResilience) === 2,
    'parseBackupAuditLog procesa correctamente entradas con corchetes en el mensaje y 5 corchetes con host');
assertTrue($parsedResilience[0]['event'] === 'BACKUP_COMPLETED' && $parsedResilience[0]['severity'] === 'ok',
    'parseBackupAuditLog reconoce formato de 5 corchetes con hostname corporativo');
assertTrue($parsedResilience[1]['event'] === 'RSYNC_COMPLETED' && $parsedResilience[1]['severity'] === 'notice' && str_contains($parsedResilience[1]['message'], '[DRY-RUN]'),
    'parseBackupAuditLog no confunde corchetes del mensaje en formato legacy de 4 corchetes');
assertTrue(isset($parsedResilience[1]['action']) && $parsedResilience[1]['action'] !== '',
    'parseBackupAuditLog incluye el campo action para interoperabilidad con CLI y Web');

// Pruebas de invocación pública directa de humanizeSystemLog y patrones enriquecidos
$sshPamOpen = $sysSvcInst->humanizeSystemLog('sshd-session', 'pam_unix(sshd:session): session opened for user sistemas(uid=1000) by (uid=0)');
assertTrue(str_contains($sshPamOpen['human'], 'Sesión de usuario abierta para sistemas') && $sshPamOpen['action_label'] === 'SSH',
    'humanizeSystemLog traduce apertura de sesión pam en sshd-session');

$sudoPamOpen = $sysSvcInst->humanizeSystemLog('sudo', 'pam_unix(sudo:session): session opened for user root(uid=0) by sistemas(uid=1000)');
assertTrue(str_contains($sudoPamOpen['human'], 'Sesión administrativa abierta para root por sistemas') && $sudoPamOpen['action_label'] === 'Sudo',
    'humanizeSystemLog traduce apertura de sesión pam en comandos sudo');

$logindNew = $sysSvcInst->humanizeSystemLog('systemd-logind', 'New session 42 of user sistemas.');
assertTrue(str_contains($logindNew['human'], 'Nueva sesión 42 iniciada para el usuario sistemas') && $logindNew['action_label'] === 'Sesión',
    'humanizeSystemLog traduce creación de sesión en systemd-logind');

// Pruebas de configuración y sincronización de zona horaria local
$currentTimezone = date_default_timezone_get();
assertTrue($currentTimezone !== '', 'La zona horaria de PHP está configurada');
$nowStr = date('Y-m-d H:i:s');
assertTrue(preg_match('/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/', $nowStr) === 1,
    'date() genera formato de marca de tiempo válido con la zona horaria activa');

$metrics = $sysSvcInst->getSystemMetrics();
assertTrue(isset($metrics['server_time']) && isset($metrics['server_timestamp']), 'getSystemMetrics retorna server_time y server_timestamp');
assertTrue(isset($metrics['timezone']) && isset($metrics['timezone_abbr']), 'getSystemMetrics retorna timezone y timezone_abbr');
assertTrue(isset($metrics['timezone_offset']) && is_int($metrics['timezone_offset']), 'getSystemMetrics retorna timezone_offset entero');
assertTrue($metrics['timezone'] === date_default_timezone_get(), 'getSystemMetrics timezone coincide con date_default_timezone_get');

// Test normalizeTimestamp con marca en el futuro simulada desde UTC
$futureUtc = date('Y-m-d H:i:s', time() + 18000);
$normalized = $sysSvcInst->normalizeTimestamp($futureUtc);
$tzOffset = (int) date('Z');
if ($tzOffset < 0) {
    assertTrue(strtotime($normalized) <= time() + 300, 'normalizeTimestamp corrige marcas futuras originadas en UTC');
} else {
    assertTrue(!empty($normalized), 'normalizeTimestamp procesa marcas válidas');
}

// Test layout template contenga los elementos del reloj en vivo y metadatos de sincronización
$layoutSrc = (string) file_get_contents(dirname(__DIR__) . '/web/templates/layout.php');
assertTrue(str_contains($layoutSrc, 'id="header-server-clock"'), 'layout.php contiene el elemento del reloj en vivo header-server-clock');
assertTrue(str_contains($layoutSrc, 'id="header-server-tz"'), 'layout.php contiene el elemento de la zona horaria header-server-tz');
assertTrue(str_contains($layoutSrc, 'window.NAS_INITIAL_SERVER_TIME'), 'layout.php inyecta window.NAS_INITIAL_SERVER_TIME');
assertTrue(str_contains($layoutSrc, 'offset:'), 'layout.php inyecta offset en window.NAS_INITIAL_SERVER_TIME');

// Test motor de reloj en app.js soporte Intl y fallback offset
$appJsSrc = (string) file_get_contents(dirname(__DIR__) . '/web/public/js/app.js');
assertTrue(str_contains($appJsSrc, '_serverClock'), 'app.js define objeto global _serverClock');
assertTrue(str_contains($appJsSrc, 'Intl.DateTimeFormat'), 'app.js utiliza Intl.DateTimeFormat para la zona IANA del servidor');
assertTrue(str_contains($appJsSrc, 'getUTCHours'), 'app.js incluye fallback matemático getUTCHours inmune al cliente');

// 27. Pruebas de mejoras de Respaldo, Auditoría de ACL y Modales
$bkpTestSvc = new BackupService();
$bkpTestSvc->binDir = sys_get_temp_dir() . '/nas_test_bin_' . bin2hex(random_bytes(4));
$bkpTestSvc->cronDir = sys_get_temp_dir() . '/nas_test_cron_' . bin2hex(random_bytes(4));
$bkpTestSvc->credDir = sys_get_temp_dir() . '/nas_test_cred_' . bin2hex(random_bytes(4));
$bkpTestSvc->bkpRoot = sys_get_temp_dir() . '/nas_test_bkp_' . bin2hex(random_bytes(4));
$bkpTestSvc->logRoot = sys_get_temp_dir() . '/nas_test_log_' . bin2hex(random_bytes(4));
@mkdir($bkpTestSvc->binDir, 0755, true);
@mkdir($bkpTestSvc->cronDir, 0755, true);
@mkdir($bkpTestSvc->credDir, 0700, true);
@mkdir($bkpTestSvc->bkpRoot, 0755, true);
@mkdir($bkpTestSvc->logRoot, 0755, true);

// 27.1 BackupService::updateTask validaciones
$badIdRes = $bkpTestSvc->updateTask('bad id!', []);
assertTrue(!$badIdRes['success'], 'BackupService::updateTask rechaza identificadores con caracteres inválidos');

$badLenRes = $bkpTestSvc->updateTask('a', []);
assertTrue(!$badLenRes['success'], 'BackupService::updateTask rechaza identificadores con longitud menor a 2');

// 27.2 BackupService::getRunningBackups
$runningRes = $bkpTestSvc->getRunningBackups();
assertTrue(is_array($runningRes) && isset($runningRes['running']) && is_bool($runningRes['running']), 'BackupService::getRunningBackups devuelve campo booleano running');
assertTrue(isset($runningRes['tasks']) && is_array($runningRes['tasks']), 'BackupService::getRunningBackups devuelve lista de tareas');
assertTrue(isset($runningRes['rsync_active']) && is_bool($runningRes['rsync_active']), 'BackupService::getRunningBackups devuelve campo booleano rsync_active');
assertTrue(isset($runningRes['locks']) && is_array($runningRes['locks']), 'BackupService::getRunningBackups devuelve lista de locks');

// 27.3 BackupController endpoints
$bkpCtrlTest = new \App\Controllers\BackupController();

// update endpoint
$reqEmpty = new Request();
ob_start();
$bkpCtrlTest->update($reqEmpty);
$outEmpty = ob_get_clean();
$jsonEmpty = json_decode((string) $outEmpty, true);
assertTrue(!empty($jsonEmpty) && empty($jsonEmpty['success']), 'BackupController::update rechaza peticiones con identificador vacío');

$reqBadId = new Request([], [], [], [], [], ['id' => 'INVALID CHARS!!']);
ob_start();
$bkpCtrlTest->update($reqBadId);
$outBadId = ob_get_clean();
$jsonBadId = json_decode((string) $outBadId, true);
assertTrue(!empty($jsonBadId) && empty($jsonBadId['success']), 'BackupController::update rechaza identificadores no conformes a regex');

// running endpoint
$reqRunning = new Request();
ob_start();
$bkpCtrlTest->running($reqRunning);
$outRunning = ob_get_clean();
$jsonRunning = json_decode((string) $outRunning, true);
assertTrue(!empty($jsonRunning) && !empty($jsonRunning['success']) && isset($jsonRunning['data']['running']), 'BackupController::running devuelve respuesta exitosa con estructura running');

// 27.4 SambaService::auditShareAcls y auditAcls
$tmpAuditSmb = tempnam(sys_get_temp_dir(), 'nas_audit_smb_');
file_put_contents($tmpAuditSmb, "[global]\nworkgroup = TEAM-JOFRATO\n\n[AUDIT_SHARE]\npath = /srv/nas/AUDIT_SHARE\nvalid users = @grp_ventas\nwrite list = @grp_admin\nread only = no\n");
$sambaAuditSvc = new SambaService($tmpAuditSmb);

$auditMissing = $sambaAuditSvc->auditShareAcls('NO_EXISTE');
assertTrue(!$auditMissing['success'], 'SambaService::auditShareAcls reporta success=false para recurso inexistente');
assertTrue(!$auditMissing['in_sync'], 'SambaService::auditShareAcls reporta in_sync=false para recurso inexistente');

$auditSingle = $sambaAuditSvc->auditShareAcls('AUDIT_SHARE');
assertTrue(isset($auditSingle['in_sync']) && isset($auditSingle['discrepancies']), 'SambaService::auditShareAcls devuelve campos in_sync y discrepancies');

$auditAll = $sambaAuditSvc->auditAcls();
assertTrue(isset($auditAll['shares']) && isset($auditAll['total']) && isset($auditAll['out_of_sync_count']), 'SambaService::auditAcls devuelve resumen estructurado');

// Recuperación de credenciales previas en testConnection
@mkdir($bkpTestSvc->credDir, 0700, true);
file_put_contents($bkpTestSvc->credDir . '/test_conn_cred.cred', "username=AdminCorp\npassword=SecretPass123\ndomain=CORP\n");
$testConnRes = $bkpTestSvc->testConnection(['task_id' => 'test_conn_cred', 'proto' => 'cifs', 'ip' => '10.10.1.50', 'share' => 'Ventas']);
assertTrue(!empty($testConnRes['success']), 'BackupService::testConnection recupera credenciales almacenadas si password viene vacío');
@unlink($bkpTestSvc->credDir . '/test_conn_cred.cred');

// Rechazo de updateTask si la tarea está en curso
$runningMockSvc = new class extends BackupService {
    public function getTaskStatus(string $taskId): string {
        return 'running';
    }
};
$runningMockSvc->binDir = $bkpTestSvc->binDir;
$runningMockSvc->cronDir = $bkpTestSvc->cronDir;
$runningMockSvc->credDir = $bkpTestSvc->credDir;
@mkdir($runningMockSvc->binDir, 0755, true);
file_put_contents($runningMockSvc->binDir . '/backup_tarea_en_curso.sh', "#!/bin/bash\n");
$updRunningRes = $runningMockSvc->updateTask('tarea_en_curso', ['proto' => 'cifs', 'ip' => '10.10.1.50', 'share' => 'Ventas']);
assertTrue(empty($updRunningRes['success']) && str_contains($updRunningRes['error'] ?? '', 'ejecución'), 'BackupService::updateTask rechaza actualización si la tarea se encuentra en ejecución');
@unlink($runningMockSvc->binDir . '/backup_tarea_en_curso.sh');

// 27.5 SambaController::auditAccess
$sambaCtrlTest = new \App\Controllers\SambaController();
$reqAuditAcc = new Request();
ob_start();
$sambaCtrlTest->auditAccess($reqAuditAcc);
$outAuditAcc = ob_get_clean();
$jsonAuditAcc = json_decode((string) $outAuditAcc, true);
assertTrue(!empty($jsonAuditAcc) && !empty($jsonAuditAcc['success']) && isset($jsonAuditAcc['data']['shares']), 'SambaController::auditAccess devuelve listado de auditoría');

// 27.6 Verificaciones de Modales y Scripts en Frontend
assertTrue(str_contains($layoutSrc, 'id="modal-edit-backup"'), 'layout.php contiene modal #modal-edit-backup');
assertTrue(str_contains($layoutSrc, 'id="modal-zip-backup-warning"'), 'layout.php contiene modal #modal-zip-backup-warning');
assertTrue(str_contains($layoutSrc, 'modal-backdrop modal-static" id="modal-zip-backup-warning"'), 'layout.php protege #modal-zip-backup-warning con modal-static');
assertTrue(str_contains($layoutSrc, 'modal-backdrop modal-static" id="modal-delete-file"'), 'layout.php protege #modal-delete-file con modal-static');
assertTrue(str_contains($layoutSrc, 'modal-backdrop modal-static" id="modal-edit-backup"'), 'layout.php protege #modal-edit-backup con modal-static');
assertTrue(str_contains($layoutSrc, 'id="edit-bkp-id"'), 'layout.php contiene campo edit-bkp-id para edicion');
assertTrue(str_contains($layoutSrc, 'id="edit-bkp-proto"'), 'layout.php contiene selector edit-bkp-proto');

assertTrue(str_contains($appJsSrc, 'openEditBackup'), 'app.js define función openEditBackup');
assertTrue(str_contains($appJsSrc, 'submitEditBackup'), 'app.js define función submitEditBackup');
assertTrue(str_contains($appJsSrc, 'toggleEditBackupFields'), 'app.js define función toggleEditBackupFields');
assertTrue(str_contains($appJsSrc, '/api/backups/running'), 'app.js consulta /api/backups/running antes de comprimir ZIP');
assertTrue(str_contains($appJsSrc, 'modal-pulse'), 'app.js aplica efecto modal-pulse al interactuar con modales estáticos o formularios');
assertTrue(str_contains($appJsSrc, 'modal-static'), 'app.js protege modales con clase .modal-static');
assertTrue(str_contains($appJsSrc, '/api/shares/access/audit'), 'app.js audita previamente en repairAcls');

// Limpieza de directorios temporales de prueba
@unlink($tmpAuditSmb);
@rmdir($bkpTestSvc->binDir);
@rmdir($bkpTestSvc->cronDir);
@rmdir($bkpTestSvc->credDir);
@rmdir($bkpTestSvc->bkpRoot);
@rmdir($bkpTestSvc->logRoot);

echo "\n==================================================\n";
echo "RESULTADO: $passed pasadas, $failed fallidas.\n";
echo "==================================================\n";

if ($failed > 0) {
    exit(1);
}


