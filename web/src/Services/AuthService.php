<?php

declare(strict_types=1);

namespace App\Services;

/**
 * Servicio de autenticación y validación de credenciales del sistema (Linux / Samba).
 * Permite validar usuarios del sistema mediante SMB/PAM y restringir el acceso
 * al panel web exclusivamente a cuentas administrativas (administrador, sistemas, grp_samba, sudo).
 */
class AuthService
{
    public bool $dryRun = false;

    /**
     * Almacén en memoria de usuarios de prueba (sin contraseñas fijas en código fuente).
     */
    public static array $mockUsers = [];

    /**
     * Obtiene las credenciales para la suite de pruebas unitarias desde variables de entorno
     * o desde el almacén en memoria configurado en el test runner.
     */
    public static function getTestCredentials(): array
    {
        $creds = self::$mockUsers;
        $adminPass = getenv('NAS_TEST_ADMIN_PASS');
        if ($adminPass !== false && $adminPass !== '' && !isset($creds['administrador'])) {
            $creds['administrador'] = (string) $adminPass;
        }
        $sistemasPass = getenv('NAS_TEST_SISTEMAS_PASS');
        if ($sistemasPass !== false && $sistemasPass !== '' && !isset($creds['sistemas'])) {
            $creds['sistemas'] = (string) $sistemasPass;
        }
        return $creds;
    }

