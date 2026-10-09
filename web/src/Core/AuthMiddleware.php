<?php

declare(strict_types=1);

namespace App\Core;

/**
 * Middleware de protección de rutas y endpoints de API.
 * Garantiza que toda petición esté respaldada por una sesión autenticada válida,
 * permitiendo únicamente las rutas públicas del flujo de inicio de sesión.
 */
class AuthMiddleware
{
    /**
     * Inicializa la sesión con directivas de cookies seguras si no estuviera activa.
     */
    public static function initSession(): void
    {
        if (session_status() === PHP_SESSION_NONE) {
            if (!headers_sent()) {
                $isHttps = (!empty($_SERVER['HTTPS']) && $_SERVER['HTTPS'] !== 'off')
                    || (isset($_SERVER['SERVER_PORT']) && (int) $_SERVER['SERVER_PORT'] === 443);

                @session_set_cookie_params([
                    'lifetime' => 0,
                    'path' => '/',
                    'domain' => '',
                    'secure' => $isHttps,
                    'httponly' => true,
                    'samesite' => 'Lax',
                ]);
            }

            @session_start();
        }

        if (empty($_SESSION['csrf_token'])) {
            $_SESSION['csrf_token'] = bin2hex(random_bytes(32));
        }
    }

    /**
     * Obtiene el token CSRF actual de la sesión, generándolo si aún no existiera.
     */
    public static function getCsrfToken(): string
    {
        self::initSession();
        if (empty($_SESSION['csrf_token'])) {
            $_SESSION['csrf_token'] = bin2hex(random_bytes(32));
        }
        return (string) $_SESSION['csrf_token'];
    }

    /**
     * Valida de manera segura en tiempo constante un token CSRF provisto.
     */
    public static function verifyCsrfToken(?string $token): bool
    {
        self::initSession();
        $sessionToken = $_SESSION['csrf_token'] ?? '';
        if (empty($sessionToken) || empty($token)) {
            return false;
        }
        return hash_equals($sessionToken, $token);
    }

    /**
     * Valida si la petición actual está autorizada para continuar.
     *
     * @param Request $request
     * @return bool True si la petición procede, o detiene la ejecución emitiendo 401/403 o redirección.
     */
    public static function check(Request $request): bool
    {
        self::initSession();

        $path = $request->getPath();
        $method = $request->getMethod();

        // Rutas públicas exentas de autenticación
        $publicPaths = [
            '/login',
            '/api/auth/login',
        ];

        $isPublic = in_array($path, $publicPaths, true);

        // Si no es ruta pública, requerir sesión de usuario activa
        if (!$isPublic) {
            if (empty($_SESSION['nas_user']) || !is_array($_SESSION['nas_user'])) {
                if ($request->isJson()) {
                    Response::error('Acceso no autorizado. Inicia sesión en el panel para continuar.', 401);
                } else {
                    if (!headers_sent()) {
                        header('Location: /login');
                    }
                    if (getenv('APP_ENV') !== 'testing') {
                        exit;
                    }
                }
                return false;
            }
        }

        // Autorización por rol:
        // - superadmin/admin: acceso total al panel.
        // - operator (grp_web): submódulos delegados (dashboard, archivos, logs, recursos,
        //   backups, usuarios, servicios, diagnóstico y red) sin almacenamiento/terminal/dominio.
        if (!$isPublic) {
            $role = (string) ($_SESSION['nas_user']['role'] ?? '');
            $isAdmin = !empty($_SESSION['nas_user']['is_admin']) || in_array($role, ['admin', 'superadmin'], true);
            if (!$isAdmin && !self::isOperatorAllowed($path)) {
                if (getenv('APP_ENV') !== 'testing') {
                    \App\Services\AuditService::log('access_denied', $path, 'FAILED', ['role' => $role ?: 'operator', 'method' => $method]);
                }
                if ($request->isJson()) {
                    Response::error('Acceso denegado: se requieren privilegios de administrador.', 403);
                } else {
                    if (!headers_sent()) {
                        http_response_code(403);
                    }
                    echo '<h1>403 - Acceso restringido a administradores</h1>';
                    if (getenv('APP_ENV') !== 'testing') {
                        exit;
                    }
                }
                return false;
            }
        }

        // Restricción por rol de servidor activo:
        if (!self::isServerRoleAllowed($path)) {
            if ($request->isJson()) {
                Response::error('Módulo o recurso no habilitado para el rol actual del servidor.', 403);
            } else {
                if (!headers_sent()) {
                    http_response_code(403);
                }
                echo '<h1>403 - Módulo no habilitado para el rol actual del servidor</h1>';
                if (getenv('APP_ENV') !== 'testing') {
                    exit;
                }
            }
            return false;
        }

        // Rate limiting en SQLite para operaciones críticas
        if (!self::checkCriticalRateLimit($request)) {
            return false;
        }

        // Validación estricta de CSRF para métodos mutantes (POST, PUT, DELETE, PATCH)
        if (in_array($method, ['POST', 'PUT', 'DELETE', 'PATCH'], true)) {
            // Permitir formulario y API pública de login sin token CSRF previo
            if ($path !== '/login' && $path !== '/api/auth/login') {
                $token = $request->getCsrfToken();
                if (!self::verifyCsrfToken($token)) {
                    if ($request->isJson()) {
                        Response::error('Token CSRF inválido o ausente.', 403);
                    } else {
                        if (!headers_sent()) {
                            http_response_code(403);
                        }
                        echo '<h1>403 - Solicitud rechazada (Token CSRF inválido)</h1>';
                        if (getenv('APP_ENV') !== 'testing') {
                            exit;
                        }
                    }
                    return false;
                }
            }
        }

        return true;
    }

