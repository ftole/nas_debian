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

        // Autorización por rol: los usuarios web (no-admin) solo acceden a
        // Dashboard, Archivos y Logs; el resto de módulos es exclusivo de admin.
        if (!$isPublic) {
            $isAdmin = !empty($_SESSION['nas_user']['is_admin']);
            if (!$isAdmin && !self::isWebUserAllowed($path)) {
                if (getenv('APP_ENV') !== 'testing') {
                    \App\Services\AuditService::log('access_denied', $path, 'FAILED', ['role' => 'web', 'method' => $method]);
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
     * Rutas permitidas a un usuario web (no administrador): Dashboard, Archivos y Logs.
     */
    private static function isWebUserAllowed(string $path): bool
    {
        if (in_array($path, ['/', '/dashboard', '/files', '/logs'], true)) {
            return true;
        }
        if (in_array($path, ['/api/metrics', '/api/logs', '/api/auth/me'], true)) {
            return true;
        }
        return str_starts_with($path, '/api/files');
    }
}
