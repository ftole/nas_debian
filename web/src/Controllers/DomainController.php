<?php

declare(strict_types=1);

namespace App\Controllers;

use App\Core\Request;
use App\Core\Response;
use App\Services\DomainService;

/**
 * Controlador de Integración con Dominio Active Directory.
 * Enruta la verificación, descubrimiento y unión/desvinculación del servidor NAS.
 */
class DomainController
{
    private DomainService $domainService;

    public function __construct(?DomainService $service = null)
    {
        $this->domainService = $service ?? new DomainService();
    }

    public function status(Request $request): void
    {
        $status = $this->domainService->getStatus();
        Response::json([
            'success' => true,
            'data' => $status,
        ]);
    }

    public function discover(Request $request): void
    {
        $body = $request->getBody();
        $domain = (string) ($body['domain'] ?? '');

        if ($domain === '') {
            Response::error('Debe indicar el nombre del dominio a descubrir.', 400);
            return;
        }

        $result = $this->domainService->discover($domain);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Dominio no localizado.', 404, $result);
            return;
        }

        Response::json($result);
    }

    public function join(Request $request): void
    {
        $body = $request->getBody();
        $domain = (string) ($body['domain'] ?? '');
        $user = (string) ($body['user'] ?? '');
        $password = (string) ($body['password'] ?? '');
        $ou = !empty($body['ou']) ? (string) $body['ou'] : null;

        if ($domain === '' || $user === '' || $password === '') {
            Response::error('Dominio, usuario y contraseña son campos obligatorios.', 400);
            return;
        }

        $result = $this->domainService->join($domain, $user, $password, $ou);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al unirse al dominio Active Directory.', 400);
            return;
        }

        Response::json($result);
    }

    public function leave(Request $request): void
    {
        $body = $request->getBody();
        $domain = (string) ($body['domain'] ?? '');

        $result = $this->domainService->leave($domain);
        if (!($result['success'] ?? false)) {
            Response::error($result['error'] ?? 'Fallo al desvincular el dominio.', 400);
            return;
        }

        Response::json($result);
    }
}
