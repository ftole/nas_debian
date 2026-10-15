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

// Restaurar rol por defecto
putenv('NAS_SERVER_ROLE=ARCHIVOS_BACKUP');

// Restaurar rutas originales y limpiar temporales
SystemService::$sambaAuditPath = $origSambaPath;
SystemService::$adminAuditPath = $origAdminPath;
SystemService::$backupAuditPath = $origBackupPath;
@unlink($tempSamba);
@unlink($tempAdmin);
@unlink($tempBackup);

echo "\n==================================================\n";
echo "RESULTADO: $passed pasadas, $failed fallidas.\n";
echo "==================================================\n";

if ($failed > 0) {
    exit(1);
}
