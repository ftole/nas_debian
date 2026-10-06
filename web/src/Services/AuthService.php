<?php

declare(strict_types=1);

namespace App\Services;

/**
 * Servicio de autenticación y validación de credenciales del sistema (Linux / Samba).
 * Permite validar usuarios del sistema mediante SMB/PAM y restringir el acceso
 * al panel web exclusivamente a cuentas administrativas (administrador, sistemas, grp_sistemas, sudo).
 */
class AuthService
{
    public bool $dryRun = false;

    /**
     * Credenciales predeterminadas para entorno de pruebas o desarrollo.
     */
    public static array $mockUsers = [
        'administrador' => 'Admin123#',
        'sistemas' => 'Ead2026#',
    ];

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

        // Modo suite de pruebas unitarias o desarrollo explícito
        if ($this->dryRun || getenv('APP_ENV') === 'testing') {
            if (isset(self::$mockUsers[$cleanUsername])) {
                $expected = self::$mockUsers[$cleanUsername];
                if ($password === $expected || ($cleanUsername === 'administrador' && in_array($password, ['Admin123#', 'admin123', 'Ead2026#'], true))) {
                    return [
                        'success' => true,
                        'user' => [
                            'username' => $cleanUsername,
                            'is_admin' => true,
                            'role' => 'Administrador de Sistemas',
                        ],
                    ];
                }
            }
            return [
                'success' => false,
                'error' => 'Usuario o contraseña incorrectos.',
            ];
        }

        // Entorno Debian 13 en producción: Validar contra Samba mediante smbclient
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

        // Validar si la cuenta pertenece a un grupo administrativo (sudo, grp_sistemas) o es administrador/sistemas
        $isAuthorized = false;
        if (in_array($cleanUsername, ['administrador', 'sistemas'], true)) {
            $isAuthorized = true;
        } else {
            $grpRes = SystemService::runCommand(['id', '-Gn', $cleanUsername]);
            if ($grpRes['code'] === 0) {
                $groups = preg_split('/\s+/', trim($grpRes['stdout']));
                if (in_array('grp_sistemas', $groups, true) || in_array('sudo', $groups, true)) {
                    $isAuthorized = true;
                }
            }
        }

        if (!$isAuthorized) {
            return [
                'success' => false,
                'error' => 'Acceso denegado: La cuenta no cuenta con permisos administrativos en el panel.',
            ];
        }

        return [
            'success' => true,
            'user' => [
                'username' => $cleanUsername,
                'is_admin' => true,
                'role' => 'Administrador de Sistemas',
            ],
        ];
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
}
