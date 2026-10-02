<?php

declare(strict_types=1);

namespace App\Core;

/**
 * Enrutador de peticiones para rutas de vistas y endpoints de API REST.
 */
class Router
{
    private array $routes = [];

    public function get(string $path, callable|array $handler): void
    {
        $this->addRoute('GET', $path, $handler);
    }

    public function post(string $path, callable|array $handler): void
    {
        $this->addRoute('POST', $path, $handler);
    }

    public function delete(string $path, callable|array $handler): void
    {
        $this->addRoute('DELETE', $path, $handler);
    }

    private function addRoute(string $method, string $path, callable|array $handler): void
    {
        $pattern = '#^' . preg_replace('#\{([a-zA-Z0-9_]+)\}#', '(?P<$1>[^/]+)', $path) . '$#';
        $this->routes[] = [
            'method' => $method,
            'path' => $path,
            'pattern' => $pattern,
            'handler' => $handler,
        ];
    }

    public function dispatch(Request $request): void
    {
        $reqMethod = $request->getMethod();
        $reqPath = $request->getPath();

        foreach ($this->routes as $route) {
            if ($route['method'] !== $reqMethod) {
                continue;
            }

            if (preg_match($route['pattern'], $reqPath, $matches)) {
                $params = array_filter($matches, '\is_string', ARRAY_FILTER_USE_KEY);
                $handler = $route['handler'];

                if (is_array($handler) && count($handler) === 2) {
                    [$class, $method] = $handler;
                    $controller = new $class();
                    $controller->$method($request, $params);
                    return;
                }

                if (is_callable($handler)) {
                    $handler($request, $params);
                    return;
                }
            }
        }

        if ($request->isJson()) {
            Response::error('Ruta no encontrada: ' . $reqPath, 404);
        } else {
            http_response_code(404);
            echo "<h1>404 - Página no encontrada</h1>";
            echo "<p>Ruta solicitada: " . htmlspecialchars($reqPath, ENT_QUOTES, 'UTF-8') . "</p>";
        }
    }
}
