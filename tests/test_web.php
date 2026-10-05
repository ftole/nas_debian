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

// 14. Pruebas de Request::getClientIp
$_SERVER['REMOTE_ADDR'] = '192.168.1.150';
unset($_SERVER['HTTP_X_FORWARDED_FOR'], $_SERVER['HTTP_CLIENT_IP']);
$ipReq1 = new Request();
assertTrue($ipReq1->getClientIp() === '192.168.1.150', 'Request::getClientIp resuelve REMOTE_ADDR');

$_SERVER['HTTP_X_FORWARDED_FOR'] = '10.20.30.40, 192.168.1.150';
$ipReq2 = new Request();
assertTrue($ipReq2->getClientIp() === '10.20.30.40', 'Request::getClientIp prioriza primera IP de HTTP_X_FORWARDED_FOR');

unset($_SERVER['HTTP_X_FORWARDED_FOR']);
$_SERVER['HTTP_CLIENT_IP'] = '172.16.5.99';
$ipReq3 = new Request();
assertTrue($ipReq3->getClientIp() === '172.16.5.99', 'Request::getClientIp resuelve HTTP_CLIENT_IP');

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

// Detección de comandos interactivos incompatibles (su, sudo -i, nano, top)
$suWarn = $terminal->execute('su -', '/srv/nas');
assertTrue($suWarn['success'] && str_contains($suWarn['output'], 'Aviso de terminal interactiva'), 'TerminalService detecta su - y sugiere alternativas sin error fatal');

$nanoWarn = $terminal->execute('nano /etc/samba/smb.conf', '/srv/nas');
assertTrue($nanoWarn['success'] && str_contains($nanoWarn['output'], 'editor interactivo'), 'TerminalService detecta nano y previene cuelgues');

$topWarn = $terminal->execute('top', '/srv/nas');
assertTrue($topWarn['success'] && str_contains($topWarn['output'], 'monitor interactivo'), 'TerminalService detecta top y sugiere ps o dashboard');

// Comando help nativo
$helpRes = $terminal->execute('help', '/srv/nas');
assertTrue($helpRes['success'] && str_contains($helpRes['output'], 'Comandos rápidos del sistema'), 'TerminalService provee guía de comandos con help');

// Historial en base de datos
$history = $terminal->getHistory(5);
assertTrue(is_array($history), 'TerminalService::getHistory retorna historial estructurado');

// 22. Pruebas de FileExplorerService
$fileExp = new FileExplorerService();

// Verificación de protección Jail Traversal
$safePath = $fileExp->resolveSafePath('../../../etc/passwd');
assertTrue($safePath === null || !str_contains($safePath, 'etc/passwd'), 'FileExplorerService::resolveSafePath bloquea escape de directorio (Path Traversal)');

// Directorio temporal para pruebas completas de explorador
$tempExpDir = sys_get_temp_dir() . '/nas_test_explorer_' . uniqid();
@mkdir($tempExpDir, 0770, true);
file_put_contents($tempExpDir . '/documento.txt', 'Contenido confidencial');

FileExplorerService::setRootDir($tempExpDir);
$listRes = $fileExp->listDirectory('');
assertTrue($listRes['success'], 'FileExplorerService::listDirectory retorna éxito en directorio válido');
assertTrue(is_array($listRes['items']) && count($listRes['items']) >= 1, 'FileExplorerService lista archivos existentes');
assertTrue(($listRes['items'][0]['name'] ?? '') === 'documento.txt', 'FileExplorerService detecta nombre de archivo');
assertTrue(isset($listRes['data']) && is_array($listRes['data']['items']), 'FileExplorerService retorna bloque anidado data');
assertTrue(isset($listRes['items'][0]['relative_path']) && isset($listRes['items'][0]['modified_at']), 'FileExplorerService provee relative_path y modified_at');
assertTrue(isset($listRes['items'][0]['owner']) && isset($listRes['items'][0]['group']), 'FileExplorerService provee propietario y grupo POSIX');

// Crear subcarpeta
$mkdirRes = $fileExp->createDirectory('', 'Subcarpeta_Test');
assertTrue($mkdirRes['success'] && is_dir($tempExpDir . '/Subcarpeta_Test'), 'FileExplorerService::createDirectory crea subdirectorio');

// Renombrar archivo
$renameRes = $fileExp->renameItem('documento.txt', 'documento_renombrado.txt');
assertTrue($renameRes['success'] && file_exists($tempExpDir . '/documento_renombrado.txt'), 'FileExplorerService::renameItem renombra elemento');

// Eliminar archivo
$delRes = $fileExp->deleteItem('documento_renombrado.txt');
assertTrue($delRes['success'] && !file_exists($tempExpDir . '/documento_renombrado.txt'), 'FileExplorerService::deleteItem elimina elemento');

// Limpieza de temporal
@rmdir($tempExpDir . '/Subcarpeta_Test');
@unlink($tempExpDir . '/documento_renombrado.txt');
@rmdir($tempExpDir);
FileExplorerService::setRootDir(null);

// 23. Pruebas de DomainService
$domain = new DomainService();
$domainStatus = $domain->getStatus();
assertTrue(isset($domainStatus['joined']), 'DomainService::getStatus retorna estado joined');
assertTrue(isset($domainStatus['workgroup']), 'DomainService::getStatus retorna grupo de trabajo');

$invalidDisc = $domain->discover('!!dominio_invalido!!');
assertTrue(!$invalidDisc['success'], 'DomainService::discover rechaza nombres de dominio con formato inválido');

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