    /**
     * Autentica a un usuario validando credenciales contra Samba y verificando privilegios de administración.
     *
     * @param string $username Nombre de la cuenta de usuario.
     * @param string $password Contraseña de la cuenta.
     * @return array Resultado de autenticación: ['success' => bool, 'user' => array, 'error' => ?string]
     */
    public function authenticate(string $username, string $password): array
    {
        $cleanUsername = strtolower(trim($username));

        if (empty($cleanUsername) || empty($password)) {
            return [
                'success' => false,
                'error' => 'Nombre de usuario y contraseña son obligatorios.',
            ];
        }

        if (!preg_match('/^[a-z0-9_-]{2,32}$/', $cleanUsername)) {
            return [
                'success' => false,
                'error' => 'Formato de nombre de usuario inválido.',
            ];
        }

        // Modo suite de pruebas unitarias explícito
        if (getenv('APP_ENV') === 'testing') {
            $testCreds = self::getTestCredentials();
            if (isset($testCreds[$cleanUsername])) {
                $expected = $testCreds[$cleanUsername];
                $isValid = ($password === $expected);
                if (!$isValid && $cleanUsername === 'administrador') {
                    $tokens = array_filter(explode(',', (string) (getenv('NAS_TEST_ADMIN_TOKENS') ?: '')));
                    if (in_array($password, $tokens, true)) {
                        $isValid = true;
                    }
                }
                if ($isValid) {
                    $isSuper = ($cleanUsername === 'administrador');
                    return [
                        'success' => true,
                        'user' => [
                            'username' => $cleanUsername,
                            'is_superadmin' => $isSuper,
                            'is_admin' => true,
                            'can_web' => true,
                            'role' => $isSuper ? 'superadmin' : 'admin',
                            'role_label' => $isSuper ? 'Superadministrador' : 'Administrador de Sistemas',
                        ],
                    ];
                }
            }
            return [
                'success' => false,
                'error' => 'Usuario o contraseña incorrectos.',
            ];
        }

        // En entornos Windows de desarrollo sin Samba real, rechazar si se fuerza producción
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                'success' => false,
                'error' => 'Autenticación real no disponible en entorno Windows local.',
            ];
        }

        // Entorno Debian 13 en producción: Validar contra Samba mediante smbclient.
        // Primero, verificar que el usuario exista en la base de cuentas de Samba (pdbedit)
        // para prevenir autenticaciones espurias debidas al mapeo a invitado (map to guest = Bad User).
        $userCheck = SystemService::sudo(['pdbedit', '-v', '-u', $cleanUsername]);
        if ($userCheck['code'] !== 0) {
            return [
                'success' => false,
                'error' => 'Usuario o contraseña incorrectos.',
            ];
        }

        $res = SystemService::sudo(
            ['smbclient', '//127.0.0.1/IPC$', '-U', $cleanUsername, '-c', 'exit'],
            $password . "\n"
        );

        // Fallback a ejecución directa si sudo fallara
        if ($res['code'] !== 0 && str_contains($res['stderr'], 'sudo:')) {
            $res = SystemService::runCommand(
                ['smbclient', '//127.0.0.1/IPC$', '-U', $cleanUsername, '-c', 'exit'],
                $password . "\n"
            );
        }

        $allOutput = $res['stdout'] . ' ' . $res['stderr'];
        if ($res['code'] !== 0 || str_contains($allOutput, 'NT_STATUS_LOGON_FAILURE')) {
            return [
                'success' => false,
                'error' => 'Usuario o contraseña incorrectos.',
            ];
        }

        // Determinar rol: superadmin (grp_superadmin), admin (sudo/grp_samba) u operador (grp_web)
        $groups = [];
        $grpRes = SystemService::runCommand(['id', '-Gn', $cleanUsername]);
        if ($grpRes['code'] === 0) {
            $groups = preg_split('/\s+/', trim($grpRes['stdout']));
        }

        $configuredSuper = $this->getConfiguredSuperadmin();
        $isSuper = in_array('grp_superadmin', $groups, true)
            || ($configuredSuper !== '' && $cleanUsername === $configuredSuper);
        $isAdmin = $isSuper
            || in_array($cleanUsername, ['administrador', 'sistemas'], true)
            || in_array('grp_samba', $groups, true)
            || in_array('sudo', $groups, true);
        $isOperator = in_array('grp_web', $groups, true);
        $canWeb = $isAdmin || $isOperator;

        if (!$canWeb) {
            return [
                'success' => false,
                'error' => 'Acceso denegado: La cuenta no tiene permisos de acceso al panel web.',
            ];
        }

        $role = $isSuper ? 'superadmin' : ($isAdmin ? 'admin' : 'operator');
        $roleLabel = [
            'superadmin' => 'Superadministrador',
            'admin' => 'Administrador de Sistemas',
            'operator' => 'Operador Web',
        ][$role];

        return [
            'success' => true,
            'user' => [
                'username' => $cleanUsername,
                'is_superadmin' => $isSuper,
                'is_admin' => $isAdmin,
                'can_web' => $canWeb,
                'role' => $role,
                'role_label' => $roleLabel,
            ],
        ];
    }

    /**
     * Lee el superadministrador canónico registrado por el despliegue (/etc/nas/superadmin).
     */
    public function getConfiguredSuperadmin(): string
    {
        foreach (['/etc/nas/superadmin'] as $file) {
            if (is_readable($file)) {
                $name = strtolower(trim((string) @file_get_contents($file)));
                if ($name !== '' && preg_match('/^[a-z0-9_-]{2,32}$/', $name)) {
                    return $name;
                }
            }
        }
        return '';
    }

    /**
     * Cierra la sesión activa en el servidor y limpia las cookies asociadas.
     */
    public function logout(): void
    {
        if (session_status() === PHP_SESSION_NONE) {
            @session_start();
        }

        $_SESSION = [];

        if (ini_get('session.use_cookies') && !headers_sent()) {
            $params = session_get_cookie_params();
            setcookie(
                session_name(),
                '',
                time() - 42000,
                $params['path'],
                $params['domain'],
                $params['secure'],
                $params['httponly']
            );
        }

        if (session_status() === PHP_SESSION_ACTIVE) {
            @session_destroy();
        }
    }

    public const MAX_FAILED_ATTEMPTS = 5;
    public const LOCKOUT_MINUTES = 5;

    /**
     * Verifica si una dirección IP se encuentra temporalmente bloqueada por superar el límite de intentos.
     */
    public function isRateLimited(string $ip): bool
    {
        try {
            $rows = DatabaseService::query(
                "SELECT COUNT(*) as cnt FROM login_attempts WHERE ip = :ip AND attempted_at >= datetime('now', '-" . self::LOCKOUT_MINUTES . " minutes')",
                ['ip' => $ip]
            );
            $count = (int) ($rows[0]['cnt'] ?? 0);
            return $count >= self::MAX_FAILED_ATTEMPTS;
        } catch (\Throwable $e) {
            error_log('Error comprobando rate limit: ' . $e->getMessage());
            return false;
        }
    }

    /**
     * Registra un intento de inicio de sesión fallido en SQLite.
     */
    public function recordFailedAttempt(string $ip, string $username): void
    {
        try {
            DatabaseService::insert('login_attempts', [
                'ip' => $ip,
                'username' => $username,
            ]);
        } catch (\Throwable $e) {
            error_log('Error registrando intento fallido: ' . $e->getMessage());
        }
    }

    /**
     * Limpia los intentos fallidos registrados para una IP tras un inicio de sesión exitoso.
     */
    public function clearFailedAttempts(string $ip): void
    {
        try {
            DatabaseService::execute(
                'DELETE FROM login_attempts WHERE ip = :ip',
                ['ip' => $ip]
            );
        } catch (\Throwable $e) {
            error_log('Error limpiando intentos fallidos: ' . $e->getMessage());
        }
    }

    /**
     * Comprueba y registra una petición contra el Rate Limiter global en SQLite.
     * Retorna true si la petición es admitida, o false si excede el límite permitido.
     *
     * @param string $key Identificador único (ej: ip:ruta o usuario:ruta)
     * @param int $maxHits Número máximo de peticiones permitidas en la ventana de tiempo.
     * @param int $windowSeconds Duración de la ventana de tiempo en segundos.
     * @return bool True si está dentro del límite, false si fue excedido.
     */
    public static function checkRateLimit(string $key, int $maxHits = 10, int $windowSeconds = 60): bool
    {
        try {
            // Poda probabilística (5%) de registros antiguos fuera de la ventana
            if (random_int(1, 20) === 1) {
                DatabaseService::execute(
                    "DELETE FROM api_rate_limits WHERE created_at < datetime('now', '-" . ($windowSeconds * 2) . " seconds')"
                );
            }

            $rows = DatabaseService::query(
                "SELECT COUNT(*) as cnt FROM api_rate_limits WHERE key = :key AND created_at >= datetime('now', '-{$windowSeconds} seconds')",
                ['key' => $key]
            );
            $count = (int) ($rows[0]['cnt'] ?? 0);
            if ($count >= $maxHits) {
                return false;
            }

            DatabaseService::insert('api_rate_limits', ['key' => $key]);
            return true;
        } catch (\Throwable $e) {
            error_log('Error comprobando api rate limit: ' . $e->getMessage());
            return true;
        }
    }

    /**
     * Limpia los registros de rate limit para una clave específica (útil en testing o desbloqueo manual).
     */
    public static function clearRateLimits(string $key): void
    {
        try {
            DatabaseService::execute(
                'DELETE FROM api_rate_limits WHERE key = :key',
                ['key' => $key]
            );
        } catch (\Throwable $e) {
            error_log('Error limpiando api rate limit: ' . $e->getMessage());
        }
    }
}
