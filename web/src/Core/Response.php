<?php

declare(strict_types=1);

namespace App\Core;

/**
 * Emisión estructurada de respuestas JSON y renderizado de plantillas HTML.
 */
class Response
{
    public static function json(array $data, int $statusCode = 200): void
    {
        http_response_code($statusCode);
        header('Content-Type: application/json; charset=utf-8');
        header('X-Content-Type-Options: nosniff');
        echo json_encode($data, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        exit;
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
        header('Content-Type: text/html; charset=utf-8');
        header('X-Frame-Options: SAMEORIGIN');
        header('X-Content-Type-Options: nosniff');
        
        if (file_exists($viewPath)) {
            require $viewPath;
        } else {
            http_response_code(500);
            echo "Vista no encontrada: " . htmlspecialchars($viewPath, ENT_QUOTES, 'UTF-8');
        }
        exit;
    }
}
