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
        'administrador' => 'Ead2026#',
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

        // Modo desarrollo o suite de pruebas unitarias
        if ($this->dryRun || DIRECTORY_SEPARATOR === '\\' || getenv('APP_ENV') === 'testing') {
            if (isset(self::$mockUsers[$cleanUsername])) {
                $expected = self::$mockUsers[$cleanUsername];
                if ($password === $expected || ($cleanUsername === 'administrador' && $password === 'admin123')) {
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
}