    /**
     * Rutas permitidas al rol operador (grp_web).
     * No incluye almacenamiento, terminal, dominio, actualizaciones ni reinicio.
     */
    private static function isOperatorAllowed(string $path): bool
    {
        $views = ['/', '/dashboard', '/files', '/logs', '/shares', '/backups', '/users', '/permissions', '/services', '/diagnostics', '/networking'];
        if (in_array($path, $views, true)) {
            return true;
        }

        if (in_array($path, ['/api/metrics', '/api/logs', '/api/auth/me', '/api/diagnostics', '/api/services', '/api/services/list'], true)) {
            return true;
        }

        foreach (['/api/files', '/api/shares', '/api/backups', '/api/users', '/api/groups', '/api/services/'] as $prefix) {
            if (str_starts_with($path, $prefix)) {
                return true;
            }
        }

        return false;
    }

    /**
     * Valida si el módulo o ruta solicitada está permitido para el rol activo del servidor.
     * En rol ARCHIVOS, los módulos de backup están deshabilitados (403).
     */
    public static function isServerRoleAllowed(string $path): bool
    {
        $serverRole = \App\Services\SystemService::getServerRole();
        if ($serverRole === 'ARCHIVOS') {
            if ($path === '/backups' || str_starts_with($path, '/api/backups')) {
                return false;
            }
        }
        return true;
    }

    /**
     * Aplica Rate Limiting estricto mediante SQLite para operaciones críticas del sistema.
     */
    public static function checkCriticalRateLimit(Request $request): bool
    {
        $path = $request->getPath();
        $isCritical = false;

        $criticalExact = [
            '/api/storage/format',
            '/api/storage/lvm',
            '/api/storage/subvolume',
            '/api/storage/scrub',
            '/api/terminal/session',
            '/api/terminal/exec',
            '/api/system/reboot',
        ];

        if (in_array($path, $criticalExact, true)) {
            $isCritical = true;
        } elseif (preg_match('#^/api/backups/[^/]+/run$#', $path)) {
            $isCritical = true;
        }

        if ($isCritical) {
            $ip = $request->getIp();
            $key = 'rate_crit:' . $ip . ':' . $path;
            if (!\App\Services\AuthService::checkRateLimit($key, 10, 60)) {
                if ($request->isJson()) {
                    Response::error('Límite de solicitudes críticas excedido. Intente más tarde.', 429);
                } else {
                    if (!headers_sent()) {
                        http_response_code(429);
                    }
                    echo '<h1>429 - Límite de solicitudes excedido</h1>';
                    if (getenv('APP_ENV') !== 'testing') {
                        exit;
                    }
                }
                return false;
            }
        }

        return true;
    }
}
