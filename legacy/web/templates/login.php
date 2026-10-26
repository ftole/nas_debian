<?php

declare(strict_types=1);

/**
 * Plantilla de Inicio de Sesión • Panel Web NAS & Central de Respaldos (Debian 13).
 * 100% Offline • Cero dependencias externas • Modern Slate Design.
 *
 * @var string $hostname Nombre del servidor.
 * @var string $serverIp Dirección IP del host.
 * @var ?string $error Mensaje de error si la autenticación previa falló.
 */

$safeHostname = htmlspecialchars((string) ($hostname ?? 'SRV-NAS'), ENT_QUOTES, 'UTF-8');
$safeServerIp = htmlspecialchars((string) ($serverIp ?? '10.10.1.2'), ENT_QUOTES, 'UTF-8');
?>
<!DOCTYPE html>
<html lang="es" data-theme="dark">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1, shrink-to-fit=no">
  <title>Iniciar Sesión • <?= $safeHostname ?> (Debian 13)</title>
  <script>
    (function() {
      try {
        var savedTheme = localStorage.getItem('nas_theme');
        if (savedTheme) {
          document.documentElement.setAttribute('data-theme', savedTheme);
        }
      } catch (e) {}
    })();
  </script>
  <link rel="stylesheet" href="/css/app.css?v=<?= file_exists(__DIR__ . '/../public/css/app.css') ? filemtime(__DIR__ . '/../public/css/app.css') : '2' ?>">
  <style>
    body.login-page {
      display: flex;
      align-items: center;
      justify-content: center;
      min-height: 100vh;
      margin: 0;
      background-color: var(--bg-body);
      background-image: radial-gradient(circle at 50% 20%, rgba(0, 102, 204, 0.08) 0%, transparent 60%);
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
      color: var(--text-main);
    }
    .login-container {
      width: 100%;
      max-width: 440px;
      padding: 20px;
      box-sizing: border-box;
    }
    .login-card {
      background: var(--bg-card);
      border: 1px solid var(--border-color);
      border-radius: 6px;
      box-shadow: 0 8px 30px rgba(0,0,0,0.35);
      overflow: hidden;
    }
    .login-header {
      padding: 24px 28px 20px;
      background: var(--bg-card-header);
      border-bottom: 1px solid var(--border-color);
      display: flex;
      flex-direction: column;
      gap: 12px;
    }
    .login-brand {
      display: flex;
      align-items: center;
      gap: 10px;
    }
    .login-brand-icon {
      width: 32px;
      height: 32px;
      color: var(--accent-primary);
    }
    .login-title-group h1 {
      margin: 0;
      font-size: 18px;
      font-weight: 700;
      color: var(--text-main);
      display: flex;
      align-items: center;
      gap: 8px;
    }
    .login-subtitle {
      margin: 4px 0 0;
      font-size: 12px;
      color: var(--text-secondary);
    }
    .login-body {
      padding: 28px;
      display: flex;
      flex-direction: column;
      gap: 18px;
    }
    .login-alert {
      display: none;
      padding: 10px 14px;
      border-radius: 4px;
      font-size: 13px;
      line-height: 1.4;
      background: rgba(201, 25, 11, 0.15);
      border: 1px solid var(--accent-danger);
      color: #ff857a;
    }
    .login-alert.visible {
      display: flex;
      align-items: center;
      gap: 8px;
    }
    .form-group {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }
    .form-group label {
      font-size: 13px;
      font-weight: 600;
      color: var(--text-main);
    }
    .input-with-icon {
      position: relative;
      display: flex;
      align-items: center;
    }
    .input-with-icon input {
      width: 100%;
      padding: 10px 38px 10px 12px;
      font-size: 14px;
      border: 1px solid var(--border-color);
      border-radius: 4px;
      background: var(--bg-input);
      color: var(--text-main);
      box-sizing: border-box;
      outline: none;
      transition: border-color 0.15s ease;
    }
    .input-with-icon input:focus {
      border-color: var(--accent-primary);
    }
    .input-addon-btn {
      position: absolute;
      right: 8px;
      background: none;
      border: none;
      color: var(--text-secondary);
      cursor: pointer;
      padding: 4px;
      display: flex;
      align-items: center;
      justify-content: center;
    }
    .input-addon-btn:hover {
      color: var(--text-main);
    }
    .login-footer {
      padding: 14px 28px;
      background: var(--bg-card-header);
      border-top: 1px solid var(--border-color);
      display: flex;
      justify-content: space-between;
      align-items: center;
      font-size: 11px;
      color: var(--text-muted);
    }
  </style>
