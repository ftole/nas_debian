<?php

declare(strict_types=1);

namespace App\Core;

/**
 * Emisión estructurada de respuestas JSON y renderizado de plantillas HTML.
 */
class Response
{
    public static int $lastStatusCode = 200;

    public static function getStatusCode(): int
    {
        return self::$lastStatusCode;
    }

    public static function json(array $data, int $statusCode = 200): void
    {
        self::$lastStatusCode = $statusCode;
        if (!headers_sent()) {
            http_response_code($statusCode);
            header('Content-Type: application/json; charset=utf-8');
            // Las cabeceras de seguridad (X-Frame-Options, X-Content-Type-Options, CSP)
            // las emite Nginx en el host virtual para evitar duplicados.
        }
        echo json_encode($data, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        if (getenv('APP_ENV') !== 'testing') {
            exit;
        }
    }

    public static function success(mixed $data = null, string $message = 'Operación completada con éxito.'): void
    {
        self::json([
            'success' => true,
            'message' => $message,
            'data' => $data,
        ], 200);
    }

    public static function error(string $message, int $statusCode = 400, array $extra = []): void
    {
        $payload = array_merge([
            'success' => false,
            'error' => $message,
        ], $extra);

        self::json($payload, $statusCode);
    }

    public static function html(string $viewPath, array $data = []): void
    {
        extract($data, EXTR_SKIP);
        if (!headers_sent()) {
            header('Content-Type: text/html; charset=utf-8');
            // Cabeceras de seguridad delegadas a Nginx (server block de nas-web).
        }
        
        if (file_exists($viewPath)) {
            require $viewPath;
        } else {
            if (!headers_sent()) {
                http_response_code(500);
            }
            echo "Vista no encontrada: " . htmlspecialchars($viewPath, ENT_QUOTES, 'UTF-8');
        }
        if (getenv('APP_ENV') !== 'testing') {
            exit;
        }
    }
}
