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

    public function getQuery(string $key, mixed $default = null): mixed
    {
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

    public function getClientIp(): string
    {
        if (!empty($_SERVER['HTTP_X_FORWARDED_FOR'])) {
            $ips = explode(',', $_SERVER['HTTP_X_FORWARDED_FOR']);
            $candidate = trim($ips[0]);
            if (filter_var($candidate, FILTER_VALIDATE_IP)) {
                return $candidate;
            }
        }
        if (!empty($_SERVER['HTTP_CLIENT_IP']) && filter_var($_SERVER['HTTP_CLIENT_IP'], FILTER_VALIDATE_IP)) {
            return $_SERVER['HTTP_CLIENT_IP'];
        }
        return $_SERVER['REMOTE_ADDR'] ?? '127.0.0.1';
    }
}
