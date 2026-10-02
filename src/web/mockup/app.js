/**
 * ==============================================================================
 * NAS DEBIAN • LÓGICA DE INTERACTIVIDAD (VANILLA JAVASCRIPT)
 * Prototipo Web a la Medida para TEAM-JOFRATO (Debian 13)
 * 100% Offline • Cero dependencias externas • Rendimiento Instantáneo
 * ==============================================================================
 */

// Estado global de la aplicación
const AppState = {
  currentView: 'dashboard',
  currentTheme: localStorage.getItem('nas_theme') || 'dark',
  isScrubRunning: false,
  isTrimRunning: false,
  activeBackupTimers: [],

  // 1. Datos de Recursos Compartidos (Samba Shares)
  shares: [
    {
      id: 'SISTEMAS',
      name: 'SISTEMAS',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '1',
      schemeName: 'Lectura y Escritura por Grupo',
      groups: ['grp_sistemas'],
      writeList: '',
      readOnly: 'no',
      path: '/srv/nas/SISTEMAS',
      status: 'Activo',
      comment: 'Directorio departamental de Tecnología y Dirección'
    },
    {
      id: 'CAMPANA_UNO_OPERACIONES',
      name: 'CAMPANA_UNO_OPERACIONES',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '2',
      schemeName: 'Solo Lectura General + Escritura Exclusiva',
      groups: ['grp_empleados'],
      writeList: 'grp_sistemas',
      readOnly: 'no',
      path: '/srv/nas/CAMPANA_UNO_OPERACIONES',
      status: 'Activo',
      comment: 'Documentación operativa y plantillas de campaña 1'
    },
    {
      id: 'CAMPANA_DOS_FINANZAS',
      name: 'CAMPANA_DOS_FINANZAS',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '1',
      schemeName: 'Lectura y Escritura por Grupo',
      groups: ['grp_sistemas', 'grp_finanzas'],
      writeList: '',
      readOnly: 'no',
      path: '/srv/nas/CAMPANA_DOS_FINANZAS',
      status: 'Activo',
      comment: 'Libros contables e informes financieros en Excel (+100 puestos)'
    },
    {
      id: 'BACKUPS_WINDOWS$',
      name: 'BACKUPS_WINDOWS$',
      vis: 'Oculto ($)',
      browseable: 'no',
      scheme: '3',
      schemeName: 'Solo Lectura Estricta',
      groups: ['grp_sistemas'],
      writeList: '',
      readOnly: 'yes',
      path: '/srv/nas/BACKUPS_HISTORICOS/windows',
      status: 'Activo',
      comment: 'Repositorio de snapshots CIFS / Active Directory'
    },
    {
      id: 'BACKUPS_LINUX$',
      name: 'BACKUPS_LINUX$',
      vis: 'Oculto ($)',
      browseable: 'no',
      scheme: '3',
      schemeName: 'Solo Lectura Estricta',
      groups: ['grp_sistemas'],
      writeList: '',
      readOnly: 'yes',
      path: '/srv/nas/BACKUPS_HISTORICOS/linux',
      status: 'Activo',
      comment: 'Repositorio de snapshots SSH / Linux de servidores de producción'
    },
    {
      id: 'PUBLICO_DOCUMENTOS',
      name: 'PUBLICO_DOCUMENTOS',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '4',
      schemeName: 'Acceso Público / Invitados (Sin Contraseña)',
      groups: ['Todos (Invitados)'],
      writeList: '',
      readOnly: 'no',
      path: '/srv/nas/PUBLICO',
      status: 'Activo',
      comment: 'Plantillas y formatos públicos de la organización'
    }
  ],

  // 2. Datos de Usuarios del Sistema y Samba
  users: [
    {
      uid: 'admin_nas',
      fullName: 'Administrador Maestro TI',
      groups: ['sudo', 'adm', 'grp_sistemas'],
      sambaActive: true,
      shell: '/bin/bash',
      lastLogin: 'Hoy 10:14 (Consola Local)'
    },
    {
      uid: 'carlos_m',
      fullName: 'Carlos Morales - Cobranzas C1',
      groups: ['grp_c1_cobranzas', 'grp_empleados'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hoy 09:30 (SMB Windows)'
    },
    {
      uid: 'laura_s',
      fullName: 'Laura Salinas - Ventas C2',
      groups: ['grp_c2_ventas', 'grp_empleados'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Ayer 18:22 (SMB Windows)'
    },
    {
      uid: 'patricia_r',
      fullName: 'Patricia Ramos - Finanzas',
      groups: ['grp_finanzas', 'grp_empleados'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hoy 08:45 (SMB Windows)'
    },
    {
      uid: 'mario_v',
      fullName: 'Mario Valenzuela - Auditoría',
      groups: ['grp_auditoria'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hace 3 días'
    },
    {
      uid: 'backup_svc',
      fullName: 'Servicio de Réplica Automatizado',
      groups: ['grp_sistemas'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hoy 06:00 (Daemon)'
    }
  ],

  // 3. Grupos de Seguridad (grp_*)
  groups: [
    { name: 'grp_sistemas', level: 'Maestro (2770)', members: ['admin_nas', 'backup_svc'], shares: ['SISTEMAS', 'BACKUPS_WINDOWS$', 'BACKUPS_LINUX$', 'CAMPANA_DOS_FINANZAS'] },
    { name: 'grp_empleados', level: 'General Empleados', members: ['carlos_m', 'laura_s', 'patricia_r'], shares: ['CAMPANA_UNO_OPERACIONES'] },
    { name: 'grp_c1_cobranzas', level: 'Departamental C1', members: ['carlos_m'], shares: ['CAMPANA_UNO_OPERACIONES'] },
    { name: 'grp_c2_ventas', level: 'Departamental C2', members: ['laura_s'], shares: ['CAMPANA_DOS_FINANZAS'] },
    { name: 'grp_finanzas', level: 'Departamental Finanzas', members: ['patricia_r'], shares: ['CAMPANA_DOS_FINANZAS'] },
    { name: 'grp_auditoria', level: 'Solo Lectura Auditoría', members: ['mario_v'], shares: ['CAMPANA_UNO_OPERACIONES', 'SISTEMAS'] }
  ],

  // 4. Central de Respaldos (Tareas)
  backupTasks: [
    {
      id: 'bkp_win_facturacion',
      proto: 'CIFS / SMB 3.1.1',
      src: '//192.168.1.10/Facturas_EAD',
      user: 'TEAM-JOFRATO\\svc_backup',
      credFile: '/etc/backup-credentials/bkp_win_facturacion.cred',
      cron: '0 2 * * *',
      cronDesc: 'Todos los días a las 02:00 AM',
      retention: 15,
      snapsCount: 14,
      lastRun: 'Hoy 02:00:22',
      lastStatus: 'Éxito',
      sizeLogical: '18.4 GB',
      sizeReal: '2.1 GB',
      dedupRate: '88.6%'
    },
    {
      id: 'bkp_lin_servidor_web',
      proto: 'SSH (rsync -aAXH)',
      src: 'root@192.168.1.20:/var/www/html',
      user: 'root (known_hosts_backup)',
      credFile: '/root/.ssh/known_hosts_backup',
      cron: '30 3 * * *',
      cronDesc: 'Todos los días a las 03:30 AM',
      retention: 10,
      snapsCount: 10,
      lastRun: 'Hoy 03:30:15',
      lastStatus: 'Éxito',
      sizeLogical: '4.2 GB',
      sizeReal: '380 MB',
      dedupRate: '91.0%'
    },
    {
      id: 'bkp_win_contabilidad',
      proto: 'CIFS / SMB 3.1.1',
      src: '//192.168.1.15/Contabilidad$',
      user: 'CONTAB\\admin_finanzas',
      credFile: '/etc/backup-credentials/bkp_win_contabilidad.cred',
      cron: '0 */6 * * *',
      cronDesc: 'Cada 6 horas (00:00, 06:00, 12:00, 18:00)',
      retention: 20,
      snapsCount: 14,
      lastRun: 'Hoy 06:00:10',
      lastStatus: 'Éxito',
      sizeLogical: '32.1 GB',
      sizeReal: '4.8 GB',
      dedupRate: '85.1%'
    }
  ],

  // 5. Historial simulado de Snapshots por tarea
  snapshotsData: {
    'bkp_win_facturacion': [
      { name: 'snapshot_2026-10-02_020000', date: '2026-10-02 02:00:22', apparent: '18.4 GB', real: '142 MB', dedup: '99.2%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-10-01_020000', date: '2026-10-01 02:00:19', apparent: '18.3 GB', real: '210 MB', dedup: '98.8%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-09-30_020000', date: '2026-09-30 02:00:24', apparent: '18.2 GB', real: '185 MB', dedup: '99.0%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-09-29_020000', date: '2026-09-29 02:00:15', apparent: '18.1 GB', real: '310 MB', dedup: '98.3%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-09-28_020000', date: '2026-09-28 02:00:18', apparent: '18.0 GB', real: '1.2 GB', dedup: '93.3%', status: 'Snapshot Base' }
    ],
    'bkp_lin_servidor_web': [
      { name: 'snapshot_2026-10-02_033000', date: '2026-10-02 03:30:15', apparent: '4.2 GB', real: '32 MB', dedup: '99.2%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-10-01_033000', date: '2026-10-01 03:30:12', apparent: '4.1 GB', real: '45 MB', dedup: '98.9%', status: 'Atómico / Íntegro' }
    ],
    'bkp_win_contabilidad': [
      { name: 'snapshot_2026-10-02_060000', date: '2026-10-02 06:00:10', apparent: '32.1 GB', real: '84 MB', dedup: '99.7%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-10-02_000000', date: '2026-10-02 00:00:08', apparent: '32.0 GB', real: '115 MB', dedup: '99.6%', status: 'Atómico / Íntegro' }
    ]
  },

  // 6. Logs de Sistema y Samba
  systemLogs: [
    { time: '11:15:02', level: 'INFO', src: 'smbd', text: 'smbd[1420]: Conexión exitosa desde 10.10.1.45 (Windows 11) recurso [CAMPANA_DOS_FINANZAS] usuario [patricia_r]' },
    { time: '11:14:58', level: 'INFO', src: 'wsdd2', text: 'wsdd2[812]: Sonda LLMNR/WSD respondida para host SRV-NAS hacia cliente 10.10.1.45' },
    { time: '11:00:00', level: 'OK',   src: 'kernel', text: 'sysctl: vm.dirty_bytes=268435456 (256MB) y vfs_cache_pressure=30 activos sin saturación de I/O' },
    { time: '06:00:14', level: 'OK',   src: 'backup', text: 'backup_runner: Tarea [bkp_win_contabilidad] finalizada exitosamente en 14.8s. Snapshots: 14/20' },
    { time: '03:30:18', level: 'OK',   src: 'backup', text: 'backup_runner: Tarea [bkp_lin_servidor_web] rsync con StrictHostKeyChecking=accept-new exitoso' },
    { time: '02:00:25', level: 'OK',   src: 'backup', text: 'backup_runner: Tarea [bkp_win_facturacion] deduplicación 88.6% (0 bytes adicionales en inodos compartidos)' },
    { time: '01:00:00', level: 'INFO', src: 'cron',   text: 'cron[620]: Verificación preventiva de espacio en disco en /srv/nas: 35% de ocupación (umbral seguro <85%)' }
  ],

  // 7. Feed de eventos recientes para el dashboard
  activityFeed: [
    { badge: 'Éxito', badgeClass: 'badge-ok', title: 'Snapshot completado: bkp_win_facturacion', desc: 'Deduplicación 88.6% • 0 bytes adicionales en inodos compartidos' },
    { badge: 'Samba', badgeClass: 'badge-blue', title: 'Sesión SMB iniciada por carlos_m desde 10.10.1.34', desc: 'Acceso concedido a recurso [CAMPANA_UNO_OPERACIONES]' },
    { badge: 'WSDD2', badgeClass: 'badge-purple', title: 'Descubrimiento de red WSD respondido para host SRV-NAS', desc: 'Visible en explorador de Windows 10/11 sin SMBv1 ni NetBIOS obsoleto' },
    { badge: 'Scrub', badgeClass: 'badge-ok', title: 'Auditoría mensual BTRFS: 1,420,892 bloques validados', desc: '0 errores de corrupción silenciosa detectados en /srv/nas' }
  ]
};

// ==============================================================================
// Inicialización y Manejo del DOM
// ==============================================================================
document.addEventListener('DOMContentLoaded', () => {
  initTheme();
  setupNavigation();
  renderSharesTable();
  renderUsersTable();
  renderGroupsTable();
  renderGroupCheckboxes();
  renderBackupTasksTable();
  renderSystemLogs();
  renderDashboardActivityFeed();
  updateAllCounters();
  setupModals();
  setupForms();
  updateNewSharePreview();
});

// ==============================================================================
// Actualización Dinámica de Contadores y Badges
// ==============================================================================
function updateAllCounters() {
  // 1. Badges del menú lateral (Sidebar)
  const badgeShares = document.getElementById('badge-shares');
  if (badgeShares) badgeShares.innerText = AppState.shares.length;

  const badgeUsers = document.getElementById('badge-users');
  if (badgeUsers) badgeUsers.innerText = AppState.users.length;

  const badgeBackups = document.getElementById('badge-backups');
  if (badgeBackups) badgeBackups.innerText = AppState.backupTasks.length;

  // 2. Subtabs de usuarios y grupos
  const subtabUsers = document.getElementById('subtab-users-label');
  if (subtabUsers) subtabUsers.innerText = `Usuarios del Sistema y Samba (${AppState.users.length})`;

  const subtabGroups = document.getElementById('subtab-groups-label');
  if (subtabGroups) subtabGroups.innerText = `Grupos de Seguridad grp_* (${AppState.groups.length})`;

  // 3. Chips de filtro de recursos Samba
  const totalShares = AppState.shares.length;
  const visibleShares = AppState.shares.filter(s => s.vis === 'Visible').length;
  const hiddenShares = AppState.shares.filter(s => s.vis !== 'Visible' || s.name.endsWith('$')).length;
  const activeShares = AppState.shares.filter(s => s.status === 'Activo').length;
  const disabledShares = totalShares - activeShares;

  document.querySelectorAll('#shares-chips .chip-btn').forEach(btn => {
    const filter = btn.getAttribute('data-filter');
    if (filter === 'all') btn.innerText = `Todos (${totalShares})`;
    else if (filter === 'visible') btn.innerText = `Visibles (${visibleShares})`;
    else if (filter === 'hidden') btn.innerText = `Ocultos $ (${hiddenShares})`;
    else if (filter === 'active') btn.innerText = `Activos (${activeShares})`;
    else if (filter === 'disabled') btn.innerText = `Deshabilitados (${disabledShares})`;
  });
}

// ==============================================================================
// Tema Oscuro / Claro
// ==============================================================================
function initTheme() {
  document.documentElement.setAttribute('data-theme', AppState.currentTheme);
  updateThemeIcon();
}

function toggleTheme() {
  AppState.currentTheme = AppState.currentTheme === 'dark' ? 'light' : 'dark';
  document.documentElement.setAttribute('data-theme', AppState.currentTheme);
  localStorage.setItem('nas_theme', AppState.currentTheme);
  updateThemeIcon();
  showToast(`Modo ${AppState.currentTheme === 'dark' ? 'Oscuro' : 'Claro'} activado`, 'info');
}

function updateThemeIcon() {
  const iconSpan = document.getElementById('theme-toggle-icon');
  const textSpan = document.getElementById('theme-toggle-text');
  if (AppState.currentTheme === 'dark') {
    if (iconSpan) iconSpan.innerHTML = '<use href="#icon-sun"></use>';
    if (textSpan) textSpan.innerText = 'Modo Claro';
  } else {
    if (iconSpan) iconSpan.innerHTML = '<use href="#icon-moon"></use>';
    if (textSpan) textSpan.innerText = 'Modo Oscuro';
  }
}

// ==============================================================================
// Navegación entre Pestañas / Vistas
// ==============================================================================
function setupNavigation() {
  const navItems = document.querySelectorAll('.nav-item');
  navItems.forEach(item => {
    item.addEventListener('click', () => {
      const viewId = item.getAttribute('data-view');
      switchView(viewId);
      document.querySelector('.sidebar').classList.remove('open');
    });
  });

  const mobileToggle = document.getElementById('mobile-menu-btn');
  if (mobileToggle) {
    mobileToggle.addEventListener('click', () => {
      document.querySelector('.sidebar').classList.toggle('open');
    });
  }
}

function switchView(viewId) {
  AppState.currentView = viewId;

  document.querySelectorAll('.nav-item').forEach(item => {
    if (item.getAttribute('data-view') === viewId) {
      item.classList.add('active');
    } else {
      item.classList.remove('active');
    }
  });

  document.querySelectorAll('.view-section').forEach(sec => {
    sec.classList.remove('active');
  });
  const targetView = document.getElementById(`view-${viewId}`);
  if (targetView) targetView.classList.add('active');

  const breadcrumb = document.getElementById('current-view-title');
  const viewNames = {
    'dashboard': 'Dashboard General',
    'shares': 'Recursos Compartidos (Samba)',
    'users': 'Usuarios y Grupos de Red',
    'backups': 'Central de Respaldos',
    'diagnostics': 'Mantenimiento y Diagnóstico'
  };
  if (breadcrumb) breadcrumb.innerText = viewNames[viewId] || 'Inicio';
}

// ==============================================================================
// Notificaciones Toast Flotantes
// ==============================================================================
function showToast(message, type = 'info') {
  const container = document.getElementById('toast-container');
  if (!container) return;

  const toast = document.createElement('div');
  toast.className = `toast toast-${type}`;

  const iconName = type === 'success' ? 'check-circle' :
                   type === 'danger'  ? 'x-circle' :
                   type === 'warning' ? 'alert-triangle' : 'info';

  toast.innerHTML = `
    <svg class="icon"><use href="#icon-${iconName}"></use></svg>
    <span>${message}</span>
  `;

  container.appendChild(toast);
  setTimeout(() => {
    if (toast.parentNode) toast.parentNode.removeChild(toast);
  }, 4000);
}

// ==============================================================================
// Pestaña 1: Dashboard y Feed de Actividad
// ==============================================================================
function renderDashboardActivityFeed() {
  const container = document.getElementById('dashboard-activity-feed');
  if (!container) return;

  container.innerHTML = AppState.activityFeed.slice(0, 5).map((item, idx) => {
    const isLast = idx === AppState.activityFeed.length - 1;
    const borderStyle = isLast ? '' : 'border-bottom:1px solid var(--border-light); padding-bottom:8px;';
    return `
      <div style="display:flex; align-items:flex-start; gap:10px; font-size:12.5px; ${borderStyle}">
        <span class="badge ${item.badgeClass}" style="font-size:10px;">${item.badge}</span>
        <div>
          <div>${item.title}</div>
          <div style="color:var(--text-muted); font-size:11px;">${item.desc}</div>
        </div>
      </div>
    `;
  }).join('');
}

function refreshDashboardMetrics() {
  const btn = document.getElementById('btn-refresh-metrics');
  const icon = document.getElementById('icon-refresh-metrics');
  if (icon) icon.classList.add('spin');
  if (btn) btn.disabled = true;

  setTimeout(() => {
    if (icon) icon.classList.remove('spin');
    if (btn) btn.disabled = false;
    updateAllCounters();
    showToast('Métricas del sistema Debian 13 actualizadas en tiempo real', 'success');
  }, 750);
}

// ==============================================================================
// Pestaña 2: Recursos Compartidos (Samba Shares)
// ==============================================================================
let currentShareFilter = 'all';

function renderSharesTable(filterText = '') {
  const tbody = document.getElementById('shares-table-body');
  if (!tbody) return;

  const filtered = AppState.shares.filter(s => {
    if (currentShareFilter === 'visible' && s.vis !== 'Visible') return false;
    if (currentShareFilter === 'hidden' && s.vis !== 'Oculto ($)' && !s.name.endsWith('$')) return false;
    if (currentShareFilter === 'active' && s.status !== 'Activo') return false;
    if (currentShareFilter === 'disabled' && s.status === 'Activo') return false;

    if (filterText) {
      const q = filterText.toLowerCase();
      return s.name.toLowerCase().includes(q) ||
             s.path.toLowerCase().includes(q) ||
             s.comment.toLowerCase().includes(q) ||
             s.groups.some(g => g.toLowerCase().includes(q));
    }
    return true;
  });

  if (filtered.length === 0) {
    tbody.innerHTML = `<tr><td colspan="7" style="text-align: center; color: var(--text-muted); padding: 30px;">No se encontraron recursos que coincidan con la búsqueda.</td></tr>`;
    return;
  }

  tbody.innerHTML = filtered.map(s => {
    const isHidden = s.name.endsWith('$') || s.vis === 'Oculto ($)';
    const isChecked = s.status === 'Activo' ? 'checked' : '';
    const badgeVis = isHidden 
      ? `<span class="badge badge-purple"><svg class="icon icon-sm"><use href="#icon-eye-off"></use></svg> Oculto ($)</span>`
      : `<span class="badge badge-ok"><svg class="icon icon-sm"><use href="#icon-eye"></use></svg> Visible</span>`;

    const groupsHtml = s.groups.map(g => `<span class="tag-pill">${g}</span>`).join(' ');
    const writeListHtml = s.writeList ? `<br><small style="color:var(--accent-warning);">Escritura: <b>${s.writeList}</b></small>` : '';

    return `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <svg class="icon" style="color:var(--accent-primary);"><use href="#icon-folder"></use></svg>
            <div>
              <strong>${s.name}</strong>
              <div style="font-size:11px; color:var(--text-muted);">\\\\10.10.1.2\\${s.name}</div>
            </div>
          </div>
        </td>
        <td>${badgeVis}</td>
        <td>
          <div style="font-size:12.5px; font-weight:600;">${s.schemeName}</div>
          <div style="font-size:11px; color:var(--text-secondary);">${s.comment}</div>
        </td>
        <td>
          ${groupsHtml}
          ${writeListHtml}
        </td>
        <td><code style="color:var(--accent-cyan);">${s.path}</code></td>
        <td>
          <label class="toggle-switch">
            <input type="checkbox" ${isChecked} onchange="toggleShareStatus('${s.id}')">
            <span class="toggle-slider"></span>
          </label>
        </td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="viewShareConfig('${s.id}')" title="Ver Configuración smb.conf">
              <svg class="icon"><use href="#icon-file"></use></svg>
            </button>
            <button class="btn btn-danger btn-sm" onclick="deleteShare('${s.id}')" title="Eliminar Recurso">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function filterShares(type, btnElement) {
  currentShareFilter = type;
  document.querySelectorAll('#shares-chips .chip-btn').forEach(btn => btn.classList.remove('active'));
  if (btnElement) btnElement.classList.add('active');
  renderSharesTable(document.getElementById('shares-search-input')?.value || '');
}

function toggleShareStatus(shareId) {
  const share = AppState.shares.find(s => s.id === shareId);
  if (!share) return;
  share.status = share.status === 'Activo' ? 'Deshabilitado' : 'Activo';
  showToast(`Recurso [${share.name}] ${share.status.toLowerCase()} en Samba`, share.status === 'Activo' ? 'success' : 'warning');
  updateAllCounters();
  renderSharesTable(document.getElementById('shares-search-input')?.value || '');
}

function deleteShare(shareId) {
  if (confirm(`¿Estás seguro de que deseas eliminar el recurso compartido [${shareId}]?`)) {
    AppState.shares = AppState.shares.filter(s => s.id !== shareId);
    // Eliminar también referencia en grupos
    AppState.groups.forEach(g => {
      g.shares = g.shares.filter(s => s !== shareId);
    });
    showToast(`Recurso [${shareId}] eliminado de smb.conf`, 'danger');
    updateAllCounters();
    renderSharesTable(document.getElementById('shares-search-input')?.value || '');
    renderGroupsTable();
  }
}

function viewShareConfig(shareId) {
  const share = AppState.shares.find(s => s.id === shareId);
  if (!share) return;

  const smbSnippet = generateSmbConfSnippet(share);
  document.getElementById('modal-share-preview-code').innerText = smbSnippet;
  openModal('modal-share-preview');
}

// ==============================================================================
// Generador y Validaciones de smb.conf en Modal
// ==============================================================================
function updateNewSharePreview() {
  const nameInput = document.getElementById('new-share-name');
  if (!nameInput) return;

  const isHiddenRadio = document.getElementById('new-share-vis-hidden');
  const isHidden = isHiddenRadio ? isHiddenRadio.checked : false;
  const pathInput = document.getElementById('new-share-path');
  const commentInput = document.getElementById('new-share-comment');
  const schemeVal = document.querySelector('input[name="new-share-scheme"]:checked')?.value || '1';

  let rawName = nameInput.value.trim().replace(/\s+/g, '_').replace(/[^A-Za-z0-9_$-]/g, '');
  if (isHidden) {
    if (!rawName.endsWith('$')) rawName += '$';
  } else {
    rawName = rawName.replace(/\$$/, '');
  }

  const cleanDir = rawName.replace(/\$$/, '');
  if (pathInput && !pathInput.dataset.userEdited && cleanDir) {
    pathInput.value = `/srv/nas/${cleanDir}`;
  }

  const selectedGroups = Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value);

  const previewObj = {
    name: rawName || 'RECURSO_NUEVO',
    path: (pathInput && pathInput.value) || '/srv/nas/RECURSO',
    comment: (commentInput && commentInput.value) || 'Carpeta compartida',
    browseable: isHidden ? 'no' : 'yes',
    scheme: schemeVal,
    groups: selectedGroups.length ? selectedGroups : ['grp_sistemas'],
    writeList: document.getElementById('new-share-writelist-select')?.value || 'grp_sistemas'
  };

  const code = generateSmbConfSnippet(previewObj);
  const previewBox = document.getElementById('new-share-smb-preview');
  if (previewBox) previewBox.innerText = code;
}

function generateSmbConfSnippet(s) {
  let validUsersStr = '';
  let writeListStr = '';
  let readOnlyStr = 'no';
  let guestOkStr = 'no';
  let maskStr = '0770';

  if (s.scheme === '1') {
    readOnlyStr = 'no';
    validUsersStr = s.groups && s.groups.length ? s.groups.map(g => `@${g}`).join(' ') : '@grp_sistemas';
  } else if (s.scheme === '2') {
    readOnlyStr = 'no';
    validUsersStr = s.groups && s.groups.length ? s.groups.map(g => `@${g}`).join(' ') : '@grp_sistemas';
    writeListStr = `@${s.writeList || 'grp_sistemas'}`;
  } else if (s.scheme === '3') {
    readOnlyStr = 'yes';
    validUsersStr = s.groups && s.groups.length ? s.groups.map(g => `@${g}`).join(' ') : '@grp_sistemas';
  } else if (s.scheme === '4') {
    readOnlyStr = 'no';
    guestOkStr = 'yes';
    maskStr = '0777';
  }

  const lines = [
    `[${s.name}]`,
    `   comment = ${s.comment || 'Carpeta de Red'}`,
    `   path = ${s.path}`,
    `   browseable = ${s.browseable}`,
    `   available = yes`,
    `   read only = ${readOnlyStr}`,
    `   guest ok = ${guestOkStr}`
  ];

  if (validUsersStr) {
    lines.push(`   valid users = ${validUsersStr}`);
  }
  if (writeListStr) {
    lines.push(`   write list = ${writeListStr}`);
  }

  lines.push(
    `   create mask = ${maskStr}`,
    `   directory mask = ${maskStr}`,
    `   force create mode = ${maskStr}`,
    `   force directory mode = ${maskStr}`,
    `   vfs objects = acl_xattr streams_xattr`,
    `   store dos attributes = yes`,
    `   inherit permissions = yes`
  );

  return lines.join('\n');
}

// ==============================================================================
// Inyección Dinámica de Checkboxes de Grupos en Modales
// ==============================================================================
function renderGroupCheckboxes() {
  // 1. Grupos en modal de nuevo recurso
  const shareGroupsList = document.getElementById('new-share-groups-list');
  if (shareGroupsList) {
    shareGroupsList.innerHTML = AppState.groups.map(g => `
      <label style="display:flex; align-items:center; gap:6px; font-size:12.5px; cursor:pointer;">
        <input type="checkbox" class="new-share-grp-chk" value="${g.name}" ${g.name === 'grp_sistemas' ? 'checked' : ''}>
        ${g.name} ${g.name === 'grp_sistemas' ? '(Admin)' : ''}
      </label>
    `).join('');

    shareGroupsList.querySelectorAll('.new-share-grp-chk').forEach(c => {
      c.addEventListener('change', () => {
        updateWriteListOptions();
        updateNewSharePreview();
      });
    });
  }

  // 2. Select de write list en Scheme 2
  updateWriteListOptions();

  // 3. Grupos en modal de nuevo usuario
  const userGroupsList = document.getElementById('new-user-groups-list');
  if (userGroupsList) {
    userGroupsList.innerHTML = AppState.groups.map(g => `
      <label style="display:flex; align-items:center; gap:6px; font-size:12.5px; cursor:pointer;">
        <input type="checkbox" class="new-user-grp-chk" value="${g.name}" ${g.name === 'grp_empleados' ? 'checked' : ''}>
        ${g.name}
      </label>
    `).join('');
  }
}

function updateWriteListOptions() {
  const writeSelect = document.getElementById('new-share-writelist-select');
  if (!writeSelect) return;

  const checkedGroups = Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value);
  const groupsToUse = checkedGroups.length ? checkedGroups : AppState.groups.map(g => g.name);

  const prevVal = writeSelect.value;
  writeSelect.innerHTML = groupsToUse.map(g => `<option value="${g}">${g}</option>`).join('');
  if (groupsToUse.includes(prevVal)) {
    writeSelect.value = prevVal;
  }
}

// ==============================================================================
// Pestaña 3: Usuarios y Grupos
// ==============================================================================
function renderUsersTable(filterText = '') {
  const tbody = document.getElementById('users-table-body');
  if (!tbody) return;

  const filtered = AppState.users.filter(u => {
    if (!filterText) return true;
    const q = filterText.toLowerCase();
    return u.uid.toLowerCase().includes(q) ||
           u.fullName.toLowerCase().includes(q) ||
           u.groups.some(g => g.toLowerCase().includes(q));
  });

  tbody.innerHTML = filtered.map(u => {
    const isSambaOk = u.sambaActive 
      ? `<span class="badge badge-ok"><svg class="icon icon-sm"><use href="#icon-check-circle"></use></svg> Sincronizado</span>`
      : `<span class="badge badge-err"><svg class="icon icon-sm"><use href="#icon-x-circle"></use></svg> Bloqueado</span>`;

    const groupsHtml = u.groups.map(g => `<span class="tag-pill">${g}</span>`).join(' ');

    return `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:10px;">
            <div style="width:32px; height:32px; border-radius:50%; background:var(--accent-primary-light); color:var(--accent-primary); display:flex; align-items:center; justify-content:center; font-weight:700;">
              ${u.uid.charAt(0).toUpperCase()}
            </div>
            <div>
              <strong>${u.uid}</strong>
              <div style="font-size:11px; color:var(--text-muted);">${u.lastLogin}</div>
            </div>
          </div>
        </td>
        <td>${u.fullName}</td>
        <td>${groupsHtml}</td>
        <td>${isSambaOk}</td>
        <td><code style="font-size:11.5px; color:var(--text-secondary);">${u.shell}</code></td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="openChangePasswordModal('${u.uid}')" title="Cambiar Contraseña">
              <svg class="icon"><use href="#icon-key"></use></svg>
            </button>
            <button class="btn btn-secondary btn-sm" onclick="toggleUserSamba('${u.uid}')" title="${u.sambaActive ? 'Bloquear en Samba' : 'Desbloquear en Samba'}">
              <svg class="icon"><use href="#icon-${u.sambaActive ? 'lock' : 'unlock'}"></use></svg>
            </button>
            <button class="btn btn-danger btn-sm" onclick="deleteUser('${u.uid}')" title="Eliminar Usuario">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function toggleUserSamba(uid) {
  const user = AppState.users.find(u => u.uid === uid);
  if (!user) return;
  user.sambaActive = !user.sambaActive;
  showToast(`Usuario [${uid}] ${user.sambaActive ? 'habilitado' : 'bloqueado'} en Samba (smbpasswd)`, user.sambaActive ? 'success' : 'warning');
  renderUsersTable(document.getElementById('users-search-input')?.value || '');
}

function deleteUser(uid) {
  if (uid === 'admin_nas') {
    showToast('No se puede eliminar el usuario administrador principal del sistema', 'danger');
    return;
  }
  if (confirm(`¿Eliminar al usuario ${uid} del sistema Debian y de la base de datos Samba?`)) {
    AppState.users = AppState.users.filter(u => u.uid !== uid);
    // Eliminarlo de los miembros de todos los grupos
    AppState.groups.forEach(g => {
      g.members = g.members.filter(m => m !== uid);
    });
    showToast(`Usuario [${uid}] eliminado satisfactoriamente`, 'danger');
    updateAllCounters();
    renderUsersTable();
    renderGroupsTable();
  }
}

function renderGroupsTable() {
  const tbody = document.getElementById('groups-table-body');
  if (!tbody) return;

  tbody.innerHTML = AppState.groups.map(g => {
    const membersHtml = g.members.length 
      ? g.members.map(m => `<span class="tag-pill" style="color:var(--text-main);">${m}</span>`).join(' ')
      : '<span style="color:var(--text-muted); font-size:12px;">(Sin miembros)</span>';

    const sharesHtml = g.shares.length
      ? g.shares.map(s => `<span class="badge badge-gray" style="font-size:10.5px;">${s}</span>`).join(' ')
      : '<span style="color:var(--text-muted); font-size:12px;">(Ninguno)</span>';

    return `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <svg class="icon" style="color:var(--accent-purple);"><use href="#icon-users"></use></svg>
            <strong style="color:var(--accent-purple);">${g.name}</strong>
          </div>
        </td>
        <td><span class="badge badge-purple">${g.level}</span></td>
        <td>${membersHtml}</td>
        <td>${sharesHtml}</td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="openEditGroupModal('${g.name}')">
              Gestionar
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function openEditGroupModal(groupName) {
  const group = AppState.groups.find(g => g.name === groupName);
  if (!group) return;

  const nameInput = document.getElementById('edit-group-name');
  const nameDisplay = document.getElementById('edit-group-name-display');
  const membersList = document.getElementById('edit-group-members-list');
  const btnDelete = document.getElementById('btn-delete-group');

  if (nameInput) nameInput.value = groupName;
  if (nameDisplay) nameDisplay.innerText = groupName;

  if (btnDelete) {
    btnDelete.style.display = groupName === 'grp_sistemas' ? 'none' : 'inline-flex';
  }

  if (membersList) {
    membersList.innerHTML = AppState.users.map(u => {
      const isMember = group.members.includes(u.uid);
      return `
        <div class="member-check-item">
          <label class="member-check-label">
            <input type="checkbox" class="edit-group-user-chk" value="${u.uid}" ${isMember ? 'checked' : ''}>
            <span><strong>${u.uid}</strong> <small style="color:var(--text-muted); font-weight:normal;">(${u.fullName})</small></span>
          </label>
        </div>
      `;
    }).join('');
  }

  openModal('modal-edit-group');
}

function deleteCurrentGroup() {
  const groupName = document.getElementById('edit-group-name')?.value;
  if (!groupName || groupName === 'grp_sistemas') {
    showToast('No se puede eliminar el grupo maestro grp_sistemas', 'danger');
    return;
  }

  if (confirm(`¿Estás seguro de eliminar el grupo de seguridad [${groupName}]?`)) {
    AppState.groups = AppState.groups.filter(g => g.name !== groupName);
    // Retirar del perfil de cada usuario
    AppState.users.forEach(u => {
      u.groups = u.groups.filter(g => g !== groupName);
    });
    // Retirar de shares
    AppState.shares.forEach(s => {
      s.groups = s.groups.filter(g => g !== groupName);
    });

    closeModal('modal-edit-group');
    showToast(`Grupo [${groupName}] eliminado`, 'danger');
    updateAllCounters();
    renderGroupsTable();
    renderUsersTable();
    renderGroupCheckboxes();
  }
}

function switchUserTab(tabName) {
  document.getElementById('subtab-users-btn').classList.toggle('active', tabName === 'users');
  document.getElementById('subtab-groups-btn').classList.toggle('active', tabName === 'groups');
  document.getElementById('panel-subtab-users').style.display = tabName === 'users' ? 'block' : 'none';
  document.getElementById('panel-subtab-groups').style.display = tabName === 'groups' ? 'block' : 'none';
}

function generateSecurePassword() {
  const chars = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789!@#$%&*';
  let pass = '';
  for (let i = 0; i < 14; i++) {
    pass += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  const passInput = document.getElementById('new-user-pass');
  if (passInput) {
    passInput.value = pass;
    passInput.type = 'text';
  }
  showToast('Contraseña robusta generada automáticamente', 'info');
}

// ==============================================================================
// Pestaña 4: Central de Respaldos Multiplataforma
// ==============================================================================
function renderBackupTasksTable() {
  const tbody = document.getElementById('backup-tasks-table-body');
  if (!tbody) return;

  tbody.innerHTML = AppState.backupTasks.map(t => {
    const isWindows = t.proto.includes('CIFS');
    const badgeProto = isWindows
      ? `<span class="badge badge-blue"><svg class="icon icon-sm"><use href="#icon-server"></use></svg> CIFS / Windows</span>`
      : `<span class="badge badge-purple"><svg class="icon icon-sm"><use href="#icon-terminal"></use></svg> SSH / Linux</span>`;

    return `
      <tr>
        <td>
          <strong>${t.id}</strong>
          <div style="font-size:11px; color:var(--text-muted);">${t.user}</div>
        </td>
        <td>${badgeProto}</td>
        <td><code style="color:var(--accent-primary);">${t.src}</code></td>
        <td>
          <code>${t.cron}</code>
          <div style="font-size:11px; color:var(--text-secondary);">${t.cronDesc}</div>
        </td>
        <td>${t.retention} snaps</td>
        <td><strong style="color:var(--accent-success);">${t.snapsCount}</strong> en disco</td>
        <td>
          <div>${t.lastRun}</div>
          <span class="badge badge-ok" style="font-size:10.5px;">✔ ${t.lastStatus}</span>
        </td>
        <td>
          <div class="table-actions">
            <button class="btn btn-primary btn-sm" onclick="runBackupTask('${t.id}')" title="Ejecutar Respaldo Inmediatamente">
              <svg class="icon"><use href="#icon-play"></use></svg> Ejecutar
            </button>
            <button class="btn btn-secondary btn-sm" onclick="openSnapshotsModal('${t.id}')" title="Ver Snapshots Deduplicados">
              <svg class="icon"><use href="#icon-clock"></use></svg> Historial
            </button>
            <button class="btn btn-danger btn-sm" onclick="deleteBackupTask('${t.id}')" title="Eliminar Tarea">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function deleteBackupTask(taskId) {
  if (confirm(`¿Estás seguro de eliminar la tarea de respaldo ${taskId} y su cronograma asociado?`)) {
    AppState.backupTasks = AppState.backupTasks.filter(t => t.id !== taskId);
    delete AppState.snapshotsData[taskId];
    showToast(`Tarea de backup [${taskId}] eliminada`, 'danger');
    updateAllCounters();
    renderBackupTasksTable();
  }
}

// Ejecución Simulada en Vivo de la Tarea de Respaldo (Multiplataforma)
function runBackupTask(taskId) {
  const task = AppState.backupTasks.find(t => t.id === taskId);
  if (!task) return;

  // Limpiar timers previos si se canceló o reabrió
  AppState.activeBackupTimers.forEach(id => clearTimeout(id));
  AppState.activeBackupTimers = [];

  const terminal = document.getElementById('backup-runner-terminal');
  terminal.innerHTML = '';
  document.getElementById('runner-task-title').innerText = `${taskId} (${task.proto})`;
  openModal('modal-backup-runner');

  const isWindows = task.proto.includes('CIFS');
  const nowStr = new Date().toISOString().replace(/T/, '_').replace(/:/g, '').slice(0, 15);
  const stagingDir = `/srv/nas/BACKUPS_HISTORICOS/${taskId}/.inprogress_${nowStr}`;

  // Líneas de ejecución técnica adaptadas según el rol y protocolo (CIFS Windows vs SSH Linux)
  const lines = [
    { t: 0,    level: 'term-info', text: `[1/8] Adquiriendo candado de exclusión mutua /var/lock/backup_${taskId}.lock... (flock OK)` },
    { t: 500,  level: 'term-info', text: `[2/8] Evaluando capacidad en /srv/nas mediante df -Pk...` },
    { t: 1000, level: 'term-ok',   text: `✔ Ocupación actual: 35%. 2.6 TB libres (>2 GB umbral crítico de aborto). Procediendo.` },
    { t: 1600, level: 'term-info', text: `[3/8] Creando staging atómico temporal: ${stagingDir}` },
    { t: 2400, level: 'term-info', text: isWindows
        ? `[4/8] Conectando a origen Windows CIFS [${task.src}] con credenciales AD (0600 root:root)...`
        : `[4/8] Negociando túnel SSH seguro con host Linux origen y StrictHostKeyChecking=accept-new...` },
    { t: 3200, level: 'term-ok',   text: isWindows
        ? `✔ Montaje temporal CIFS exitoso (ro,vers=3.1.1,noserverino,cache=none,soft,timeo=30) en /mnt/backup_sources/${taskId}`
        : `✔ Llave de host registrada en /root/.ssh/known_hosts_backup. Túnel SSH autenticado sin intermediarios.` },
    { t: 4000, level: 'term-cmd',  text: isWindows
        ? `[5/8] Ejecutando rsync -aAXH --numeric-ids --link-dest=../snapshot_reciente /mnt/backup_sources/${taskId}/ ${stagingDir}/`
        : `[5/8] Ejecutando rsync -aAXH --numeric-ids -v -z --timeout=60 --link-dest=../snapshot_reciente ${task.src}/ ${stagingDir}/` },
    { t: 5200, level: 'term-info', text: `     Analizando árbol de archivos... 41,890 archivos idénticos enlazados vía Hardlinks (0 bytes extra).` },
    { t: 6200, level: 'term-info', text: `     Transfiriendo archivos modificados (45.2 MB) a tasa sostenida...` },
    { t: 7200, level: 'term-ok',   text: `✔ Sincronización rsync completada. Código de retorno: 0 (Sin errores de I/O)` },
    { t: 8000, level: 'term-info', text: `[6/8] Promoción atómica de copia íntegra: mv ${stagingDir} snapshot_${nowStr}` },
    { t: 8600, level: 'term-info', text: `[7/8] Evaluando política de retención (${task.retention} snapshots máximos)... Total: ${task.snapsCount + 1}. Dentro de límite.` },
    { t: 9200, level: 'term-info', text: isWindows
        ? `[8/8] Desmontando recurso CIFS en /mnt/backup_sources/${taskId} y liberando descriptor flock...`
        : `[8/8] Cerrando socket SSH y liberando descriptor de candado flock...` },
    { t: 9800, level: 'term-ok',   text: `======================================================================` },
    { t: 9900, level: 'term-ok',   text: `✔ SNAPSHOT COMPLETADO EXITOSAMENTE. AHORRO POR HARDLINKS: >90%` },
    { t: 10000, level: 'term-ok',  text: `======================================================================` }
  ];

  lines.forEach(l => {
    const timerId = setTimeout(() => {
      const now = new Date().toLocaleTimeString();
      const div = document.createElement('div');
      div.className = 'terminal-line';
      div.innerHTML = `<span class="term-time">[${now}]</span> <span class="${l.level}">${l.text}</span>`;
      terminal.appendChild(div);
      terminal.scrollTop = terminal.scrollHeight;

      if (l.t >= 10000) {
        showToast(`Respaldo de [${taskId}] completado con éxito`, 'success');
        task.lastRun = 'Hace unos instantes';
        task.snapsCount += 1;

        // Registrar el nuevo snapshot en el historial dinámico
        const todayDate = new Date();
        const dateFormatted = `${todayDate.toISOString().slice(0, 10)} ${todayDate.toTimeString().slice(0, 8)}`;
        if (!AppState.snapshotsData[taskId]) AppState.snapshotsData[taskId] = [];
        AppState.snapshotsData[taskId].unshift({
          name: `snapshot_${nowStr}`,
          date: dateFormatted,
          apparent: task.sizeLogical,
          real: '124 MB',
          dedup: '98.7%',
          status: 'Atómico / Íntegro'
        });

        // Registrar evento en feed y logs
        const logTime = todayDate.toTimeString().slice(0, 8);
        AppState.systemLogs.unshift({
          time: logTime,
          level: 'OK',
          src: 'backup',
          text: `backup_runner: Tarea [${taskId}] snapshot snapshot_${nowStr} promovido atómicamente.`
        });
        AppState.activityFeed.unshift({
          badge: 'Éxito',
          badgeClass: 'badge-ok',
          title: `Snapshot completado: ${taskId}`,
          desc: `Promoción atómica • Snapshots en disco: ${task.snapsCount}`
        });

        renderBackupTasksTable();
        renderSystemLogs();
        renderDashboardActivityFeed();
      }
    }, l.t);

    AppState.activeBackupTimers.push(timerId);
  });
}

function openSnapshotsModal(taskId) {
  const snaps = AppState.snapshotsData[taskId] || [];
  const tbody = document.getElementById('snapshots-table-body');
  document.getElementById('modal-snapshots-task-name').innerText = taskId;

  if (snaps.length === 0) {
    tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--text-muted); padding:20px;">No hay snapshots históricos registrados todavía para esta tarea.</td></tr>`;
  } else {
    tbody.innerHTML = snaps.map(s => `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <svg class="icon" style="color:var(--accent-primary);"><use href="#icon-clock"></use></svg>
            <strong>${s.name}</strong>
          </div>
        </td>
        <td>${s.date}</td>
        <td>${s.apparent}</td>
        <td><strong style="color:var(--accent-success);">${s.real}</strong></td>
        <td><span class="badge badge-ok">${s.dedup}</span></td>
        <td><span class="badge badge-blue">${s.status}</span></td>
      </tr>
    `).join('');
  }

  openModal('modal-snapshots');
}

// ==============================================================================
// Pestaña 5: Mantenimiento y Diagnóstico
// ==============================================================================
function runBtrfsScrubSimulation() {
  if (AppState.isScrubRunning) {
    showToast('Una auditoría BTRFS Scrub ya se encuentra en ejecución', 'warning');
    return;
  }

  const btn = document.getElementById('btn-run-scrub');
  const bar = document.getElementById('scrub-progress-bar');
  const statusTxt = document.getElementById('scrub-status-text');

  if (!btn || !bar) return;

  AppState.isScrubRunning = true;
  btn.disabled = true;
  statusTxt.innerText = 'Ejecutando btrfs scrub start -B /srv/nas...';
  bar.style.width = '0%';

  let progress = 0;
  const interval = setInterval(() => {
    progress += 20;
    bar.style.width = `${progress}%`;
    if (progress >= 100) {
      clearInterval(interval);
      AppState.isScrubRunning = false;
      btn.disabled = false;
      statusTxt.innerText = '✔ Scrub finalizado: 1,420,892 bloques verificados. 0 errores detectados (Bit Rot 0%).';
      showToast('Auditoría BTRFS Scrub finalizada sin inconsistencias', 'success');
    }
  }, 450);
}

function runFstrimSimulation() {
  if (AppState.isTrimRunning) {
    showToast('El descarte flash fstrim ya está en progreso', 'warning');
    return;
  }

  const btn = document.getElementById('btn-run-trim');
  if (!btn) return;

  AppState.isTrimRunning = true;
  btn.disabled = true;
  showToast('Ejecutando fstrim -va en unidades SSD flash...', 'info');

  setTimeout(() => {
    AppState.isTrimRunning = false;
    btn.disabled = false;
    document.getElementById('trim-status-text').innerText = '✔ Último descarte manual exitoso: 114.6 GiB recortados en /srv/nas';
    showToast('fstrim completado: Sectores flash descartados correctamente', 'success');
  }, 1200);
}

function runLogrotateSimulation() {
  showToast('Rotando logs del servidor con copytruncate y compresión gzip...', 'info');
  setTimeout(() => {
    showToast('Bitácoras rotadas exitosamente (/var/log/nas-backups.1.gz)', 'success');
  }, 900);
}

function testConnectivitySimulation() {
  const hostInput = document.getElementById('test-conn-host');
  const host = hostInput ? hostInput.value.trim() : '10.10.1.2';
  const type = document.getElementById('test-conn-type')?.value || '445';
  const out = document.getElementById('test-conn-result');
  const btn = document.getElementById('btn-test-conn');

  if (!host) {
    showToast('Ingresa una dirección IP o nombre de host válido para la prueba', 'danger');
    return;
  }

  if (!out) return;
  btn.disabled = true;
  out.innerHTML = `<span style="color:var(--accent-primary);">Probando conectividad con ${host} vía ${type === 'ICMP' ? 'ICMP Ping' : `puerto ${type}`}...</span>`;

  setTimeout(() => {
    btn.disabled = false;
    let detailText = '';
    let protoTitle = '';

    if (type === 'ICMP') {
      protoTitle = '✔ Respuesta de Ping ICMP exitosa en 1.2 ms';
      detailText = '4 paquetes transmitidos, 4 paquetes recibidos, 0% packet loss. RTT min/avg/max = 0.8/1.2/2.1 ms.';
    } else if (type === '445') {
      protoTitle = '✔ Puerto 445/tcp (SMB/CIFS) abierto y escuchando';
      detailText = 'Handshake TCP establecido en 2.4 ms. Dialecto SMB 3.1.1 soportado con cifrado AES-128-GCM.';
    } else if (type === '22') {
      protoTitle = '✔ Puerto 22/tcp (SSH) abierto y escuchando';
      detailText = 'Banner SSH-2.0-OpenSSH_9.2p1 Debian 13 recibido. Negociación criptográfica correcta.';
    } else if (type === '5357') {
      protoTitle = '✔ Puerto 5357/tcp (WSD Discovery) activo';
      detailText = 'Servicio wsdd2 respondiendo sondas de descubrimiento para clientes Windows 10/11.';
    }

    out.innerHTML = `
      <div style="color:var(--accent-success); font-weight:700;">${protoTitle}</div>
      <div style="color:var(--text-secondary); margin-top:4px;">Destino: ${host} | Protocolo: ${type}</div>
      <div style="color:var(--text-muted); font-size:11px;">${detailText}</div>
    `;
    showToast(`Conectividad con ${host} (${type}) verificada`, 'success');
  }, 700);
}

function renderSystemLogs() {
  const container = document.getElementById('system-logs-container');
  if (!container) return;

  container.innerHTML = AppState.systemLogs.map(l => {
    const levelClass = l.level === 'OK' ? 'term-ok' : l.level === 'WARN' ? 'term-warn' : l.level === 'ERR' ? 'term-err' : 'term-info';
    return `<div class="terminal-line"><span class="term-time">[${l.time}]</span> <span class="badge badge-gray" style="font-size:10px; margin-right:6px;">${l.src}</span> <span class="${levelClass}">[${l.level}]</span> ${l.text}</div>`;
  }).join('');
}

function copySystemLogs() {
  const text = AppState.systemLogs.map(l => `[${l.time}] [${l.src}] [${l.level}] ${l.text}`).join('\n');
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(() => {
      showToast('Bitácoras copiadas al portapapeles', 'success');
    }).catch(() => {
      fallbackCopy(text);
    });
  } else {
    fallbackCopy(text);
  }
}

function fallbackCopy(text) {
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand('copy');
    showToast('Bitácoras copiadas al portapapeles', 'success');
  } catch (e) {
    showToast('No se pudo copiar automáticamente', 'warning');
  }
  document.body.removeChild(ta);
}

// ==============================================================================
// Manejo de Modales
// ==============================================================================
function setupModals() {
  document.querySelectorAll('.modal-overlay').forEach(modal => {
    modal.addEventListener('click', (e) => {
      if (e.target === modal) {
        closeModal(modal.id);
      }
    });
  });

  document.querySelectorAll('.modal-close').forEach(btn => {
    btn.addEventListener('click', () => {
      const modal = btn.closest('.modal-overlay');
      if (modal) closeModal(modal.id);
    });
  });
}

function openModal(modalId) {
  const modal = document.getElementById(modalId);
  if (modal) modal.classList.add('open');
  if (modalId === 'modal-new-share') {
    updateNewSharePreview();
  }
}

function closeModal(modalId) {
  const modal = document.getElementById(modalId);
  if (modal) modal.classList.remove('open');
}

function openChangePasswordModal(uid) {
  const inputUid = document.getElementById('change-pass-uid');
  if (inputUid) inputUid.value = uid;
  const display = document.getElementById('change-pass-user-display');
  if (display) display.innerText = uid;
  const pass1 = document.getElementById('change-pass-new');
  const pass2 = document.getElementById('change-pass-confirm');
  if (pass1) pass1.value = '';
  if (pass2) pass2.value = '';
  openModal('modal-change-password');
}

// ==============================================================================
// Configuración de Formularios y Eventos
// ==============================================================================
function setupForms() {
  // Búsqueda en recursos compartidos
  const shareSearch = document.getElementById('shares-search-input');
  if (shareSearch) {
    shareSearch.addEventListener('input', (e) => {
      renderSharesTable(e.target.value);
    });
  }

  // Búsqueda en usuarios
  const userSearch = document.getElementById('users-search-input');
  if (userSearch) {
    userSearch.addEventListener('input', (e) => {
      renderUsersTable(e.target.value);
    });
  }

  // Eventos de creación de recurso
  const nameInput = document.getElementById('new-share-name');
  if (nameInput) {
    nameInput.addEventListener('input', updateNewSharePreview);
  }
  const pathInput = document.getElementById('new-share-path');
  if (pathInput) {
    pathInput.addEventListener('input', () => {
      pathInput.dataset.userEdited = 'true';
      updateNewSharePreview();
    });
  }
  const commentInput = document.getElementById('new-share-comment');
  if (commentInput) {
    commentInput.addEventListener('input', updateNewSharePreview);
  }
  document.querySelectorAll('input[name="new-share-vis"]').forEach(r => {
    r.addEventListener('change', updateNewSharePreview);
  });
  document.querySelectorAll('input[name="new-share-scheme"]').forEach(r => {
    r.addEventListener('change', (e) => {
      document.querySelectorAll('.radio-tile').forEach(t => t.classList.remove('selected'));
      e.target.closest('.radio-tile').classList.add('selected');
      const scheme2Wrap = document.getElementById('new-share-scheme2-wrapper');
      const groupsWrap = document.getElementById('new-share-groups-container');
      if (scheme2Wrap) {
        scheme2Wrap.style.display = e.target.value === '2' ? 'block' : 'none';
      }
      if (groupsWrap) {
        groupsWrap.style.display = e.target.value === '4' ? 'none' : 'block';
      }
      updateNewSharePreview();
    });
  });

  const writelistSelect = document.getElementById('new-share-writelist-select');
  if (writelistSelect) {
    writelistSelect.addEventListener('change', updateNewSharePreview);
  }

  // Guardar Nuevo Recurso
  const formNewShare = document.getElementById('form-new-share');
  if (formNewShare) {
    formNewShare.addEventListener('submit', (e) => {
      e.preventDefault();
      const rawName = document.getElementById('new-share-name').value.trim();
      const isHidden = document.getElementById('new-share-vis-hidden').checked;
      let finalName = rawName.replace(/\s+/g, '_').replace(/[^A-Za-z0-9_$-]/g, '');
      if (isHidden && !finalName.endsWith('$')) finalName += '$';
      if (!isHidden) finalName = finalName.replace(/\$$/, '');

      if (!finalName) {
        showToast('El nombre del recurso no es válido', 'danger');
        return;
      }

      if (AppState.shares.some(s => s.id === finalName)) {
        showToast(`Ya existe un recurso con el nombre [${finalName}]`, 'danger');
        return;
      }

      const schemeVal = document.querySelector('input[name="new-share-scheme"]:checked')?.value || '1';
      const schemeNames = {
        '1': 'Lectura y Escritura por Grupo',
        '2': 'Solo Lectura General + Escritura Exclusiva',
        '3': 'Solo Lectura Estricta',
        '4': 'Acceso Público / Invitados (Sin Contraseña)'
      };

      const selectedGroups = schemeVal === '4'
        ? ['Todos (Invitados)']
        : (Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value).length
            ? Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value)
            : ['grp_sistemas']);

      const newShare = {
        id: finalName,
        name: finalName,
        vis: isHidden ? 'Oculto ($)' : 'Visible',
        browseable: isHidden ? 'no' : 'yes',
        scheme: schemeVal,
        schemeName: schemeNames[schemeVal],
        groups: selectedGroups,
        writeList: schemeVal === '2' ? (document.getElementById('new-share-writelist-select')?.value || 'grp_sistemas') : '',
        readOnly: schemeVal === '3' ? 'yes' : 'no',
        path: document.getElementById('new-share-path').value || `/srv/nas/${finalName.replace(/\$$/, '')}`,
        status: 'Activo',
        comment: document.getElementById('new-share-comment').value || `Carpeta compartida ${finalName}`
      };

      AppState.shares.push(newShare);

      // Actualizar los shares asignados a cada grupo
      if (schemeVal !== '4') {
        selectedGroups.forEach(gName => {
          const grp = AppState.groups.find(g => g.name === gName);
          if (grp && !grp.shares.includes(finalName)) {
            grp.shares.push(finalName);
          }
        });
      }

      showToast(`Recurso [${finalName}] creado con éxito en smb.conf`, 'success');
      closeModal('modal-new-share');
      updateAllCounters();
      renderSharesTable();
      renderGroupsTable();
      formNewShare.reset();
      if (pathInput) pathInput.dataset.userEdited = '';
    });
  }

  // Guardar Nuevo Usuario
  const formNewUser = document.getElementById('form-new-user');
  if (formNewUser) {
    formNewUser.addEventListener('submit', (e) => {
      e.preventDefault();
      const uid = document.getElementById('new-user-uid').value.trim().toLowerCase();
      const fullName = document.getElementById('new-user-fullname').value.trim();
      const selectedGroups = Array.from(document.querySelectorAll('.new-user-grp-chk:checked')).map(c => c.value);

      if (!uid.match(/^[a-z][a-z0-9_-]*$/)) {
        showToast('El usuario debe empezar con letra minúscula y contener solo letras y números', 'danger');
        return;
      }

      if (AppState.users.some(u => u.uid === uid)) {
        showToast(`El usuario ${uid} ya existe`, 'danger');
        return;
      }

      const assignedGroups = selectedGroups.length ? selectedGroups : ['grp_empleados'];

      AppState.users.push({
        uid: uid,
        fullName: fullName || 'Empleado EAD',
        groups: assignedGroups,
        sambaActive: true,
        shell: '/usr/sbin/nologin',
        lastLogin: 'Nunca (Nuevo)'
      });

      // Añadir al array de miembros de cada grupo
      assignedGroups.forEach(gName => {
        const grp = AppState.groups.find(g => g.name === gName);
        if (grp && !grp.members.includes(uid)) {
          grp.members.push(uid);
        }
      });

      showToast(`Usuario [${uid}] creado y sincronizado con Samba`, 'success');
      closeModal('modal-new-user');
      updateAllCounters();
      renderUsersTable();
      renderGroupsTable();
      formNewUser.reset();
    });
  }

  // Guardar Nuevo Grupo
  const formNewGroup = document.getElementById('form-new-group');
  if (formNewGroup) {
    formNewGroup.addEventListener('submit', (e) => {
      e.preventDefault();
      const nameSuffix = document.getElementById('new-group-name').value.trim().toLowerCase().replace(/[^a-z0-9_]/g, '');
      const groupFullName = `grp_${nameSuffix}`;

      if (!nameSuffix) {
        showToast('El nombre del grupo no puede estar vacío', 'danger');
        return;
      }

      if (AppState.groups.some(g => g.name === groupFullName)) {
        showToast(`El grupo ${groupFullName} ya existe`, 'danger');
        return;
      }

      AppState.groups.push({
        name: groupFullName,
        level: 'Departamental',
        members: [],
        shares: []
      });

      showToast(`Grupo de seguridad [${groupFullName}] creado`, 'success');
      closeModal('modal-new-group');
      updateAllCounters();
      renderGroupsTable();
      renderGroupCheckboxes();
      formNewGroup.reset();
    });
  }

  // Guardar Miembros Editados en Grupo
  const formEditGroup = document.getElementById('form-edit-group');
  if (formEditGroup) {
    formEditGroup.addEventListener('submit', (e) => {
      e.preventDefault();
      const groupName = document.getElementById('edit-group-name').value;
      const grp = AppState.groups.find(g => g.name === groupName);
      if (!grp) return;

      const checkedUids = Array.from(document.querySelectorAll('.edit-group-user-chk:checked')).map(c => c.value);
      grp.members = checkedUids;

      // Sincronizar grupos en cada usuario
      AppState.users.forEach(u => {
        const shouldBeMember = checkedUids.includes(u.uid);
        const isMember = u.groups.includes(groupName);
        if (shouldBeMember && !isMember) {
          u.groups.push(groupName);
        } else if (!shouldBeMember && isMember) {
          u.groups = u.groups.filter(g => g !== groupName);
        }
      });

      showToast(`Miembros de [${groupName}] actualizados`, 'success');
      closeModal('modal-edit-group');
      renderGroupsTable();
      renderUsersTable();
    });
  }

  // Guardar Cambio de Clave
  const formChangePassword = document.getElementById('form-change-password');
  if (formChangePassword) {
    formChangePassword.addEventListener('submit', (e) => {
      e.preventDefault();
      const uid = document.getElementById('change-pass-uid')?.value;
      const pass1 = document.getElementById('change-pass-new')?.value;
      const pass2 = document.getElementById('change-pass-confirm')?.value;

      if (!pass1 || pass1.length < 6) {
        showToast('La contraseña debe tener al menos 6 caracteres', 'danger');
        return;
      }

      if (pass1 !== pass2) {
        showToast('Las contraseñas no coinciden. Intenta de nuevo.', 'danger');
        return;
      }

      showToast(`Contraseña de [${uid}] actualizada en Debian y Samba`, 'success');
      closeModal('modal-change-password');
      formChangePassword.reset();
    });
  }

  // Guardar Nueva Tarea de Backup
  const formNewBackup = document.getElementById('form-new-backup');
  if (formNewBackup) {
    formNewBackup.addEventListener('submit', (e) => {
      e.preventDefault();
      const id = document.getElementById('new-bkp-id').value.trim().toLowerCase().replace(/[^a-z0-9_]/g, '');
      const proto = document.getElementById('new-bkp-proto').value;
      const src = document.getElementById('new-bkp-src').value.trim();
      const user = document.getElementById('new-bkp-user').value.trim();
      const cron = document.getElementById('new-bkp-cron').value.trim() || '0 2 * * *';
      const retention = parseInt(document.getElementById('new-bkp-retention').value) || 15;

      if (!id || !src) {
        showToast('Completa todos los campos obligatorios', 'danger');
        return;
      }

      if (AppState.backupTasks.some(t => t.id === id)) {
        showToast(`Ya existe una tarea con el identificador [${id}]`, 'danger');
        return;
      }

      AppState.backupTasks.push({
        id: id,
        proto: proto,
        src: src,
        user: user || 'root',
        credFile: `/etc/backup-credentials/${id}.cred`,
        cron: cron,
        cronDesc: 'Programado vía cron.d',
        retention: retention,
        snapsCount: 0,
        lastRun: 'Pendiente de primer ciclo',
        lastStatus: 'Listo',
        sizeLogical: '0 B',
        sizeReal: '0 B',
        dedupRate: 'N/A'
      });

      AppState.snapshotsData[id] = [];

      showToast(`Tarea de respaldo [${id}] configurada en /etc/cron.d/backup_${id}`, 'success');
      closeModal('modal-new-backup');
      updateAllCounters();
      renderBackupTasksTable();
      formNewBackup.reset();
    });
  }
}
