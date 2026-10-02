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
    }

    /**
     * Valida si la petición actual está autorizada para continuar.
     *
     * @param Request $request
     * @return bool True si la petición procede, o detiene la ejecución emitiendo 401 o redirección.
     */
    public static function check(Request $request): bool
    {
        self::initSession();

        $path = $request->getPath();

        // Rutas públicas exentas de autenticación
        $publicPaths = [
            '/login',
            '/api/auth/login',
            '/logout',
            '/api/auth/logout',
        ];

        if (in_array($path, $publicPaths, true)) {
            return true;
        }

        // Verificar existencia de usuario autenticado en la sesión
        if (!empty($_SESSION['nas_user']) && is_array($_SESSION['nas_user'])) {
            return true;
        }

        // Petición no autenticada: Responder JSON 401 o redireccionar a /login
        if ($request->isJson()) {
            Response::error('Acceso no autorizado. Inicia sesión en el panel para continuar.', 401);
        } else {
            header('Location: /login');
            exit;
        }

        return false;
    }
}
