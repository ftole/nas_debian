<?php

declare(strict_types=1);

namespace App\Services;

use Throwable;

/**
 * Servicio de Integración con Active Directory (Debian 13).
 * Administra la verificación, descubrimiento, unión y desvinculación a dominios
 * Windows corporativos mediante realmd, SSSD, adcli y Kerberos.
 */
class DomainService
{
    /**
     * Consulta el estado actual de pertenencia al dominio de Active Directory.
     */
    public function getStatus(): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                'joined' => false,
                'status' => 'standalone',
                'workgroup' => SystemService::getWorkgroup(),
                'message' => 'Servidor operando en modo Autónomo (Standalone) - Entorno simulado Windows.',
            ];
        }

        $cmd = ['sudo', '-n', '/usr/sbin/realm', 'list'];
        $res = SystemService::runCommand($cmd);

        if ($res['code'] === 0 && !empty(trim($res['stdout']))) {
            $parsed = $this->parseRealmList($res['stdout']);
            if (!empty($parsed['domain'])) {
                $this->updateDomainDb($parsed['domain'], $parsed['realm'] ?? $parsed['domain'], 'joined');
                return array_merge(['joined' => true, 'status' => 'joined'], $parsed);
            }
        }

        $this->updateDomainDb('', '', 'standalone');
        return [
            'joined' => false,
            'status' => 'standalone',
            'workgroup' => SystemService::getWorkgroup(),
            'message' => 'Servidor operando en modo Autónomo (Standalone). No está integrado en Active Directory.',
        ];
    }

    /**
     * Realiza el descubrimiento de controladores de dominio y bosque AD.
     */
    public function discover(string $rawDomain): array
    {
        $domain = trim($rawDomain);
        if ($domain === '' || !preg_match('/^[a-zA-Z0-9.-]+$/', $domain)) {
            return [
                'success' => false,
                'error' => 'Nombre de dominio no válido. Formato esperado: ej. EMPRESA.LOCAL',
            ];
        }

        if (DIRECTORY_SEPARATOR === '\\') {
            return [
                'success' => true,
                'domain' => strtoupper($domain),
                'realm' => strtoupper($domain),
                'type' => 'kerberos',
                'server_software' => 'active-directory',
                'client_software' => 'sssd',
                'message' => 'Dominio detectado con éxito (Simulación local).',
            ];
        }

        $cmd = ['/usr/sbin/realm', 'discover', $domain];
        $res = SystemService::runCommand($cmd);

        if ($res['code'] !== 0 || empty(trim($res['stdout']))) {
            return [
                'success' => false,
                'error' => sprintf(
                    'No se pudo localizar el controlador del dominio "%s". Verifique que el DNS del NAS apunte a la IP del Controlador de Dominio de Active Directory.',
                    $domain
                ),
                'details' => $res['stderr'] ?: $res['stdout'],
            ];
        }

        $parsed = $this->parseRealmList($res['stdout']);
        return [
            'success' => true,
            'domain' => $parsed['domain'] ?? $domain,
            'realm' => $parsed['realm'] ?? strtoupper($domain),
            'server_software' => $parsed['server_software'] ?? 'active-directory',
            'client_software' => $parsed['client_software'] ?? 'sssd',
            'message' => sprintf('Dominio "%s" detectado correctamente en la red.', $domain),
        ];
    }

    /**
     * Une el servidor NAS al dominio Active Directory suministrando credenciales de forma segura por stdin.
     */
    public function join(string $domain, string $user, string $password, ?string $ou = null): array
    {
        $cleanDomain = trim($domain);
        $cleanUser = trim($user);

        if ($cleanDomain === '' || $cleanUser === '' || $password === '') {
            return [
                'success' => false,
                'error' => 'Dominio, usuario y contraseña son obligatorios para la unión.',
            ];
        }

        if (DIRECTORY_SEPARATOR === '\\') {
            $this->updateDomainDb($cleanDomain, strtoupper($cleanDomain), 'joined');
            AuditService::log('domain_join', $cleanDomain, 'SUCCESS', ['user' => $cleanUser]);
            return [
                'success' => true,
                'message' => sprintf('Servidor unido con éxito al dominio %s (Simulación).', $cleanDomain),
                'domain' => $cleanDomain,
            ];
        }

        $cmd = ['sudo', '-n', '/usr/sbin/realm', 'join', '--user=' . $cleanUser];
        if ($ou !== null && trim($ou) !== '') {
            $cmd[] = '--computer-ou=' . trim($ou);
        }
        $cmd[] = $cleanDomain;

        $res = SystemService::runCommand($cmd, $password . "\n", 60);

        if ($res['code'] !== 0) {
            AuditService::log('domain_join', $cleanDomain, 'FAILED', [
                'user' => $cleanUser,
                'error' => $res['stderr'] ?: $res['stdout'],
            ]);
            return [
                'success' => false,
                'error' => 'Error al unirse al dominio: ' . ($res['stderr'] ?: $res['stdout']),
            ];
        }

        // Reiniciar servicio SSSD para refrescar cachés de dominio
        SystemService::runCommand(['sudo', '-n', 'systemctl', 'restart', 'sssd']);

        $this->updateDomainDb($cleanDomain, strtoupper($cleanDomain), 'joined');
        AuditService::log('domain_join', $cleanDomain, 'SUCCESS', ['user' => $cleanUser]);

        return [
            'success' => true,
            'message' => sprintf('El servidor NAS se unió exitosamente al dominio Active Directory "%s".', $cleanDomain),
            'domain' => $cleanDomain,
        ];
    }

    /**
     * Desvincula el host de Active Directory.
     */
    public function leave(string $domain = ''): array
    {
        if (DIRECTORY_SEPARATOR === '\\') {
            $this->updateDomainDb('', '', 'standalone');
            AuditService::log('domain_leave', $domain ?: 'active_directory', 'SUCCESS');
            return [
                'success' => true,
                'message' => 'Servidor desvinculado del dominio (Simulación).',
            ];
        }

        $cmd = ['sudo', '-n', '/usr/sbin/realm', 'leave'];
        if ($domain !== '') {
            $cmd[] = trim($domain);
        }

        $res = SystemService::runCommand($cmd, null, 30);
        if ($res['code'] !== 0) {
            return [
                'success' => false,
                'error' => 'Fallo al desvincular el dominio: ' . ($res['stderr'] ?: $res['stdout']),
            ];
        }

        SystemService::runCommand(['sudo', '-n', 'systemctl', 'restart', 'sssd']);

        $this->updateDomainDb('', '', 'standalone');
        AuditService::log('domain_leave', $domain ?: 'active_directory', 'SUCCESS');

        return [
            'success' => true,
            'message' => 'Servidor desvinculado con éxito de Active Directory.',
        ];
    }

    private function parseRealmList(string $output): array
    {
        $info = [];
        $lines = explode("\n", $output);
        foreach ($lines as $line) {
            $line = trim($line);
            if (empty($line)) continue;

            if (str_contains($line, ':')) {
                [$key, $val] = explode(':', $line, 2);
                $key = strtolower(trim($key));
                $val = trim($val);

                if ($key === 'realm-name' || $key === 'domain-name') {
                    $info['domain'] = $val;
                    $info['realm'] = strtoupper($val);
                } elseif ($key === 'type') {
                    $info['type'] = $val;
                } elseif ($key === 'server-software') {
                    $info['server_software'] = $val;
                } elseif ($key === 'client-software') {
                    $info['client_software'] = $val;
                } elseif ($key === 'configured') {
                    $info['configured'] = $val;
                }
            }
        }
        return $info;
    }

    private function updateDomainDb(string $domain, string $realm, string $status): void
    {
        try {
            DatabaseService::execute(
                'INSERT INTO domain_config (id, domain, realm, joined_at, status)
                 VALUES (1, :domain, :realm, CURRENT_TIMESTAMP, :status)
                 ON CONFLICT(id) DO UPDATE SET
                    domain = excluded.domain,
                    realm = excluded.realm,
                    joined_at = excluded.joined_at,
                    status = excluded.status',
                [
                    'domain' => $domain,
                    'realm' => $realm,
                    'status' => $status,
                ]
            );
        } catch (Throwable) {
            // Tolerante
        }
    }
}
