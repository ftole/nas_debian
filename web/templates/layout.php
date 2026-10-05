<?php

declare(strict_types=1);

/**
 * Plantilla Maestra del Panel Web de Administración NAS & Central de Respaldos (Debian 13).
 * 100% Offline • Cero dependencias externas • Estilo sobrio Cockpit / PatternFly 4.
 *
 * @var array $metrics Métricas del sistema (hostname, CPU, RAM, uptime, kernel).
 * @var array $storage Resumen de almacenamiento (/srv/nas, fs, disco).
 * @var array $services Estado de los demonios clave del NAS.
 */

$hostname = htmlspecialchars((string) ($metrics['hostname'] ?? 'SRV-NAS'), ENT_QUOTES, 'UTF-8');
$uptime = htmlspecialchars((string) ($metrics['uptime'] ?? 'N/A'), ENT_QUOTES, 'UTF-8');
$kernel = htmlspecialchars((string) ($metrics['kernel'] ?? php_uname('r')), ENT_QUOTES, 'UTF-8');
$cpuModel = htmlspecialchars((string) ($metrics['cpu_model'] ?? 'x86_64'), ENT_QUOTES, 'UTF-8');
$cpuPct = (float) ($metrics['cpu_usage_pct'] ?? 0);
$ramTotal = (float) ($metrics['ram_total_gb'] ?? 0);
$ramUsed = (float) ($metrics['ram_used_gb'] ?? 0);
$ramPct = (float) ($metrics['ram_usage_pct'] ?? 0);

