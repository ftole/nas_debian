<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\AuthMiddleware;
use App\Core\Request;
use App\Core\Response;
use App\Services\AuditService;
use App\Services\AuthService;
use App\Services\SystemService;

/**
 * Controlador de autenticación, ciclo de vida de sesiones y cierre de sesión.
 */
class AuthController
{
    private AuthService $auth;
    private SystemService $system;

    public function __construct()
    {
        $this->auth = new AuthService();
        $this->system = new SystemService();
    }

    /**
     * Muestra la pantalla de inicio de sesión o redirige a la raíz si ya hay sesión activa.
     */
    public function showLogin(Request $request): void
    {
        AuthMiddleware::initSession();

        if (!empty($_SESSION['nas_user'])) {
            header('Location: /');
            exit;
        }

        $metrics = $this->system->getSystemMetrics();
        $hostname = $metrics['hostname'] ?? 'SRV-NAS';
        $serverIp = $_SERVER['SERVER_ADDR'] ?? '10.10.1.2';
        $error = $_SESSION['login_error'] ?? null;
        unset($_SESSION['login_error']);

        $templatePath = dirname(__DIR__, 2) . '/templates/login.php';

        Response::html($templatePath, [
            'hostname' => $hostname,
            'serverIp' => $serverIp,
            'error' => $error,
        ]);
    }

    /**
     * Procesa la solicitud de inicio de sesión tanto desde formulario HTML como vía API JSON.
     */
    public function login(Request $request): void
    {
        AuthMiddleware::initSession();

        $username = (string) $request->get('username', '');
        $password = (string) $request->get('password', '');
        $clientIp = $request->getClientIp();

        if ($this->auth->isRateLimited($clientIp)) {
            $err = 'Demasiados intentos fallidos. Por favor espera 5 minutos antes de volver a intentar.';
            AuditService::log('login_rate_limited', $username ?: 'desconocido', 'FAILED', ['ip' => $clientIp]);
            if ($request->isJson()) {
                Response::error($err, 429);
                return;
            }
            $_SESSION['login_error'] = $err;
            header('Location: /login');
            exit;
        }

        if (trim($username) === '' || trim($password) === '') {
            $err = 'Por favor ingresa tu usuario y contraseña.';
            $this->auth->recordFailedAttempt($clientIp, $username ?: 'desconocido');
            AuditService::log('login_failure', $username ?: 'desconocido', 'FAILED', ['error' => 'Campos vacíos']);
            if ($request->isJson()) {
                Response::error($err, 400);
                return;
            }
            $_SESSION['login_error'] = $err;
            header('Location: /login');
            exit;
        }

        $res = $this->auth->authenticate($username, $password);

        if (!$res['success']) {
            $err = $res['error'] ?? 'Usuario o contraseña incorrectos.';
            $this->auth->recordFailedAttempt($clientIp, $username);
            AuditService::log('login_failure', $username, 'FAILED', ['error' => $err]);
            if ($request->isJson()) {
                Response::error($err, 401);
                return;
            }
            $_SESSION['login_error'] = $err;
            header('Location: /login');
            exit;
        }

        $this->auth->clearFailedAttempts($clientIp);

        // Proteger contra Session Fixation
        session_regenerate_id(true);
        $_SESSION['nas_user'] = $res['user'];
        AuditService::log('login_success', $username, 'SUCCESS', ['role' => $res['user']['role'] ?? 'Administrador']);

        if ($request->isJson()) {
            Response::success($res['user'], 'Sesión iniciada con éxito.');
            return;
        }

        header('Location: /');
        exit;
    }

    /**
     * Cierra la sesión activa y redirige a la pantalla de login.
     */
    public function logout(Request $request): void
    {
        $currentUser = $_SESSION['nas_user']['username'] ?? 'desconocido';
        $this->auth->logout();
        AuditService::log('logout', $currentUser, 'SUCCESS');

        if ($request->isJson()) {
            Response::success(null, 'Sesión finalizada con éxito.');
            return;
        }

        header('Location: /login');
        exit;
    }

    /**
     * Devuelve los datos del usuario actualmente autenticado en la sesión.
     */
    public function me(Request $request): void
    {
        AuthMiddleware::initSession();

        if (empty($_SESSION['nas_user'])) {
            Response::error('No hay una sesión activa.', 401);
            return;
        }

        Response::success($_SESSION['nas_user']);
    }
}