</head>
<body class="login-page">

  <!-- Iconos SVG Inline (100% Offline) -->
  <svg style="display:none;">
    <defs>
      <symbol id="icon-server" viewBox="0 0 24 24"><rect x="2" y="2" width="20" height="8" rx="2" ry="2"/><rect x="2" y="14" width="20" height="8" rx="2" ry="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></symbol>
      <symbol id="icon-lock" viewBox="0 0 24 24"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></symbol>
      <symbol id="icon-eye" viewBox="0 0 24 24"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></symbol>
      <symbol id="icon-eye-off" viewBox="0 0 24 24"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></symbol>
      <symbol id="icon-alert-triangle" viewBox="0 0 24 24"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></symbol>
      <symbol id="icon-sun" viewBox="0 0 24 24"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></symbol>
    </defs>
  </svg>

  <div class="login-container">
    <div class="login-card">
      <div class="login-header">
        <div class="login-brand">
          <svg class="login-brand-icon"><use href="#icon-server"></use></svg>
          <div class="login-title-group">
            <h1>
              <span>NAS</span>
              <span style="opacity:0.4; font-weight:300;">|</span>
              <span style="color:var(--accent-primary);"><?= $safeHostname ?></span>
            </h1>
            <p class="login-subtitle">Servidor NAS Departamental & Central de Respaldos</p>
          </div>
        </div>
      </div>

      <div class="login-body">
        <div id="login-alert" class="login-alert <?= !empty($error) ? 'visible' : '' ?>">
          <svg class="icon icon-sm" style="flex-shrink:0;"><use href="#icon-alert-triangle"></use></svg>
          <span id="login-alert-text"><?= htmlspecialchars((string) ($error ?? ''), ENT_QUOTES, 'UTF-8') ?></span>
        </div>

        <form id="login-form" method="POST" action="/login">
          <input type="hidden" name="csrf_token" id="csrf_token" value="<?= htmlspecialchars(\App\Core\AuthMiddleware::getCsrfToken(), ENT_QUOTES, 'UTF-8') ?>">
          <div class="form-group" style="margin-bottom:16px;">
            <label for="username">Usuario del sistema:</label>
            <div class="input-with-icon">
              <input type="text" id="username" name="username" required autofocus placeholder="ej. sistemas o administrador" autocomplete="username">
            </div>
          </div>

          <div class="form-group" style="margin-bottom:20px;">
            <label for="password">Contraseña:</label>
            <div class="input-with-icon">
              <input type="password" id="password" name="password" required placeholder="Contraseña de acceso" autocomplete="current-password">
              <button type="button" class="input-addon-btn" id="btn-toggle-pass" title="Mostrar/ocultar contraseña" onclick="togglePassVisibility()">
                <svg class="icon icon-sm" id="icon-pass-toggle"><use href="#icon-eye"></use></svg>
              </button>
            </div>
          </div>

          <button type="submit" class="btn btn-primary" id="btn-submit" style="width:100%; justify-content:center; padding:10px;">
            <svg class="icon"><use href="#icon-lock"></use></svg>
            <span>Iniciar sesión</span>
          </button>
        </form>
      </div>

      <div class="login-footer">
        <span>IP: <strong><?= $safeServerIp ?></strong></span>
        <span>Debian 13 (Trixie)</span>
      </div>
    </div>
  </div>

  <script>
    function togglePassVisibility() {
      const pass = document.getElementById('password');
      const icon = document.getElementById('icon-pass-toggle');
      if (pass.type === 'password') {
        pass.type = 'text';
        icon.innerHTML = '<use href="#icon-eye-off"></use>';
      } else {
        pass.type = 'password';
        icon.innerHTML = '<use href="#icon-eye"></use>';
      }
    }

    document.getElementById('login-form').addEventListener('submit', async function(e) {
      e.preventDefault();
      const user = document.getElementById('username').value.trim();
      const pass = document.getElementById('password').value;
      const btn = document.getElementById('btn-submit');
      const alertBox = document.getElementById('login-alert');
      const alertText = document.getElementById('login-alert-text');

      if (!user || !pass) return;

      btn.disabled = true;
      btn.style.opacity = '0.7';

      try {
        const res = await fetch('/api/auth/login', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'Accept': 'application/json',
            'X-CSRF-Token': document.getElementById('csrf_token') ? document.getElementById('csrf_token').value : ''
          },
          body: JSON.stringify({ username: user, password: pass })
        });

        const data = await res.json();

        if (res.ok && data.success) {
          window.location.href = '/';
        } else {
          alertText.textContent = data.error || 'Credenciales inválidas. Verifica usuario y contraseña.';
          alertBox.classList.add('visible');
          btn.disabled = false;
          btn.style.opacity = '1';
        }
      } catch (err) {
        // Fallback a envío de formulario estándar si fetch fallara
        document.getElementById('login-form').submit();
      }
    });
  </script>
</body>
</html>