$fsType = htmlspecialchars((string) ($storage['filesystem'] ?? 'ext4'), ENT_QUOTES, 'UTF-8');
$diskTotal = (float) ($storage['total_gb'] ?? 0);
$diskUsed = (float) ($storage['used_gb'] ?? 0);
$diskPct = (float) ($storage['usage_percent'] ?? 0);
$deviceType = htmlspecialchars((string) ($storage['device_type'] ?? 'Disco'), ENT_QUOTES, 'UTF-8');
$activeView = htmlspecialchars((string) ($activeView ?? 'dashboard'), ENT_QUOTES, 'UTF-8');
$sessionUser = htmlspecialchars((string) ($_SESSION['nas_user']['username'] ?? 'sistemas'), ENT_QUOTES, 'UTF-8');
?>
<!DOCTYPE html>
<html lang="es" data-theme="dark">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1, shrink-to-fit=no">
  <title>Panel Web • <?= $hostname ?> (Debian 13)</title>
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
</head>
<body>

  <!-- ==============================================================================
       Definición de Iconos SVG Inline (100% Offline • Cero dependencias externas)
       ============================================================================== -->
  <svg style="display:none;">
    <defs>
      <symbol id="icon-server" viewBox="0 0 24 24"><rect x="2" y="2" width="20" height="8" rx="2" ry="2"/><rect x="2" y="14" width="20" height="8" rx="2" ry="2"/><line x1="6" y1="6" x2="6.01" y2="6"/><line x1="6" y1="18" x2="6.01" y2="18"/></symbol>
      <symbol id="icon-hard-drive" viewBox="0 0 24 24"><line x1="22" y1="12" x2="2" y2="12"/><path d="M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><line x1="6" y1="16" x2="6.01" y2="16"/><line x1="10" y1="16" x2="10.01" y2="16"/></symbol>
      <symbol id="icon-cpu" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/><line x1="9" y1="1" x2="9" y2="4"/><line x1="15" y1="1" x2="15" y2="4"/><line x1="9" y1="20" x2="9" y2="23"/><line x1="15" y1="20" x2="15" y2="23"/><line x1="20" y1="9" x2="23" y2="9"/><line x1="20" y1="15" x2="23" y2="15"/><line x1="1" y1="9" x2="4" y2="9"/><line x1="1" y1="15" x2="4" y2="15"/></symbol>
      <symbol id="icon-dashboard" viewBox="0 0 24 24"><rect x="3" y="3" width="7" height="9"/><rect x="14" y="3" width="7" height="5"/><rect x="14" y="12" width="7" height="9"/><rect x="3" y="16" width="7" height="5"/></symbol>
      <symbol id="icon-folder" viewBox="0 0 24 24"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></symbol>
      <symbol id="icon-users" viewBox="0 0 24 24"><path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></symbol>
      <symbol id="icon-shield" viewBox="0 0 24 24"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></symbol>
      <symbol id="icon-wrench" viewBox="0 0 24 24"><path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/></symbol>
      <symbol id="icon-sun" viewBox="0 0 24 24"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></symbol>
      <symbol id="icon-moon" viewBox="0 0 24 24"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></symbol>
      <symbol id="icon-search" viewBox="0 0 24 24"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></symbol>
      <symbol id="icon-plus" viewBox="0 0 24 24"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></symbol>
      <symbol id="icon-trash" viewBox="0 0 24 24"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><line x1="10" y1="11" x2="10" y2="17"/><line x1="14" y1="11" x2="14" y2="17"/></symbol>
      <symbol id="icon-check-circle" viewBox="0 0 24 24"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></symbol>
      <symbol id="icon-alert-triangle" viewBox="0 0 24 24"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></symbol>
      <symbol id="icon-info" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></symbol>
      <symbol id="icon-x-circle" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></symbol>
      <symbol id="icon-lock" viewBox="0 0 24 24"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></symbol>
      <symbol id="icon-unlock" viewBox="0 0 24 24"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/></symbol>
      <symbol id="icon-key" viewBox="0 0 24 24"><path d="M21 2l-2 2m-1.5 1.5L14 9l-1.5-1.5-3 3L8 9l-3 3 1.5 1.5L2 18v4h4l4.5-4.5 1.5 1.5 3-3-1.5-1.5 3.5-3.5 1.5 1.5 2-2z"/></symbol>
      <symbol id="icon-clock" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></symbol>
      <symbol id="icon-terminal" viewBox="0 0 24 24"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></symbol>
      <symbol id="icon-play" viewBox="0 0 24 24"><polygon points="5 3 19 12 5 21 5 3"/></symbol>
      <symbol id="icon-stop" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" rx="2"/></symbol>
      <symbol id="icon-refresh" viewBox="0 0 24 24"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></symbol>
      <symbol id="icon-eye" viewBox="0 0 24 24"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></symbol>
      <symbol id="icon-eye-off" viewBox="0 0 24 24"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></symbol>
      <symbol id="icon-menu" viewBox="0 0 24 24"><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="18" x2="21" y2="18"/></symbol>
      <symbol id="icon-file" viewBox="0 0 24 24"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><polyline points="13 2 13 9 20 9"/></symbol>
      <symbol id="icon-file-text" viewBox="0 0 24 24"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10 9 9 9 8 9"/></symbol>
      <symbol id="icon-upload" viewBox="0 0 24 24"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></symbol>
      <symbol id="icon-edit" viewBox="0 0 24 24"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></symbol>
      <symbol id="icon-network" viewBox="0 0 24 24"><rect x="2" y="2" width="20" height="8" rx="2"/><path d="M6 10v4m12-4v4M12 10v12m-8 0h16"/></symbol>
      <symbol id="icon-services" viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></symbol>
      <symbol id="icon-apps" viewBox="0 0 24 24"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></symbol>
      <symbol id="icon-domain" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></symbol>
      <symbol id="icon-power" viewBox="0 0 24 24"><path d="M18.36 6.64a9 9 0 1 1-12.73 0"/><line x1="12" y1="2" x2="12" y2="12"/></symbol>
      <symbol id="icon-download" viewBox="0 0 24 24"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></symbol>
      <symbol id="icon-copy" viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></symbol>
      <symbol id="icon-grid" viewBox="0 0 24 24"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/></symbol>
      <symbol id="icon-list" viewBox="0 0 24 24"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></symbol>
      <symbol id="icon-image" viewBox="0 0 24 24"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></symbol>
      <symbol id="icon-music" viewBox="0 0 24 24"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></symbol>
      <symbol id="icon-video" viewBox="0 0 24 24"><polygon points="23 7 16 12 23 17 23 7"/><rect x="1" y="5" width="15" height="14" rx="2" ry="2"/></symbol>
      <symbol id="icon-archive" viewBox="0 0 24 24"><polyline points="21 8 21 21 3 21 3 8"/><rect x="1" y="3" width="22" height="5"/><line x1="10" y1="12" x2="14" y2="12"/></symbol>
      <symbol id="icon-restore" viewBox="0 0 24 24"><polyline points="1 4 1 10 7 10"/><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/></symbol>
    </defs>
  </svg>

  <!-- ==============================================================================
       BARRA SUPERIOR GLOBAL (MASTHEAD)
       ============================================================================== -->
  <header class="masthead">
    <div class="masthead-main">
      <button class="mobile-toggle" id="mobile-menu-btn" title="Alternar menú de navegación">
        <svg class="icon"><use href="#icon-menu"></use></svg>
      </button>
      <div class="masthead-brand">
        <svg class="icon"><use href="#icon-server"></use></svg>
        <span class="brand-logo-text">
          <span>NAS</span>
          <span class="brand-divider">|</span>
          <span class="brand-hostname" id="masthead-hostname"><?= $hostname ?></span>
        </span>
        <span class="brand-badge">Debian 13</span>
        <span class="brand-ip"><?= htmlspecialchars($_SERVER['SERVER_ADDR'] ?? '10.10.1.2', ENT_QUOTES, 'UTF-8') ?></span>
      </div>
    </div>

    <div class="masthead-tools">
      <div class="header-meta-pill">
        <span>Rol:</span> <strong>ARCHIVOS & BACKUP</strong>
      </div>
      <div class="header-meta-pill">
        <span>Workgroup:</span> <strong>TEAM-JOFRATO</strong>
      </div>
      <button class="btn btn-secondary btn-sm" onclick="switchView('terminal')" title="Abrir Consola Web Interactiva">
        <svg class="icon"><use href="#icon-terminal"></use></svg>
        <span class="btn-text-responsive">Terminal</span>
      </button>
      <button class="btn btn-secondary btn-sm" onclick="openModal('modal-reboot-server')" title="Reiniciar Servidor NAS">
        <svg class="icon" style="color:var(--accent-danger);"><use href="#icon-power"></use></svg>
        <span class="btn-text-responsive">Reiniciar</span>
      </button>
      <button class="btn btn-secondary btn-sm" id="btn-refresh-metrics" onclick="refreshDashboardMetrics()" title="Refrescar métricas del servidor">
        <svg class="icon" id="icon-refresh-metrics"><use href="#icon-refresh"></use></svg>
      </button>
      <button class="btn btn-secondary btn-sm" id="btn-theme-toggle" onclick="toggleTheme()" title="Alternar tema Claro / Oscuro">
        <svg class="icon icon-sm" id="theme-toggle-icon"><use href="#icon-sun"></use></svg>
      </button>
      <div class="user-pill" title="Sesión activa: <?= $sessionUser ?>">
        <span class="status-dot status-ok"></span>
        <span class="user-name"><?= $sessionUser ?></span>
      </div>
      <a href="/logout" class="btn btn-secondary btn-sm" id="btn-logout" title="Cerrar sesión segura">
        <svg class="icon"><use href="#icon-power"></use></svg>
        <span class="btn-text-responsive">Cerrar sesión</span>
      </a>
    </div>
  </header>

  <!-- Contenedor Principal (Sidebar + Contenido) -->
  <div class="app-layout">

    <!-- ==============================================================================
         BARRA LATERAL (SIDEBAR) JERÁRQUICA
         ============================================================================== -->
    <aside class="sidebar">
      <nav class="sidebar-nav">

        <div class="nav-section-title">ALMACENAMIENTO Y RECURSOS</div>

        <div class="nav-item <?= $activeView === 'files' ? 'active' : '' ?>" data-view="files">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-folder"></use></svg>
            <span>Archivos</span>
          </div>
          <span class="nav-badge" id="badge-files">Root</span>
        </div>

        <div class="nav-item <?= $activeView === 'shares' ? 'active' : '' ?>" data-view="shares">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-folder"></use></svg>
            <span>Redes compartidas</span>
          </div>
          <span class="nav-badge" id="badge-shares">...</span>
        </div>

        <div class="nav-item <?= $activeView === 'backups' ? 'active' : '' ?>" data-view="backups">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-shield"></use></svg>
            <span>Respaldos</span>
          </div>
          <span class="nav-badge" id="badge-backups">...</span>
        </div>

        <div class="nav-item <?= $activeView === 'storage' ? 'active' : '' ?>" data-view="storage">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-hard-drive"></use></svg>
            <span>Almacenamiento</span>
          </div>
          <span class="nav-badge" id="badge-storage"><?= round($diskTotal / 1024, 1) ?> TB</span>
        </div>

        <div class="nav-section-title">ADMINISTRACIÓN Y SISTEMA</div>

        <div class="nav-item <?= $activeView === 'dashboard' ? 'active' : '' ?>" data-view="dashboard">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-dashboard"></use></svg>
            <span>Vista general</span>
          </div>
          <span class="nav-badge">OK</span>
        </div>

        <div class="nav-item <?= $activeView === 'terminal' ? 'active' : '' ?>" data-view="terminal">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-terminal"></use></svg>
            <span>Terminal</span>
          </div>
          <span class="nav-badge">CLI</span>
        </div>

        <div class="nav-item <?= $activeView === 'users' ? 'active' : '' ?>" data-view="users">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-users"></use></svg>
            <span>Usuarios y grupos</span>
          </div>
          <span class="nav-badge" id="badge-users">...</span>
        </div>

        <div class="nav-item <?= $activeView === 'domain' ? 'active' : '' ?>" data-view="domain">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-domain"></use></svg>
            <span>Dominio AD</span>
          </div>
          <span class="nav-badge" id="badge-domain">AD</span>
        </div>

        <div class="nav-item <?= $activeView === 'services' ? 'active' : '' ?>" data-view="services">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-services"></use></svg>
            <span>Servicios</span>
          </div>
          <span class="nav-badge" id="badge-services">OK</span>
        </div>

        <div class="nav-item <?= $activeView === 'logs' ? 'active' : '' ?>" data-view="logs">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-file"></use></svg>
            <span>Registros (Logs)</span>
          </div>
          <span class="nav-badge" id="badge-logs">Live</span>
        </div>

        <div class="nav-item <?= $activeView === 'diagnostics' ? 'active' : '' ?>" data-view="diagnostics">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-wrench"></use></svg>
            <span>Diagnóstico</span>
          </div>
          <span class="nav-badge" id="badge-diagnostics">Live</span>
        </div>

        <div class="nav-item <?= $activeView === 'networking' ? 'active' : '' ?>" data-view="networking">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-network"></use></svg>
            <span>Redes</span>
          </div>
          <span class="nav-badge">1 Gbps</span>
        </div>

        <div class="nav-item <?= $activeView === 'updates' ? 'active' : '' ?>" data-view="updates">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-download"></use></svg>
            <span>Actualizaciones</span>
          </div>
          <span class="nav-badge badge-ok">Al día</span>
        </div>

        <div class="nav-item <?= $activeView === 'applications' ? 'active' : '' ?>" data-view="applications">
          <div class="nav-item-left">
            <svg class="icon"><use href="#icon-apps"></use></svg>
            <span>Componentes</span>
          </div>
          <span class="nav-badge">Nativo</span>
        </div>

      </nav>

      <div class="sidebar-footer">
        <div class="server-quick-pill">
          <span><span class="status-dot status-ok"></span> Debian 13 (Trixie)</span>
          <span class="badge badge-gray" style="font-size:10px;"><?= $hostname ?></span>
        </div>
        <div class="sidebar-footer-sub">
          <span>Kernel <?= $kernel ?></span>
          <span>Uptime: <?= $uptime ?></span>
        </div>
      </div>
    </aside>

    <div class="sidebar-backdrop" id="sidebar-backdrop"></div>

    <!-- ==============================================================================
         ÁREA DE CONTENIDO PRINCIPAL
         ============================================================================== -->
    <main class="main-content">

      <!-- 1. VISTA GENERAL (DASHBOARD) -->
      <section id="view-dashboard" class="view-section <?= $activeView === 'dashboard' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-dashboard"></use></svg> Vista general del servidor</h2>
            <p>Servidor NAS departamental & Central de copias de seguridad de alta resiliencia</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-primary" onclick="switchView('shares'); openModal('modal-new-share');">
              <svg class="icon"><use href="#icon-plus"></use></svg> Nueva red compartida
            </button>
            <button class="btn btn-secondary" onclick="switchView('backups'); openModal('modal-new-backup');">
              <svg class="icon"><use href="#icon-shield"></use></svg> Nuevo respaldo
            </button>
          </div>
        </div>

        <div class="kpi-grid">
          <!-- Tarjeta 1: Almacenamiento -->
          <div class="kpi-card">
            <div class="kpi-header">
              <span>Almacenamiento (/srv/nas)</span>
              <div class="kpi-icon-wrap"><svg class="icon"><use href="#icon-hard-drive"></use></svg></div>
            </div>
            <div class="kpi-val" id="kpi-storage-val"><?= $diskUsed ?> GB <small style="font-size:12px; color:var(--text-secondary);">/ <?= $diskTotal ?> GB</small></div>
            <div class="progress-bar-wrap">
              <div class="progress-bar-fill" id="kpi-storage-bar" style="width: <?= $diskPct ?>%; background: var(--accent-primary);"></div>
            </div>
            <div class="kpi-sub">
              <span>Sistema: <strong><?= strtoupper($fsType) ?></strong> • <?= $deviceType ?></span>
            </div>
          </div>

          <!-- Tarjeta 2: RAM -->
          <div class="kpi-card">
            <div class="kpi-header">
              <span>Memoria RAM</span>
              <div class="kpi-icon-wrap"><svg class="icon"><use href="#icon-cpu"></use></svg></div>
            </div>
            <div class="kpi-val" id="kpi-ram-val"><?= $ramUsed ?> GB <small style="font-size:12px; color:var(--text-secondary);">/ <?= $ramTotal ?> GB</small></div>
            <div class="progress-bar-wrap">
              <div class="progress-bar-fill" id="kpi-ram-bar" style="width: <?= $ramPct ?>%; background: var(--accent-primary);"></div>
            </div>
            <div class="kpi-sub">
              <span>CPU: <?= $cpuPct ?>% • <?= $cpuModel ?></span>
            </div>
          </div>

          <!-- Tarjeta 3: Red y Samba -->
          <div class="kpi-card">
            <div class="kpi-header">
              <span>Servicios de Red</span>
              <div class="kpi-icon-wrap"><svg class="icon"><use href="#icon-server"></use></svg></div>
            </div>
            <div class="kpi-val" style="color:var(--accent-success-text);">Samba 4 & Nginx</div>
            <div style="display:flex; gap:6px; flex-wrap:wrap; margin-top:4px;">
              <span class="badge badge-ok">smbd OK</span>
              <span class="badge badge-ok">wsdd2 OK</span>
              <span class="badge badge-ok">nginx OK</span>
              <span class="badge badge-ok">php-fpm ondemand</span>
            </div>
            <div class="kpi-sub">
              <span>Optimización Office VFS activa</span>
            </div>
          </div>

          <!-- Tarjeta 4: Resiliencia -->
          <div class="kpi-card">
            <div class="kpi-header">
              <span>Resiliencia & Protección</span>
              <div class="kpi-icon-wrap"><svg class="icon"><use href="#icon-wrench"></use></svg></div>
            </div>
            <div class="kpi-val"><?= $deviceType ?></div>
            <div style="display:flex; gap:6px; flex-wrap:wrap; margin-top:4px;">
              <span class="tag-pill">Staging atómico</span>
              <span class="tag-pill">Hardlinks &gt;85%</span>
            </div>
            <div class="kpi-sub">
              <span style="color:var(--accent-success-text);">✔ Resiliencia contra apagones</span>
            </div>
          </div>
        </div>

        <!-- Banner Informativo -->
        <div class="alert-box alert-box-success" style="margin-top:20px;">
          <svg class="icon icon-lg"><use href="#icon-check-circle"></use></svg>
          <div>
            <strong>Entorno Web Nativo Activo:</strong> Nginx-light con PHP-FPM bajo demanda (<code>pm = ondemand</code>).
            Consumo en reposo ~0 MB de memoria RAM. Arquitectura MVC desacoplada, tipado estricto y ejecución segura con <code>proc_open</code>.
          </div>
        </div>

        <!-- Grilla de Operaciones Frecuentes y Feed -->
        <div class="card-grid-2" style="margin-top:20px;">
          <div class="panel-card">
            <div class="panel-card-head">
              <h3><svg class="icon"><use href="#icon-wrench"></use></svg> Operaciones frecuentes</h3>
            </div>
            <div class="panel-card-body" style="display:flex; flex-direction:column; gap:10px;">
              <button class="btn btn-secondary" style="justify-content:flex-start;" onclick="switchView('shares'); openModal('modal-new-share');">
                <svg class="icon" style="color:var(--accent-primary);"><use href="#icon-folder"></use></svg>
                <span>Crear nueva carpeta compartida en Samba con ACLs granulares</span>
              </button>
              <button class="btn btn-secondary" style="justify-content:flex-start;" onclick="switchView('users'); openModal('modal-new-user');">
                <svg class="icon" style="color:var(--accent-primary);"><use href="#icon-users"></use></svg>
                <span>Dar de alta un usuario y sincronizar credenciales smbpasswd</span>
              </button>
              <button class="btn btn-secondary" style="justify-content:flex-start;" onclick="switchView('backups'); openModal('modal-new-backup');">
                <svg class="icon" style="color:var(--accent-success);"><use href="#icon-shield"></use></svg>
                <span>Programar tarea de réplica remota (CIFS 3.1.1 o SSH Linux)</span>
              </button>
              <button class="btn btn-secondary" style="justify-content:flex-start;" onclick="switchView('storage'); runStorageScrub();">
                <svg class="icon" style="color:var(--accent-warning);"><use href="#icon-refresh"></use></svg>
                <span>Iniciar auditoría criptográfica BTRFS scrub contra Bit Rot</span>
              </button>
            </div>
          </div>

          <div class="panel-card">
            <div class="panel-card-head">
              <h3><svg class="icon"><use href="#icon-clock"></use></svg> Registro de eventos en vivo</h3>
              <span class="badge badge-ok">journalctl</span>
            </div>
            <div class="panel-card-body" id="dashboard-activity-feed" style="display:flex; flex-direction:column; gap:8px;">
              <p style="color:var(--text-secondary);">Cargando bitácoras recientes...</p>
            </div>
          </div>
        </div>
      </section>

      <!-- 2. REGISTROS (LOGS & AUDITORÍA) -->
      <section id="view-logs" class="view-section <?= $activeView === 'logs' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-file"></use></svg> Auditoría y registros del sistema</h2>
            <p>Trazabilidad integral: accesos Samba a archivos, auditoría administrativa, réplicas y eventos journald</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-secondary" onclick="exportOrCopyLogs()" title="Copiar registros visibles al portapapeles">
              <svg class="icon"><use href="#icon-copy"></use></svg> Copiar
            </button>
            <button class="btn btn-secondary" onclick="loadLogs()" title="Actualizar registros">
              <svg class="icon"><use href="#icon-refresh"></use></svg> Actualizar
            </button>
          </div>
        </div>

        <!-- Barra de Filtros, Categorías y Búsqueda -->
        <div class="filter-toolbar">
          <div class="filter-chips" id="logs-filter-chips">
            <button type="button" class="chip-btn active" data-source="all" onclick="setLogSource('all')">Todos</button>
            <button type="button" class="chip-btn" data-source="samba_audit" onclick="setLogSource('samba_audit')">Auditoría Samba (Archivos)</button>
            <button type="button" class="chip-btn" data-source="admin" onclick="setLogSource('admin')">Auditoría Web / Admin</button>
            <button type="button" class="chip-btn" data-source="backup" onclick="setLogSource('backup')">Respaldos</button>
            <button type="button" class="chip-btn" data-source="system" onclick="setLogSource('system')">Sistema (journald)</button>
          </div>

          <div style="display:flex; align-items:center; gap:10px;">
            <div class="search-box">
              <svg class="icon" style="color:var(--text-muted);"><use href="#icon-search"></use></svg>
              <input type="text" id="logs-search-input" placeholder="Buscar por usuario, IP, acción o recurso..." oninput="debounceLogSearch()">
            </div>
            <select id="logs-limit-select" class="form-control" style="width:90px; padding:4px 8px; font-size:12px; background:var(--bg-input); color:var(--text-main); border:1px solid var(--border-color); border-radius:var(--radius-md);" onchange="loadLogs()">
              <option value="50">50</option>
              <option value="100" selected>100</option>
              <option value="250">250</option>
              <option value="500">500</option>
            </select>
          </div>
        </div>

        <div class="panel-card">
          <div class="table-responsive">
            <table class="nas-table">
              <thead id="logs-table-head">
                <tr>
                  <th style="width:160px;">Timestamp</th>
                  <th style="width:100px;">Origen</th>
                  <th style="width:130px;">Usuario / IP</th>
                  <th style="width:150px;">Acción / Evento</th>
                  <th style="width:160px;">Objetivo / Recurso</th>
                  <th style="width:90px;">Estado</th>
                  <th>Detalles</th>
                </tr>
              </thead>
              <tbody id="logs-table-body">
                <tr><td colspan="7" style="text-align:center;">Cargando registros...</td></tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <!-- 3. ALMACENAMIENTO (STORAGE) -->
      <section id="view-storage" class="view-section <?= $activeView === 'storage' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-hard-drive"></use></svg> Almacenamiento y discos físicos</h2>
            <p>Monitoreo de particiones, integridad de datos y mantenimiento BTRFS / TRIM</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-secondary" onclick="runStorageScrub()">
              <svg class="icon"><use href="#icon-shield"></use></svg> Iniciar Scrub BTRFS
            </button>
            <button class="btn btn-secondary" onclick="runStorageTrim()">
              <svg class="icon"><use href="#icon-wrench"></use></svg> Ejecutar TRIM SSD
            </button>
          </div>
        </div>

        <div class="panel-card" style="margin-bottom:20px;">
          <div class="panel-card-head">
            <h3>Dispositivos de bloque reconocidos (lsblk)</h3>
          </div>
          <div class="table-responsive">
            <table class="nas-table">
              <thead>
                <tr>
                  <th>Dispositivo</th>
                  <th>Modelo</th>
                  <th>Tamaño</th>
                  <th>Tipo</th>
                  <th>Punto de montaje</th>
                  <th>Filesystem</th>
                </tr>
              </thead>
              <tbody id="storage-disks-body">
                <tr><td colspan="6" style="text-align:center;">Cargando inventario de discos...</td></tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <!-- 4. REDES (NETWORKING) -->
      <section id="view-networking" class="view-section <?= $activeView === 'networking' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-network"></use></svg> Configuración y visibilidad de red</h2>
            <p>Protocolos de descubrimiento SMB 3.1.1, WSDD2 / LLMNR para clientes Windows 10/11</p>
          </div>
        </div>

        <div class="panel-card">
          <div class="panel-card-body" style="display:flex; flex-direction:column; gap:16px;">
            <div class="info-row">
              <span class="info-label">NetBIOS Name:</span>
              <strong class="info-val"><?= $hostname ?></strong>
            </div>
            <div class="info-row">
              <span class="info-label">Workgroup:</span>
              <strong class="info-val">TEAM-JOFRATO</strong>
            </div>
            <div class="info-row">
              <span class="info-label">Protocolo SMB:</span>
              <strong class="info-val">SMB 3.1.1 (Min: SMB 2.02)</strong>
            </div>
            <div class="info-row">
              <span class="info-label">Descubrimiento WSD:</span>
              <strong class="info-val" style="color:var(--accent-success-text);">WSDD2 Activo (Puertos 3702, 5355, 5357)</strong>
            </div>
            <div class="info-row">
              <span class="info-label">Ruta de Red Windows:</span>
              <strong class="info-val"><code>\\<?= $hostname ?></code> o <code>\\<?= htmlspecialchars($_SERVER['SERVER_ADDR'] ?? '10.10.1.2', ENT_QUOTES, 'UTF-8') ?></code></strong>
            </div>
          </div>
        </div>
      </section>

      <!-- 5. SERVICIOS (SERVICES) -->
      <section id="view-services" class="view-section <?= $activeView === 'services' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-services"></use></svg> Demonios y servicios del sistema</h2>
            <p>Control de ejecución de Samba, WSDD2, Nginx, PHP-FPM y Cron</p>
          </div>
        </div>

        <div class="panel-card">
          <div class="table-responsive">
            <table class="nas-table">
              <thead>
                <tr>
                  <th>Servicio</th>
                  <th>Descripción</th>
                  <th>Estado</th>
                  <th style="width:140px; text-align:right;">Acciones</th>
                </tr>
              </thead>
              <tbody id="services-table-body">
                <tr><td colspan="4" style="text-align:center;">Cargando servicios...</td></tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <!-- EXPLORADOR DE ARCHIVOS (FILES) -->
      <section id="view-files" class="view-section <?= $activeView === 'files' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-folder"></use></svg> Explorador de Archivos y Recursos</h2>
            <p>Gestión directa de carpetas compartidas y respaldos con cuadrícula estilo Windows, previsualizador y papelera</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-secondary" id="btn-open-trash" onclick="toggleTrashView()" title="Papelera de reciclaje y recuperación de archivos">
              <svg class="icon" style="color:var(--accent-warning);"><use href="#icon-trash"></use></svg>
              <span>Papelera</span>
              <span class="badge badge-warn" id="badge-trash-count" style="margin-left:4px;">0</span>
            </button>
            <button class="btn btn-secondary" onclick="refreshCurrentFileView()" title="Actualizar lista de archivos">
              <svg class="icon"><use href="#icon-refresh"></use></svg> Actualizar
            </button>
            <button class="btn btn-secondary" id="btn-new-folder" onclick="openNewFolderModal()">
              <svg class="icon"><use href="#icon-plus"></use></svg> Nueva carpeta
            </button>
            <button class="btn btn-primary" id="btn-upload-files" onclick="triggerFileInput()">
              <svg class="icon"><use href="#icon-upload"></use></svg> Subir archivos
            </button>
          </div>
        </div>

        <div class="panel-card">
          <!-- Vista Normal de Archivos -->
          <div id="files-normal-view">
            <!-- Barra de navegación y herramientas -->
            <div class="explorer-toolbar">
              <div style="display:flex; align-items:center; gap:10px; flex-wrap:wrap;">
                <label for="files-root-select" style="font-weight:600; font-size:13px; color:var(--text-secondary);">Raíz:</label>
                <select id="files-root-select" onchange="changeFilesRoot(this.value)" style="width:auto; padding:5px 10px;">
                  <option value="nas">Recursos Compartidos (/srv/nas)</option>
                  <option value="backups">Repositorio de Backups (/srv/nas/BACKUPS_HISTORICOS)</option>
                </select>
              </div>
              <div class="file-breadcrumbs" id="files-breadcrumbs">
                <span class="file-breadcrumb-current">/srv/nas</span>
              </div>
              <div style="display:flex; align-items:center; gap:8px;">
                <!-- Selector de Vista: Cuadrícula Windows / Lista Detallada -->
                <div class="view-toggle-group">
                  <button type="button" class="view-toggle-btn active" id="btn-view-grid" onclick="setFileViewMode('grid')" title="Vista en cuadrícula (iconos grandes estilo Windows)">
                    <svg class="icon"><use href="#icon-grid"></use></svg>
                  </button>
                  <button type="button" class="view-toggle-btn" id="btn-view-list" onclick="setFileViewMode('list')" title="Vista en lista detallada">
                    <svg class="icon"><use href="#icon-list"></use></svg>
                  </button>
                </div>
                <button class="btn btn-secondary btn-sm" id="btn-download-zip" onclick="downloadCurrentFolderZip()" title="Descargar la carpeta actual completa comprimida en archivo .zip">
                  <svg class="icon"><use href="#icon-download"></use></svg> Descargar ZIP
                </button>
              </div>
            </div>

            <!-- Barra de progreso de subida -->
            <div id="upload-progress-bar" class="upload-progress-bar">
              <div style="display:flex; justify-content:space-between; margin-bottom:6px; font-size:13px; font-weight:600;">
                <span id="upload-file-label">Subiendo archivo...</span>
                <span id="upload-percent-label">0%</span>
              </div>
              <div class="progress-bar-wrap">
                <div id="upload-progress-fill" class="progress-bar-fill" style="width:0%; background:var(--accent-primary);"></div>
              </div>
            </div>

            <!-- Contenedor con Dropzone, Cuadrícula y Tabla -->
            <div class="file-dropzone-container" id="file-dropzone-container">
              <div class="file-dropzone-overlay" id="file-dropzone-overlay">
                <svg class="icon" style="width:48px; height:48px; color:var(--accent-primary);"><use href="#icon-upload"></use></svg>
                <div class="dropzone-text">Suelta los archivos aquí para subirlos a esta carpeta</div>
                <div style="font-size:13px; color:var(--text-muted);">Soporta transferencias directas de hasta 512 MB por archivo</div>
              </div>

              <input type="file" id="files-hidden-input" multiple style="display:none;" onchange="handleFileSelect(event)">

              <!-- Vista en Cuadrícula estilo Windows Explorer -->
              <div class="file-grid-container" id="files-grid-container">
                <div style="grid-column:1/-1; text-align:center; padding:30px; color:var(--text-muted);">Cargando archivos...</div>
              </div>

              <!-- Vista en Lista Detallada (Tabla) -->
              <div class="table-responsive" id="files-table-container" style="display:none;">
                <table class="nas-table">
                  <thead>
                    <tr>
                      <th style="width:36px;"></th>
                      <th>Nombre</th>
                      <th>Tamaño</th>
                      <th>Permisos</th>
                      <th>Propietario / Grupo</th>
                      <th>Última modificación</th>
                      <th style="width:170px; text-align:right;">Acciones</th>
                    </tr>
                  </thead>
                  <tbody id="files-table-body">
                    <tr><td colspan="7" style="text-align:center;">Cargando archivos...</td></tr>
                  </tbody>
                </table>
              </div>
            </div>
          </div>

          <!-- Vista de Papelera de Reciclaje -->
          <div id="files-trash-view" style="display:none;">
            <div class="trash-toolbar">
              <div style="display:flex; align-items:center; gap:10px;">
                <button class="btn btn-secondary btn-sm" onclick="exitTrashView()" title="Regresar al explorador de archivos">
                  <svg class="icon"><use href="#icon-restore"></use></svg> Volver a Archivos
                </button>
                <div style="font-size:13px; font-weight:600; color:var(--accent-warning); display:flex; align-items:center; gap:6px;">
                  <svg class="icon"><use href="#icon-trash"></use></svg> Papelera de Reciclaje Confinada (/srv/nas/.trash)
                </div>
              </div>
              <div style="display:flex; align-items:center; gap:8px;">
                <button class="btn btn-danger btn-sm" id="btn-empty-trash" onclick="openEmptyTrashModal()" title="Eliminar definitivamente todos los elementos">
                  <svg class="icon"><use href="#icon-trash"></use></svg> Vaciar papelera
                </button>
              </div>
            </div>

            <div style="padding:12px 20px; background:rgba(245, 158, 11, 0.08); border-bottom:1px solid var(--border-color); font-size:12.5px; color:var(--text-secondary); display:flex; align-items:center; gap:8px;">
              <svg class="icon" style="color:var(--accent-warning); flex-shrink:0;"><use href="#icon-info"></use></svg>
              <span>Los archivos eliminados se protegen aquí. Puedes restaurarlos en cualquier momento a su ubicación original o purgarlos definitivamente.</span>
            </div>

            <div class="table-responsive">
              <table class="nas-table">
                <thead>
                  <tr>
                    <th style="width:36px;"></th>
                    <th>Nombre original</th>
                    <th>Ruta previa</th>
                    <th>Tamaño</th>
                    <th>Eliminado por</th>
                    <th>Fecha de eliminación</th>
                    <th style="width:140px; text-align:right;">Acciones</th>
                  </tr>
                </thead>
                <tbody id="trash-table-body">
                  <tr><td colspan="7" style="text-align:center;">Cargando papelera...</td></tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </section>

      <!-- TERMINAL WEB REAL -->
      <section id="view-terminal" class="view-section <?= $activeView === 'terminal' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-terminal"></use></svg> Terminal interactiva del sistema</h2>
            <p>Ejecución directa de comandos en bash con permisos controlados de administración</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-secondary" onclick="clearTerminal()">
              <svg class="icon"><use href="#icon-refresh"></use></svg> Limpiar consola
            </button>
          </div>
        </div>

        <div class="terminal-container">
          <div class="terminal-bar">
            <span>Terminal NAS • Debian 13 (Trixie)</span>
            <span style="opacity:0.8;">bash | cwd: <span id="term-bar-cwd">/srv/nas</span></span>
          </div>
          <div class="terminal-chips">
            <span style="font-size:11.5px; color:var(--text-muted); align-self:center; margin-right:4px;">Comandos rápidos:</span>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('uptime')">uptime</button>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('df -h /srv/nas')">df -h</button>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('free -m')">free -m</button>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('smbstatus')">smbstatus</button>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('systemctl status smbd')">status smbd</button>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('systemctl status wsdd2')">status wsdd2</button>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('realm list')">realm list</button>
            <button type="button" class="terminal-chip" onclick="runQuickCommand('ls -la')">ls -la</button>
          </div>
          <div class="terminal-body" id="terminal-output">Servidor NAS Debian 13 (Trixie) - Consola Web de Administración
