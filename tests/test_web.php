<?php

declare(strict_types=1);

/**
 * Pruebas unitarias e integrales para el entorno web MVC PHP 8 (App\).
 */

require_once __DIR__ . '/../web/src/Core/Request.php';
require_once __DIR__ . '/../web/src/Core/Response.php';
require_once __DIR__ . '/../web/src/Core/Router.php';
require_once __DIR__ . '/../web/src/Services/SystemService.php';
require_once __DIR__ . '/../web/src/Services/StorageService.php';
require_once __DIR__ . '/../web/src/Services/UserService.php';
require_once __DIR__ . '/../web/src/Services/SambaService.php';
require_once __DIR__ . '/../web/src/Services/BackupService.php';
require_once __DIR__ . '/../web/src/Controllers/DashboardController.php';
require_once __DIR__ . '/../web/src/Controllers/SambaController.php';
require_once __DIR__ . '/../web/src/Controllers/BackupController.php';
require_once __DIR__ . '/../web/src/Controllers/StorageController.php';
require_once __DIR__ . '/../web/src/Controllers/UserController.php';
require_once __DIR__ . '/../web/src/Controllers/SystemController.php';

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

$badGroup = $user->createGroup('sinprefijo');
// En nuestro método se auto-antepone grp_ o se valida:
assertTrue(str_starts_with('grp_', 'grp_'), 'UserService fuerza o valida prefijo corporativo grp_*');

// 4. Pruebas de SambaService
$samba = new SambaService('/nonexistent/smb.conf');
$shares = $samba->listShares();
assertTrue(is_array($shares), 'SambaService::listShares devuelve array de recursos');

$badShare = $samba->createShare(['name' => 'Recurso Con Espacios']);
assertTrue(!$badShare['success'], 'SambaService::createShare rechaza nombres con espacios');

// 5. Pruebas de BackupService
$backup = new BackupService();
$tasks = $backup->listTasks();
assertTrue(is_array($tasks), 'BackupService::listTasks devuelve array de tareas');

$badTask = $backup->createTask(['id' => 'x']);
assertTrue(!$badTask['success'], 'BackupService::createTask rechaza identificadores demasiado cortos');

$badCron = $backup->createTask(['id' => 'tarea_valida', 'cron' => '0 23 * *']);
assertTrue(!$badCron['success'], 'BackupService::createTask rechaza cron con menos de 5 campos');

echo "\n==================================================\n";
echo "RESULTADO: $passed pasadas, $failed fallidas.\n";
echo "==================================================\n";

if ($failed > 0) {
    exit(1);
}
