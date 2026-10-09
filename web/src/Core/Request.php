<?php

declare(strict_types=1);

namespace App\Core;

/**
 * Abstracción HTTP para lectura de parámetros, headers y cuerpo JSON de la petición.
 */
class Request
{
    private string $method;
    private string $path;
    private array $queryParams;
    private array $parsedBody;

    public function __construct()
    {
        $rawMethod = strtoupper($_SERVER['REQUEST_METHOD'] ?? 'GET');
        $this->method = ($rawMethod === 'HEAD') ? 'GET' : $rawMethod;
        
        // Soporte para URL rewriting o parámetro de fallback ?route=...
        $uri = $_SERVER['REQUEST_URI'] ?? '/';
        $pathOnly = (string) parse_url($uri, PHP_URL_PATH);
        
        if (isset($_GET['route']) && is_string($_GET['route'])) {
            $this->path = '/' . ltrim($_GET['route'], '/');
        } else {
            $this->path = '/' . trim($pathOnly, '/');
            if ($this->path !== '/' && str_ends_with($this->path, '/')) {
                $this->path = rtrim($this->path, '/');
            }
        }

        $this->queryParams = $_GET;
        $this->parsedBody = $this->parseRequestBody();
    }

    private function parseRequestBody(): array
    {
        if ($this->method === 'GET') {
            return [];
        }

        $contentType = $_SERVER['CONTENT_TYPE'] ?? '';
        if (str_contains($contentType, 'application/json')) {
            $raw = file_get_contents('php://input');
            if (!empty($raw)) {
                $decoded = json_decode($raw, true);
                if (is_array($decoded)) {
                    return $decoded;
                }
            }
            return [];
        }

        return $_POST;
    }

    public function getMethod(): string
    {
        return $this->method;
    }

    public function getPath(): string
    {
        return $this->path;
    }

    public function getQueryParams(): array
    {
        return $this->queryParams;
    }

    public function getQuery(?string $key = null, mixed $default = null): mixed
    {
        if ($key === null) {
            return $this->queryParams;
        }
        return $this->queryParams[$key] ?? $default;
    }

    public function getBody(): array
    {
        return $this->parsedBody;
    }

    public function get(string $key, mixed $default = null): mixed
    {
        return $this->parsedBody[$key] ?? $this->queryParams[$key] ?? $default;
    }

    public function isJson(): bool
    {
        $accept = $_SERVER['HTTP_ACCEPT'] ?? '';
        return str_contains($accept, 'application/json') || str_starts_with($this->path, '/api');
    }

    public static array $trustedProxies = ['127.0.0.1', '::1'];

    public function getHeader(string $name): ?string
    {
        $key = 'HTTP_' . strtoupper(str_replace('-', '_', $name));
        if (isset($_SERVER[$key])) {
            return (string) $_SERVER[$key];
        }
        $directKey = strtoupper(str_replace('-', '_', $name));
        if (isset($_SERVER[$directKey])) {
            return (string) $_SERVER[$directKey];
        }
        if (function_exists('getallheaders')) {
            $headers = getallheaders();
            if (is_array($headers)) {
                foreach ($headers as $k => $v) {
                    if (strcasecmp((string) $k, $name) === 0) {
                        return (string) $v;
                    }
                }
            }
        }
        return null;
    }

    public function getCsrfToken(): ?string
    {
        $headerToken = $this->getHeader('X-CSRF-Token') ?? $this->getHeader('X-XSRF-Token');
        if (!empty($headerToken)) {
            return $headerToken;
        }
        $body = $this->getBody();
        if (!empty($body['csrf_token']) && is_string($body['csrf_token'])) {
            return $body['csrf_token'];
        }
        if (!empty($_POST['csrf_token']) && is_string($_POST['csrf_token'])) {
            return $_POST['csrf_token'];
        }
        return null;
    }

    /**
     * Resuelve la dirección IP del cliente basándose estrictamente en REMOTE_ADDR,
     * examinando cabeceras de proxy únicamente cuando la conexión procede de un proxy de confianza.
     */
    public function getClientIp(): string
    {
        $remoteAddr = $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';

        if (in_array($remoteAddr, self::$trustedProxies, true)) {
            if (!empty($_SERVER['HTTP_X_FORWARDED_FOR'])) {
                $ips = explode(',', $_SERVER['HTTP_X_FORWARDED_FOR']);
                $candidate = trim($ips[0]);
                if (filter_var($candidate, FILTER_VALIDATE_IP)) {
                    return $candidate;
                }
            }
            if (!empty($_SERVER['HTTP_X_REAL_IP']) && filter_var($_SERVER['HTTP_X_REAL_IP'], FILTER_VALIDATE_IP)) {
                return $_SERVER['HTTP_X_REAL_IP'];
            }
            if (!empty($_SERVER['HTTP_CLIENT_IP']) && filter_var($_SERVER['HTTP_CLIENT_IP'], FILTER_VALIDATE_IP)) {
                return $_SERVER['HTTP_CLIENT_IP'];
            }
        }

        return filter_var($remoteAddr, FILTER_VALIDATE_IP) ? $remoteAddr : '127.0.0.1';
    }

    /**
     * Alias de conveniencia para getClientIp().
     */
    public function getIp(): string
    {
        return $this->getClientIp();
    }
}