Sesión activa: <?= $sessionUser ?> | Directorio de trabajo: /srv/nas
Escribe 'help' o cualquier comando del sistema para ejecutar.

</div>
          <div class="terminal-input-row">
            <span class="terminal-prompt-prefix" id="terminal-prompt-prefix"><?= $sessionUser ?>@<?= $hostname ?>:<span id="term-prompt-cwd">/srv/nas</span>$</span>
            <input type="text" id="terminal-input" class="terminal-input" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false" placeholder="Escribe un comando bash y pulsa Enter (↑/↓ para historial)...">
          </div>
        </div>
      </section>

      <!-- 7. REDES COMPARTIDAS (SHARES) -->
      <section id="view-shares" class="view-section <?= $activeView === 'shares' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-folder"></use></svg> Carpetas y recursos compartidos (Samba)</h2>
            <p>Recursos visibles y ocultos ($) con 4 esquemas de permisos y aceleración Office</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-primary" onclick="openModal('modal-new-share')">
              <svg class="icon"><use href="#icon-plus"></use></svg> Crear recurso compartido
            </button>
          </div>
        </div>

        <div class="panel-card">
          <div class="table-responsive">
            <table class="nas-table">
              <thead>
                <tr>
                  <th>Recurso</th>
                  <th>Ruta en disco</th>
                  <th>Esquema de permisos</th>
                  <th>Visibilidad</th>
                  <th>Usuarios / Grupos</th>
                  <th style="width:100px; text-align:right;">Acciones</th>
                </tr>
              </thead>
              <tbody id="shares-table-body">
                <tr><td colspan="6" style="text-align:center;">Cargando recursos compartidos...</td></tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <!-- 8. CENTRAL DE RESPALDOS (BACKUPS) -->
      <section id="view-backups" class="view-section <?= $activeView === 'backups' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-shield"></use></svg> Central de copias de seguridad</h2>
            <p>Réplicas remotas CIFS 3.1.1 y SSH Linux con deduplicación por Hardlinks (&gt;85% ahorro)</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-primary" onclick="openModal('modal-new-backup')">
              <svg class="icon"><use href="#icon-plus"></use></svg> Nueva tarea de backup
            </button>
          </div>
        </div>

        <div class="panel-card">
          <div class="table-responsive">
            <table class="nas-table">
              <thead>
                <tr>
                  <th>Identificador</th>
                  <th>Protocolo</th>
                  <th>Origen remoto</th>
                  <th>Horario Cron</th>
                  <th>Retención</th>
                  <th>Último estado</th>
                  <th style="width:160px; text-align:right;">Acciones</th>
                </tr>
              </thead>
              <tbody id="backups-table-body">
                <tr><td colspan="7" style="text-align:center;">Cargando tareas de backup...</td></tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>

      <!-- 9. USUARIOS Y GRUPOS (USERS) -->
      <section id="view-users" class="view-section <?= $activeView === 'users' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-users"></use></svg> Usuarios y grupos departamentales</h2>
            <p>Sincronización estricta con smbpasswd y prefijo corporativo mandatorio 'grp_*'</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-primary" onclick="openModal('modal-new-user')">
              <svg class="icon"><use href="#icon-plus"></use></svg> Crear usuario
            </button>
            <button class="btn btn-secondary" onclick="openModal('modal-new-group')">
              <svg class="icon"><use href="#icon-plus"></use></svg> Crear grupo
            </button>
          </div>
        </div>

        <div class="card-grid-2">
          <!-- Tabla de Usuarios -->
          <div class="panel-card">
            <div class="panel-card-head">
              <h3>Usuarios registrados</h3>
            </div>
            <div class="table-responsive">
              <table class="nas-table">
                <thead>
                  <tr>
                    <th>Usuario</th>
                    <th>UID</th>
                    <th>Rol</th>
                    <th>Samba</th>
                    <th style="width:80px; text-align:right;">Acción</th>
                  </tr>
                </thead>
                <tbody id="users-table-body">
                  <tr><td colspan="5" style="text-align:center;">Cargando usuarios...</td></tr>
                </tbody>
              </table>
            </div>
          </div>

          <!-- Tabla de Grupos -->
          <div class="panel-card">
            <div class="panel-card-head">
              <h3>Grupos corporativos (grp_*)</h3>
            </div>
            <div class="table-responsive">
              <table class="nas-table">
                <thead>
                  <tr>
                    <th>Grupo</th>
                    <th>GID</th>
                    <th>Miembros</th>
                    <th style="width:80px; text-align:right;">Acción</th>
                  </tr>
                </thead>
                <tbody id="groups-table-body">
                  <tr><td colspan="4" style="text-align:center;">Cargando grupos...</td></tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </section>

      <!-- 10. ACTUALIZACIONES (UPDATES) -->
      <section id="view-updates" class="view-section <?= $activeView === 'updates' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-download"></use></svg> Actualizaciones de software</h2>
            <p>Control de versiones del servidor NAS y parches del sistema Debian 13</p>
          </div>
        </div>

        <div class="panel-card">
          <div class="panel-card-body" id="updates-container">
            <p>Cargando información de versiones...</p>
          </div>
        </div>
      </section>

      <!-- 11. COMPONENTES (APPLICATIONS) -->
      <section id="view-applications" class="view-section <?= $activeView === 'applications' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-apps"></use></svg> Módulos y componentes del servidor</h2>
            <p>Tecnologías integradas para alto rendimiento, concurrencia y resiliencia</p>
          </div>
        </div>

        <div class="card-grid-2">
          <div class="panel-card">
            <div class="panel-card-head"><h3>Samba 4 & VFS</h3></div>
            <div class="panel-card-body">Módulos <code>acl_xattr</code> y <code>streams_xattr</code> para prevención de cuellos de botella en Excel/Office con +100 equipos.</div>
          </div>
          <div class="panel-card">
            <div class="panel-card-head"><h3>Nginx-light & PHP-FPM</h3></div>
            <div class="panel-card-body">Servidor HTTP ultraligero con PHP 8 y gestor bajo demanda (<code>pm = ondemand</code>), consumo ~0 MB RAM en reposo.</div>
          </div>
          <div class="panel-card">
            <div class="panel-card-head"><h3>BTRFS con Zstandard</h3></div>
            <div class="panel-card-body">Compresión transparente Zstd:3, sumas de comprobación criptográficas y auditoría mensual contra Bit Rot.</div>
          </div>
          <div class="panel-card">
            <div class="panel-card-head"><h3>Rsync Multiplataforma</h3></div>
            <div class="panel-card-body">Motor diferencial con preservación de inodos (hardlinks), staging atómico <code>.inprogress_*</code> y CIFS 3.1.1 / SSH.</div>
          </div>
        </div>
      </section>

      <!-- 12. DOMINIO AD (DOMAIN) -->
      <section id="view-domain" class="view-section <?= $activeView === 'domain' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-domain"></use></svg> Integración con Active Directory (AD)</h2>
            <p>Gestión corporativa de dominio con Kerberos, SSSD y realmd en Debian 13</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-secondary" onclick="loadDomainStatus()">
              <svg class="icon"><use href="#icon-refresh"></use></svg> Actualizar estado
            </button>
          </div>
        </div>

        <div class="card-grid-2">
          <!-- Tarjeta de Estado del Dominio -->
          <div class="panel-card">
            <div class="panel-card-head">
              <h3>Estado de membresía de dominio</h3>
            </div>
            <div class="panel-card-body" id="domain-status-card">
              <div style="text-align:center; padding:20px; color:var(--text-muted);">Consultando estado de dominio...</div>
            </div>
          </div>

          <!-- Tarjeta de Descubrimiento de Dominio -->
          <div class="panel-card">
            <div class="panel-card-head">
              <h3>Descubrir Controlador de Dominio (DC)</h3>
            </div>
            <div class="panel-card-body">
              <p style="margin-bottom:14px; font-size:13px; color:var(--text-secondary);">Comprueba la conectividad DNS y Kerberos con el controlador antes de unirte:</p>
              <form id="form-domain-discover" onsubmit="handleDomainDiscover(event)">
                <div class="form-group">
                  <label for="discover-domain-name">Nombre de dominio FQDN:</label>
                  <input type="text" id="discover-domain-name" placeholder="ej. corp.miempresa.local" required>
                </div>
                <button type="submit" class="btn btn-secondary" id="btn-discover-domain">
                  <svg class="icon"><use href="#icon-network"></use></svg> Probar y descubrir
                </button>
              </form>
              <div id="domain-discover-result" style="margin-top:14px; display:none;"></div>
            </div>
          </div>
        </div>

        <!-- Formulario de Unión al Dominio -->
        <div class="panel-card" style="margin-top:20px;" id="domain-join-panel">
          <div class="panel-card-head">
            <h3>Unir este servidor NAS a Active Directory</h3>
          </div>
          <div class="panel-card-body">
            <form id="form-domain-join" onsubmit="handleDomainJoin(event)">
              <div class="card-grid-2">
                <div class="form-group">
                  <label for="join-domain-name">Nombre de Dominio (FQDN):</label>
                  <input type="text" id="join-domain-name" placeholder="ej. EMPRESA.LOCAL" required>
                </div>
                <div class="form-group">
                  <label for="join-admin-user">Usuario Administrador del Dominio:</label>
                  <input type="text" id="join-admin-user" placeholder="ej. Administrator o admin_ad" required>
                </div>
              </div>
              <div class="card-grid-2">
                <div class="form-group">
                  <label for="join-admin-pass">Contraseña de Administrador:</label>
                  <input type="password" id="join-admin-pass" placeholder="Contraseña de la cuenta con permisos en AD" required>
                </div>
                <div class="form-group">
                  <label for="join-ou">Unidad Organizativa (OU) Opcional:</label>
                  <input type="text" id="join-ou" placeholder="ej. OU=Servidores,DC=empresa,DC=local">
                </div>
              </div>
              <div style="margin-top:10px;">
                <button type="submit" class="btn btn-primary" id="btn-join-domain">
                  <svg class="icon"><use href="#icon-domain"></use></svg> Unir al Dominio
                </button>
              </div>
            </form>
          </div>
        </div>
      </section>

      <!-- 13. DIAGNÓSTICO EN VIVO (DIAGNOSTICS) -->
      <section id="view-diagnostics" class="view-section <?= $activeView === 'diagnostics' ? 'active' : '' ?>">
        <div class="page-head">
          <div>
            <h2><svg class="icon" style="color:var(--accent-primary);"><use href="#icon-wrench"></use></svg> Diagnóstico y salud integral del sistema</h2>
            <p>Auditoría en tiempo real de demonios, almacenamiento, configuración Samba y tareas programadas</p>
          </div>
          <div class="page-head-actions">
            <button class="btn btn-primary" onclick="loadDiagnostics()">
              <svg class="icon"><use href="#icon-refresh"></use></svg> Ejecutar diagnóstico completo
            </button>
          </div>
        </div>

        <div class="kpi-grid">
          <div class="kpi-card">
            <div class="kpi-header">
              <span>Estado General</span>
              <div class="kpi-icon-wrap"><svg class="icon"><use href="#icon-check-circle"></use></svg></div>
            </div>
            <div class="kpi-val" id="diag-overall-val"><span class="badge badge-ok">OK</span></div>
            <div class="kpi-sub">
              <span id="diag-overall-sub">Subsistemas operativos</span>
            </div>
          </div>

          <div class="kpi-card">
            <div class="kpi-header">
              <span>Demonios Activos</span>
              <div class="kpi-icon-wrap"><svg class="icon"><use href="#icon-services"></use></svg></div>
            </div>
            <div class="kpi-val" id="diag-services-val">... / ...</div>
            <div class="kpi-sub">
              <span>Servicios clave en ejecución</span>
            </div>
          </div>

          <div class="kpi-card">
            <div class="kpi-header">
              <span>Configuración Samba</span>
              <div class="kpi-icon-wrap"><svg class="icon"><use href="#icon-folder"></use></svg></div>
            </div>
            <div class="kpi-val" id="diag-samba-val"><span class="badge badge-ok">Válida</span></div>
            <div class="kpi-sub">
              <span>Sintaxis testparm sin errores</span>
            </div>
          </div>
        </div>

        <div class="panel-card" style="margin-top:20px;">
          <div class="panel-card-head">
            <h3>Resultado de la última comprobación</h3>
          </div>
          <div class="panel-card-body" id="diagnostics-summary-container">
            <p>Cargando diagnóstico en vivo del servidor...</p>
          </div>
        </div>
      </section>

    </main>
  </div>

  <!-- ==============================================================================
       MODALES DE OPERACIÓN
       ============================================================================== -->

  <!-- Modal: Nuevo Recurso Compartido -->
  <div class="modal-backdrop" id="modal-new-share">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3>Crear nueva carpeta compartida</h3>
        <button class="modal-close" onclick="closeModal('modal-new-share')">&times;</button>
      </div>
      <div class="modal-body">
        <form id="form-new-share" onsubmit="submitNewShare(event)">
          <div class="form-group">
            <label for="share-name">Nombre del recurso compartido:</label>
            <input type="text" id="share-name" required placeholder="ej. CONTABILIDAD" pattern="[A-Za-z0-9_-]+">
            <small>Usa mayúsculas recomendadas sin espacios.</small>
          </div>
          <div class="form-group">
            <label for="share-comment">Descripción / Comentario:</label>
            <input type="text" id="share-comment" placeholder="ej. Documentos contables y fiscales">
          </div>
          <div class="form-group">
            <label for="share-scheme">Esquema de permisos granular:</label>
            <select id="share-scheme" onchange="toggleSchemeFields()">
              <option value="1">1. Lectura y Escritura por Grupo</option>
              <option value="2">2. Solo Lectura General + Escritura Exclusiva</option>
              <option value="3">3. Solo Lectura Estricta (Histórico)</option>
              <option value="4">4. Acceso Público / Invitados (guest ok)</option>
            </select>
          </div>
          <div class="form-group" id="group-share-groups">
            <label>Grupos autorizados:</label>
            <div id="share-groups-list" style="display:flex; flex-direction:column; gap:6px; max-height:120px; overflow-y:auto; padding:6px; background:var(--bg-body); border-radius:4px;">
              <!-- Llenado dinámicamente -->
            </div>
          </div>
          <div class="form-group" id="group-share-write-group" style="display:none;">
            <label for="share-write-group">Grupo con permiso exclusivo de escritura:</label>
            <select id="share-write-group"></select>
          </div>
          <div class="form-group">
            <label style="display:flex; align-items:center; gap:8px;">
              <input type="checkbox" id="share-hidden">
              <span>Recurso oculto (agrega sufijo <code>$</code> al nombre para no difundir en red)</span>
            </label>
          </div>
          <div class="modal-footer" style="padding:0; margin-top:20px;">
            <button type="button" class="btn btn-secondary" onclick="closeModal('modal-new-share')">Cancelar</button>
            <button type="submit" class="btn btn-primary">Crear recurso</button>
          </div>
        </form>
      </div>
    </div>
  </div>

  <!-- Modal: Nueva Tarea de Backup -->
  <div class="modal-backdrop" id="modal-new-backup">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3>Programar nueva tarea de respaldo</h3>
        <button class="modal-close" onclick="closeModal('modal-new-backup')">&times;</button>
      </div>
      <div class="modal-body">
        <form id="form-new-backup" onsubmit="submitNewBackup(event)">
          <div class="form-group">
            <label for="bkp-id">Identificador único de la tarea:</label>
            <input type="text" id="bkp-id" required placeholder="ej. srv_win_ventas" pattern="[a-z0-9_-]+">
          </div>
          <div class="form-group">
            <label for="bkp-proto">Protocolo de replicación:</label>
            <select id="bkp-proto" onchange="toggleBackupFields()">
              <option value="cifs">CIFS / SMB 3.1.1 (Servidor Windows)</option>
              <option value="ssh">SSH + Rsync (Servidor Linux Remoto)</option>
            </select>
          </div>
          <div class="form-group">
            <label for="bkp-ip">Dirección IP o Nombre del Host Remoto:</label>
            <input type="text" id="bkp-ip" required placeholder="ej. 10.10.1.50">
          </div>
          <div class="form-group" id="field-bkp-share">
            <label for="bkp-share">Recurso compartido remoto:</label>
            <input type="text" id="bkp-share" placeholder="ej. Facturacion o C$">
          </div>
          <div class="form-group" id="field-bkp-path" style="display:none;">
            <label for="bkp-path">Ruta absoluta remota en Linux:</label>
            <input type="text" id="bkp-path" placeholder="ej. /var/www">
          </div>
          <div class="form-group">
            <label for="bkp-user">Usuario remoto (ej. Administrador o DOMINIO\usuario):</label>
            <input type="text" id="bkp-user" required placeholder="ej. Administrador">
          </div>
          <div class="form-group">
            <label for="bkp-pass">Contraseña:</label>
            <input type="password" id="bkp-pass" required>
          </div>
          <div class="form-group">
            <label for="bkp-cron">Frecuencia programada (Cron):</label>
            <select id="bkp-cron">
              <option value="0 23 * * *">Diario a las 23:00 hrs (Recomendado)</option>
              <option value="0 */6 * * *">Cada 6 horas</option>
              <option value="0 * * * *">Cada hora en punto</option>
            </select>
          </div>
          <div class="form-group">
            <label for="bkp-retention">Snapshots a conservar antes de rotar:</label>
            <input type="number" id="bkp-retention" value="30" min="1" max="365">
          </div>
          <div class="modal-footer" style="padding:0; margin-top:20px;">
            <button type="button" class="btn btn-secondary" onclick="closeModal('modal-new-backup')">Cancelar</button>
            <button type="submit" class="btn btn-primary">Programar respaldo</button>
          </div>
        </form>
      </div>
    </div>
  </div>

  <!-- Modal: Nuevo Usuario -->
  <div class="modal-backdrop" id="modal-new-user">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3>Dar de alta usuario</h3>
        <button class="modal-close" onclick="closeModal('modal-new-user')">&times;</button>
      </div>
      <div class="modal-body">
        <form id="form-new-user" onsubmit="submitNewUser(event)">
          <div class="form-group">
            <label for="user-uname">Nombre de usuario:</label>
            <input type="text" id="user-uname" required placeholder="ej. operador1" pattern="[a-z0-9_-]+">
          </div>
          <div class="form-group">
            <label for="user-pass">Contraseña:</label>
            <input type="password" id="user-pass" required minlength="6">
          </div>
          <div class="form-group">
            <label>Grupos a asignar:</label>
            <div id="user-groups-list" style="display:flex; flex-direction:column; gap:6px; max-height:120px; overflow-y:auto; padding:6px; background:var(--bg-body); border-radius:4px;">
              <!-- Llenado dinámicamente -->
            </div>
          </div>
          <div class="form-group">
            <label style="display:flex; align-items:center; gap:8px;">
              <input type="checkbox" id="user-is-admin">
              <span>Privilegios administrativos (acceso sudo y grp_sistemas)</span>
            </label>
          </div>
          <div class="modal-footer" style="padding:0; margin-top:20px;">
            <button type="button" class="btn btn-secondary" onclick="closeModal('modal-new-user')">Cancelar</button>
            <button type="submit" class="btn btn-primary">Crear usuario</button>
          </div>
        </form>
      </div>
    </div>
  </div>

  <!-- Modal: Nuevo Grupo -->
  <div class="modal-backdrop" id="modal-new-group">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3>Crear grupo departamental</h3>
        <button class="modal-close" onclick="closeModal('modal-new-group')">&times;</button>
      </div>
      <div class="modal-body">
        <form id="form-new-group" onsubmit="submitNewGroup(event)">
          <div class="form-group">
            <label for="group-name">Nombre del grupo (prefijo <code>grp_</code> mandatorio):</label>
            <input type="text" id="group-name" required placeholder="ej. contabilidad">
            <small>Se guardará como <code>grp_contabilidad</code>.</small>
          </div>
          <div class="modal-footer" style="padding:0; margin-top:20px;">
            <button type="button" class="btn btn-secondary" onclick="closeModal('modal-new-group')">Cancelar</button>
            <button type="submit" class="btn btn-primary">Crear grupo</button>
          </div>
        </form>
      </div>
    </div>
  </div>

  <!-- Modal: Registro de Tarea de Backup -->
  <div class="modal-backdrop" id="modal-backup-logs">
    <div class="modal-dialog" style="max-width:700px;">
      <div class="modal-header">
        <h3 id="modal-backup-logs-title">Bitácora de respaldo</h3>
        <button class="modal-close" onclick="closeModal('modal-backup-logs')">&times;</button>
      </div>
      <div class="modal-body">
        <pre id="modal-backup-logs-content" style="background:var(--bg-terminal); color:var(--text-terminal); padding:12px; border-radius:4px; max-height:400px; overflow-y:auto; font-family:monospace; font-size:12px; white-space:pre-wrap;"></pre>
      </div>
      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-backup-logs')">Cerrar</button>
      </div>
    </div>
  </div>

  <!-- Modal: Nueva Carpeta en Explorador -->
  <div class="modal-backdrop" id="modal-new-folder">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3>Crear nueva carpeta</h3>
        <button class="modal-close" onclick="closeModal('modal-new-folder')">&times;</button>
      </div>
      <div class="modal-body">
        <form id="form-new-folder" onsubmit="submitNewFolder(event)">
          <div class="form-group">
            <label for="new-folder-name">Nombre de la carpeta:</label>
            <input type="text" id="new-folder-name" required placeholder="ej. Documentos_2026" pattern="[A-Za-z0-9._-]+">
            <small>Sin espacios ni caracteres especiales (/ \ : * ? &quot; &lt; &gt; |).</small>
          </div>
          <div class="modal-footer" style="padding:0; margin-top:20px;">
            <button type="button" class="btn btn-secondary" onclick="closeModal('modal-new-folder')">Cancelar</button>
            <button type="submit" class="btn btn-primary">Crear carpeta</button>
          </div>
        </form>
      </div>
    </div>
  </div>

  <!-- Modal: Renombrar Archivo o Carpeta -->
  <div class="modal-backdrop" id="modal-rename-file">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3>Renombrar elemento</h3>
        <button class="modal-close" onclick="closeModal('modal-rename-file')">&times;</button>
      </div>
      <div class="modal-body">
        <form id="form-rename-file" onsubmit="submitRenameFile(event)">
          <input type="hidden" id="rename-file-oldpath">
          <div class="form-group">
            <label for="rename-file-newname">Nuevo nombre:</label>
            <input type="text" id="rename-file-newname" required placeholder="Nuevo nombre">
          </div>
          <div class="modal-footer" style="padding:0; margin-top:20px;">
            <button type="button" class="btn btn-secondary" onclick="closeModal('modal-rename-file')">Cancelar</button>
            <button type="submit" class="btn btn-primary">Renombrar</button>
          </div>
        </form>
      </div>
    </div>
  </div>

  <!-- Modal: Eliminar Archivo o Carpeta -->
  <div class="modal-backdrop" id="modal-delete-file">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3 id="modal-delete-title"><svg class="icon" style="color:var(--accent-warning);"><use href="#icon-trash"></use></svg> Eliminar elemento</h3>
        <button class="modal-close" onclick="closeModal('modal-delete-file')">&times;</button>
      </div>
      <div class="modal-body">
        <input type="hidden" id="delete-file-path">
        <p>¿Qué deseas hacer con <strong id="delete-file-name-label">este elemento</strong>?</p>

        <div style="margin:16px 0; display:flex; flex-direction:column; gap:10px;">
          <label style="display:flex; align-items:flex-start; gap:10px; cursor:pointer; padding:12px; border-radius:var(--radius-sm); border:1px solid var(--border-color); background:var(--bg-card-header);">
            <input type="radio" name="delete_mode" id="del-mode-trash" value="trash" checked style="margin-top:3px;">
            <div>
              <strong style="color:var(--text-main); font-size:13.5px;">Mover a la papelera (Recomendado)</strong>
              <div style="color:var(--text-muted); font-size:12px; margin-top:2px;">El elemento se traslada de forma segura a la papelera confinada y podrá restaurarse en cualquier momento con su ruta exacta.</div>
            </div>
          </label>

          <label style="display:flex; align-items:flex-start; gap:10px; cursor:pointer; padding:12px; border-radius:var(--radius-sm); border:1px solid var(--border-color); background:var(--bg-card-header);">
            <input type="radio" name="delete_mode" id="del-mode-permanent" value="permanent" style="margin-top:3px;">
            <div>
              <strong style="color:var(--accent-danger); font-size:13.5px;">Eliminar permanentemente del disco</strong>
              <div style="color:var(--text-muted); font-size:12px; margin-top:2px;">Se eliminará directamente de los bloques de almacenamiento sin posibilidad de recuperación.</div>
            </div>
          </label>
        </div>
      </div>
      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-delete-file')">Cancelar</button>
        <button type="button" class="btn btn-danger" id="btn-confirm-delete" onclick="confirmDeleteFile()">Proceder</button>
      </div>
    </div>
  </div>

  <!-- Modal: Previsualizador de Archivos 100% Offline -->
  <div class="modal-backdrop" id="modal-file-preview">
    <div class="modal-dialog modal-dialog-preview" style="max-width:960px; width:95vw; max-height:90vh; display:flex; flex-direction:column;">
      <div class="modal-header" style="flex-shrink:0;">
        <div style="display:flex; align-items:center; gap:10px; overflow:hidden;">
          <svg class="icon" id="preview-header-icon" style="flex-shrink:0; width:22px; height:22px; color:var(--accent-primary);"><use href="#icon-file-text"></use></svg>
          <div style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">
            <h3 id="preview-file-name" style="margin:0; font-size:15px; font-weight:600; text-overflow:ellipsis; overflow:hidden; white-space:nowrap;">Nombre_archivo.txt</h3>
            <div id="preview-file-meta" style="font-size:11.5px; color:var(--text-muted); margin-top:2px;">0 B • 0 líneas</div>
          </div>
        </div>
        <div style="display:flex; align-items:center; gap:8px; flex-shrink:0;">
          <button type="button" class="btn btn-secondary btn-sm" id="btn-preview-copy" onclick="copyPreviewContent()" title="Copiar contenido al portapapeles" style="display:none;">
            <svg class="icon"><use href="#icon-copy"></use></svg> Copiar
          </button>
          <button type="button" class="btn btn-secondary btn-sm" id="btn-preview-download" onclick="downloadPreviewFile()" title="Descargar archivo">
            <svg class="icon"><use href="#icon-download"></use></svg> Descargar
          </button>
          <button class="modal-close" onclick="closeModal('modal-file-preview')">&times;</button>
        </div>
      </div>
      <div class="modal-body" id="preview-modal-body" style="padding:0; flex:1; overflow-y:auto; display:flex; flex-direction:column; min-height:350px;">
        <div id="preview-loading" style="display:flex; flex-direction:column; align-items:center; justify-content:center; padding:50px; color:var(--text-muted); gap:12px;">
          <svg class="icon spin" style="width:32px; height:32px; color:var(--accent-primary);"><use href="#icon-refresh"></use></svg>
          <span>Cargando previsualización...</span>
        </div>
        <div id="preview-code-container" class="preview-code-wrap" style="display:none;">
          <div class="preview-line-numbers" id="preview-line-numbers"></div>
          <pre class="preview-code-content" id="preview-code-content"></pre>
        </div>
        <div id="preview-image-container" class="preview-media-wrap" style="display:none;">
          <img id="preview-img-element" class="preview-image" src="" alt="Previsualización de imagen">
        </div>
        <div id="preview-pdf-container" style="display:none; flex:1; height:600px; width:100%;">
          <iframe id="preview-pdf-frame" class="preview-pdf-frame" src="" style="width:100%; height:100%; border:none;"></iframe>
        </div>
        <div id="preview-media-container" class="preview-media-wrap" style="display:none;">
          <video id="preview-video-element" controls style="max-width:100%; max-height:480px; border-radius:var(--radius-sm); display:none;"></video>
          <audio id="preview-audio-element" controls style="width:100%; max-width:460px; display:none;"></audio>
        </div>
        <div id="preview-binary-container" class="preview-media-wrap" style="display:none; padding:40px; text-align:center;">
          <svg class="icon" style="width:64px; height:64px; color:var(--text-muted); margin-bottom:12px;"><use href="#icon-archive"></use></svg>
          <h4 id="preview-binary-name" style="margin-bottom:6px; font-size:16px;">archivo.bin</h4>
          <p id="preview-binary-details" style="color:var(--text-muted); font-size:13px; margin-bottom:16px;">Formato binario</p>
          <button type="button" class="btn btn-primary" onclick="downloadPreviewFile()">
            <svg class="icon"><use href="#icon-download"></use></svg> Descargar archivo
          </button>
        </div>
      </div>
    </div>
  </div>

  <!-- Modal: Vaciar Papelera -->
  <div class="modal-backdrop" id="modal-empty-trash">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3><svg class="icon" style="color:var(--accent-danger);"><use href="#icon-trash"></use></svg> Vaciar papelera de reciclaje</h3>
        <button class="modal-close" onclick="closeModal('modal-empty-trash')">&times;</button>
      </div>
      <div class="modal-body">
        <p>¿Estás seguro de que deseas eliminar permanentemente <strong>todos los elementos</strong> de la papelera?</p>
        <p style="color:var(--accent-danger); font-size:12.5px; margin-top:8px;">Esta acción eliminará de forma irreversible todos los archivos y carpetas archivados en la papelera.</p>
      </div>
      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-empty-trash')">Cancelar</button>
        <button type="button" class="btn btn-danger" onclick="confirmEmptyTrash()">Vaciar papelera permanentemente</button>
      </div>
    </div>
  </div>

  <!-- Modal: Confirmar Reinicio -->
  <div class="modal-backdrop" id="modal-reboot-server">
    <div class="modal-dialog">
      <div class="modal-header">
        <h3>Reiniciar Servidor NAS</h3>
        <button class="modal-close" onclick="closeModal('modal-reboot-server')">&times;</button>
      </div>
      <div class="modal-body">
        <p>¿Estás seguro de que deseas reiniciar el servidor? Todas las sesiones de red y transferencias activas se interrumpirán momentáneamente.</p>
      </div>
      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-reboot-server')">Cancelar</button>
        <button type="button" class="btn btn-danger" onclick="confirmRebootServer()">Reiniciar ahora</button>
      </div>
    </div>
  </div>

  <!-- Contenedor de Alertas Toast -->
  <div id="toast-container" style="position:fixed; bottom:20px; right:20px; z-index:9999; display:flex; flex-direction:column; gap:10px;"></div>

  <script>
    window.SERVER_ACTIVE_VIEW = "<?= $activeView ?>";
  </script>
  <script src="/js/app.js?v=<?= file_exists(__DIR__ . '/../public/js/app.js') ? filemtime(__DIR__ . '/../public/js/app.js') : '2' ?>"></script>
</body>
</html>
