/**
 * NAS DEBIAN • FRONTEND JS (Vanilla ES6+)
 * Comunicación asíncrona mediante Fetch con endpoints JSON de la API MVC.
 * 100% Offline • Cero dependencias externas.
 */

'use strict';

// ==============================================================================
// 1. Estado Global y Utilidades UI
// ==============================================================================
const AppState = {
  activeView: 'dashboard',
  refreshInterval: null,
  groupsCache: [],
  logSource: 'all',
  logSearchTimer: null,
  logsCache: [],
  files: {
    root: 'nas',
    currentPath: '',
    items: [],
    viewMode: localStorage.getItem('nas_file_view') || 'grid',
    inTrash: false,
    trashItems: [],
    selectedItemPath: null,
    currentPreviewFile: null,
    currentPreviewText: '',
    isEditingPreview: false,
  },
  terminal: {
    cwd: '/srv/nas',
    history: [],
    historyIndex: -1,
    isExecuting: false,
    ptyStarted: false,
    ptyTimer: null,
  },
  domain: {
    status: null,
  },
};

function showToast(message, type = 'info') {
  const container = document.getElementById('toast-container');
  if (!container) return;

  const toast = document.createElement('div');
  toast.className = `alert-box alert-box-${type === 'error' ? 'danger' : (type === 'success' ? 'success' : 'info')}`;
  toast.style.boxShadow = '0 4px 12px rgba(0,0,0,0.3)';
  toast.style.minWidth = '280px';
  toast.style.transition = 'opacity 0.3s ease';

  const msgDiv = document.createElement('div');
  msgDiv.textContent = String(message ?? '');
  toast.appendChild(msgDiv);
  container.appendChild(toast);

  setTimeout(() => {
    toast.style.opacity = '0';
    setTimeout(() => toast.remove(), 300);
  }, 4000);
}

// Reloj del Servidor en Vivo Sincronizado
const _serverClock = {
  timeDiffMs: 0,
  tzIana: 'America/Mexico_City',
  tzAbbr: 'CST',
  tzOffsetSec: -21600,
  formatter: null,
  timer: null
};

function initServerClock(serverConfig) {
  if (!serverConfig) return;
  if (typeof serverConfig === 'object') {
    const initialTimestampSec = serverConfig.timestamp || Math.floor(Date.now() / 1000);
    _serverClock.timeDiffMs = (initialTimestampSec * 1000) - Date.now();
    if (serverConfig.timezone) _serverClock.tzIana = serverConfig.timezone;
    if (serverConfig.tz) _serverClock.tzAbbr = serverConfig.tz;
    if (typeof serverConfig.offset === 'number') _serverClock.tzOffsetSec = serverConfig.offset;
  } else {
    const initialTimestampSec = Number(serverConfig) || Math.floor(Date.now() / 1000);
    _serverClock.timeDiffMs = (initialTimestampSec * 1000) - Date.now();
    if (arguments[1]) _serverClock.tzAbbr = arguments[1];
  }

  try {
    if (_serverClock.tzIana) {
      _serverClock.formatter = new Intl.DateTimeFormat('en-GB', {
        timeZone: _serverClock.tzIana,
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hour12: false
      });
    }
  } catch (e) {
    _serverClock.formatter = null;
  }

  updateServerClockDisplay();
  if (!_serverClock.timer) {
    _serverClock.timer = setInterval(updateServerClockDisplay, 1000);
  }
}

function updateServerClockDisplay(newTzAbbr) {
  const clockEl = document.getElementById('header-server-clock');
  if (!clockEl) return;

  if (newTzAbbr && typeof newTzAbbr === 'string') {
    _serverClock.tzAbbr = newTzAbbr;
  }

  let timeStr = '';
  const nowServerMs = Date.now() + _serverClock.timeDiffMs;

  if (_serverClock.formatter) {
    try {
      timeStr = _serverClock.formatter.format(new Date(nowServerMs));
    } catch (e) {
      timeStr = '';
    }
  }

  if (!timeStr) {
    const serverDate = new Date(nowServerMs + (_serverClock.tzOffsetSec * 1000));
    const pad = n => String(n).padStart(2, '0');
    timeStr = `${pad(serverDate.getUTCHours())}:${pad(serverDate.getUTCMinutes())}:${pad(serverDate.getUTCSeconds())}`;
  }

  clockEl.textContent = timeStr;

  const tzEl = document.getElementById('header-server-tz');
  if (tzEl && _serverClock.tzAbbr) {
    tzEl.textContent = _serverClock.tzAbbr;
  }

  const pillEl = document.getElementById('header-clock-pill');
  if (pillEl) {
    pillEl.title = `Hora local del servidor NAS: ${timeStr} ${_serverClock.tzAbbr} (${_serverClock.tzIana || 'Local'})`;
  }
}

function openModal(modalId) {
  let modal = document.getElementById(modalId);
  if (!modal && modalId === 'modal-format-disk') {
    modal = document.getElementById('modal-storage-manage');
  } else if (!modal && modalId === 'modal-storage-manage') {
    modal = document.getElementById('modal-format-disk');
  }
  if (modal) {
    modal.classList.add('active');
    modal.classList.add('open');
  }
}

function closeModal(modalId) {
  let modal = document.getElementById(modalId);
  if (!modal && modalId === 'modal-format-disk') {
    modal = document.getElementById('modal-storage-manage');
  } else if (!modal && modalId === 'modal-storage-manage') {
    modal = document.getElementById('modal-format-disk');
  }
  if (modal) {
    modal.classList.remove('active');
    modal.classList.remove('open');
    if (modalId === 'modal-file-preview') {
      const vidEl = document.getElementById('preview-video-element');
      if (vidEl) {
        vidEl.pause();
        vidEl.removeAttribute('src');
        vidEl.load();
      }
      const audEl = document.getElementById('preview-audio-element');
      if (audEl) {
        audEl.pause();
        audEl.removeAttribute('src');
        audEl.load();
      }
      const imgEl = document.getElementById('preview-img-element');
      if (imgEl) {
        imgEl.removeAttribute('src');
      }
      const pdfFrame = document.getElementById('preview-pdf-frame');
      if (pdfFrame) {
        pdfFrame.src = '';
      }
      AppState.files.currentPreviewFile = null;
      AppState.files.isEditingPreview = false;
    }
  }
}

function toggleTheme() {
  const html = document.documentElement;
  const current = html.getAttribute('data-theme') || 'dark';
  const next = current === 'dark' ? 'light' : 'dark';
  html.setAttribute('data-theme', next);

  try {
    localStorage.setItem('nas_theme', next);
  } catch (e) {
    console.warn('No se pudo persistir la preferencia de tema en localStorage:', e);
  }
}

function escapeHtml(str) {
  if (typeof str !== 'string') return String(str ?? '');
  return str.replace(/[&<>'"]/g,
    tag => ({
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      "'": '&#39;',
      '"': '&quot;'
    }[tag] || tag)
  );
}

// ==============================================================================
// 2. Navegación entre Vistas
// ==============================================================================
const VALID_VIEWS = [
  'dashboard', 'files', 'logs', 'storage', 'networking', 'services', 'terminal',
  'shares', 'backups', 'users', 'diagnostics', 'updates', 'applications', 'domain'
];

// Módulos permitidos al rol operador (grp_web)
const WEB_USER_VIEWS = ['dashboard', 'files', 'logs', 'shares', 'backups', 'users', 'services', 'diagnostics', 'networking'];

// Ajusta la interfaz según el rol de la sesión (administrador / superadministrador / operador).
function applyRoleUi() {
  const isAdmin = window.NAS_IS_ADMIN !== false;
  const isSuper = window.NAS_IS_SUPER === true;

  ['user-admin-role-wrap', 'edit-user-admin-role-wrap'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.style.display = isAdmin ? '' : 'none';
  });
  ['user-superadmin-role-wrap', 'edit-user-superadmin-role-wrap'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.style.display = isSuper ? '' : 'none';
  });

  // Ocultar módulo de respaldos en frontend si el rol exclusivo es ARCHIVOS
  if (window.NAS_SERVER_ROLE === 'ARCHIVOS') {
    document.querySelectorAll('[data-view="backups"]').forEach(el => el.style.display = 'none');
    const backupView = document.getElementById('view-backups');
    if (backupView) backupView.style.display = 'none';
  }
}

function switchView(viewName, updateHash = true) {
  if (viewName === 'permissions') {
    viewName = 'users';
  }
  if (viewName === 'diagnostics') {
    viewName = 'services';
  }
  if (viewName === 'updates') {
    viewName = 'applications';
  }
  if (!VALID_VIEWS.includes(viewName)) {
    viewName = 'dashboard';
  }

  // Si el servidor opera en rol ARCHIVOS, denegar acceso a backups
  if (window.NAS_SERVER_ROLE === 'ARCHIVOS' && viewName === 'backups') {
    viewName = 'dashboard';
  }

  // Los operadores solo pueden ver los módulos delegados; el resto se redirige al dashboard.
  if (window.NAS_IS_ADMIN === false && !WEB_USER_VIEWS.includes(viewName)) {
    viewName = 'dashboard';
  }

  // La terminal web está confinada exclusivamente al superadministrador.
  if (viewName === 'terminal' && window.NAS_IS_SUPER !== true) {
    viewName = 'dashboard';
  }

  AppState.activeView = viewName;

  if (updateHash && window.location.hash !== '#' + viewName) {
    window.location.hash = '#' + viewName;
  }

  // Actualizar sidebar nav (soporta alias unificados)
  document.querySelectorAll('.sidebar-nav .nav-item').forEach(item => {
    const itemTarget = item.getAttribute('data-view');
    const isMatch = itemTarget === viewName ||
      (viewName === 'services' && itemTarget === 'diagnostics') ||
      (viewName === 'diagnostics' && itemTarget === 'services') ||
      (viewName === 'applications' && itemTarget === 'updates') ||
      (viewName === 'updates' && itemTarget === 'applications');
    if (isMatch) {
      item.classList.add('active');
    } else {
      item.classList.remove('active');
    }
  });

  // Actualizar secciones de vista (soporta alias unificados)
  document.querySelectorAll('.view-section').forEach(sec => {
    const isSecMatch = sec.id === `view-${viewName}` ||
      (viewName === 'services' && sec.id === 'view-diagnostics') ||
      (viewName === 'diagnostics' && sec.id === 'view-services') ||
      (viewName === 'applications' && sec.id === 'view-updates') ||
      (viewName === 'updates' && sec.id === 'view-applications');
    if (isSecMatch) {
      sec.classList.add('active');
    } else {
      sec.classList.remove('active');
    }
  });

  // Cerrar menú móvil si estuviera abierto
  const sidebar = document.querySelector('.sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  if (sidebar && sidebar.classList.contains('open')) {
    sidebar.classList.remove('open');
    if (backdrop) backdrop.classList.remove('active');
  }

  // Carga de datos según la vista
  if (viewName !== 'terminal') {
    stopTerminalPolling();
  }
  switch (viewName) {
    case 'dashboard':
      refreshDashboardMetrics();
      break;
    case 'files':
      loadFiles();
      updateTrashBadge();
      break;
    case 'shares':
      loadShares();
      break;
    case 'backups':
      loadBackups();
      break;
    case 'storage':
      loadStorage();
      break;
    case 'users':
      loadUsersAndGroups();
      loadAccessMatrix();
      break;
    case 'services':
    case 'diagnostics':
      loadServices();
      loadDiagnostics();
      break;
    case 'logs':
      loadLogs();
      break;
    case 'networking':
      loadNetworking();
      break;
    case 'terminal':
      loadTerminal();
      break;
    case 'applications':
    case 'updates':
      loadUpdates();
      loadApplications();
      break;
    case 'domain':
      loadDomainStatus();
      break;
  }
}

// ==============================================================================
// 3. Clientes API Asíncronos (Fetch) & Autenticación
// ==============================================================================
function getCsrfToken() {
  const meta = document.querySelector('meta[name="csrf-token"]');
  return meta ? meta.getAttribute('content') : '';
}

async function apiFetch(endpoint, options = {}) {
  try {
    const method = (options.method || 'GET').toUpperCase();
    const headers = {
      'Accept': 'application/json',
      'Content-Type': 'application/json',
      ...(options.headers || {}),
    };

    if (['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)) {
      const csrf = getCsrfToken();
      if (csrf) {
        headers['X-CSRF-Token'] = csrf;
      }
    }

    const res = await fetch(endpoint, {
      ...options,
      method,
      headers,
    });

    if (res.status === 401) {
      showToast('Sesión no autorizada o expirada. Redirigiendo...', 'warning');
      setTimeout(() => {
        window.location.href = '/login';
      }, 800);
      throw new Error('Sesión expirada.');
    }

    let data;
    try {
      data = await res.json();
    } catch (e) {
      throw new Error(`Error en respuesta del servidor (${res.status} ${res.statusText || 'Error'})`);
    }

    if (!res.ok || data.success === false) {
      throw new Error(data.error || data.message || data.output || `Error ${res.status}`);
    }
    return data;
  } catch (err) {
    if (err.message !== 'Sesión expirada.' && !options.silentToast) {
      showToast(err.message, 'error');
    }
    throw err;
  }
}

async function logoutSession() {
  const btn = document.getElementById('btn-logout');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }
  try {
    await apiFetch('/logout', { method: 'POST' });
  } catch (e) {
    // Si la llamada falla o redirige
  } finally {
    window.location.href = '/login';
  }
}

// ==============================================================================
// 4. Módulo: Dashboard & Métricas
// ==============================================================================
async function refreshDashboardMetrics() {
  const icon = document.getElementById('icon-refresh-metrics');
  if (icon) icon.classList.add('spin');

  try {
    const res = await apiFetch('/api/metrics');
    const d = res.data || res;

    if (d.system) {
      if (d.system.server_timestamp) {
        _serverClock.timeDiffMs = (d.system.server_timestamp * 1000) - Date.now();
        if (d.system.timezone && d.system.timezone !== _serverClock.tzIana) {
          _serverClock.tzIana = d.system.timezone;
          try {
            _serverClock.formatter = new Intl.DateTimeFormat('en-GB', {
              timeZone: _serverClock.tzIana,
              hour: '2-digit',
              minute: '2-digit',
              second: '2-digit',
              hour12: false
            });
          } catch (e) {
            _serverClock.formatter = null;
          }
        }
        if (typeof d.system.timezone_offset === 'number') {
          _serverClock.tzOffsetSec = d.system.timezone_offset;
        }
        updateServerClockDisplay(d.system.timezone_abbr);
      }
      const ramVal = document.getElementById('kpi-ram-val');
      const ramBar = document.getElementById('kpi-ram-bar');
      if (ramVal) ramVal.innerHTML = `${d.system.ram_used_gb} GB <small style="font-size:12px; color:var(--text-secondary);">/ ${d.system.ram_total_gb} GB</small>`;
      if (ramBar) ramBar.style.width = `${d.system.ram_usage_pct}%`;
    }

    if (d.storage) {
      const sVal = document.getElementById('kpi-storage-val');
      const sBar = document.getElementById('kpi-storage-bar');
      if (sVal) sVal.innerHTML = `${d.storage.used_gb} GB <small style="font-size:12px; color:var(--text-secondary);">/ ${d.storage.total_gb} GB</small>`;
      if (sBar) sBar.style.width = `${d.storage.usage_percent}%`;
    }

    if (d.counts) {
      const bShares = document.getElementById('badge-shares');
      const bBackups = document.getElementById('badge-backups');
      const bUsers = document.getElementById('badge-users');
      if (bShares) bShares.textContent = d.counts.shares;
      if (bBackups) bBackups.textContent = d.counts.backups;
      if (bUsers) bUsers.textContent = d.counts.users;
    }

    if (d.services && Array.isArray(d.services)) {
      const netBadges = document.getElementById('kpi-network-badges');
      if (netBadges) {
        netBadges.innerHTML = d.services
          .filter(s => ['smbd', 'wsdd2', 'nginx'].includes(s.service) || (s.service && s.service.includes('fpm')))
          .map(s => {
            const label = (s.service && s.service.includes('fpm')) ? 'php-fpm' : s.service;
            const cls = s.active ? 'badge-ok' : 'badge-danger';
            const statusTxt = s.active ? ((s.service && s.service.includes('fpm')) ? 'ondemand' : 'OK') : 'OFF';
            return `<span class="badge ${cls}">${escapeHtml(label)} ${statusTxt}</span>`;
          })
          .join(' ');
      }
    }

    loadDashboardActivity();
  } catch (e) {
    console.error('Error actualizando métricas:', e);
  } finally {
    if (icon) setTimeout(() => icon.classList.remove('spin'), 500);
  }
}

async function loadDashboardActivity() {
  const feed = document.getElementById('dashboard-activity-feed');
  if (!feed) return;

  try {
    const res = await apiFetch('/api/logs?limit=5');
    const logs = res.data || [];
    if (logs.length === 0) {
      feed.innerHTML = '<p style="color:var(--text-secondary);">Sin actividad reciente registrada.</p>';
      return;
    }

    feed.innerHTML = logs.map(l => {
      const label = l.action_label || l.event_label || l.action || l.event || l.unit || 'Evento';
      const detail = l.message || (l.target ? `${l.target}` : '');
      const badge = l.badge || 'blue';
      const tsShort = (l.timestamp || '').substring(11, 19) || l.timestamp || '';
      return `
        <div style="font-size:12px; border-bottom:1px solid var(--border-color); padding:4px 0; display:flex; justify-content:space-between; align-items:center; gap:8px;">
          <div style="display:flex; align-items:center; gap:6px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">
            <span class="badge badge-${badge}">${escapeHtml(label)}</span>
            <span style="color:var(--text-secondary);">${escapeHtml(detail)}</span>
          </div>
          <span style="font-size:10.5px; color:var(--text-muted); font-family:monospace; white-space:nowrap;">${escapeHtml(tsShort)}</span>
        </div>
      `;
    }).join('');
  } catch (e) {
    feed.innerHTML = '<p style="color:var(--accent-danger);">No se pudo cargar la actividad reciente.</p>';
  }
}

// ==============================================================================
// 5. Módulo: Recursos Compartidos (Samba)
// ==============================================================================
let _sharesData = [];

async function loadShares() {
  const tbody = document.getElementById('shares-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/shares');
    _sharesData = res.data || [];

    if (_sharesData.length === 0) {
      tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;">No hay recursos compartidos creados actualmente.</td></tr>';
      return;
    }

    tbody.innerHTML = _sharesData.map(s => `
      <tr>
        <td><strong>[${escapeHtml(s.name)}]</strong></td>
        <td><code>${escapeHtml(s.path)}</code></td>
        <td><span class="tag-pill">${escapeHtml(s.scheme_name)}</span></td>
        <td>${s.hidden ? '<span class="badge badge-gray">Oculto ($)</span>' : '<span class="badge badge-ok">Visible</span>'}</td>
        <td>${(s.valid_users || []).map(u => `<span class="tag-pill">${escapeHtml(u)}</span>`).join(' ') || (s.guest_ok ? '<em>Invitados</em>' : '<em>Sistemas</em>')}</td>
        <td style="text-align:right; white-space:nowrap;">
          <button class="btn btn-secondary btn-sm" title="Copiar ruta UNC" onclick="copyShareUnc('${escapeHtml(s.name)}')">
            <svg class="icon icon-sm"><use href="#icon-file-text"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Permisos de acceso" onclick="openSharePermissions('${escapeHtml(s.name)}')">
            <svg class="icon icon-sm"><use href="#icon-shield"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Editar" onclick="openEditShare('${escapeHtml(s.name)}')">
            <svg class="icon icon-sm"><use href="#icon-edit"></use></svg>
          </button>
          ${s.name === 'SISTEMAS' ? '' : `
            <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteShare('${escapeHtml(s.name)}')">
              <svg class="icon icon-sm"><use href="#icon-trash"></use></svg>
            </button>
          `}
        </td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--accent-danger);">Error al cargar recursos compartidos: ${escapeHtml(e.message)}</td></tr>`;
  }
}

async function openNewShare() {
  await populateShareGroupOptions();
  if (window.NAS_SERVER_ROLE === 'BACKUP') {
    const hiddenChk = document.getElementById('share-hidden');
    if (hiddenChk) {
      hiddenChk.checked = true;
      hiddenChk.disabled = true;
    }
    const schemeSelect = document.getElementById('share-scheme');
    if (schemeSelect) {
      const opt4 = schemeSelect.querySelector('option[value="4"]');
      if (opt4) opt4.style.display = 'none';
      if (schemeSelect.value === '4') schemeSelect.value = '1';
    }
  }
  toggleSchemeFields();
  openModal('modal-new-share');
}

function toggleSchemeFields() {
  const scheme = document.getElementById('share-scheme').value;
  const grpBox = document.getElementById('group-share-groups');
  const writeBox = document.getElementById('group-share-write-group');

  if (scheme === '4') {
    grpBox.style.display = 'none';
    writeBox.style.display = 'none';
  } else if (scheme === '2') {
    grpBox.style.display = 'block';
    writeBox.style.display = 'block';
  } else {
    grpBox.style.display = 'block';
    writeBox.style.display = 'none';
  }
}

async function populateShareGroupOptions() {
  const container = document.getElementById('share-groups-list');
  const writeSelect = document.getElementById('share-write-group');
  if (!container) return;

  try {
    const res = await apiFetch('/api/groups');
    AppState.groupsCache = res.data || [];
    let selectable = AppState.groupsCache;
    if (window.NAS_SERVER_ROLE === 'BACKUP') {
      selectable = selectable.filter(g => g.name === 'grp_samba' || g.name === 'grp_backups');
    }

    if (!selectable.length) {
      container.innerHTML = '<em style="color:var(--text-muted);">No hay grupos disponibles. Crea uno en “Usuarios y grupos”.</em>';
      if (writeSelect) writeSelect.innerHTML = '';
      return;
    }

    container.innerHTML = selectable.map(g => {
      const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
      return `
      <label style="display:flex; align-items:center; gap:8px;">
        <input type="checkbox" name="share_group" value="${escapeHtml(g.name)}" ${g.name === 'grp_samba' ? 'checked' : ''}>
        <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
      </label>`;
    }).join('');

    if (writeSelect) {
      writeSelect.innerHTML = selectable.map(g => `
        <option value="${escapeHtml(g.name)}">${escapeHtml(g.name)}</option>
      `).join('');
    }
  } catch (e) {
    console.error('Error cargando grupos para selector:', e);
  }
}

async function populateEditShareGroupOptions(selectedGroups, selectedWriteGroup) {
  const container = document.getElementById('edit-share-groups-list');
  const writeSelect = document.getElementById('edit-share-write-group');
  if (!container) return;

  try {
    const res = await apiFetch('/api/groups');
    let groups = res.data || [];
    if (window.NAS_SERVER_ROLE === 'BACKUP') {
      groups = groups.filter(g => g.name === 'grp_samba' || g.name === 'grp_backups');
    }
    const sel = (selectedGroups || []).map(g => String(g).replace('@', ''));
    container.innerHTML = groups.map(g => {
      const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
      return `
      <label style="display:flex; align-items:center; gap:8px;">
        <input type="checkbox" name="edit_share_group" value="${escapeHtml(g.name)}" ${sel.includes(g.name) ? 'checked' : ''}>
        <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
      </label>`;
    }).join('');

    if (writeSelect) {
      writeSelect.innerHTML = groups.map(g => `
        <option value="${escapeHtml(g.name)}" ${g.name === selectedWriteGroup ? 'selected' : ''}>${escapeHtml(g.name)}</option>
      `).join('');
    }
  } catch (e) {
    console.error('Error cargando grupos para edición:', e);
  }
}

async function openEditShare(name) {
  const s = _sharesData.find(x => x.name === name);
  if (!s) return;
  document.getElementById('edit-share-name').value = name;
  document.getElementById('edit-share-comment').value = s.comment || '';
  document.getElementById('edit-share-scheme').value = String(s.scheme || 1);
  const hiddenInput = document.getElementById('edit-share-hidden');
  if (hiddenInput) {
    if (window.NAS_SERVER_ROLE === 'BACKUP') {
      hiddenInput.checked = true;
      hiddenInput.disabled = true;
    } else {
      hiddenInput.checked = !!s.hidden;
      hiddenInput.disabled = false;
    }
  }
  const schemeSelect = document.getElementById('edit-share-scheme');
  if (schemeSelect && window.NAS_SERVER_ROLE === 'BACKUP') {
    const opt4 = schemeSelect.querySelector('option[value="4"]');
    if (opt4) opt4.style.display = 'none';
    if (schemeSelect.value === '4') schemeSelect.value = '1';
  }

  const groupTokens = (s.valid_users || []).filter(t => t.startsWith('@'));
  const writeToken = (s.write_list || [])[0] || '';
  await populateEditShareGroupOptions(groupTokens, String(writeToken).replace('@', ''));
  toggleEditSchemeFields();
  openModal('modal-edit-share');
}

function toggleEditSchemeFields() {
  const scheme = document.getElementById('edit-share-scheme').value;
  document.getElementById('edit-group-share-groups').style.display = (scheme === '4') ? 'none' : 'block';
  document.getElementById('edit-group-share-write-group').style.display = (scheme === '2') ? 'block' : 'none';
}

async function submitEditShare(event) {
  event.preventDefault();
  const name = document.getElementById('edit-share-name').value;
  const comment = document.getElementById('edit-share-comment').value;
  const scheme = parseInt(document.getElementById('edit-share-scheme').value, 10);
  let hidden = document.getElementById('edit-share-hidden').checked;
  const writeGroup = document.getElementById('edit-share-write-group').value;
  const groups = Array.from(document.querySelectorAll('input[name="edit_share_group"]:checked')).map(cb => cb.value);

  if (window.NAS_SERVER_ROLE === 'BACKUP') {
    hidden = true;
  }

  const btn = document.getElementById('btn-submit-edit-share') || event.target.querySelector('button[type="submit"]');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    await apiFetch('/api/shares/update', {
      method: 'POST',
      body: JSON.stringify({ name, comment, scheme, hidden, groups, write_group: writeGroup }),
    });
    showToast(`Recurso [${name}] actualizado.`, 'success');
    closeModal('modal-edit-share');
    loadShares();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function submitNewShare(event) {
  event.preventDefault();
  let name = document.getElementById('share-name').value;
  const comment = document.getElementById('share-comment').value;
  const scheme = parseInt(document.getElementById('share-scheme').value, 10);
  let hidden = document.getElementById('share-hidden').checked;
  const writeGroup = document.getElementById('share-write-group').value;

  if (window.NAS_SERVER_ROLE === 'BACKUP') {
    hidden = true;
    if (!name.endsWith('$')) {
      name += '$';
    }
  }

  const selectedGroups = Array.from(document.querySelectorAll('input[name="share_group"]:checked'))
    .map(cb => cb.value);
  const unit = document.getElementById('share-unit')?.value || 'principal';

  const btn = document.getElementById('btn-submit-new-share') || event.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/shares', {
      method: 'POST',
      body: JSON.stringify({
        name,
        comment,
        scheme,
        hidden,
        unit,
        groups: selectedGroups,
        write_group: writeGroup,
      }),
    });

    showToast(`Recurso compartido [${name}] creado exitosamente.`, 'success');
    closeModal('modal-new-share');
    document.getElementById('form-new-share').reset();
    loadShares();
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

async function deleteShare(name) {
  if (!window.confirm(`¿Confirmas la eliminación del recurso compartido [${name}]? (Los archivos en disco se conservarán)`)) {
    return;
  }

  try {
    await apiFetch('/api/shares/delete', {
      method: 'POST',
      body: JSON.stringify({ name }),
    });

    showToast(`Recurso [${name}] eliminado.`, 'success');
    loadShares();
  } catch (e) {
    // Ya mostrado
  }
}

function openSharePermissions(shareName) {
  switchView('users');
  setTimeout(() => {
    const permContainer = document.getElementById('perm-matrix-container') || document.querySelector('.perm-matrix-card');
    if (permContainer) {
      permContainer.scrollIntoView({ behavior: 'smooth' });
    }
  }, 100);
}

function repairSambaPermissions(shareName = null) {
  return repairAcls(shareName);
}

// ==============================================================================
// 6. Módulo: Central de Respaldos (Backups)
// ==============================================================================
let _backupRefreshTimer = null;

let _backupsData = [];

async function loadBackups() {
  const tbody = document.getElementById('backups-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/backups');
    const tasks = res.data || [];
    _backupsData = tasks;

    if (tasks.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" style="text-align:center;">No hay tareas de backup programadas actualmente.</td></tr>';
      scheduleBackupRefresh(false);
      return;
    }

    let anyRunning = false;
    tbody.innerHTML = tasks.map(t => {
      if (t.running) anyRunning = true;
      const statusBadge = t.last_status === 'OK' ? 'badge-ok'
        : (t.last_status === 'Error' ? 'badge-err'
          : (t.last_status === 'En curso' ? 'badge-blue' : 'badge-gray'));
      const pct = Math.max(0, Math.min(100, Number(t.percent || 0)));
      const fillColor = t.last_status === 'Error' ? 'var(--accent-danger)' : 'var(--accent-success)';

      return `
      <tr>
        <td><strong>${escapeHtml(t.id)}</strong></td>
        <td><span class="tag-pill">${escapeHtml(t.protocol)}</span></td>
        <td><code>${escapeHtml(t.source || t.destination || 'N/A')}</code></td>
        <td>${escapeHtml(t.cron_desc)}</td>
        <td>${t.retention} snapshots</td>
        <td>
          <span class="badge ${statusBadge}">${escapeHtml(t.last_status)}</span>
          <div class="progress-bar-wrap" style="margin-top:5px; height:6px;">
            <div class="progress-bar-fill" style="width:${pct}%; background:${fillColor};"></div>
          </div>
          <div style="font-size:11px; color:var(--text-muted); margin-top:3px;">
            ${pct}% · ${escapeHtml(t.elapsed || '00:00:00')} / ${escapeHtml(t.remaining || '--:--:--')}${t.speed ? ' · ' + escapeHtml(t.speed) : ''}
          </div>
        </td>
        <td style="text-align:right; white-space:nowrap;">
          ${t.running ? `
            <button class="btn btn-secondary btn-sm" title="Detener respaldo en curso" style="color:var(--accent-danger);" onclick="stopBackupTask('${escapeHtml(t.id)}')">
              <svg class="icon icon-sm" style="color:var(--accent-danger);"><use href="#icon-stop"></use></svg>
            </button>
          ` : `
            <button class="btn btn-secondary btn-sm" title="Ejecutar ahora" onclick="runBackupTask('${escapeHtml(t.id)}')">
              <svg class="icon icon-sm" style="color:var(--accent-success);"><use href="#icon-play"></use></svg>
            </button>
          `}
          <button class="btn btn-secondary btn-sm" title="Editar tarea" onclick="openEditBackup('${escapeHtml(t.id)}')">
            <svg class="icon icon-sm"><use href="#icon-edit"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Ver bitácora" onclick="viewBackupLogs('${escapeHtml(t.id)}')">
            <svg class="icon icon-sm"><use href="#icon-file"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Eliminar tarea" style="color:var(--accent-danger);" onclick="deleteBackupTask('${escapeHtml(t.id)}')">
            <svg class="icon icon-sm"><use href="#icon-trash"></use></svg>
          </button>
        </td>
      </tr>
    `;
    }).join('');

    scheduleBackupRefresh(anyRunning);
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger);">Error al cargar tareas de backup: ${escapeHtml(e.message)}</td></tr>`;
    scheduleBackupRefresh(false);
  }
}

function scheduleBackupRefresh(anyRunning) {
  if (_backupRefreshTimer) {
    clearTimeout(_backupRefreshTimer);
    _backupRefreshTimer = null;
  }
  if (anyRunning && AppState.activeView === 'backups') {
    _backupRefreshTimer = setTimeout(() => {
      if (AppState.activeView === 'backups') loadBackups();
    }, 4000);
  }
}

async function testBackupConnection() {
  const btn = document.getElementById('btn-test-backup');
  const payload = {
    proto: document.getElementById('bkp-proto').value,
    ip: document.getElementById('bkp-ip').value,
    share: document.getElementById('bkp-share').value,
    path: document.getElementById('bkp-path').value,
    port: 22,
    user: document.getElementById('bkp-user').value,
    password: document.getElementById('bkp-pass').value,
  };
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  try {
    const res = await apiFetch('/api/backups/test', { method: 'POST', body: JSON.stringify(payload) });
    showToast(res.message || 'Conexión de prueba correcta.', 'success');
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

function toggleBackupFields() {
  const proto = document.getElementById('bkp-proto').value;
  const shareFld = document.getElementById('field-bkp-share');
  const pathFld = document.getElementById('field-bkp-path');

  if (proto === 'cifs') {
    shareFld.style.display = 'block';
    pathFld.style.display = 'none';
  } else {
    shareFld.style.display = 'none';
    pathFld.style.display = 'block';
  }
}

async function submitNewBackup(event) {
  event.preventDefault();
  const id = document.getElementById('bkp-id').value;
  const proto = document.getElementById('bkp-proto').value;
  const ip = document.getElementById('bkp-ip').value;
  const share = document.getElementById('bkp-share').value;
  const path = document.getElementById('bkp-path').value;
  const user = document.getElementById('bkp-user').value;
  const password = document.getElementById('bkp-pass').value;
  const cron = document.getElementById('bkp-cron').value;
  const retention = parseInt(document.getElementById('bkp-retention').value, 10);
  const dest_unit = document.getElementById('bkp-dest-unit')?.value || 'principal';

  const btn = document.getElementById('btn-submit-new-backup') || event.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/backups', {
      method: 'POST',
      body: JSON.stringify({
        id,
        proto,
        ip,
        share,
        path,
        user,
        password,
        cron,
        retention,
        dest_unit,
      }),
    });

    showToast(`Tarea de respaldo [${id}] programada con éxito.`, 'success');
    closeModal('modal-new-backup');
    document.getElementById('form-new-backup').reset();
    loadBackups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

let _editBkpPort = 22;

function openEditBackup(id) {
  const t = (_backupsData || []).find(x => x.id === id);
  if (!t) {
    showToast(`No se encontró la tarea de respaldo [${id}].`, 'error');
    return;
  }

  const idEl = document.getElementById('edit-bkp-id');
  if (idEl) idEl.value = t.id;

  const protoStr = (t.protocol || '').toLowerCase();
  const isSsh = protoStr.includes('ssh') || protoStr.includes('linux');
  const proto = isSsh ? 'ssh' : 'cifs';
  const protoEl = document.getElementById('edit-bkp-proto');
  if (protoEl) protoEl.value = proto;

  let ip = t.ip || '';
  let share = t.share || '';
  let path = t.path || '';
  let user = t.user || (isSsh ? 'root' : 'Administrador');
  _editBkpPort = t.port || 22;

  const src = t.source || t.destination || '';
  if (!isSsh) {
    const m = src.match(/^[\/\\]+([^/\\]+)[\/\\]+(.+)$/);
    if (m) {
      if (!ip) ip = m[1];
      if (!share) share = m[2];
    }
  } else {
    const m = src.match(/^([^@]+)@([^:]+)(?::(\d+))?:(.*)$/);
    if (m) {
      if (!t.user) user = m[1];
      if (!ip) ip = m[2];
      if (m[3]) _editBkpPort = parseInt(m[3], 10);
      if (!path) path = m[4];
    }
  }

  const ipEl = document.getElementById('edit-bkp-ip');
  if (ipEl) ipEl.value = ip;
  const shareEl = document.getElementById('edit-bkp-share');
  if (shareEl) shareEl.value = share;
  const pathEl = document.getElementById('edit-bkp-path');
  if (pathEl) pathEl.value = path;
  const userEl = document.getElementById('edit-bkp-user');
  if (userEl) userEl.value = user;
  const passEl = document.getElementById('edit-bkp-pass');
  if (passEl) passEl.value = '';

  const cronEl = document.getElementById('edit-bkp-cron');
  if (cronEl && t.cron) {
    if (cronEl.querySelector(`option[value="${t.cron}"]`)) {
      cronEl.value = t.cron;
    } else {
      let customOpt = cronEl.querySelector('option[data-custom="1"]');
      if (!customOpt) {
        customOpt = document.createElement('option');
        customOpt.setAttribute('data-custom', '1');
        cronEl.appendChild(customOpt);
      }
      customOpt.value = t.cron;
      customOpt.textContent = `Personalizado (${t.cron})`;
      cronEl.value = t.cron;
    }
  }

  const retEl = document.getElementById('edit-bkp-retention');
  if (retEl) retEl.value = t.retention || 30;

  const unitEl = document.getElementById('edit-bkp-dest-unit');
  if (unitEl) {
    if (_storageUnitsData && _storageUnitsData.length > 0) {
      const curr = t.dest_unit || 'principal';
      unitEl.innerHTML = _storageUnitsData.map(u => {
        return `<option value="${escapeHtml(u.id)}" ${u.id === curr ? 'selected' : ''}>${escapeHtml(u.name)} (${escapeHtml(u.path)})${u.total ? ' · ' + escapeHtml(u.used) + ' / ' + escapeHtml(u.total) : ''}</option>`;
      }).join('');
    }
    unitEl.value = t.dest_unit || 'principal';
  }

  toggleEditBackupFields();
  openModal('modal-edit-backup');
}
window.editBackup = openEditBackup;
window.openEditBackup = openEditBackup;

function toggleEditBackupFields() {
  const proto = document.getElementById('edit-bkp-proto')?.value || 'cifs';
  const fShare = document.getElementById('field-edit-bkp-share');
  const fPath = document.getElementById('field-edit-bkp-path');
  if (fShare) fShare.style.display = (proto === 'cifs') ? 'block' : 'none';
  if (fPath) fPath.style.display = (proto === 'ssh') ? 'block' : 'none';
}

async function submitEditBackup(event) {
  event.preventDefault();
  const id = document.getElementById('edit-bkp-id').value;
  const proto = document.getElementById('edit-bkp-proto').value;
  const ip = document.getElementById('edit-bkp-ip').value;
  const share = document.getElementById('edit-bkp-share')?.value || '';
  const path = document.getElementById('edit-bkp-path')?.value || '';
  const user = document.getElementById('edit-bkp-user').value;
  const password = document.getElementById('edit-bkp-pass').value;
  const cron = document.getElementById('edit-bkp-cron').value;
  const retention = parseInt(document.getElementById('edit-bkp-retention').value, 10) || 30;
  const dest_unit = document.getElementById('edit-bkp-dest-unit')?.value || 'principal';

  const btn = document.getElementById('btn-submit-edit-backup') || event.target.querySelector('button[type="submit"]');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    const payload = {
      id,
      proto,
      ip,
      share,
      path,
      user,
      password,
      cron,
      retention,
      dest_unit,
    };
    if (proto === 'ssh' && _editBkpPort) {
      payload.port = _editBkpPort;
    }

    const res = await apiFetch('/api/backups/update', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
    showToast(res.message || `Tarea de respaldo [${id}] actualizada con éxito.`, 'success');
    closeModal('modal-edit-backup');
    loadBackups();
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

async function testEditBackupConnection() {
  const btn = document.getElementById('btn-test-edit-backup');
  const payload = {
    id: document.getElementById('edit-bkp-id').value,
    task_id: document.getElementById('edit-bkp-id').value,
    proto: document.getElementById('edit-bkp-proto').value,
    ip: document.getElementById('edit-bkp-ip').value,
    share: document.getElementById('edit-bkp-share')?.value || '',
    path: document.getElementById('edit-bkp-path')?.value || '',
    port: _editBkpPort || 22,
    user: document.getElementById('edit-bkp-user').value,
    password: document.getElementById('edit-bkp-pass').value,
  };
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  try {
    const res = await apiFetch('/api/backups/test', {
      method: 'POST',
      body: JSON.stringify(payload),
    });
    showToast(res.message || 'Conexión exitosa con el origen de respaldo.', 'success');
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function runBackupTask(taskId) {
  try {
    await apiFetch(`/api/backups/${encodeURIComponent(taskId)}/run`, { method: 'POST' });
    showToast(`Respaldo de [${taskId}] iniciado en segundo plano.`, 'info');
    setTimeout(loadBackups, 1500);
  } catch (e) {
    // Ya mostrado
  }
}

async function stopBackupTask(taskId) {
  if (!window.confirm(`¿Deseas detener la ejecución en curso del respaldo [${taskId}]?`)) {
    return;
  }
  try {
    const res = await apiFetch(`/api/backups/${encodeURIComponent(taskId)}/stop`, { method: 'POST' });
    showToast(res.message || `Proceso de respaldo [${taskId}] detenido.`, 'info');
    setTimeout(loadBackups, 1000);
  } catch (e) {
    // Ya mostrado por apiFetch
  }
}

async function viewBackupLogs(taskId) {
  const title = document.getElementById('modal-backup-logs-title');
  const content = document.getElementById('modal-backup-logs-content');
  if (title) title.textContent = `Bitácora de respaldo: ${taskId}`;
  if (content) content.textContent = 'Cargando registro...';

  openModal('modal-backup-logs');

  try {
    const res = await apiFetch(`/api/backups/${encodeURIComponent(taskId)}/logs`);
    if (content) content.textContent = res.data?.logs ?? res.logs ?? 'Sin contenido registrado aún.';
  } catch (e) {
    if (content) content.textContent = 'Error al leer la bitácora: ' + e.message;
  }
}

async function deleteBackupTask(taskId) {
  if (!window.confirm(`¿Deseas eliminar la tarea de backup [${taskId}]? Los snapshots históricos ya creados se preservarán.`)) {
    return;
  }

  try {
    await apiFetch('/api/backups/delete', {
      method: 'POST',
      body: JSON.stringify({ id: taskId }),
    });

    showToast(`Tarea [${taskId}] eliminada.`, 'success');
    loadBackups();
  } catch (e) {
    // Ya mostrado
  }
}

// ==============================================================================
// 7. Módulo: Almacenamiento & Mantenimiento
// ==============================================================================
let _scrubPollTimer = null;

async function loadStorage() {
  const tbody = document.getElementById('storage-disks-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/storage/disks');
    const disks = res.data?.disks ?? res.disks ?? [];
    const overview = res.data?.overview ?? res.overview ?? {};

    // Actualizar tarjetas de inteligencia BTRFS y TRIM
    updateStorageIntelligence(overview);

    if (disks.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" style="text-align:center;">No se detectaron discos de bloque adicionales.</td></tr>';
      return;
    }

    tbody.innerHTML = disks.map(d => {
      const devPath = d.device || ('/dev/' + d.name);
      return `
      <tr>
        <td><strong>${escapeHtml(d.name)}</strong>${d.protected ? ' <span class="badge badge-err">SO · protegido</span>' : (d.in_use ? ' <span class="badge badge-warn">En uso</span>' : ' <span class="badge badge-ok">Disponible</span>')}</td>
        <td>${escapeHtml(d.model)}</td>
        <td>${escapeHtml(d.size)}</td>
        <td>${escapeHtml(d.type)}</td>
        <td><code>${escapeHtml(d.mount || 'Sin montar')}</code></td>
        <td>${escapeHtml(d.fstype || 'N/A')}</td>
        <td style="text-align:right;">
          ${!d.protected ? `<button type="button" class="btn btn-secondary btn-sm" onclick="openStorageManage('${escapeHtml(devPath)}')"><svg class="icon icon-sm"><use href="#icon-wrench"></use></svg> Gestionar</button>` : '<span class="text-muted" style="font-size:11px;">Protegido</span>'}
        </td>
      </tr>
    `}).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger);">Error al cargar inventario: ${escapeHtml(e.message)}</td></tr>`;
  }
}

function updateStorageIntelligence(overview) {
  const scrubBadge = document.getElementById('stg-scrub-badge');
  const scrubInfo = document.getElementById('stg-scrub-info');
  const scrubBtn = document.getElementById('btn-scrub-storage');

  const trimBadge = document.getElementById('stg-trim-badge');
  const trimInfo = document.getElementById('stg-trim-info');
  const trimBtn = document.getElementById('btn-trim-storage');

  const isBtrfs = Boolean(overview?.is_btrfs);
  const supportsTrim = Boolean(overview?.supports_trim);
  const devType = overview?.device_type || 'HDD (Mecánico)';
  const fsType = (overview?.filesystem || 'ext4').toUpperCase();

  // 1. Diagnóstico BTRFS Scrub
  if (!isBtrfs) {
    if (scrubBadge) {
      scrubBadge.className = 'badge badge-warn';
      scrubBadge.textContent = 'No disponible (' + fsType + ')';
    }
    if (scrubInfo) {
      scrubInfo.innerHTML = '<span style="color:var(--text-secondary);">El volumen <code>' + escapeHtml(overview?.path || '/srv/nas') + '</code> está formateado en <strong>' + escapeHtml(fsType) + '</strong>. La verificación criptográfica contra Bit Rot requiere BTRFS.</span>';
    }
    if (scrubBtn) {
      scrubBtn.title = 'Requiere sistema de archivos BTRFS (actual: ' + fsType + ')';
    }
  } else {
    checkBtrfsScrubStatus(false);
  }

  // 2. Diagnóstico SSD TRIM
  if (!supportsTrim) {
    if (trimBadge) {
      trimBadge.className = 'badge badge-warn';
      trimBadge.textContent = 'Incompatible (' + devType + ')';
    }
    if (trimInfo) {
      trimInfo.innerHTML = '<span style="color:var(--text-secondary);">El volumen <code>' + escapeHtml(overview?.path || '/srv/nas') + '</code> reside en un <strong>' + escapeHtml(devType) + '</strong>. TRIM está reservado exclusivamente para unidades flash (SSD / NVMe) para evitar errores del controlador mecánico.</span>';
    }
    if (trimBtn) {
      trimBtn.title = 'TRIM deshabilitado: el almacenamiento reside en un ' + devType;
    }
  } else {
    if (trimBadge) {
      trimBadge.className = 'badge badge-ok';
      trimBadge.textContent = 'Compatible (SSD / NVMe)';
    }
    if (trimInfo) {
      trimInfo.innerHTML = '<span style="color:var(--accent-success-text);">Unidad de estado sólido SSD/NVMe detectada. Listo para descartar bloques de celdas flash eliminadas.</span>';
    }
    if (trimBtn) {
      trimBtn.title = 'Ejecutar orden discard fstrim para optimizar celdas flash';
    }
  }
}

/**
 * Renderiza el estado "En curso" en la tarjeta de Auditoría BTRFS Scrub.
 * Se reutiliza tanto en el sondeo periódico como en el refresco inmediato tras iniciar.
 */
function renderScrubRunning(rate = '', timeLeft = '') {
  const scrubBadge = document.getElementById('stg-scrub-badge');
  const scrubInfo = document.getElementById('stg-scrub-info');

  if (scrubBadge) {
    scrubBadge.className = 'badge badge-warn';
    scrubBadge.textContent = 'En curso';
  }

  if (scrubInfo) {
    scrubInfo.innerHTML = `
      <div style="display:flex; flex-direction:column; gap:6px; width:100%;">
        <div style="display:flex; justify-content:space-between; font-size:12px;">
          <strong>Verificando sumas criptográficas bloque a bloque...</strong>
          <span>${escapeHtml(rate || 'Analizando')}</span>
        </div>
        <div style="height:6px; background:var(--border-color); border-radius:3px; overflow:hidden;">
          <div style="height:100%; width:100%; background:var(--accent-primary); animation:pulse 1.5s infinite;"></div>
        </div>
        <span class="text-muted" style="font-size:11px;">${timeLeft ? 'Tiempo restante estimado: ' + escapeHtml(timeLeft) : 'Examinando bloques de datos y metadatos...'}</span>
      </div>
    `;
  }
}

async function checkBtrfsScrubStatus(isPolling = false) {
  try {
    const res = await apiFetch('/api/storage/scrub');
    const d = res.data || res;

    const isRunning = Boolean(d.running || d.status === 'running');

    if (isRunning) {
      renderScrubRunning(d.rate || '', d.time_left || '');
    } else {
      const scrubBadge = document.getElementById('stg-scrub-badge');
      const scrubInfo = document.getElementById('stg-scrub-info');

      if (scrubBadge) {
        scrubBadge.className = 'badge badge-ok';
        scrubBadge.textContent = d.status === 'finished' ? 'Completado' : 'Listo';
      }

      if (scrubInfo) {
        scrubInfo.innerHTML = `
          <div style="display:flex; justify-content:space-between; align-items:center; width:100%;">
            <span>${escapeHtml(d.summary || 'Auditoría no iniciada recientemente.')}</span>
            ${d.duration ? `<small class="text-muted" style="margin-left:8px;">Duración: ${escapeHtml(d.duration)}</small>` : ''}
          </div>
        `;
      }
    }

    return !isRunning;
  } catch (e) {
    return true;
  }
}

/**
 * Refresca la tarjeta de Auditoría BTRFS Scrub (badge + resultado) desde el servidor.
 * Si el scrub acaba de iniciarse pero el kernel todavía no lo reporta como "running",
 * conserva el estado "En curso" en lugar de sobrescribirlo con un resultado obsoleto.
 */
async function refreshScrubCard(started = false) {
  const finished = await checkBtrfsScrubStatus();
  if (started && finished) {
    renderScrubRunning('Iniciando...');
  }
  return finished;
}

function pollBtrfsScrubProgress() {
  if (_scrubPollTimer) clearInterval(_scrubPollTimer);

  checkBtrfsScrubStatus(true);
  _scrubPollTimer = setInterval(async () => {
    const isFinished = await checkBtrfsScrubStatus(true);
    if (isFinished) {
      clearInterval(_scrubPollTimer);
      _scrubPollTimer = null;
      showToast('Auditoría BTRFS scrub finalizada exitosamente.', 'success');
    }
  }, 3000);
}

async function runStorageScrub() {
  try {
    const stg = await apiFetch('/api/storage');
    const overview = stg.data?.overview || stg.overview || {};
    const isBtrfs = Boolean(overview.is_btrfs);
    const fsName = overview.filesystem || 'ext4';

    if (!isBtrfs) {
      showToast('Operación cancelada: el volumen está en ' + fsName.toUpperCase() + '. BTRFS Scrub requiere un filesystem Btrfs.', 'warning');
      return;
    }

    openModal('modal-audit-btrfs');
  } catch (e) {
    // Ya mostrado por apiFetch
  }
}

async function confirmAuditBtrfs() {
  closeModal('modal-audit-btrfs');
  const btn = document.getElementById('btn-scrub-storage');
  const confirmBtn = document.getElementById('btn-confirm-audit-btrfs');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  if (confirmBtn) { confirmBtn.disabled = true; }

  let started = false;
  try {
    const res = await apiFetch('/api/storage/scrub', { method: 'POST' });
    started = true;
    showToast(res.message || 'Auditoría BTRFS scrub iniciada.', 'info');
    pollBtrfsScrubProgress();
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
    if (confirmBtn) { confirmBtn.disabled = false; }
    // Refrescar la tarjeta de Auditoría BTRFS con el estado real del servidor
    await refreshScrubCard(started);
  }
}

async function runStorageTrim() {
  const btn = document.getElementById('btn-trim-storage');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    const stg = await apiFetch('/api/storage');
    const overview = stg.data?.overview || stg.overview || {};
    const supportsTrim = Boolean(overview.supports_trim);
    const devType = overview.device_type || 'HDD (Mecánico)';

    if (!supportsTrim) {
      showToast('TRIM no ejecutable: el almacenamiento reside en un ' + devType + '. Solo aplica a unidades SSD / NVMe.', 'warning');
      return;
    }

    const proceed = window.confirm(
      'Optimización TRIM para SSD (fstrim)\n\n' +
      'Esta operación envía comandos discard al controlador para liberar celdas de almacenamiento flash de bloques ya eliminados, manteniendo la velocidad sostenida y prolongando la vida útil del SSD.\n\n' +
      '¿Deseas ejecutar TRIM ahora?'
    );
    if (!proceed) return;

    const res = await apiFetch('/api/storage/trim', { method: 'POST' });
    const freed = res.data?.bytes_trimmed || res.message || 'bloques descartados';
    showToast('TRIM SSD completado: ' + freed, 'success');

    const trimInfo = document.getElementById('stg-trim-info');
    if (trimInfo) {
      trimInfo.innerHTML = '<span style="color:var(--accent-success-text); font-weight:500;">Último TRIM ejecutado: ' + escapeHtml(freed) + '</span>';
    }
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

let _currentDeviceInspection = null;

async function onStorageDeviceChange() {
  const sel = document.getElementById('stg-device');
  const inspectBox = document.getElementById('stg-inspect-box');
  const inspectDetails = document.getElementById('stg-inspect-details');
  const confirmLabel = document.getElementById('stg-confirm-label');
  const confirmInput = document.getElementById('stg-confirm');
  const confirmHint = document.getElementById('stg-confirm-hint');

  if (!sel || !sel.value) {
    if (inspectBox) inspectBox.style.display = 'none';
    _currentDeviceInspection = null;
    return;
  }

  const dev = sel.value;
  try {
    if (inspectBox) {
      inspectBox.style.display = 'block';
      if (inspectDetails) inspectDetails.innerHTML = '<span style="color:var(--text-muted);">Inspeccionando particiones y firmas del disco...</span>';
    }
    const res = await apiFetch('/api/storage/inspect', {
      method: 'POST',
      body: JSON.stringify({ device: dev }),
      silentToast: true,
    });
    const info = res.data || res;
    _currentDeviceInspection = info;

    let html = `<div>Dispositivo: <code>${escapeHtml(info.device || dev)}</code>`;
    if (info.type) html += ` · Tipo: <strong>${escapeHtml(info.type)}</strong>`;
    if (info.size) html += ` · Capacidad: <strong>${escapeHtml(info.size)}</strong>`;
    html += '</div>';

    if (info.has_data) {
      html += `<div style="color:var(--accent-warning); margin-top:4px; font-weight:600;">⚠️ ATENCIÓN: El dispositivo contiene particiones o firmas preexistentes.</div>`;
      if (info.signatures && info.signatures.length > 0) {
        html += `<div style="margin-top:2px;">Firmas detectadas: <code>${info.signatures.map(s => escapeHtml(s.type || s.label || s.uuid || '')).filter(Boolean).join(', ')}</code></div>`;
      }
      if (info.partitions && info.partitions.length > 0) {
        html += `<div style="margin-top:2px;">Particiones: ${info.partitions.map(p => `<code>${escapeHtml(p.name)} (${escapeHtml(p.size || '')})</code>`).join(', ')}</div>`;
      }
      html += `<div style="color:var(--accent-danger); margin-top:4px;">Se requiere escribir <strong>SI-DESTRUIR-DATOS</strong> para confirmar la eliminación irreversible.</div>`;
      if (confirmLabel) confirmLabel.innerHTML = 'Confirmación obligatoria (escribe <code>SI-DESTRUIR-DATOS</code>):';
      if (confirmInput) confirmInput.placeholder = 'SI-DESTRUIR-DATOS';
      if (confirmHint) confirmHint.textContent = 'Este disco contiene datos. Escribe SI-DESTRUIR-DATOS para confirmar.';
    } else {
      html += `<div style="color:var(--accent-success-text); margin-top:4px;">✓ Dispositivo limpio sin firmas ni particiones previas detectadas.</div>`;
      if (confirmLabel) confirmLabel.innerHTML = 'Confirmación (escribe <code>SI-FORMATEAR</code>):';
      if (confirmInput) confirmInput.placeholder = 'SI-FORMATEAR';
      if (confirmHint) confirmHint.textContent = 'Escribe SI-FORMATEAR para proceder.';
    }
    if (inspectDetails) inspectDetails.innerHTML = html;
  } catch (err) {
    if (inspectBox) inspectBox.style.display = 'block';
    if (inspectDetails) inspectDetails.innerHTML = `<span style="color:var(--accent-warning);">No se pudo inspeccionar el dispositivo (${escapeHtml(err.message || 'error')}). Se aplicará verificación estricta.</span>`;
  }
}

async function openStorageManage(preselectedDevice = null) {
  openModal('modal-storage-manage');
  const sel = document.getElementById('stg-device');
  if (sel) {
    sel.innerHTML = '<option value="">Cargando discos disponibles...</option>';
  }
  try {
    const res = await apiFetch('/api/storage/disks');
    const disks = res.data?.disks || res.disks || [];
    if (!sel) return;

    const usable = disks.filter(d => !d.protected);
    sel.innerHTML = usable.length
      ? usable.map(d => {
        const dev = d.device || ('/dev/' + d.name);
        const isSel = (preselectedDevice && (dev === preselectedDevice || d.name === preselectedDevice)) ? 'selected' : '';
        return `<option value="${escapeHtml(dev)}" ${isSel}>${escapeHtml(dev)} — ${escapeHtml(d.size)} ${escapeHtml(d.model || '')}${d.mount ? ' (montado ' + escapeHtml(d.mount) + ')' : ''}</option>`;
      }).join('')
      : '<option value="">(sin discos de datos disponibles)</option>';
    toggleStorageOp();
    await onStorageDeviceChange();
  } catch (e) {
    showToast(e.message || 'Error al obtener la lista de discos.', 'error');
  }
}

function openFormatDisk(dev) {
  openStorageManage(dev);
}

function toggleStorageOp() {
  const op = document.getElementById('stg-op').value;
  document.getElementById('stg-lvm-wrap').style.display = op === 'lvm' ? 'block' : 'none';
  document.getElementById('stg-subvol-wrap').style.display = op === 'subvolume' ? 'block' : 'none';
  const unitWrap = document.getElementById('stg-unit-wrap');
  if (unitWrap) unitWrap.style.display = op === 'add_unit' ? 'block' : 'none';
  document.getElementById('stg-fs-wrap').style.display = (op === 'subvolume' || op === 'extend') ? 'none' : 'block';
}

async function submitStorageManage(event) {
  event.preventDefault();
  const op = document.getElementById('stg-op').value;
  const device = document.getElementById('stg-device').value;
  const confirmText = (document.getElementById('stg-confirm').value || '').trim();
  const fstype = document.getElementById('stg-fstype').value;

  if (!device) {
    showToast('No hay ningún disco de datos seleccionado.', 'warning');
    return;
  }

  const hasData = Boolean(_currentDeviceInspection?.has_data);
  if (hasData) {
    if (confirmText !== 'SI-DESTRUIR-DATOS') {
      showToast('Este disco contiene datos preexistentes. Debes escribir estrictamente SI-DESTRUIR-DATOS para confirmar la eliminación.', 'warning');
      return;
    }
  } else {
    if (confirmText !== 'SI-FORMATEAR' && confirmText !== 'SI-DESTRUIR-DATOS') {
      showToast('Escribe SI-FORMATEAR para confirmar la operación.', 'warning');
      return;
    }
  }

  let url = '/api/storage/format';
  let payload = { device, fstype, confirm: confirmText };

  if (op === 'extend') {
    url = '/api/storage/extend-pool';
    payload = { device, confirm: confirmText };
  } else if (op === 'add_unit') {
    const unitId = (document.getElementById('stg-unit-id')?.value || '').trim();
    if (!unitId || !/^[a-zA-Z0-9_-]{1,32}$/.test(unitId)) {
      showToast('Ingresa un identificador válido para la unidad (1-32 caracteres alfanuméricos).', 'warning');
      return;
    }
    url = '/api/storage/add-unit';
    payload = { device, unit_id: unitId, fstype, confirm: confirmText };
  } else if (op === 'lvm') {
    url = '/api/storage/lvm';
    payload = {
      disk: device,
      vg: document.getElementById('stg-vg').value,
      lv: document.getElementById('stg-lv').value,
      size: document.getElementById('stg-size').value,
      fstype,
      confirm: confirmText,
    };
  } else if (op === 'subvolume') {
    url = '/api/storage/subvolume';
    payload = { device, subvolume: document.getElementById('stg-subvol').value, confirm: confirmText };
  }

  const btn = document.getElementById('btn-submit-storage');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  try {
    try {
      await apiFetch(url, { method: 'POST', body: JSON.stringify(payload), silentToast: true });
    } catch (err) {
      const msg = String(err.message || '');
      if (/montad|desmont/i.test(msg)) {
        const proceed = window.confirm('El dispositivo (o una de sus particiones, como /srv/nas) está montado.\n\n¿Deseas DESMONTARLO y continuar con la operación? Los datos del disco serán destruidos.');
        if (!proceed) {
          showToast('Operación cancelada por el usuario.', 'warning');
          return;
        }
        await apiFetch(url, { method: 'POST', body: JSON.stringify({ ...payload, unmount: true }), silentToast: true });
      } else {
        throw err;
      }
    }
    showToast('Operación de almacenamiento completada exitosamente.', 'success');
    closeModal('modal-storage-manage');
    document.getElementById('stg-confirm').value = '';
    loadStorage();
    loadStorageUnits();
  } catch (e) {
    showToast(e.message || 'No se pudo completar la operación.', 'error');
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

let _storageUnitsData = [];

async function loadStorageUnits() {
  try {
    const res = await apiFetch('/api/storage/units', { silentToast: true });
    const units = res.data?.units || res.units || [];
    _storageUnitsData = units;

    // Poblar selector en modal-new-share
    const shareUnitSel = document.getElementById('share-unit');
    if (shareUnitSel) {
      const curr = shareUnitSel.value || 'principal';
      shareUnitSel.innerHTML = units.map(u => {
        return `<option value="${escapeHtml(u.id)}" ${u.id === curr ? 'selected' : ''}>${escapeHtml(u.name)} (${escapeHtml(u.path)})${u.total ? ' · ' + escapeHtml(u.used) + ' / ' + escapeHtml(u.total) : ''}</option>`;
      }).join('');
    }

    // Poblar selectores en modal-new-backup y modal-edit-backup
    const bkpDestSel = document.getElementById('bkp-dest-unit');
    if (bkpDestSel) {
      const curr = bkpDestSel.value || 'principal';
      bkpDestSel.innerHTML = units.map(u => {
        return `<option value="${escapeHtml(u.id)}" ${u.id === curr ? 'selected' : ''}>${escapeHtml(u.name)} (${escapeHtml(u.path)})${u.total ? ' · ' + escapeHtml(u.used) + ' / ' + escapeHtml(u.total) : ''}</option>`;
      }).join('');
    }
    const editBkpDestSel = document.getElementById('edit-bkp-dest-unit');
    if (editBkpDestSel) {
      const curr = editBkpDestSel.value || 'principal';
      editBkpDestSel.innerHTML = units.map(u => {
        return `<option value="${escapeHtml(u.id)}" ${u.id === curr ? 'selected' : ''}>${escapeHtml(u.name)} (${escapeHtml(u.path)})${u.total ? ' · ' + escapeHtml(u.used) + ' / ' + escapeHtml(u.total) : ''}</option>`;
      }).join('');
    }

    // Poblar optgroup en Explorador de Archivos
    const filesUnitsOptgroup = document.getElementById('files-units-optgroup');
    if (filesUnitsOptgroup) {
      const additionalUnits = units.filter(u => u.id !== 'principal');
      if (additionalUnits.length > 0) {
        filesUnitsOptgroup.style.display = '';
        filesUnitsOptgroup.innerHTML = additionalUnits.map(u => {
          const rootKey = 'unidades/' + u.id;
          return `<option value="${escapeHtml(rootKey)}">Unidad: ${escapeHtml(u.name)} (${escapeHtml(u.path)})</option>`;
        }).join('');
      } else {
        filesUnitsOptgroup.style.display = 'none';
        filesUnitsOptgroup.innerHTML = '';
      }
    }
  } catch (e) {
    // Silencioso
  }
}

// ==============================================================================
// 8. Módulo: Usuarios & Grupos
// ==============================================================================
const PROTECTED_USERS = ['root', 'administrador', 'sistemas'];
let _usersData = [];
let _groupsData = [];
let _usersFilter = 'all';
let _activeGroup = '';
const _expandedUserGroups = new Set();

function toggleUserGroupsExpand(username) {
  if (_expandedUserGroups.has(username)) {
    _expandedUserGroups.delete(username);
  } else {
    _expandedUserGroups.add(username);
  }
  loadUsersAndGroups();
}

function renderUserGroupPills(username, groups) {
  const allGroups = (groups || []).filter(g => g.startsWith('grp_') || ['sudo', 'adm'].includes(g));
  if (!allGroups.length) {
    return '<span style="color:var(--text-muted); font-size:12px;"><em>Sin grupos</em></span>';
  }
  const isExpanded = _expandedUserGroups.has(username);
  if (allGroups.length <= 2 || isExpanded) {
    const pills = allGroups.map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ');
    if (isExpanded && allGroups.length > 2) {
      return pills + ` <span class="tag-pill tag-pill-more" title="Ver menos" onclick="toggleUserGroupsExpand('${escapeHtml(username)}')">« Menos</span>`;
    }
    return pills;
  }
  const visible = allGroups.slice(0, 2).map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ');
  const remaining = allGroups.length - 2;
  return `${visible} <span class="tag-pill tag-pill-more" title="Ver ${remaining} grupos más" onclick="toggleUserGroupsExpand('${escapeHtml(username)}')">+${remaining} más</span>`;
}

function selectRoleCard(formPrefix, role) {
  const containerId = formPrefix === 'user' ? 'new-user-role-cards' : 'edit-user-role-cards';
  const container = document.getElementById(containerId);
  if (container) {
    container.querySelectorAll('.role-card').forEach(card => {
      card.classList.toggle('active', card.dataset.role === role);
    });
  }

  const roleInput = document.getElementById(`${formPrefix}-role-val`);
  if (roleInput) {
    roleInput.value = role;
  }

  const isSuper = role === 'superadmin';
  const isAdmin = role === 'admin' || isSuper;
  const canWeb = role === 'operator' || isAdmin;

  const adminChk = document.getElementById(`${formPrefix}-is-admin`);
  if (adminChk) adminChk.checked = isAdmin;

  const webChk = document.getElementById(`${formPrefix}-can-web`);
  if (webChk) webChk.checked = canWeb;

  const superChk = document.getElementById(`${formPrefix}-is-superadmin`);
  if (superChk) superChk.checked = isSuper;

  updateEffectiveAccessPreview(formPrefix === 'edit-user');
}

function generateAndSetPassword(inputId) {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789!@#$%&*-_';
  let pass = '';
  const array = new Uint32Array(14);
  window.crypto.getRandomValues(array);
  for (let i = 0; i < 14; i++) {
    pass += chars[array[i] % chars.length];
  }
  const el = document.getElementById(inputId);
  if (el) {
    el.value = pass;
    el.type = 'text';
    const eyeIcon = document.getElementById(`${inputId}-eye`);
    if (eyeIcon) {
      eyeIcon.innerHTML = '<use href="#icon-eye-off"></use>';
    }
    showToast('Contraseña segura generada.', 'info');
  }
}

function togglePasswordVisibility(inputId) {
  const el = document.getElementById(inputId);
  const eyeIcon = document.getElementById(`${inputId}-eye`);
  if (!el) return;
  if (el.type === 'password') {
    el.type = 'text';
    if (eyeIcon) eyeIcon.innerHTML = '<use href="#icon-eye-off"></use>';
  } else {
    el.type = 'password';
    if (eyeIcon) eyeIcon.innerHTML = '<use href="#icon-eye"></use>';
  }
}

function copyUncPath(customVal = null) {
  const val = (customVal !== null && customVal !== undefined)
    ? String(customVal)
    : (document.getElementById('welcome-unc-path')?.value || '');
  if (!val) return;

  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(val).then(() => {
      showToast(`Ruta UNC copiada: ${val}`, 'success');
    }).catch(() => {
      const input = document.getElementById('welcome-unc-path');
      if (input) {
        input.select();
        document.execCommand('copy');
      }
      showToast(`Ruta UNC copiada: ${val}`, 'success');
    });
  } else {
    const input = document.getElementById('welcome-unc-path');
    if (input) {
      input.select();
      document.execCommand('copy');
    }
    showToast(`Ruta UNC: ${val}`, 'info');
  }
}

function copyShareUnc(shareName) {
  const host = window.NAS_HOSTNAME || 'SRV-NAS';
  copyUncPath(`\\\\${host}\\${shareName}`);
}

function filterGroupMembers() {
  const query = (document.getElementById('group-member-search')?.value || '').toLowerCase().trim();
  const rows = document.querySelectorAll('#group-members-list > div');
  rows.forEach(r => {
    const text = (r.getAttribute('data-username') || r.querySelector('.member-name')?.textContent || r.textContent).toLowerCase();
    r.style.display = (!query || text.includes(query)) ? 'flex' : 'none';
  });
}

async function updateEffectiveAccessPreview(isEdit) {
  const prefix = isEdit ? 'edit-user' : 'user';
  const previewBox = document.getElementById(`${prefix}-effective-access-preview`);
  if (!previewBox) return;

  const sambaEnabled = document.getElementById(`${prefix}-samba-enabled`)?.checked ?? true;
  if (!sambaEnabled) {
    previewBox.innerHTML = '<em style="color:var(--text-muted); font-size:12px;">Acceso Samba desactivado (sin acceso a recursos de red).</em>';
    return;
  }

  if (!_permData || !_permData.shares || !_permData.shares.length) {
    try {
      const res = await apiFetch('/api/shares/access');
      _permData = res.data || { shares: [], groups: [], users: [] };
    } catch (e) {
      previewBox.innerHTML = '<em style="color:var(--text-muted); font-size:12px;">No se pudo cargar la lista de recursos compartidos.</em>';
      return;
    }
  }

  const shares = _permData.shares || [];
  if (!shares.length) {
    previewBox.innerHTML = '<em style="color:var(--text-muted); font-size:12px;">No hay recursos compartidos configurados en el servidor.</em>';
    return;
  }

  const groupCheckboxes = document.querySelectorAll(`input[name="${isEdit ? 'edit_user_group' : 'user_group'}"]:checked`);
  const selectedGroups = Array.from(groupCheckboxes).map(cb => cb.value);
  const role = document.getElementById(`${prefix}-role-val`)?.value || 'standard';
  const isAdmin = role === 'admin' || role === 'superadmin';

  const groupRows = _permData.groups || [];
  let html = '';

  shares.forEach(share => {
    let label = 'Sin acceso';
    let badgeClass = 'badge-gray';

    if (isAdmin) {
      label = 'Lectura y escritura (Admin)';
      badgeClass = 'badge-ok';
    } else {
      let level = 'none';
      for (const gname of selectedGroups) {
        const grow = groupRows.find(r => r.name === gname);
        if (grow) {
          const perm = grow[share];
          if (perm === 'write') {
            level = 'write';
            break;
          } else if (perm === 'read' && level !== 'write') {
            level = 'read';
          }
        }
      }
      if (level === 'write') {
        label = 'Lectura y escritura';
        badgeClass = 'badge-ok';
      } else if (level === 'read') {
        label = 'Solo lectura';
        badgeClass = 'badge-blue';
      }
    }

    html += `
      <div class="access-preview-item">
        <span class="share-name"><svg class="icon icon-sm"><use href="#icon-folder"></use></svg> [${escapeHtml(share)}]</span>
        <span class="badge ${badgeClass}">${label}</span>
      </div>`;
  });

  previewBox.innerHTML = html;
}

async function loadUsersAndGroups() {
  const usersTbody = document.getElementById('users-table-body');
  const groupsTbody = document.getElementById('groups-table-body');

  try {
    const [uRes, gRes] = await Promise.all([
      apiFetch('/api/users'),
      apiFetch('/api/groups'),
    ]);

    _usersData = uRes.data || [];
    _groupsData = gRes.data || [];

    if (usersTbody) {
      const isAdminCaller = window.NAS_IS_ADMIN !== false;
      usersTbody.innerHTML = _usersData.map(u => {
        const isProtected = PROTECTED_USERS.includes(u.username) || !!u.is_superadmin;
        const rol = u.is_superadmin
          ? '<span class="badge badge-err">Superadmin</span>'
          : (u.is_admin
            ? '<span class="badge badge-warn">Admin</span>'
            : (u.can_web ? '<span class="badge badge-blue">Operador</span>' : '<span class="badge badge-gray">Estándar</span>'));
        const netEnabled = !!(u.samba_enabled ?? u.enabled);
        const canManage = !isProtected && (isAdminCaller || !u.is_admin);
        return `
        <tr data-username="${escapeHtml(u.username)}" data-admin="${u.is_admin ? '1' : '0'}" data-enabled="${netEnabled ? '1' : '0'}" data-samba="${u.is_samba ? '1' : '0'}">
          <td><strong>${escapeHtml(u.username)}</strong><div style="font-size:11px; color:var(--text-muted);">${escapeHtml(u.full_name || '')}</div></td>
          <td>${u.uid}</td>
          <td>${rol}</td>
          <td>${renderUserGroupPills(u.username, u.groups)}</td>
          <td>${netEnabled ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Suspendido</span>'}</td>
          <td>${u.is_samba ? '<span class="badge badge-ok">Sync</span>' : '<span class="badge badge-err">Sin SMB</span>'}</td>
          <td style="text-align:right; white-space:nowrap;">
            ${canManage ? `
              <button class="btn btn-secondary btn-sm" title="Editar" onclick="openEditUser('${escapeHtml(u.username)}')"><svg class="icon icon-sm"><use href="#icon-edit"></use></svg></button>
              <button class="btn btn-secondary btn-sm" title="${netEnabled ? 'Suspender acceso a red' : 'Reactivar acceso a red'}" onclick="toggleUser('${escapeHtml(u.username)}', ${netEnabled ? 'false' : 'true'})"><svg class="icon icon-sm"><use href="#icon-${netEnabled ? 'lock' : 'unlock'}"></use></svg></button>
              <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteUser('${escapeHtml(u.username)}')"><svg class="icon icon-sm"><use href="#icon-trash"></use></svg></button>
            ` : `<span class="badge badge-gray">${isProtected ? (u.is_superadmin ? 'Inmutable' : 'Protegida') : 'Solo lectura'}</span>`}
          </td>
        </tr>`;
      }).join('') || '<tr><td colspan="7" style="text-align:center;">Sin usuarios encontrados.</td></tr>';
      applyUsersFilters();
    }

    if (groupsTbody) {
      const isSuperCaller = !!window.NAS_IS_SUPER;
      const isAdminCaller = window.NAS_IS_ADMIN !== false;

      groupsTbody.innerHTML = _groupsData.map(g => {
        const isSpecial = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
        let canManageMembers = false;
        if (g.name === 'grp_superadmin') {
          canManageMembers = isSuperCaller;
        } else if (isSpecial) {
          canManageMembers = isAdminCaller;
        } else {
          canManageMembers = isAdminCaller;
        }

        return `
        <tr>
          <td><strong>${escapeHtml(g.name)}</strong></td>
          <td>${g.gid}</td>
          <td>${(g.members || []).map(m => `<span class="tag-pill">${escapeHtml(m)}</span>`).join(' ') || '<em>Sin miembros</em>'}</td>
          <td style="text-align:right; white-space:nowrap;">
            ${isSpecial ? '<span class="badge badge-gray" style="margin-right:6px;">Especial</span>' : ''}
            ${canManageMembers ? `
              <button class="btn btn-secondary btn-sm" title="Miembros" onclick="openGroupMembers('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-users"></use></svg></button>
            ` : ''}
            ${!isSpecial && isAdminCaller ? `
              <button class="btn btn-secondary btn-sm" title="Renombrar" onclick="openRenameGroup('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-edit"></use></svg></button>
              <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteGroup('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-trash"></use></svg></button>
            ` : ''}
            ${!canManageMembers && (!isAdminCaller || isSpecial) && !isSpecial ? '<span class="badge badge-gray">Solo lectura</span>' : ''}
          </td>
        </tr>
      `;
      }).join('') || '<tr><td colspan="4" style="text-align:center;">Sin grupos creados.</td></tr>';
    }

    populateUserGroupOptions(_groupsData);
  } catch (e) {
    console.error('Error cargando usuarios y grupos:', e);
  }
}

function populateUserGroupOptions(groups) {
  const container = document.getElementById('user-groups-list');
  if (!container) return;

  const list = groups || [];
  if (!list.length) {
    container.innerHTML = '<em style="color:var(--text-muted);">No hay grupos disponibles. ' +
      '<a href="#" onclick="event.preventDefault(); closeModal(\'modal-new-user\'); openModal(\'modal-new-group\');">Crear un grupo</a>.</em>';
    return;
  }

  container.innerHTML = list.map(g => {
    const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
    return `<label style="display:flex; align-items:center; gap:8px; ${special ? 'opacity:0.65;' : ''}">
      <input type="checkbox" name="user_group" value="${escapeHtml(g.name)}" ${special ? 'disabled' : ''} onchange="updateEffectiveAccessPreview(false)">
      <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
    </label>`;
  }).join('');
}

function setUsersFilter(filter) {
  _usersFilter = filter;
  document.querySelectorAll('#users-filter-chips .chip-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.filter === filter);
  });
  applyUsersFilters();
}

function applyUsersFilters() {
  const q = (document.getElementById('users-search')?.value || '').toLowerCase().trim();
  document.querySelectorAll('#users-table-body tr[data-username]').forEach(tr => {
    let ok = true;
    if (_usersFilter === 'admin') ok = tr.dataset.admin === '1';
    else if (_usersFilter === 'blocked') ok = tr.dataset.enabled !== '1';
    else if (_usersFilter === 'samba') ok = tr.dataset.samba === '1';
    if (ok && q) ok = tr.dataset.username.includes(q);
    tr.style.display = ok ? '' : 'none';
  });
}

function openEditUser(username) {
  const u = _usersData.find(x => x.username === username);
  if (!u) return;
  document.getElementById('edit-user-uname').value = username;
  document.getElementById('edit-user-fullname').value = u.full_name || '';
  document.getElementById('edit-user-pass').value = '';
  document.getElementById('edit-user-samba-enabled').checked = !!(u.samba_enabled ?? u.enabled);

  // Determinar rol
  let userRole = 'standard';
  if (u.is_superadmin) {
    userRole = 'superadmin';
  } else if (u.is_admin) {
    userRole = 'admin';
  } else if (u.can_web) {
    userRole = 'operator';
  }
  selectRoleCard('edit-user', userRole);

  const editCards = document.getElementById('edit-user-role-cards');
  if (editCards) {
    editCards.querySelectorAll('.role-card').forEach(card => {
      if (u.is_superadmin) {
        card.classList.toggle('disabled', card.dataset.role !== 'superadmin');
      } else {
        card.classList.remove('disabled');
      }
    });
  }

  const checked = (u.groups || []).filter(g => g.startsWith('grp_'));
  const container = document.getElementById('edit-user-groups-list');
  container.innerHTML = _groupsData.map(g => {
    const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
    const sel = (!special && checked.includes(g.name)) ? 'checked' : '';
    return `<label style="display:flex; align-items:center; gap:8px; ${special ? 'opacity:0.65;' : ''}">
      <input type="checkbox" name="edit_user_group" value="${escapeHtml(g.name)}" ${sel} ${special ? 'disabled' : ''} onchange="updateEffectiveAccessPreview(true)">
      <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
    </label>`;
  }).join('');

  updateEffectiveAccessPreview(true);
  openModal('modal-edit-user');
}

async function submitUpdateUser(event) {
  event.preventDefault();
  const username = document.getElementById('edit-user-uname').value;
  const full_name = document.getElementById('edit-user-fullname').value;
  const password = document.getElementById('edit-user-pass').value;
  const role = document.getElementById('edit-user-role-val')?.value || 'standard';
  const isAdmin = role === 'admin' || role === 'superadmin';
  const canWeb = role === 'operator' || isAdmin;
  const isSuperadmin = role === 'superadmin';
  const sambaEnabled = document.getElementById('edit-user-samba-enabled').checked;
  const groups = Array.from(document.querySelectorAll('input[name="edit_user_group"]:checked')).map(cb => cb.value);

  const btn = document.getElementById('btn-submit-edit-user') || event.target.querySelector('button[type="submit"]');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    await apiFetch('/api/users/update', {
      method: 'POST',
      body: JSON.stringify({ username, full_name, password, groups, is_admin: isAdmin, can_web: canWeb, samba_enabled: sambaEnabled, is_superadmin: isSuperadmin }),
    });
    showToast(`Usuario [${username}] actualizado.`, 'success');
    closeModal('modal-edit-user');
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function toggleUser(username, enabled) {
  const verb = enabled ? 'Desbloquear' : 'Bloquear';
  if (!window.confirm(`¿${verb} la cuenta [${username}]?`)) return;

  try {
    await apiFetch('/api/users/toggle', {
      method: 'POST',
      body: JSON.stringify({ username, enabled }),
    });
    showToast(`Cuenta [${username}] ${enabled ? 'desbloqueada' : 'bloqueada'}.`, 'success');
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  }
}

function openGroupMembers(group) {
  _activeGroup = group;
  document.getElementById('modal-group-members-title').textContent = 'Miembros del grupo ' + group;
  const searchInput = document.getElementById('group-member-search');
  if (searchInput) searchInput.value = '';

  const g = _groupsData.find(x => x.name === group);
  const members = g ? (g.members || []) : [];
  const list = document.getElementById('group-members-list');
  list.innerHTML = members.length
    ? members.map(m => `
        <div style="display:flex; align-items:center; justify-content:space-between; gap:8px; padding:4px 6px; background:var(--bg-surface); border-radius:6px;" data-username="${escapeHtml(m)}">
          <span class="member-name">${escapeHtml(m)}</span>
          <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="removeGroupMember('${escapeHtml(m)}')">Quitar</button>
        </div>`).join('')
    : '<em style="color:var(--text-muted);">Sin miembros</em>';

  const sel = document.getElementById('group-member-user');
  const available = _usersData.filter(u => !members.includes(u.username) && u.username !== 'root');
  sel.innerHTML = available.length
    ? available.map(u => `<option value="${escapeHtml(u.username)}">${escapeHtml(u.username)}</option>`).join('')
    : '<option value="">(sin usuarios disponibles)</option>';

  openModal('modal-group-members');
}

async function addGroupMember() {
  const sel = document.getElementById('group-member-user');
  const username = sel.value;
  if (!username) return;
  try {
    await apiFetch('/api/users/groups', {
      method: 'POST',
      body: JSON.stringify({ username, group: _activeGroup, action: 'add' }),
    });
    showToast(`${username} añadido a ${_activeGroup}.`, 'success');
    loadUsersAndGroups();
    openGroupMembers(_activeGroup);
  } catch (e) {
    // Ya mostrado
  }
}

async function removeGroupMember(username) {
  if (!window.confirm(`¿Quitar a [${username}] del grupo ${_activeGroup}?`)) return;
  try {
    await apiFetch('/api/users/groups', {
      method: 'POST',
      body: JSON.stringify({ username, group: _activeGroup, action: 'remove' }),
    });
    showToast(`${username} quitado de ${_activeGroup}.`, 'success');
    loadUsersAndGroups();
    openGroupMembers(_activeGroup);
  } catch (e) {
    // Ya mostrado
  }
}

function openRenameGroup(name) {
  document.getElementById('rename-group-old').value = name;
  document.getElementById('rename-group-new').value = '';
  openModal('modal-rename-group');
}

async function submitRenameGroup(event) {
  event.preventDefault();
  const oldName = document.getElementById('rename-group-old').value;
  const newName = document.getElementById('rename-group-new').value;

  const btn = document.getElementById('btn-submit-rename-group') || event.target.querySelector('button[type="submit"]');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    await apiFetch('/api/groups/rename', {
      method: 'POST',
      body: JSON.stringify({ old: oldName, new: newName }),
    });
    showToast(`Grupo renombrado a ${newName}.`, 'success');
    closeModal('modal-rename-group');
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function openNewUser() {
  if (!_groupsData.length) {
    try {
      const res = await apiFetch('/api/groups');
      _groupsData = res.data || [];
    } catch (e) {
      // Sin grupos disponibles
    }
  }
  const form = document.getElementById('form-new-user');
  if (form) form.reset();
  selectRoleCard('user', 'standard');
  populateUserGroupOptions(_groupsData);
  updateEffectiveAccessPreview(false);
  openModal('modal-new-user');
}

// ==============================================================================
// Matriz de permisos de acceso (grupos × recursos y usuarios × recursos)
// ==============================================================================
let _permData = { shares: [], groups: [], users: [] };
let _permTab = 'groups';

function setPermTab(tab) {
  _permTab = tab;
  document.querySelectorAll('#perm-filter-chips .chip-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.perm === tab);
  });
  renderPermMatrix();
}

async function loadAccessMatrix() {
  const container = document.getElementById('perm-matrix-container');
  if (!container) return;
  try {
    const res = await apiFetch('/api/shares/access');
    _permData = res.data || { shares: [], groups: [], users: [] };
    renderPermMatrix();
  } catch (e) {
    container.innerHTML = `<p style="padding:18px; color:var(--accent-danger);">Error al cargar la matriz de permisos: ${escapeHtml(e.message)}</p>`;
  }
}

function permBadge(level) {
  if (level === 'write') return '<span class="badge badge-ok">Lectura y escritura</span>';
  if (level === 'read') return '<span class="badge badge-blue">Solo lectura</span>';
  return '<span class="badge badge-gray">Sin acceso</span>';
}

function renderPermMatrix() {
  const container = document.getElementById('perm-matrix-container');
  if (!container) return;

  const shares = _permData.shares || [];
  const rows = _permTab === 'groups' ? (_permData.groups || []) : (_permData.users || []);
  if (!shares.length || !rows.length) {
    container.innerHTML = '<p style="padding:18px; color:var(--text-muted);">No hay recursos o entidades para mostrar.</p>';
    return;
  }

  const kind = _permTab === 'groups' ? 'group' : 'user';
  let html = '<table class="nas-table"><thead><tr><th>Entidad</th>';
  shares.forEach(s => { html += `<th>${escapeHtml(s)}</th>`; });
  html += '</tr></thead><tbody>';
  rows.forEach(r => {
    html += `<tr><td><strong>${escapeHtml(r.name)}</strong>${r.is_admin ? ' <span class="badge badge-warn">Admin</span>' : ''}</td>`;
    shares.forEach(s => {
      const level = r[s] || 'none';
      html += `<td><button type="button" class="chip-btn" style="width:100%;" onclick="cyclePerm('${kind}','${escapeHtml(r.name)}','${escapeHtml(s)}')">${permBadge(level)}</button></td>`;
    });
    html += '</tr>';
  });
  html += '</tbody></table>';
  container.innerHTML = html;
}

async function cyclePerm(kind, name, share) {
  const order = ['none', 'read', 'write'];
  const row = (_permTab === 'groups' ? _permData.groups : _permData.users).find(r => r.name === name);
  const current = row ? (row[share] || 'none') : 'none';
  const next = order[(order.indexOf(current) + 1) % order.length];

  try {
    await apiFetch('/api/shares/access', {
      method: 'POST',
      body: JSON.stringify({ share, kind, name, level: next }),
    });
    showToast(`Permiso de ${name} en ${share}: ${next}.`, 'success');
    loadAccessMatrix();
  } catch (e) {
    // Ya mostrado
  }
}

async function repairAcls(shareName = null) {
  try {
    showToast('Auditando consistencia de ACL contra smb.conf...', 'info');
    const auditUrl = '/api/shares/access/audit' + (shareName ? `?share=${encodeURIComponent(shareName)}` : '');
    const auditRes = await apiFetch(auditUrl);
    const auditData = auditRes.data || auditRes;

    const outOfSync = auditData.out_of_sync_count ?? 0;
    const total = auditData.total ?? (auditData.shares ? Object.keys(auditData.shares).length : 0);
    const sharesMap = auditData.shares || {};

    let force = false;

    if (shareName) {
      const sAudit = sharesMap[shareName] || auditData;
      if (sAudit && sAudit.in_sync) {
        if (!window.confirm(`El recurso [${shareName}] ya tiene sus ACL perfectamente sincronizadas con smb.conf.\n\n¿Deseas forzar la reaplicación recursiva (setfacl -R) de todos modos?`)) {
          return;
        }
        force = true;
      } else {
        const discList = ((sAudit && sAudit.discrepancies) || []).map(d => ` • ${d}`).join('\n');
        if (!window.confirm(`Se detectaron desajustes en el recurso [${shareName}]:\n${discList || 'Discrepancias en permisos'}\n\n¿Proceder con la reparación de ACL?`)) {
          return;
        }
      }
    } else {
      if (outOfSync === 0) {
        if (!window.confirm(`Todas las ACL de los ${total} recurso(s) están sincronizadas con smb.conf (cero discrepancias).\n\n¿Deseas forzar la reaplicación recursiva en todos los recursos de todos modos?`)) {
          return;
        }
        force = true;
      } else {
        const outList = Object.entries(sharesMap)
          .filter(([_, v]) => !v.in_sync)
          .map(([k, v]) => ` • [${k}]: ${(v.discrepancies || []).length} discrepancia(s)`)
          .join('\n');
        if (!window.confirm(`Auditoría de ACL: ${outOfSync} de ${total} recurso(s) requieren reparación:\n${outList}\n\n¿Proceder con la reparación inteligente (solo recursos con desajustes)?`)) {
          return;
        }
      }
    }

    const payload = { share: shareName || '', force };
    const res = await apiFetch('/api/shares/access/repair', { method: 'POST', body: JSON.stringify(payload) });
    showToast(res.message || 'ACL procesadas correctamente.', 'success');
    loadAccessMatrix();
  } catch (e) {
    // Ya mostrado por apiFetch
  }
}

async function submitNewUser(event) {
  event.preventDefault();
  const username = document.getElementById('user-uname').value;
  const full_name = document.getElementById('user-fullname').value;
  const password = document.getElementById('user-pass').value;
  const role = document.getElementById('user-role-val')?.value || 'standard';
  const isAdmin = role === 'admin' || role === 'superadmin';
  const canWeb = role === 'operator' || isAdmin;
  const isSuperadmin = role === 'superadmin';
  const sambaEnabled = document.getElementById('user-samba-enabled').checked;

  const selectedGroups = Array.from(document.querySelectorAll('input[name="user_group"]:checked'))
    .map(cb => cb.value);

  const btn = document.getElementById('btn-submit-new-user') || event.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/users', {
      method: 'POST',
      body: JSON.stringify({
        username,
        full_name,
        password,
        groups: selectedGroups,
        is_admin: isAdmin,
        can_web: canWeb,
        samba_enabled: sambaEnabled,
        is_superadmin: isSuperadmin,
      }),
    });

    showToast(`Usuario [${username}] creado y sincronizado en Samba.`, 'success');
    closeModal('modal-new-user');
    document.getElementById('form-new-user').reset();

    // Mostrar modal de bienvenida con ruta UNC
    const serverHost = window.location.hostname || 'SRV-NAS';
    const uncInput = document.getElementById('welcome-unc-path');
    if (uncInput) {
      uncInput.value = `\\\\${serverHost}`;
    }
    const infoEl = document.getElementById('welcome-user-info');
    if (infoEl) {
      const roleBadge = isSuperadmin
        ? '<span class="badge badge-err">Superadministrador</span>'
        : (isAdmin
          ? '<span class="badge badge-warn">Administrador</span>'
          : (canWeb ? '<span class="badge badge-blue">Operador Web</span>' : '<span class="badge badge-gray">Empleado de Red</span>'));
      infoEl.innerHTML = `
        <div style="margin-bottom:4px;"><strong>Usuario:</strong> ${escapeHtml(username)}</div>
        <div style="margin-bottom:4px;"><strong>Nombre:</strong> ${escapeHtml(full_name || 'N/A')}</div>
        <div><strong>Rol asignado:</strong> ${roleBadge}</div>
      `;
    }
    openModal('modal-user-welcome');

    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

async function deleteUser(username) {
  if (!window.confirm(`¿Confirmas la eliminación del usuario [${username}] y su directorio home?`)) {
    return;
  }

  try {
    await apiFetch('/api/users/delete', {
      method: 'POST',
      body: JSON.stringify({ username }),
    });

    showToast(`Usuario [${username}] eliminado.`, 'success');
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  }
}

async function submitNewGroup(event) {
  event.preventDefault();
  const groupName = document.getElementById('group-name').value;

  const btn = document.getElementById('btn-submit-new-group') || event.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/groups', {
      method: 'POST',
      body: JSON.stringify({ name: groupName }),
    });

    showToast('Grupo creado correctamente.', 'success');
    closeModal('modal-new-group');
    document.getElementById('form-new-group').reset();
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

async function deleteGroup(name) {
  if (!window.confirm(`¿Confirmas la eliminación del grupo corporativo [${name}]?`)) {
    return;
  }

  try {
    await apiFetch('/api/groups/delete', {
      method: 'POST',
      body: JSON.stringify({ name }),
    });

    showToast(`Grupo [${name}] eliminado.`, 'success');
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  }
}

// ==============================================================================
// 9. Módulo: Servicios y Demonios
// ==============================================================================
async function loadServices() {
  const tbody = document.getElementById('services-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/services');
    const services = res.data || [];

    tbody.innerHTML = services.map(s => `
      <tr>
        <td><strong><code>${escapeHtml(s.service)}</code></strong></td>
        <td>${escapeHtml(s.name)}</td>
        <td>${s.active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-danger">Inactivo</span>'}</td>
        <td style="text-align:right; white-space:nowrap;">
          ${s.active ? `
            <button class="btn btn-secondary btn-sm" onclick="reloadService('${escapeHtml(s.service)}')" title="Recargar configuración">
              <svg class="icon icon-sm"><use href="#icon-refresh"></use></svg> Recargar
            </button>
            <button class="btn btn-secondary btn-sm" onclick="restartService('${escapeHtml(s.service)}')" title="Reiniciar demonio">
              <svg class="icon icon-sm"><use href="#icon-refresh"></use></svg> Reiniciar
            </button>
            <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="stopService('${escapeHtml(s.service)}')" title="Detener demonio">
              <svg class="icon icon-sm"><use href="#icon-stop"></use></svg> Detener
            </button>
          ` : `
            <button class="btn btn-primary btn-sm" onclick="startService('${escapeHtml(s.service)}')" title="Iniciar demonio">
              <svg class="icon icon-sm"><use href="#icon-play"></use></svg> Iniciar
            </button>
          `}
        </td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="4" style="text-align:center; color:var(--accent-danger);">Error al cargar servicios: ${escapeHtml(e.message)}</td></tr>`;
  }
}

async function manageServiceAction(service, action) {
  try {
    await apiFetch('/api/services/manage', {
      method: 'POST',
      body: JSON.stringify({ service, action }),
    });

    const labels = {
      start: 'iniciado',
      stop: 'detenido',
      restart: 'reiniciado',
      reload: 'recargado',
    };
    showToast(`Servicio [${service}] ${labels[action] || action} con éxito.`, 'success');
    loadServices();
  } catch (e) {
    // Ya mostrado por apiFetch
  }
}

async function startService(service) { return manageServiceAction(service, 'start'); }
async function stopService(service) { return manageServiceAction(service, 'stop'); }
async function reloadService(service) { return manageServiceAction(service, 'reload'); }
async function restartService(service) { return manageServiceAction(service, 'restart'); }

async function runDiagnosticTest(type) {
  const resultEl = document.getElementById('diag-test-result');
  if (resultEl) {
    resultEl.innerHTML = `<span style="color:var(--text-muted);"><svg class="icon icon-sm spin"><use href="#icon-refresh"></use></svg> Ejecutando prueba ${escapeHtml(type)}...</span>`;
  }
  showToast(`Ejecutando prueba ${type}...`, 'info');

  try {
    const res = await apiFetch('/api/diagnostics/run', {
      method: 'POST',
      body: JSON.stringify({ type }),
    });

    if (resultEl) {
      const isOk = res.status === 'success';
      const color = isOk ? 'var(--accent-success)' : 'var(--accent-danger)';
      resultEl.innerHTML = `<span style="color:${color}; font-weight:600;">[${isOk ? 'OK' : 'FALLO'}] ${escapeHtml(res.message || 'Prueba completada')}</span>`;
    }
    showToast(res.message || 'Prueba de diagnóstico completada.', 'success');
  } catch (e) {
    if (resultEl) {
      resultEl.innerHTML = `<span style="color:var(--accent-danger); font-weight:600;">[ERROR] ${escapeHtml(e.message)}</span>`;
    }
    showToast(`Error: ${e.message}`, 'error');
  }
}

// ==============================================================================
// 10. Módulo: Registros del Sistema (Logs & Auditoría)
// ==============================================================================
function setLogSource(source) {
  AppState.logSource = source;
  document.querySelectorAll('#logs-filter-chips .chip-btn').forEach(btn => {
    btn.classList.toggle('active', btn.getAttribute('data-source') === source);
  });
  loadLogs();
}

function debounceLogSearch() {
  if (AppState.logSearchTimer) {
    clearTimeout(AppState.logSearchTimer);
  }
  AppState.logSearchTimer = setTimeout(() => {
    loadLogs();
  }, 300);
}

async function loadLogs() {
  const tbody = document.getElementById('logs-table-body');
  if (!tbody) return;

  const source = AppState.logSource || 'all';
  const limitSelect = document.getElementById('logs-limit-select');
  const limit = limitSelect ? parseInt(limitSelect.value, 10) || 100 : 100;
  const searchInput = document.getElementById('logs-search-input');
  const query = searchInput ? searchInput.value.trim() : '';
  const groupChk = document.getElementById('chk-group-logs');
  const grouped = groupChk ? (groupChk.checked ? 1 : 0) : 1;

  try {
    let url = `/api/logs?source=${encodeURIComponent(source)}&limit=${limit}&grouped=${grouped}`;
    if (query) {
      url += `&q=${encodeURIComponent(query)}`;
    }

    const res = await apiFetch(url);
    const logs = res.data || [];
    AppState.logsCache = logs;

    if (logs.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" style="text-align:center; padding:24px; color:var(--text-muted);">No se encontraron registros de auditoría para los criterios seleccionados.</td></tr>';
      return;
    }

    tbody.innerHTML = logs.map(l => {
      const srcBadge = matchSourceBadge(l.source);
      const userText = l.user || l.task || 'sistema';
      const ipText = l.ip ? `<span class="table-logs-ip"><code>${escapeHtml(l.ip)}</code></span>` : '';
      const actionLabel = l.action_label || l.event_label || l.action || l.event || l.unit || 'registro';
      const badgeType = l.badge || 'blue';
      const targetText = l.target || l.task || l.unit || '-';
      const status = (l.status || 'OK').toUpperCase();
      const statusBadge = (status === 'SUCCESS' || status === 'OK' || status === 'ALLOW') ? 'badge-ok' : ((status === 'FAILED' || status === 'ERR' || status === 'BLOCK') ? 'badge-err' : 'badge-warn');
      const isSystem = l.source === 'system';
      const countBadge = (l.count && l.count > 1)
        ? `<span class="badge badge-antispam" title="Evento idéntico repetido ${l.count} veces">x${l.count}</span>`
        : '';
      const details = (isSystem && l.human)
        ? l.human
        : (l.message || (l.details ? (typeof l.details === 'object' ? JSON.stringify(l.details) : l.details) : l.raw || '-'));

      return `
        <tr>
          <td class="table-logs-timestamp"><code>${escapeHtml(l.timestamp || '-')}</code></td>
          <td>${srcBadge}</td>
          <td><span class="table-logs-user">${escapeHtml(userText)}</span>${ipText}</td>
          <td><span class="badge badge-${badgeType}">${escapeHtml(actionLabel)}</span>${countBadge}</td>
          <td class="table-logs-target"><code>${escapeHtml(targetText)}</code></td>
          <td><span class="badge ${statusBadge}">${escapeHtml(status)}</span></td>
          <td class="${isSystem ? 'table-logs-details log-system' : 'table-logs-details'}">${escapeHtml(details)}</td>
        </tr>
      `;
    }).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger); padding:16px;">Error al consultar registros de auditoría: ${escapeHtml(e.message)}</td></tr>`;
  }
}

function matchSourceBadge(source) {
  switch (source) {
    case 'samba_audit':
      return '<span class="badge badge-purple">Samba</span>';
    case 'admin':
      return '<span class="badge badge-blue">Admin</span>';
    case 'backup':
      return '<span class="badge badge-ok">Backup</span>';
    case 'system':
    default:
      return '<span class="badge badge-gray">Sistema</span>';
  }
}

function exportOrCopyLogs() {
  const logs = AppState.logsCache || [];
  if (logs.length === 0) {
    showToast('No hay registros cargados para copiar.', 'warning');
    return;
  }

  const lines = logs.map(l => {
    const ts = l.timestamp || '';
    const src = (l.source || 'system').toUpperCase();
    const user = l.user || l.task || 'sistema';
    const ip = l.ip || '-';
    const countStr = (l.count && l.count > 1) ? ` [x${l.count}]` : '';
    const action = (l.action_label || l.event_label || l.action || '-') + countStr;
    const target = l.target || '-';
    const status = l.status || 'OK';
    const msg = l.human || l.message || (l.details ? (typeof l.details === 'object' ? JSON.stringify(l.details) : l.details) : '-');
    return `[${ts}] [${src}] [${user}@${ip}] [${action}] [${target}] [${status}] ${msg}`;
  });

  const text = lines.join('\n');

  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(() => {
      showToast(`${logs.length} registros copiados al portapapeles.`, 'success');
    }).catch(() => {
      fallbackCopyText(text, logs.length);
    });
  } else {
    fallbackCopyText(text, logs.length);
  }
}

function fallbackCopyText(text, count) {
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.left = '-9999px';
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand('copy');
    showToast(`${count} registros copiados al portapapeles.`, 'success');
  } catch (err) {
    showToast('No se pudo copiar al portapapeles.', 'error');
  } finally {
    document.body.removeChild(ta);
  }
}

// ==============================================================================
// 11. Módulo: Consola Terminal Web Real (Bash Interactivo)
// ==============================================================================
function initTerminal() {
  const input = document.getElementById('terminal-input');
  const output = document.getElementById('terminal-output');
  if (!input || !output) return;

  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      const cmd = input.value;
      input.value = '';
      if (cmd.trim() === '') {
        await sendTerminalKey('ENTER');
        return;
      }
      AppState.terminal.history.push(cmd);
      AppState.terminal.historyIndex = AppState.terminal.history.length;
      await sendTerminalLine(cmd);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (AppState.terminal.history.length > 0 && AppState.terminal.historyIndex > 0) {
        AppState.terminal.historyIndex--;
        input.value = AppState.terminal.history[AppState.terminal.historyIndex];
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (AppState.terminal.historyIndex < AppState.terminal.history.length - 1) {
        AppState.terminal.historyIndex++;
        input.value = AppState.terminal.history[AppState.terminal.historyIndex];
      } else {
        AppState.terminal.historyIndex = AppState.terminal.history.length;
        input.value = '';
      }
    } else if (e.key === 'Tab') {
      e.preventDefault();
      await sendTerminalKey('TAB');
    } else if (e.ctrlKey && (e.key === 'c' || e.key === 'C')) {
      e.preventDefault();
      await sendTerminalKey('CTRL_C');
    }
  });

  output.addEventListener('click', () => input.focus());
}

async function startTerminalSession() {
  if (AppState.terminal.ptyStarted) {
    startTerminalPolling();
    return;
  }
  AppState.terminal.ptyStarted = true;
  try {
    await apiFetch('/api/terminal/session', {
      method: 'POST',
      body: JSON.stringify({ cols: 200, rows: 50 }),
      silentToast: true,
    });
    startTerminalPolling();
  } catch (e) {
    AppState.terminal.ptyStarted = false;
  }
}

function startTerminalPolling() {
  if (AppState.terminal.ptyTimer) return;
  AppState.terminal.ptyTimer = setInterval(refreshTerminalPane, 900);
  refreshTerminalPane();
}

function stopTerminalPolling() {
  if (AppState.terminal.ptyTimer) {
    clearInterval(AppState.terminal.ptyTimer);
    AppState.terminal.ptyTimer = null;
  }
}

async function refreshTerminalPane() {
  const output = document.getElementById('terminal-output');
  if (!output) return;
  try {
    const res = await apiFetch('/api/terminal/capture', { silentToast: true });
    const text = res.data?.output ?? res.output ?? '';
    output.textContent = text.replace(/\s+$/, '\n');
    output.scrollTop = output.scrollHeight;
  } catch (e) {
    // Silencioso: la sesión puede estar reiniciándose
  }
}

async function sendTerminalLine(cmd) {
  try {
    await apiFetch('/api/terminal/send', {
      method: 'POST',
      body: JSON.stringify({ data: cmd }),
      silentToast: true,
    });
    setTimeout(refreshTerminalPane, 150);
  } catch (e) {
    // Ya mostrado
  }
}

async function sendTerminalKey(token) {
  try {
    await apiFetch('/api/terminal/send', {
      method: 'POST',
      body: JSON.stringify({ data: token }),
      silentToast: true,
    });
    setTimeout(refreshTerminalPane, 150);
  } catch (e) {
    // Ya mostrado
  }
}

function clearTerminal() {
  sendTerminalLine('clear');
}

function runQuickCommand(cmd) {
  const input = document.getElementById('terminal-input');
  if (input) input.focus();
  AppState.terminal.history.push(cmd);
  AppState.terminal.historyIndex = AppState.terminal.history.length;
  sendTerminalLine(cmd);
}

async function reconnectTerminal() {
  stopTerminalPolling();
  AppState.terminal.ptyStarted = false;
  const output = document.getElementById('terminal-output');
  if (output) {
    output.textContent += '\n[Reconectando sesión interactiva de terminal PTY tmux...]\n';
  }
  try {
    await apiFetch('/api/terminal/kill', { method: 'POST', silentToast: true });
  } catch (e) { }
  await startTerminalSession();
  showToast('Sesión de terminal reconectada.', 'info');
}

async function resizeTerminal(cols, rows) {
  const container = document.getElementById('terminal-output');
  let c = cols;
  let r = rows;
  if (!c || !r) {
    const width = container ? container.clientWidth : 800;
    const height = container ? container.clientHeight : 400;
    c = Math.max(80, Math.floor(width / 8.5));
    r = Math.max(24, Math.floor(height / 18));
  }
  try {
    await apiFetch('/api/terminal/resize', {
      method: 'POST',
      body: JSON.stringify({ cols: c, rows: r }),
      silentToast: true,
    });
    showToast(`Dimensiones de terminal ajustadas (${c}x${r}).`, 'info');
  } catch (e) {
    // Silencioso
  }
}

// ==============================================================================
// 12. Módulo: Actualizaciones y Reinicio
// ==============================================================================
async function loadUpdates() {
  const container = document.getElementById('updates-container');
  if (!container) return;

  try {
    const res = await apiFetch('/api/system/updates');
    const u = res.data || res;

    container.innerHTML = `
      <div style="display:flex; flex-direction:column; gap:12px;">
        <div class="info-row">
          <span class="info-label">Versión instalada (Commit):</span>
          <strong class="info-val"><code>${escapeHtml(u.installed_commit)}</code> (${escapeHtml(u.commit_date)})</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Canal de actualización:</span>
          <strong class="info-val">${escapeHtml(u.channel)}</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Estado:</span>
          <strong class="info-val" style="color:var(--accent-success-text);">Sistema actualizado</strong>
        </div>
      </div>
    `;
  } catch (e) {
    container.innerHTML = '<p style="color:var(--accent-danger);">Error al consultar actualizaciones.</p>';
  }
}

async function applySystemUpdate() {
  const proceed = window.confirm(
    'Actualización del Sistema desde GitHub\n\n' +
    'Esta acción comprobará y sincronizará la versión oficial más reciente desde el repositorio GitHub (main), aplicando parches de seguridad y mejoras al panel web y módulos CLI sin interrumpir el almacenamiento.\n\n' +
    '¿Deseas iniciar la comprobación ahora?'
  );
  if (!proceed) return;

  showToast('Comprobando actualizaciones oficiales en GitHub...', 'info');
  try {
    await loadUpdates();
    showToast('Repositorio verificado. Para aplicar cambios en caliente en el servidor ejecuta: sudo nas update', 'success');
  } catch (e) {
    showToast('Error al consultar el repositorio: ' + e.message, 'error');
  }
}

async function confirmRebootServer() {
  const btn = document.getElementById('btn-confirm-reboot');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/system/reboot', { method: 'POST' });
    closeModal('modal-reboot-server');
    showToast('Reinicio del servidor ordenado. La conexión se reanudará en 1-2 minutos.', 'warning');
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

// ==============================================================================
// 13. Utilidades Generales
// ==============================================================================

// ==============================================================================
// 14. Módulos Adicionales: Redes, Terminal, Aplicaciones, Dominio y Diagnósticos
// ==============================================================================
async function loadNetworking() {
  try {
    const res = await apiFetch('/api/metrics');
    const d = res.data || res;
    if (d && d.system) {
      const hn = document.getElementById('masthead-hostname');
      if (hn && d.system.hostname) hn.textContent = d.system.hostname;
      const netHn = document.getElementById('net-info-hostname');
      if (netHn && d.system.hostname) netHn.textContent = d.system.hostname;
      const netPath = document.getElementById('net-info-path');
      if (netPath && d.system.hostname) {
        const ip = d.system.ip || '10.10.1.2';
        netPath.innerHTML = `<code>\\\\${escapeHtml(d.system.hostname)}</code> o <code>\\\\${escapeHtml(ip)}</code>`;
      }
    }
  } catch (e) {
    // Manejado por apiFetch
  }
}

function loadTerminal() {
  const input = document.getElementById('terminal-input');
  startTerminalSession();
  if (input) {
    setTimeout(() => input.focus(), 80);
  }
}

function loadApplications() {
  // Vista informativa de tecnologías y componentes integrados
}

// ==============================================================================
// 15. Módulo: Explorador de Archivos (Cuadrícula Windows, Previsualizador & Papelera)
// ==============================================================================

function getFileTypeMeta(item) {
  if (item.is_dir) {
    return {
      icon: '#icon-folder',
      color: '#f59e0b',
      typeLabel: 'Carpeta',
      previewable: false,
    };
  }

  const name = item.name || '';
  const ext = name.includes('.') ? name.split('.').pop().toLowerCase() : '';

  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'ico'].includes(ext)) {
    return {
      icon: '#icon-image',
      color: '#a855f7',
      typeLabel: `Imagen ${ext.toUpperCase()}`,
      previewable: true,
      mediaType: 'image',
    };
  }

  if (ext === 'pdf') {
    return {
      icon: '#icon-file-text',
      color: '#ef4444',
      typeLabel: 'Documento PDF',
      previewable: true,
      mediaType: 'pdf',
    };
  }

  if (['xlsx', 'xls', 'ods', 'csv', 'tsv'].includes(ext)) {
    return {
      icon: '#icon-table',
      color: '#10b981',
      typeLabel: `Hoja de cálculo (${ext.toUpperCase()})`,
      previewable: true,
      mediaType: 'excel',
    };
  }

  if (['docx', 'doc'].includes(ext)) {
    return {
      icon: '#icon-file-text',
      color: '#3b82f6',
      typeLabel: `Documento Word (${ext.toUpperCase()})`,
      previewable: true,
      mediaType: 'word',
    };
  }

  if (['pptx', 'ppt'].includes(ext)) {
    return {
      icon: '#icon-presentation',
      color: '#f97316',
      typeLabel: `Presentación (${ext.toUpperCase()})`,
      previewable: true,
      mediaType: 'presentation',
    };
  }

  if (['mp4', 'webm', 'mov', 'avi', 'mkv'].includes(ext)) {
    return {
      icon: '#icon-video',
      color: '#f43f5e',
      typeLabel: `Video ${ext.toUpperCase()}`,
      previewable: ['mp4', 'webm'].includes(ext),
      mediaType: 'video',
    };
  }

  if (['mp3', 'wav', 'ogg', 'flac', 'm4a'].includes(ext)) {
    return {
      icon: '#icon-music',
      color: '#14b8a6',
      typeLabel: `Audio ${ext.toUpperCase()}`,
      previewable: ['mp3', 'wav', 'ogg'].includes(ext),
      mediaType: 'audio',
    };
  }

  if (['zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar'].includes(ext)) {
    return {
      icon: '#icon-archive',
      color: '#eab308',
      typeLabel: `Archivo comprimido (${ext.toUpperCase()})`,
      previewable: false,
    };
  }

  if (['txt', 'log', 'conf', 'sh', 'php', 'js', 'json', 'yml', 'yaml', 'ini', 'xml', 'sql', 'md', 'env', 'py', 'css', 'html', 'bat', 'cmd'].includes(ext)) {
    return {
      icon: '#icon-file-text',
      color: '#10b981',
      typeLabel: `Texto / Código (${ext.toUpperCase()})`,
      previewable: true,
      mediaType: 'text',
    };
  }

  return {
    icon: '#icon-file',
    color: '#94a3b8',
    typeLabel: ext ? `Archivo .${ext}` : 'Archivo',
    previewable: true,
    mediaType: 'unknown',
  };
}

function setFileViewMode(mode, triggerRender = true) {
  AppState.files.viewMode = mode;
  try {
    localStorage.setItem('nas_file_view', mode);
  } catch (e) { }

  const btnGrid = document.getElementById('btn-view-grid');
  const btnList = document.getElementById('btn-view-list');
  if (btnGrid) btnGrid.classList.toggle('active', mode === 'grid');
  if (btnList) btnList.classList.toggle('active', mode === 'list');

  if (triggerRender) {
    renderCurrentFiles();
  }
}

async function loadFiles(subpath = null) {
  if (subpath === null) {
    subpath = AppState.files.currentPath || '';
  }
  if (AppState.files.isLoading && AppState.files.currentPath === subpath) {
    return;
  }
  AppState.files.isLoading = true;
  AppState.files.currentPath = subpath;

  const gridContainer = document.getElementById('files-grid-container');
  const tbody = document.getElementById('files-table-body');

  if (gridContainer) {
    gridContainer.innerHTML = '<div style="grid-column:1/-1; text-align:center; padding:30px; color:var(--text-muted);"><svg class="icon spin" style="width:24px; height:24px; color:var(--accent-primary); margin-bottom:8px;"><use href="#icon-refresh"></use></svg><br>Cargando archivos y recursos...</div>';
  }
  if (tbody) {
    tbody.innerHTML = '<tr><td colspan="7" style="text-align:center; padding:18px;">Cargando archivos y directorios...</td></tr>';
  }

  try {
    const root = AppState.files.root || 'nas';
    const query = `/api/files/list?root=${encodeURIComponent(root)}&path=${encodeURIComponent(subpath)}`;
    const res = await apiFetch(query);

    const items = res.data?.items ?? res.items ?? [];
    const breadcrumbs = res.data?.breadcrumbs ?? res.breadcrumbs ?? [];
    const currentPath = res.data?.current_path ?? res.current_path ?? subpath ?? '';

    AppState.files.items = items;
    renderFileBreadcrumbs(breadcrumbs, currentPath);
    renderCurrentFiles();
    updateTrashBadge();
  } catch (e) {
    if (gridContainer) {
      gridContainer.innerHTML = `<div style="grid-column:1/-1; text-align:center; color:var(--accent-danger); padding:24px;">Error al cargar archivos: ${escapeHtml(e.message)}</div>`;
    }
    if (tbody) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger); padding:18px;">Error al cargar archivos: ${escapeHtml(e.message)}</td></tr>`;
    }
  } finally {
    AppState.files.isLoading = false;
  }
}

function renderCurrentFiles() {
  const isGrid = AppState.files.viewMode === 'grid';
  const gridContainer = document.getElementById('files-grid-container');
  const tableContainer = document.getElementById('files-table-container');

  if (gridContainer) gridContainer.style.display = isGrid ? 'grid' : 'none';
  if (tableContainer) tableContainer.style.display = isGrid ? 'none' : 'block';

  if (isGrid) {
    renderFileGrid(AppState.files.items);
  } else {
    renderFileTable(AppState.files.items);
  }
}

function renderFileGrid(items) {
  const container = document.getElementById('files-grid-container');
  if (!container) return;
  container.textContent = '';

  if (!items || items.length === 0) {
    container.innerHTML = `
      <div style="grid-column:1/-1; text-align:center; padding:45px 20px; color:var(--text-muted);">
        <svg class="icon" style="width:48px; height:48px; color:var(--text-muted); opacity:0.6; margin-bottom:12px;"><use href="#icon-folder"></use></svg>
        <div style="font-size:15px; font-weight:600; color:var(--text-main); margin-bottom:4px;">Esta carpeta está vacía</div>
        <div style="font-size:13px;">Arrastra archivos aquí desde tu equipo o pulsa el botón <strong>Subir archivos</strong>.</div>
      </div>
    `;
    return;
  }

  const range = document.createRange();
  range.selectNodeContents(container);
  const html = items.map(item => {
    const isDir = Boolean(item.is_dir);
    const meta = getFileTypeMeta(item);
    const relPath = item.relative_path || item.path || '';
    const isSelected = AppState.files.selectedItemPath === relPath;
    const rawSize = item.size_formatted && item.size_formatted !== '--' ? item.size_formatted : '0 B';
    const sizeStr = isDir ? `Carpeta · ${rawSize}` : rawSize;

    const previewBtn = !isDir
      ? `<button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'preview')" title="Previsualizar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-eye"></use></svg></button>`
      : '';

    const downloadBtn = isDir
      ? `<button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'download')" title="Descargar como ZIP"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-download"></use></svg></button>`
      : `<button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'download')" title="Descargar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-download"></use></svg></button>`;

    return `
      <div class="file-card ${isDir ? 'is-folder' : ''} ${isSelected ? 'selected' : ''}"
           tabindex="0"
           data-path="${escapeHtml(relPath)}"
           data-name="${escapeHtml(item.name)}"
           data-is-dir="${isDir ? 'true' : 'false'}"
           onclick="handleCardClick(this)"
           ondblclick="handleCardDblClick(this)"
           onkeydown="handleCardKeyDown(event, this)"
           title="${escapeHtml(item.name)} (${sizeStr})">
        
        <div class="file-card-actions">
          ${previewBtn}
          ${downloadBtn}
          <button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'rename')" title="Renombrar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-edit"></use></svg></button>
          <button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'move')" title="Mover"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-folder"></use></svg></button>
          <button type="button" class="file-card-action-btn btn-danger-hover" onclick="handleCardAction(event, this, 'delete')" title="Eliminar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-trash"></use></svg></button>
        </div>

        <div class="file-card-icon-wrap">
          <svg class="file-card-icon" style="color:${meta.color};"><use href="${meta.icon}"></use></svg>
        </div>

        <span class="file-card-name">${escapeHtml(item.name)}</span>
        <span class="file-card-meta">${escapeHtml(sizeStr)}</span>
      </div>
    `;
  }).join('');

  const fragment = range.createContextualFragment(html);
  container.appendChild(fragment);
}

function handleCardClick(cardEl) {
  const relPath = cardEl.getAttribute('data-path') || '';
  selectFileCard(cardEl, relPath);
}

function handleCardDblClick(cardEl) {
  const relPath = cardEl.getAttribute('data-path') || '';
  const name = cardEl.getAttribute('data-name') || '';
  const isDir = cardEl.getAttribute('data-is-dir') === 'true';
  handleItemDblClick(relPath, name, isDir);
}

function handleCardKeyDown(e, cardEl) {
  if (e.key === 'Enter') {
    e.preventDefault();
    handleCardDblClick(cardEl);
  } else if (e.key === ' ') {
    e.preventDefault();
    handleCardClick(cardEl);
  }
}

function handleCardAction(e, btn, action) {
  e.stopPropagation();
  const card = btn.closest('.file-card');
  if (!card) return;
  const relPath = card.getAttribute('data-path') || '';
  const name = card.getAttribute('data-name') || '';
  const isDir = card.getAttribute('data-is-dir') === 'true';

  if (action === 'preview') {
    previewFile(relPath, name);
  } else if (action === 'download') {
    if (isDir) downloadFolderZip(relPath);
    else downloadFile(relPath);
  } else if (action === 'rename') {
    openRenameModal(relPath, name);
  } else if (action === 'move') {
    openMoveModal(relPath, name);
  } else if (action === 'delete') {
    openDeleteModal(relPath, name);
  }
}

function renderFileTable(items) {
  const tbody = document.getElementById('files-table-body');
  if (!tbody) return;
  tbody.textContent = '';

  if (!items || items.length === 0) {
    tbody.innerHTML = `
      <tr>
        <td colspan="7" style="text-align:center; padding:35px; color:var(--text-muted);">
          Esta carpeta está vacía. Arrastra archivos aquí desde tu equipo o utiliza el botón <strong>Subir archivos</strong>.
        </td>
      </tr>
    `;
    return;
  }

  const range = document.createRange();
  range.selectNodeContents(tbody);
  const html = items.map(item => {
    const isDir = Boolean(item.is_dir);
    const meta = getFileTypeMeta(item);
    const relPath = item.relative_path || item.path || '';
    const isSelected = AppState.files.selectedItemPath === relPath;
    const nameClass = isDir ? 'file-row-name is-folder' : 'file-row-name';
    const tableSizeStr = item.size_formatted && item.size_formatted !== '--' ? item.size_formatted : '0 B';

    const previewAction = !isDir
      ? `<button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'preview')" title="Previsualizar archivo"><svg class="icon"><use href="#icon-eye"></use></svg></button>`
      : '';

    const downloadAction = isDir
      ? `<button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'download')" title="Descargar carpeta como ZIP"><svg class="icon"><use href="#icon-download"></use></svg></button>`
      : `<button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'download')" title="Descargar archivo"><svg class="icon"><use href="#icon-download"></use></svg></button>`;

    const ownerStr = escapeHtml(item.owner || 'sistemas');
    const groupStr = escapeHtml(item.group || 'grp_samba');
    const modStr = escapeHtml(item.modified_at || item.mtime || 'N/A');

    return `
      <tr class="file-table-row ${isSelected ? 'selected' : ''}"
          tabindex="0"
          data-path="${escapeHtml(relPath)}"
          data-name="${escapeHtml(item.name)}"
          data-is-dir="${isDir ? 'true' : 'false'}"
          onclick="handleTableRowSelect(this)"
          ondblclick="handleTableRowDblClick(this)"
          onkeydown="handleTableRowKeyDown(event, this)">
        <td><svg class="icon" style="color:${meta.color};"><use href="${meta.icon}"></use></svg></td>
        <td>
          <div class="${nameClass}" onclick="handleTableRowNameClick(event, this)">
            <span>${escapeHtml(item.name)}</span>
          </div>
        </td>
        <td>${escapeHtml(tableSizeStr)}</td>
        <td><code>${escapeHtml(item.permissions || '0660')}</code></td>
        <td>${ownerStr}:${groupStr}</td>
        <td style="font-size:12px; color:var(--text-muted);">${modStr}</td>
        <td style="text-align:right;">
          <div style="display:flex; justify-content:flex-end; gap:6px;">
            ${previewAction}
            ${downloadAction}
            <button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'rename')" title="Renombrar"><svg class="icon"><use href="#icon-edit"></use></svg></button>
            <button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'move')" title="Mover"><svg class="icon"><use href="#icon-folder"></use></svg></button>
            <button type="button" class="btn btn-danger btn-sm" onclick="handleTableRowAction(this, 'delete')" title="Eliminar"><svg class="icon"><use href="#icon-trash"></use></svg></button>
          </div>
        </td>
      </tr>
    `;
  }).join('');

  const fragment = range.createContextualFragment(html);
  tbody.appendChild(fragment);
}

function handleTableRowSelect(tr) {
  const relPath = tr.getAttribute('data-path') || '';
  selectTableRow(tr, relPath);
}

function handleTableRowDblClick(tr) {
  const relPath = tr.getAttribute('data-path') || '';
  const name = tr.getAttribute('data-name') || '';
  const isDir = tr.getAttribute('data-is-dir') === 'true';
  handleItemDblClick(relPath, name, isDir);
}

function handleTableRowKeyDown(e, tr) {
  if (e.key === 'Enter') {
    e.preventDefault();
    handleTableRowDblClick(tr);
  } else if (e.key === ' ') {
    e.preventDefault();
    handleTableRowSelect(tr);
  }
}

function handleTableRowNameClick(e, nameEl) {
  e.stopPropagation();
  const tr = nameEl.closest('tr');
  if (!tr) return;
  const relPath = tr.getAttribute('data-path') || '';
  const name = tr.getAttribute('data-name') || '';
  const isDir = tr.getAttribute('data-is-dir') === 'true';
  selectTableRow(tr, relPath);
  if (isDir) {
    navigateToSubpath(relPath);
  } else {
    previewFile(relPath, name);
  }
}

function selectTableRow(tr, relPath) {
  document.querySelectorAll('#files-table-body tr.selected').forEach(r => r.classList.remove('selected'));
  if (tr) tr.classList.add('selected');
  AppState.files.selectedItemPath = relPath;
}

function handleTableRowAction(btn, action) {
  const tr = btn.closest('tr');
  if (!tr) return;
  const relPath = tr.getAttribute('data-path') || '';
  const name = tr.getAttribute('data-name') || '';
  const isDir = tr.getAttribute('data-is-dir') === 'true';

  if (action === 'preview') {
    previewFile(relPath, name);
  } else if (action === 'download') {
    if (isDir) downloadFolderZip(relPath);
    else downloadFile(relPath);
  } else if (action === 'rename') {
    openRenameModal(relPath, name);
  } else if (action === 'move') {
    openMoveModal(relPath, name);
  } else if (action === 'delete') {
    openDeleteModal(relPath, name);
  }
}

function selectFileCard(cardEl, relPath) {
  document.querySelectorAll('.file-card.selected').forEach(c => c.classList.remove('selected'));
  if (cardEl) cardEl.classList.add('selected');
  AppState.files.selectedItemPath = relPath;
}

function handleItemDblClick(relPath, name, isDir) {
  if (isDir) {
    navigateToSubpath(relPath);
  } else {
    previewFile(relPath, name);
  }
}

function refreshCurrentFileView() {
  if (AppState.files.inTrash) {
    loadTrash();
  } else {
    loadFiles();
  }
}

function renderFileBreadcrumbs(breadcrumbs, currentPath) {
  const container = document.getElementById('files-breadcrumbs');
  if (!container) return;

  const rootLabel = AppState.files.root === 'backups' ? '/srv/nas/BACKUPS_HISTORICOS' : '/srv/nas';

  if (!breadcrumbs || breadcrumbs.length === 0) {
    container.innerHTML = `<span class="file-breadcrumb-current">${escapeHtml(rootLabel)}</span>`;
    return;
  }

  container.innerHTML = breadcrumbs.map((bc, idx) => {
    const isLast = idx === breadcrumbs.length - 1;
    if (isLast) {
      return `<span class="file-breadcrumb-current">${escapeHtml(bc.name)}</span>`;
    }
    return `
      <span class="file-breadcrumb-item" data-path="${escapeHtml(bc.path)}" onclick="navigateToSubpath(this.getAttribute('data-path'))">${escapeHtml(bc.name)}</span>
      <span class="file-breadcrumb-separator">/</span>
    `;
  }).join('');
}

function changeFilesRoot(rootKey) {
  AppState.files.root = rootKey;
  AppState.files.currentPath = '';
  loadFiles('');
}

function navigateToSubpath(path) {
  loadFiles(path);
}

function triggerFileInput() {
  const input = document.getElementById('files-hidden-input');
  if (input) input.click();
}

function handleFileSelect(e) {
  const files = e.target.files;
  if (files && files.length > 0) {
    uploadFiles(files);
  }
  e.target.value = '';
}

async function uploadFiles(files) {
  const progressBar = document.getElementById('upload-progress-bar');
  const progressFill = document.getElementById('upload-progress-fill');
  const fileLabel = document.getElementById('upload-file-label');
  const percentLabel = document.getElementById('upload-percent-label');
  const uploadBtn = document.getElementById('btn-upload-files');

  if (uploadBtn) {
    uploadBtn.disabled = true;
    uploadBtn.classList.add('is-loading');
  }

  if (progressBar) progressBar.style.display = 'block';

  let successCount = 0;

  try {
    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      if (fileLabel) fileLabel.textContent = `Subiendo (${i + 1}/${files.length}): ${file.name}`;
      if (percentLabel) percentLabel.textContent = '0%';
      if (progressFill) progressFill.style.width = '0%';

      try {
        await new Promise((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          xhr.open('POST', '/api/files/upload', true);
          const csrf = getCsrfToken();
          if (csrf) {
            xhr.setRequestHeader('X-CSRF-Token', csrf);
          }

          xhr.upload.onprogress = (evt) => {
            if (evt.lengthComputable) {
              const percent = Math.round((evt.loaded / evt.total) * 100);
              if (percentLabel) percentLabel.textContent = `${percent}%`;
              if (progressFill) progressFill.style.width = `${percent}%`;
            }
          };

          xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) {
              try {
                const resp = JSON.parse(xhr.responseText);
                if (resp.status === 'success' || resp.success === true) {
                  successCount++;
                  resolve(resp);
                } else {
                  reject(new Error(resp.message || resp.error || 'Error en respuesta del servidor'));
                }
              } catch (err) {
                reject(err);
              }
            } else {
              let msg = `Fallo HTTP ${xhr.status}: ${xhr.statusText}`;
              try {
                const errResp = JSON.parse(xhr.responseText);
                if (errResp.message || errResp.error) {
                  msg = errResp.message || errResp.error;
                }
              } catch (e) {
                // Conservar mensaje HTTP por defecto si no es JSON
              }
              reject(new Error(msg));
            }
          };

          xhr.onerror = () => reject(new Error('Error de red durante la subida'));

          const formData = new FormData();
          formData.append('root', AppState.files.root || 'nas');
          formData.append('path', AppState.files.currentPath || '');
          formData.append('file', file);

          xhr.send(formData);
        });
      } catch (err) {
        showToast(`Error al subir "${file.name}": ${err.message}`, 'error');
      }
    }
  } finally {
    if (uploadBtn) {
      uploadBtn.disabled = false;
      uploadBtn.classList.remove('is-loading');
    }
    if (progressBar) {
      setTimeout(() => {
        progressBar.style.display = 'none';
        if (progressFill) progressFill.style.width = '0%';
      }, 1200);
    }
    if (successCount > 0) {
      showToast(`Se subieron con éxito ${successCount} archivo(s).`, 'success');
    }
    loadFiles();
  }
}

function downloadFile(relPath) {
  const root = encodeURIComponent(AppState.files.root || 'nas');
  const path = encodeURIComponent(relPath);
  window.location.href = `/api/files/download?root=${root}&path=${path}`;
}

let _zipDownloadInProgress = false;

async function downloadFolderZip(relPath, force = false) {
  const cleanRel = String(relPath || '').replace(/^\/+|\/+$/g, '');
  if (!cleanRel) {
    showToast('Seleccione o ingrese a una carpeta específica para descargarla en formato ZIP.', 'warning');
    return;
  }
  if (_zipDownloadInProgress) {
    showToast('Ya hay una compresión ZIP en progreso, por favor espera a que finalice.', 'info');
    return;
  }

  // Alerta preventiva si hay respaldos en ejecución
  if (!force) {
    try {
      const runRes = await apiFetch('/api/backups/running');
      const runData = runRes.data || runRes;
      if (runData && runData.running) {
        const listEl = document.getElementById('zip-running-backups-list');
        if (listEl) {
          const taskNames = (runData.tasks && runData.tasks.length > 0)
            ? runData.tasks.map(t => `<strong>${escapeHtml(t)}</strong>`).join(', ')
            : 'Procesos de sincronización rsync activos';
          listEl.innerHTML = `<svg class="icon icon-sm" style="color:var(--accent-warning); margin-right:4px;"><use href="#icon-alert-triangle"></use></svg> Tareas activas: ${taskNames}`;
        }
        const btnConfirm = document.getElementById('btn-confirm-zip-download');
        if (btnConfirm) {
          btnConfirm.onclick = () => {
            closeModal('modal-zip-backup-warning');
            downloadFolderZip(relPath, true);
          };
        }
        openModal('modal-zip-backup-warning');
        return;
      }
    } catch (e) {
      console.warn('No se pudo verificar el estado de respaldos:', e);
    }
  }

  _zipDownloadInProgress = true;
  const btnZip = document.getElementById('btn-download-zip');
  if (btnZip) {
    btnZip.disabled = true;
    btnZip.classList.add('is-loading');
  }

  const folderName = cleanRel.split('/').pop() || 'carpeta';
  showToast(`Iniciando compresión ZIP de "${folderName}", por favor espera...`, 'info');

  try {
    const root = encodeURIComponent(AppState.files.root || 'nas');
    const path = encodeURIComponent(cleanRel);
    const res = await fetch(`/api/files/download?root=${root}&path=${path}&prepare=1`, {
      method: 'GET',
      credentials: 'same-origin'
    });

    if (!res.ok) {
      let errMsg = `Error al comprimir la carpeta (${res.status})`;
      try {
        const text = (await res.text()).trim();
        if (text) {
          try {
            const parsed = JSON.parse(text);
            errMsg = parsed.error || parsed.message || text;
          } catch (_) {
            errMsg = text;
          }
        }
      } catch (_) { }
      showToast(errMsg, 'error');
      return;
    }

    const contentType = (res.headers.get('Content-Type') || '').toLowerCase();
    if (contentType.includes('application/json')) {
      const data = await res.json();
      if (!data || !data.success || !data.token) {
        showToast((data && (data.error || data.message)) || 'Error al preparar el archivo ZIP.', 'error');
        return;
      }
      const downloadFilename = data.filename || `${folderName}.zip`;
      const sizeSuffix = data.size_formatted ? ` (${data.size_formatted})` : '';
      showToast(`Compresión finalizada${sizeSuffix}. Iniciando descarga de "${downloadFilename}"...`, 'success');

      const link = document.createElement('a');
      link.href = `/api/files/download?root=${root}&path=${path}&token=${encodeURIComponent(data.token)}`;
      link.download = downloadFilename;
      link.style.display = 'none';
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      return;
    }

    const blob = await res.blob();
    let downloadFilename = `${folderName}.zip`;
    const disp = res.headers.get('Content-Disposition') || '';
    const match = disp.match(/filename="?([^";]+)"?/i);
    if (match && match[1]) {
      try {
        downloadFilename = decodeURIComponent(match[1]);
      } catch (_) {
        downloadFilename = match[1];
      }
    }

    showToast(`Compresión finalizada. Iniciando descarga de "${downloadFilename}"...`, 'success');

    const blobUrl = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = blobUrl;
    link.download = downloadFilename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    setTimeout(() => URL.revokeObjectURL(blobUrl), 60000);
  } catch (err) {
    showToast(`Error durante la compresión o descarga ZIP: ${err.message || err}`, 'error');
  } finally {
    _zipDownloadInProgress = false;
    if (btnZip) {
      btnZip.disabled = false;
      btnZip.classList.remove('is-loading');
    }
  }
}

function downloadCurrentFolderZip() {
  const current = String(AppState.files.currentPath || '').replace(/^\/+|\/+$/g, '');
  const selected = String(AppState.files.selectedItemPath || '').replace(/^\/+|\/+$/g, '');
  if (!current && selected) {
    const item = (AppState.files.items || []).find(i => (i.relative_path || i.path) === selected);
    if (item && item.is_dir) {
      downloadFolderZip(selected);
      return;
    }
  }
  downloadFolderZip(current);
}

function openNewFolderModal() {
  const input = document.getElementById('new-folder-name');
  if (input) input.value = '';
  openModal('modal-new-folder');
  if (input) setTimeout(() => input.focus(), 100);
}

async function submitNewFolder(e) {
  e.preventDefault();
  const input = document.getElementById('new-folder-name');
  const name = input ? input.value.trim() : '';
  if (!name) return;

  const btn = document.getElementById('btn-submit-new-folder') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/files/mkdir', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        path: AppState.files.currentPath || '',
        name: name,
      }),
    });

    closeModal('modal-new-folder');
    showToast(`Carpeta "${name}" creada exitosamente.`, 'success');
    loadFiles();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function openRenameModal(relPath, currentName) {
  const oldpathInput = document.getElementById('rename-file-oldpath');
  const newnameInput = document.getElementById('rename-file-newname');
  if (oldpathInput) oldpathInput.value = relPath;
  if (newnameInput) newnameInput.value = currentName;
  openModal('modal-rename-file');
  if (newnameInput) setTimeout(() => newnameInput.focus(), 100);
}

async function submitRenameFile(e) {
  e.preventDefault();
  const oldPath = document.getElementById('rename-file-oldpath')?.value;
  const newName = document.getElementById('rename-file-newname')?.value.trim();
  if (!oldPath || !newName) return;

  const btn = document.getElementById('btn-submit-rename-file') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/files/rename', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        old_path: oldPath,
        new_name: newName,
      }),
    });

    closeModal('modal-rename-file');
    showToast(`Elemento renombrado a "${newName}".`, 'success');
    loadFiles();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function openMoveModal(relPath, currentName) {
  const sourceInput = document.getElementById('move-file-source');
  const labelInput = document.getElementById('move-file-name-label');
  const targetInput = document.getElementById('move-file-target');
  if (sourceInput) sourceInput.value = relPath;
  if (labelInput) labelInput.value = currentName || relPath;
  if (targetInput) targetInput.value = AppState.files.currentPath || '';
  openModal('modal-move-file');
  if (targetInput) setTimeout(() => targetInput.focus(), 100);
}

async function submitMoveFile(e) {
  e.preventDefault();
  const source = document.getElementById('move-file-source')?.value;
  const target = document.getElementById('move-file-target')?.value.trim() ?? '';
  if (!source) return;

  const btn = document.getElementById('btn-submit-move-file') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/move', {
      method: 'POST',
      body: JSON.stringify({
        path: source,
        target_dir: target,
      }),
    });

    closeModal('modal-move-file');
    showToast(res.message || 'Elemento movido exitosamente.', 'success');
    loadFiles();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function openDeleteModal(relPath, name) {
  const pathInput = document.getElementById('delete-file-path');
  const label = document.getElementById('delete-file-name-label');
  const radioTrash = document.getElementById('del-mode-trash');
  if (pathInput) pathInput.value = relPath;
  if (label) label.textContent = `"${name}"`;
  if (radioTrash) radioTrash.checked = true;
  openModal('modal-delete-file');
}

async function confirmDeleteFile() {
  const path = document.getElementById('delete-file-path')?.value;
  if (!path) return;

  const btn = document.getElementById('btn-confirm-delete');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  const isPermanent = document.getElementById('del-mode-permanent')?.checked === true;

  try {
    await apiFetch('/api/files/delete', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        path: path,
        permanent: isPermanent,
      }),
    });

    closeModal('modal-delete-file');
    const msg = isPermanent
      ? 'Elemento eliminado definitivamente del disco.'
      : 'Elemento movido a la papelera de reciclaje.';
    showToast(msg, isPermanent ? 'info' : 'success');
    loadFiles();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

// ==============================================================================
// Papelera de Reciclaje (Trash)
// ==============================================================================
function toggleTrashView() {
  AppState.files.inTrash = !AppState.files.inTrash;
  const normalView = document.getElementById('files-normal-view');
  const trashView = document.getElementById('files-trash-view');
  const btnNewFolder = document.getElementById('btn-new-folder');
  const btnUpload = document.getElementById('btn-upload-files');

  if (AppState.files.inTrash) {
    if (normalView) normalView.style.display = 'none';
    if (trashView) trashView.style.display = 'block';
    if (btnNewFolder) btnNewFolder.style.display = 'none';
    if (btnUpload) btnUpload.style.display = 'none';
    loadTrash();
  } else {
    exitTrashView();
  }
}

function exitTrashView() {
  AppState.files.inTrash = false;
  const normalView = document.getElementById('files-normal-view');
  const trashView = document.getElementById('files-trash-view');
  const btnNewFolder = document.getElementById('btn-new-folder');
  const btnUpload = document.getElementById('btn-upload-files');

  if (trashView) trashView.style.display = 'none';
  if (normalView) normalView.style.display = 'block';
  if (btnNewFolder) btnNewFolder.style.display = '';
  if (btnUpload) btnUpload.style.display = '';
  loadFiles();
}

async function loadTrash() {
  const tbody = document.getElementById('trash-table-body');
  if (tbody) {
    tbody.innerHTML = '<tr><td colspan="7" style="text-align:center; padding:18px;">Consultando papelera de reciclaje...</td></tr>';
  }

  try {
    const res = await apiFetch('/api/files/trash');
    const items = res.data?.items ?? res.items ?? [];
    const count = res.data?.count ?? res.count ?? items.length;

    updateTrashBadgeCount(count);

    if (!tbody) return;

    if (items.length === 0) {
      tbody.innerHTML = `
        <tr>
          <td colspan="7" style="text-align:center; padding:35px; color:var(--text-muted);">
            <svg class="icon" style="width:36px; height:36px; color:var(--accent-warning); opacity:0.7; margin-bottom:8px;"><use href="#icon-trash"></use></svg>
            <div>La papelera de reciclaje está vacía.</div>
          </td>
        </tr>
      `;
      return;
    }

    tbody.textContent = '';
    const range = document.createRange();
    range.selectNodeContents(tbody);
    const html = items.map(item => {
      const isDir = item.is_dir;
      const meta = getFileTypeMeta({ name: item.filename, is_dir: isDir });

      return `
        <tr class="trash-table-row" data-id="${item.id}" data-filename="${escapeHtml(item.filename)}" onclick="selectTrashRow(this)">
          <td><svg class="icon" style="color:${meta.color};"><use href="${meta.icon}"></use></svg></td>
          <td><strong>${escapeHtml(item.filename)}</strong></td>
          <td style="font-family:var(--font-mono); font-size:12px; color:var(--text-muted);">${escapeHtml(item.original_path)}</td>
          <td>${escapeHtml(item.size_formatted || '0 B')}</td>
          <td>${escapeHtml(item.deleted_by || 'sistemas')}</td>
          <td style="font-size:12px; color:var(--text-muted);">${escapeHtml(item.deleted_at || 'N/A')}</td>
          <td style="text-align:right;">
            <div style="display:flex; justify-content:flex-end; gap:6px;">
              <button type="button" class="btn btn-secondary btn-sm" onclick="handleTrashAction(this, 'restore')" title="Restaurar a su carpeta original">
                <svg class="icon" style="color:var(--accent-success);"><use href="#icon-restore"></use></svg> Restaurar
              </button>
              <button type="button" class="btn btn-danger btn-sm" onclick="handleTrashAction(this, 'delete')" title="Eliminar definitivamente">
                <svg class="icon"><use href="#icon-trash"></use></svg>
              </button>
            </div>
          </td>
        </tr>
      `;
    }).join('');

    const fragment = range.createContextualFragment(html);
    tbody.appendChild(fragment);
  } catch (err) {
    if (tbody) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger); padding:18px;">Error al cargar papelera: ${escapeHtml(err.message)}</td></tr>`;
    }
  }
}

function selectTrashRow(tr) {
  document.querySelectorAll('#trash-table-body tr.selected').forEach(r => r.classList.remove('selected'));
  if (tr) tr.classList.add('selected');
}

async function handleTrashAction(btn, action) {
  const tr = btn.closest('tr');
  if (!tr) return;
  const id = parseInt(tr.getAttribute('data-id'), 10);
  const name = tr.getAttribute('data-filename') || '';

  btn.disabled = true;
  btn.classList.add('is-loading');

  try {
    if (action === 'restore') {
      await restoreTrashItem(id, name);
    } else if (action === 'delete') {
      await deleteTrashItem(id, name);
    }
  } finally {
    btn.disabled = false;
    btn.classList.remove('is-loading');
  }
}

async function updateTrashBadge() {
  try {
    const res = await apiFetch('/api/files/trash', { silentToast: true });
    const count = res.data?.count ?? res.count ?? (res.data?.items?.length ?? 0);
    updateTrashBadgeCount(count);
  } catch (e) {
    // Silencioso
  }
}

function updateTrashBadgeCount(count) {
  const badge = document.getElementById('badge-trash-count');
  if (badge) {
    badge.textContent = String(count);
    badge.style.display = count > 0 ? 'inline-flex' : 'none';
  }
}

async function restoreTrashItem(id, name) {
  try {
    await apiFetch('/api/files/trash/restore', {
      method: 'POST',
      body: JSON.stringify({ id }),
    });

    showToast(`Elemento "${name}" restaurado correctamente a su ubicación.`, 'success');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  }
}

async function deleteTrashItem(id, name) {
  if (!window.confirm(`¿Eliminar definitivamente "${name}"? Esta acción no se puede deshacer.`)) {
    return;
  }

  try {
    await apiFetch('/api/files/trash/delete', {
      method: 'POST',
      body: JSON.stringify({ id }),
    });

    showToast(`Elemento "${name}" eliminado definitivamente.`, 'info');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  }
}

function openEmptyTrashModal() {
  openModal('modal-empty-trash');
}

async function confirmEmptyTrash() {
  const btn = document.getElementById('btn-confirm-empty-trash');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/files/trash/empty', {
      method: 'POST',
    });

    closeModal('modal-empty-trash');
    showToast('Papelera vaciada por completo.', 'info');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function loadScriptLazy(src) {
  return new Promise((resolve, reject) => {
    const existing = document.querySelector(`script[src="${src}"]`);
    if (existing) {
      if (existing.dataset.loaded === 'true') {
        return resolve();
      }
      existing.addEventListener('load', () => resolve());
      existing.addEventListener('error', () => reject(new Error(`Error al cargar librería local: ${src}`)));
      return;
    }
    const s = document.createElement('script');
    s.src = src;
    s.async = true;
    s.onload = () => {
      s.dataset.loaded = 'true';
      resolve();
    };
    s.onerror = () => reject(new Error(`Error al cargar librería local: ${src}`));
    document.head.appendChild(s);
  });
}

// ==============================================================================
// Previsualizador de Archivos (Previewer 100% Offline con Editor Integrado)
// ==============================================================================
async function previewFile(relPath, fileName) {
  if (AppState.files.currentPreviewFile?.relPath === relPath && document.getElementById('modal-file-preview')?.classList.contains('active')) {
    return;
  }
  AppState.files.currentPreviewFile = { relPath, fileName };
  AppState.files.currentPreviewText = '';
  AppState.files.isEditingPreview = false;

  const modal = document.getElementById('modal-file-preview');
  if (!modal) return;

  const titleEl = document.getElementById('preview-file-name');
  const metaEl = document.getElementById('preview-file-meta');
  const iconEl = document.getElementById('preview-header-icon');
  const copyBtn = document.getElementById('btn-preview-copy');
  const editBtn = document.getElementById('btn-preview-edit');
  const saveBtn = document.getElementById('btn-preview-save');
  const editLabel = document.getElementById('btn-preview-edit-label');
  const loading = document.getElementById('preview-loading');
  const loadingText = document.getElementById('preview-loading-text');
  const codeContainer = document.getElementById('preview-code-container');
  const contentEl = document.getElementById('preview-code-content');
  const editorEl = document.getElementById('preview-code-editor');
  const imgContainer = document.getElementById('preview-image-container');
  const pdfContainer = document.getElementById('preview-pdf-container');
  const mediaContainer = document.getElementById('preview-media-container');
  const binaryContainer = document.getElementById('preview-binary-container');
  const spreadsheetContainer = document.getElementById('preview-spreadsheet-container');
  const docxContainer = document.getElementById('preview-docx-container');
  const pptxContainer = document.getElementById('preview-pptx-container');

  if (titleEl) titleEl.textContent = fileName;
  if (metaEl) metaEl.textContent = 'Cargando información...';
  if (loadingText) loadingText.textContent = 'Cargando previsualización...';
  if (copyBtn) copyBtn.style.display = 'none';
  if (editBtn) editBtn.style.display = 'none';
  if (saveBtn) saveBtn.style.display = 'none';
  if (editLabel) editLabel.textContent = 'Editar';
  if (contentEl) contentEl.style.display = 'block';
  if (editorEl) editorEl.style.display = 'none';

  const ext = fileName.includes('.') ? fileName.split('.').pop().toLowerCase() : '';
  const meta = getFileTypeMeta({ name: fileName, is_dir: false });
  if (iconEl) {
    iconEl.style.color = meta.color;
    iconEl.querySelector('use')?.setAttribute('href', meta.icon);
  }

  // Ocultar todos los contenedores y mostrar cargando
  [codeContainer, imgContainer, pdfContainer, mediaContainer, binaryContainer, spreadsheetContainer, docxContainer, pptxContainer].forEach(c => {
    if (c) c.style.display = 'none';
  });
  if (loading) loading.style.display = 'flex';

  openModal('modal-file-preview');

  const root = encodeURIComponent(AppState.files.root || 'nas');
  const encodedPath = encodeURIComponent(relPath);

  // 1. Imágenes
  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'ico'].includes(ext)) {
    const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
    const imgEl = document.getElementById('preview-img-element');
    if (imgEl) {
      imgEl.src = rawUrl;
      imgEl.onload = () => {
        if (loading) loading.style.display = 'none';
        if (imgContainer) imgContainer.style.display = 'flex';
        if (metaEl) metaEl.textContent = `Imagen ${ext.toUpperCase()} • ${imgEl.naturalWidth}x${imgEl.naturalHeight} px`;
      };
      imgEl.onerror = () => {
        if (loading) loading.style.display = 'none';
        if (binaryContainer) {
          binaryContainer.style.display = 'block';
          const bName = document.getElementById('preview-binary-name');
          const bDet = document.getElementById('preview-binary-details');
          if (bName) bName.textContent = fileName;
          if (bDet) bDet.textContent = 'Error al cargar imagen para previsualización.';
        }
      };
    }
    return;
  }

  // 2. PDF
  if (ext === 'pdf') {
    const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}#toolbar=1`;
    const frame = document.getElementById('preview-pdf-frame');
    if (frame) frame.src = rawUrl;
    if (loading) loading.style.display = 'none';
    if (pdfContainer) pdfContainer.style.display = 'block';
    if (metaEl) metaEl.textContent = 'Documento PDF (Visualizador nativo del navegador)';
    return;
  }

  // 3. Audio / Video HTML5
  if (['mp4', 'webm', 'mp3', 'wav', 'ogg'].includes(ext)) {
    const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}`;
    const isVid = ['mp4', 'webm'].includes(ext);
    const vidEl = document.getElementById('preview-video-element');
    const audEl = document.getElementById('preview-audio-element');

    if (isVid && vidEl) {
      vidEl.src = rawUrl;
      vidEl.style.display = 'block';
      if (audEl) audEl.style.display = 'none';
    } else if (audEl) {
      audEl.src = rawUrl;
      audEl.style.display = 'block';
      if (vidEl) vidEl.style.display = 'none';
    }

    if (loading) loading.style.display = 'none';
    if (mediaContainer) mediaContainer.style.display = 'flex';
    if (metaEl) metaEl.textContent = `${isVid ? 'Video' : 'Audio'} multimedia • ${ext.toUpperCase()}`;
    return;
  }

  // 4. Hojas de Cálculo (Excel: .xlsx, .xls, .ods, .csv, .tsv) — Opción 3 SheetJS Local
  if (['xlsx', 'xls', 'ods', 'csv', 'tsv'].includes(ext)) {
    try {
      if (typeof XLSX === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor local de hojas de cálculo...';
        await loadScriptLazy('/js/vendor/xlsx.full.min.js');
      }

      if (loadingText) loadingText.textContent = 'Descargando y procesando hoja de cálculo...';

      const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
      const response = await fetch(rawUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status} al descargar archivo.`);

      const arrayBuffer = await response.arrayBuffer();
      const workbook = XLSX.read(new Uint8Array(arrayBuffer), { type: 'array', cellDates: true });

      if (!workbook.SheetNames || workbook.SheetNames.length === 0) {
        throw new Error('El libro no contiene hojas de cálculo legibles.');
      }

      AppState.files.currentWorkbook = workbook;
      AppState.files.currentSheetName = workbook.SheetNames[0];

      renderSpreadsheetWorkbook(workbook, fileName);

      if (loading) loading.style.display = 'none';
      if (spreadsheetContainer) spreadsheetContainer.style.display = 'flex';
      if (metaEl) {
        metaEl.textContent = `Hoja de cálculo (${ext.toUpperCase()}) • ${workbook.SheetNames.length} hoja(s)`;
      }
      return;
    } catch (err) {
      console.error('Error al procesar hoja de cálculo:', err);
      if (loading) loading.style.display = 'none';
      if (binaryContainer) {
        binaryContainer.style.display = 'block';
        const bName = document.getElementById('preview-binary-name');
        const bDet = document.getElementById('preview-binary-details');
        if (bName) bName.textContent = fileName;
        if (bDet) bDet.textContent = `No se pudo previsualizar la hoja de cálculo: ${err.message || 'error de procesamiento'}`;
      }
      if (metaEl) metaEl.textContent = 'Hoja de cálculo';
      return;
    }
  }

  // 5. Documentos Word (.docx) — Opción 3 docx-preview Local
  if (ext === 'docx') {
    try {
      if (typeof JSZip === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor de descompresión...';
        await loadScriptLazy('/js/vendor/jszip.min.js');
      }
      if (typeof docx === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor de documentos Word...';
        await loadScriptLazy('/js/vendor/docx-preview.min.js');
      }

      if (loadingText) loadingText.textContent = 'Renderizando documento Word...';

      const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
      const response = await fetch(rawUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status} al descargar documento.`);

      const arrayBuffer = await response.arrayBuffer();
      const docxContentEl = document.getElementById('preview-docx-content');
      if (docxContentEl) {
        docxContentEl.innerHTML = '';
        await docx.renderAsync(arrayBuffer, docxContentEl, null, {
          className: 'docx-document-rendered',
          inWrapper: false,
          ignoreWidth: false,
          ignoreHeight: false,
          breakPages: true,
          renderHeaders: true,
          renderFooters: true,
          useBase64URL: true,
        });
      }

      if (loading) loading.style.display = 'none';
      if (docxContainer) docxContainer.style.display = 'flex';
      if (metaEl) {
        metaEl.textContent = 'Documento Word (.DOCX) • Renderizado local seguro';
      }
      return;
    } catch (err) {
      console.error('Error al renderizar documento Word:', err);
      if (loading) loading.style.display = 'none';
      if (binaryContainer) {
        binaryContainer.style.display = 'block';
        const bName = document.getElementById('preview-binary-name');
        const bDet = document.getElementById('preview-binary-details');
        if (bName) bName.textContent = fileName;
        if (bDet) bDet.textContent = `No se pudo previsualizar el documento Word: ${err.message || 'error de renderizado'}`;
      }
      if (metaEl) metaEl.textContent = 'Documento Word';
      return;
    }
  }

  // 6. Presentaciones PowerPoint (.pptx) — Opción 3 Inspector de Diapositivas Local
  if (ext === 'pptx') {
    try {
      if (typeof JSZip === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor de descompresión...';
        await loadScriptLazy('/js/vendor/jszip.min.js');
      }

      if (loadingText) loadingText.textContent = 'Extrayendo diapositivas...';

      const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
      const response = await fetch(rawUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status} al descargar presentación.`);

      const arrayBuffer = await response.arrayBuffer();
      const zip = await JSZip.loadAsync(arrayBuffer);

      const slideEntries = [];
      zip.forEach((entryPath, file) => {
        const match = entryPath.match(/^ppt\/slides\/slide(\d+)\.xml$/i);
        if (match) {
          slideEntries.push({ num: parseInt(match[1], 10), file });
        }
      });

      slideEntries.sort((a, b) => a.num - b.num);

      if (slideEntries.length === 0) {
        throw new Error('La presentación no contiene diapositivas legibles.');
      }

      const parser = new DOMParser();
      const parsedSlides = [];

      for (const entry of slideEntries) {
        const xmlText = await entry.file.async('text');
        const xmlDoc = parser.parseFromString(xmlText, 'application/xml');

        const paragraphs = [];
        const pElements = xmlDoc.getElementsByTagNameNS('*', 'p');
        for (let i = 0; i < pElements.length; i++) {
          const tElements = pElements[i].getElementsByTagNameNS('*', 't');
          let text = '';
          for (let j = 0; j < tElements.length; j++) {
            text += tElements[j].textContent || '';
          }
          text = text.trim();
          if (text) paragraphs.push(text);
        }

        const title = paragraphs.length > 0 ? paragraphs[0] : `Diapositiva ${entry.num}`;
        const bodyParagraphs = paragraphs.length > 1 ? paragraphs.slice(1) : [];

        parsedSlides.push({
          num: entry.num,
          title,
          paragraphs: bodyParagraphs,
        });
      }

      AppState.files.pptxSlides = parsedSlides;
      AppState.files.pptxCurrentIndex = 0;

      renderPptxPresentation(parsedSlides);

      if (loading) loading.style.display = 'none';
      if (pptxContainer) pptxContainer.style.display = 'flex';
      if (metaEl) {
        metaEl.textContent = `Presentación PowerPoint (.PPTX) • ${parsedSlides.length} diapositiva(s)`;
      }
      return;
    } catch (err) {
      console.error('Error al extraer diapositivas PowerPoint:', err);
      if (loading) loading.style.display = 'none';
      if (binaryContainer) {
        binaryContainer.style.display = 'block';
        const bName = document.getElementById('preview-binary-name');
        const bDet = document.getElementById('preview-binary-details');
        if (bName) bName.textContent = fileName;
        if (bDet) bDet.textContent = `No se pudieron extraer diapositivas: ${err.message || 'formato no legible'}`;
      }
      if (metaEl) metaEl.textContent = 'Presentación PowerPoint';
      return;
    }
  }

  // 7. Formatos heredados binarios de Office (.doc, .ppt)
  if (['doc', 'ppt'].includes(ext)) {
    if (loading) loading.style.display = 'none';
    if (binaryContainer) {
      binaryContainer.style.display = 'block';
      const bName = document.getElementById('preview-binary-name');
      const bDet = document.getElementById('preview-binary-details');
      if (bName) bName.textContent = fileName;
      if (bDet) {
        bDet.textContent = `Formato binario heredado de Microsoft Office (${ext.toUpperCase()}). Para previsualización interactiva en el navegador, guarde el archivo en formato moderno (.${ext}x) o descárguelo directamente.`;
      }
    }
    if (metaEl) metaEl.textContent = `Documento ${ext.toUpperCase()} (Heredado)`;
    return;
  }

  // 8. Archivos de Texto / Código / Configuración
  try {
    const res = await apiFetch(`/api/files/content?root=${root}&path=${encodedPath}`);
    const d = res.data || res;

    AppState.files.currentPreviewText = d.content || '';

    const lines = d.lines_count || (d.content ? d.content.split('\n').length : 0);
    const lineNumbersEl = document.getElementById('preview-line-numbers');

    if (contentEl) {
      contentEl.textContent = d.content || '';
    }
    if (editorEl) {
      editorEl.value = d.content || '';
      editorEl.onscroll = () => {
        if (lineNumbersEl) lineNumbersEl.scrollTop = editorEl.scrollTop;
      };
      editorEl.oninput = () => {
        const val = editorEl.value;
        AppState.files.currentPreviewText = val;
        if (contentEl) contentEl.textContent = val;
        updatePreviewLineNumbers(val);
      };
    }

    updatePreviewLineNumbers(d.content || '');

    if (metaEl) {
      metaEl.textContent = `${d.size_formatted || '0 B'} • ${lines} línea(s) • ${d.extension ? d.extension.toUpperCase() : 'TEXTO'}`;
    }

    if (copyBtn) copyBtn.style.display = 'inline-flex';
    if (editBtn) editBtn.style.display = 'inline-flex';
    if (loading) loading.style.display = 'none';
    if (codeContainer) codeContainer.style.display = 'flex';

  } catch (err) {
    if (loading) loading.style.display = 'none';
    if (binaryContainer) {
      binaryContainer.style.display = 'block';
      const bName = document.getElementById('preview-binary-name');
      const bDet = document.getElementById('preview-binary-details');
      if (bName) bName.textContent = fileName;
      if (bDet) bDet.textContent = err.message || 'Archivo no legible como texto plano.';
    }
    if (metaEl) metaEl.textContent = 'Archivo binario';
  }
}

function renderSpreadsheetWorkbook(workbook, fileName) {
  const tabsEl = document.getElementById('preview-sheet-tabs');
  const tableContainer = document.getElementById('preview-spreadsheet-table-container');
  if (!tabsEl || !tableContainer) return;

  tabsEl.innerHTML = '';
  workbook.SheetNames.forEach(sheetName => {
    const tabBtn = document.createElement('button');
    tabBtn.type = 'button';
    tabBtn.className = 'preview-sheet-tab' + (sheetName === AppState.files.currentSheetName ? ' active' : '');
    tabBtn.textContent = sheetName;
    tabBtn.onclick = () => {
      AppState.files.currentSheetName = sheetName;
      document.querySelectorAll('.preview-sheet-tab').forEach(b => b.classList.remove('active'));
      tabBtn.classList.add('active');
      renderActiveSheet(workbook, sheetName);
    };
    tabsEl.appendChild(tabBtn);
  });

  renderActiveSheet(workbook, AppState.files.currentSheetName);
}

function renderActiveSheet(workbook, sheetName) {
  const tableContainer = document.getElementById('preview-spreadsheet-table-container');
  const statsEl = document.getElementById('preview-spreadsheet-stats');
  if (!tableContainer) return;

  const sheet = workbook.Sheets[sheetName];
  if (!sheet) {
    tableContainer.innerHTML = '<div style="padding:24px; text-align:center; color:var(--text-muted);">Hoja vacía</div>';
    return;
  }

  const html = XLSX.utils.sheet_to_html(sheet, { id: 'preview-sheet-table', editable: false });
  tableContainer.innerHTML = html;

  const ref = sheet['!ref'] || '';
  if (statsEl) {
    statsEl.textContent = ref ? `Rango: ${ref}` : '';
  }
}

function renderPptxPresentation(slides) {
  const countEl = document.getElementById('preview-pptx-count');
  const listEl = document.getElementById('preview-pptx-slides-list');
  if (countEl) countEl.textContent = slides.length;
  if (!listEl) return;

  listEl.innerHTML = '';
  slides.forEach((slide, idx) => {
    const thumb = document.createElement('div');
    thumb.className = 'preview-pptx-slide-thumb' + (idx === 0 ? ' active' : '');
    thumb.innerHTML = `<div style="font-weight:600; margin-bottom:2px;">#${slide.num}</div><div style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${escapeHtml(slide.title)}</div>`;
    thumb.onclick = () => {
      AppState.files.pptxCurrentIndex = idx;
      displayPptxSlide(idx);
    };
    listEl.appendChild(thumb);
  });

  displayPptxSlide(0);
}

function displayPptxSlide(index) {
  const slides = AppState.files.pptxSlides || [];
  if (!slides[index]) return;
  const slide = slides[index];

  const thumbs = document.querySelectorAll('.preview-pptx-slide-thumb');
  thumbs.forEach((t, i) => {
    if (i === index) t.classList.add('active');
    else t.classList.remove('active');
  });

  const labelEl = document.getElementById('preview-pptx-current-label');
  const titleEl = document.getElementById('preview-pptx-slide-title');
  const bodyEl = document.getElementById('preview-pptx-slide-body');
  const prevBtn = document.getElementById('btn-pptx-prev');
  const nextBtn = document.getElementById('btn-pptx-next');

  if (labelEl) labelEl.textContent = `Diapositiva ${index + 1} de ${slides.length}`;
  if (titleEl) titleEl.textContent = slide.title || `Diapositiva ${slide.num}`;

  if (bodyEl) {
    if (slide.paragraphs && slide.paragraphs.length > 0) {
      let html = '<ul style="padding-left:20px; margin:0;">';
      slide.paragraphs.forEach(p => {
        html += `<li style="margin-bottom:8px;">${escapeHtml(p)}</li>`;
      });
      html += '</ul>';
      bodyEl.innerHTML = html;
    } else {
      bodyEl.innerHTML = '<p style="color:var(--text-muted); font-style:italic;">(Esta diapositiva no contiene texto en párrafos o solo contiene elementos visuales)</p>';
    }
  }

  if (prevBtn) prevBtn.disabled = index <= 0;
  if (nextBtn) nextBtn.disabled = index >= slides.length - 1;
}

function navigatePptxSlide(delta) {
  const slides = AppState.files.pptxSlides || [];
  const newIndex = (AppState.files.pptxCurrentIndex || 0) + delta;
  if (newIndex >= 0 && newIndex < slides.length) {
    AppState.files.pptxCurrentIndex = newIndex;
    displayPptxSlide(newIndex);
  }
}

function updatePreviewLineNumbers(text) {
  const lineNumbersEl = document.getElementById('preview-line-numbers');
  if (!lineNumbersEl) return;
  const count = text === '' ? 1 : Math.max(1, text.split('\n').length);
  let numsHtml = '';
  for (let i = 1; i <= count; i++) {
    numsHtml += `${i}<br>`;
  }
  lineNumbersEl.innerHTML = numsHtml;
}

function togglePreviewEditMode() {
  const contentEl = document.getElementById('preview-code-content');
  const editorEl = document.getElementById('preview-code-editor');
  const saveBtn = document.getElementById('btn-preview-save');
  const editLabel = document.getElementById('btn-preview-edit-label');

  AppState.files.isEditingPreview = !AppState.files.isEditingPreview;

  if (AppState.files.isEditingPreview) {
    if (contentEl) contentEl.style.display = 'none';
    if (editorEl) {
      editorEl.style.display = 'block';
      editorEl.value = AppState.files.currentPreviewText || '';
      editorEl.focus();
    }
    if (saveBtn) saveBtn.style.display = 'inline-flex';
    if (editLabel) editLabel.textContent = 'Cancelar';
  } else {
    if (editorEl) editorEl.style.display = 'none';
    if (contentEl) {
      contentEl.style.display = 'block';
      contentEl.textContent = AppState.files.currentPreviewText || '';
    }
    if (saveBtn) saveBtn.style.display = 'none';
    if (editLabel) editLabel.textContent = 'Editar';
  }
}

async function saveCurrentPreviewFile() {
  if (!AppState.files.currentPreviewFile?.relPath) return;

  const editorEl = document.getElementById('preview-code-editor');
  const content = editorEl ? editorEl.value : (AppState.files.currentPreviewText || '');
  const relPath = AppState.files.currentPreviewFile.relPath;
  const root = AppState.files.root || 'nas';
  const saveBtn = document.getElementById('btn-preview-save');

  if (saveBtn) {
    saveBtn.disabled = true;
    saveBtn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/save', {
      method: 'POST',
      body: JSON.stringify({
        root: root,
        path: relPath,
        content: content,
      }),
    });

    AppState.files.currentPreviewText = content;
    const contentEl = document.getElementById('preview-code-content');
    if (contentEl) contentEl.textContent = content;

    const metaEl = document.getElementById('preview-file-meta');
    if (metaEl) {
      const ext = AppState.files.currentPreviewFile.fileName.includes('.')
        ? AppState.files.currentPreviewFile.fileName.split('.').pop().toUpperCase()
        : 'TEXTO';
      const lines = content === '' ? 1 : content.split('\n').length;
      metaEl.textContent = `${res.size_formatted || '0 B'} • ${lines} línea(s) • ${ext}`;
    }

    showToast('Archivo guardado correctamente.', 'success');
    togglePreviewEditMode();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (saveBtn) {
      saveBtn.disabled = false;
      saveBtn.classList.remove('is-loading');
    }
  }
}

function copyPreviewContent() {
  if (!AppState.files.currentPreviewText) {
    showToast('No hay contenido para copiar.', 'warning');
    return;
  }

  navigator.clipboard.writeText(AppState.files.currentPreviewText)
    .then(() => showToast('Contenido copiado al portapapeles.', 'success'))
    .catch(() => showToast('No se pudo copiar al portapapeles.', 'error'));
}

function downloadPreviewFile() {
  if (AppState.files.currentPreviewFile?.relPath) {
    downloadFile(AppState.files.currentPreviewFile.relPath);
  }
}

// ==============================================================================
// 16. Módulo: Active Directory (Domain)
// ==============================================================================
async function loadDomainStatus() {
  const card = document.getElementById('domain-status-card');
  const joinPanel = document.getElementById('domain-join-panel');
  if (!card) return;

  card.innerHTML = '<div style="text-align:center; padding:20px; color:var(--text-muted);">Consultando estado de dominio...</div>';

  try {
    const res = await apiFetch('/api/domain/status');
    const d = res.data || res;
    AppState.domain.status = d;

    const isJoined = d.joined;
    const badge = isJoined
      ? '<span class="badge badge-ok">Unido al Dominio</span>'
      : '<span class="badge badge-gray">Servidor Autónomo (Standalone)</span>';

    const sssdBadge = d.sssd_active
      ? '<span class="badge badge-ok">Activo</span>'
      : '<span class="badge badge-gray">Inactivo</span>';

    let actionBtn = '';
    if (isJoined) {
      actionBtn = `
        <div style="margin-top:16px;">
          <button type="button" class="btn btn-danger btn-sm" onclick="handleDomainLeave()">
            <svg class="icon"><use href="#icon-power"></use></svg> Desvincular del Dominio
          </button>
        </div>
      `;
      if (joinPanel) joinPanel.style.display = 'none';
    } else {
      if (joinPanel) joinPanel.style.display = 'block';
    }

    card.innerHTML = `
      <div style="display:flex; flex-direction:column; gap:12px;">
        <div class="info-row">
          <span class="info-label">Estado de Membresía:</span>
          <div class="info-val">${badge}</div>
        </div>
        <div class="info-row">
          <span class="info-label">Dominio / Realm:</span>
          <strong class="info-val">${escapeHtml(d.domain || d.realm || 'Ninguno (Modo Workgroup)')}</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Grupo de Trabajo (Workgroup):</span>
          <strong class="info-val">${escapeHtml(d.workgroup || 'TEAM-JOFRATO')}</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Controlador de Dominio (KDC):</span>
          <strong class="info-val">${escapeHtml(d.kdc || 'N/A')}</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Servicio SSSD (Autenticación):</span>
          <div class="info-val">${sssdBadge}</div>
        </div>
        ${actionBtn}
      </div>
    `;

    const badgeDomain = document.getElementById('badge-domain');
    if (badgeDomain) {
      badgeDomain.textContent = isJoined ? 'AD OK' : 'AD';
      badgeDomain.className = isJoined ? 'nav-badge badge-ok' : 'nav-badge';
    }

  } catch (err) {
    card.innerHTML = `<p style="color:var(--accent-danger);">Error al consultar estado del dominio: ${escapeHtml(err.message)}</p>`;
  }
}

async function handleDomainDiscover(e) {
  e.preventDefault();
  const domainInput = document.getElementById('discover-domain-name');
  const resultDiv = document.getElementById('domain-discover-result');
  const btn = document.getElementById('btn-discover-domain');
  if (!domainInput || !resultDiv) return;

  const domain = domainInput.value.trim();
  if (!domain) return;

  resultDiv.style.display = 'block';
  resultDiv.innerHTML = '<p style="color:var(--text-muted); font-size:13px;">Buscando controladores de dominio Kerberos / DNS...</p>';
  if (btn) btn.disabled = true;

  try {
    const res = await apiFetch('/api/domain/discover', {
      method: 'POST',
      body: JSON.stringify({ domain }),
    });

    const d = res.data || res;
    resultDiv.innerHTML = `
      <div style="background:var(--bg-surface); padding:12px; border-radius:6px; border:1px solid var(--border-color); font-size:13px;">
        <div style="color:var(--accent-success-text); font-weight:600; margin-bottom:6px;">Controlador de Dominio Detectado</div>
        <div><strong>Dominio:</strong> ${escapeHtml(d.domain_name || domain)}</div>
        <div><strong>Realm:</strong> ${escapeHtml(d.realm_name || 'N/A')}</div>
        <div><strong>Servidores KDC:</strong> ${escapeHtml(Array.isArray(d.kdc) ? d.kdc.join(', ') : (d.kdc || 'N/A'))}</div>
        <div><strong>Software:</strong> ${escapeHtml(d.server_software || 'Active Directory')}</div>
      </div>
    `;
    showToast(`Dominio "${domain}" descubierto exitosamente.`, 'success');
  } catch (err) {
    resultDiv.innerHTML = `<div style="color:var(--accent-danger); font-size:13px;">No se pudo descubrir el dominio: ${escapeHtml(err.message)}</div>`;
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function handleDomainJoin(e) {
  e.preventDefault();
  const domain = document.getElementById('join-domain-name')?.value.trim();
  const user = document.getElementById('join-admin-user')?.value.trim();
  const password = document.getElementById('join-admin-pass')?.value;
  const ou = document.getElementById('join-ou')?.value.trim();
  const btn = document.getElementById('btn-join-domain');

  if (!domain || !user || !password) {
    showToast('Por favor completa todos los campos requeridos para la unión.', 'error');
    return;
  }

  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Uniendo al dominio...';
  }

  try {
    await apiFetch('/api/domain/join', {
      method: 'POST',
      body: JSON.stringify({ domain, user, password, ou }),
    });

    showToast(`Servidor unido con éxito al dominio ${domain}.`, 'success');
    const passInput = document.getElementById('join-admin-pass');
    if (passInput) passInput.value = '';
    loadDomainStatus();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = '<svg class="icon"><use href="#icon-domain"></use></svg> Unir al Dominio';
    }
  }
}

async function handleDomainLeave() {
  const user = window.prompt('Introduce el usuario administrador del dominio para desvincular (ej. Administrator):');
  if (!user) return;
  const password = window.prompt('Introduce la contraseña del administrador del dominio:');
  if (!password) return;

  try {
    await apiFetch('/api/domain/leave', {
      method: 'POST',
      body: JSON.stringify({ user, password }),
    });

    showToast('El servidor fue desvinculado del dominio Active Directory.', 'info');
    loadDomainStatus();
  } catch (err) {
    // Ya mostrado por apiFetch
  }
}

async function loadDiagnostics() {
  const overallVal = document.getElementById('diag-overall-val');
  const overallSub = document.getElementById('diag-overall-sub');
  const servicesVal = document.getElementById('diag-services-val');
  const sambaVal = document.getElementById('diag-samba-val');
  const container = document.getElementById('diagnostics-summary-container');

  if (container) {
    container.innerHTML = '<p>Ejecutando auditoría y pruebas de salud del servidor...</p>';
  }

  try {
    const res = await apiFetch('/api/diagnostics');
    const d = res.data || res;

    if (overallVal) {
      if (d.overall_status === 'OK') {
        overallVal.innerHTML = '<span class="badge badge-ok">Saludable (OK)</span>';
      } else {
        overallVal.innerHTML = '<span class="badge badge-warning">Atención requerida</span>';
      }
    }

    if (overallSub) {
      overallSub.textContent = `Host: ${d.hostname} | Uptime: ${d.uptime}`;
    }

    if (servicesVal) {
      servicesVal.textContent = `${d.services_active} / ${d.services_total} Activos`;
    }

    if (sambaVal) {
      sambaVal.innerHTML = d.testparm_ok
        ? '<span class="badge badge-ok">Sintaxis Válida</span>'
        : '<span class="badge badge-danger">Error de Configuración</span>';
    }

    if (container) {
      const servicesArray = Array.isArray(d.services) ? d.services : Object.values(d.services || {});
      const servicesList = servicesArray.map(item => {
        const isOk = item.active;
        const svcName = item.service || '';
        return `
          <div style="display:flex; justify-content:space-between; align-items:center; padding:6px 0; border-bottom:1px solid var(--border-color); font-size:13px;">
            <span><code>${escapeHtml(svcName)}</code> &bull; ${escapeHtml(item.name || '')}</span>
            ${isOk ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-danger">Inactivo</span>'}
          </div>
        `;
      }).join('');

      container.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:16px;">
          <div>
            <h4 style="margin:0 0 8px; font-size:14px;">Estado de Demonios Clave</h4>
            <div style="background:var(--bg-body); border-radius:4px; padding:10px;">
              ${servicesList}
            </div>
          </div>
          <div style="display:flex; gap:20px; flex-wrap:wrap; font-size:13px;">
            <div><strong>Almacenamiento (/srv/nas):</strong> ${d.storage.used_gb} GB / ${d.storage.total_gb} GB (${d.storage.usage_percent}%)</div>
            <div><strong>Recursos Samba:</strong> ${d.shares_count} activos</div>
            <div><strong>Tareas de Respaldo:</strong> ${d.backups_count} programadas</div>
          </div>
        </div>
      `;
    }

    showToast('Diagnóstico del sistema completado con éxito.', 'success');
  } catch (err) {
    if (container) {
      container.innerHTML = `<p style="color:var(--accent-danger);">Error al ejecutar diagnóstico: ${escapeHtml(err.message)}</p>`;
    }
  }
}

// ==============================================================================
// 15. Inicialización Robusta al Cargar el DOM
// ==============================================================================
function initApp() {
  if (window.NAS_INITIAL_SERVER_TIME) {
    initServerClock(window.NAS_INITIAL_SERVER_TIME);
  }
  applyRoleUi();

  // Configurar listeners de navegación sidebar
  document.querySelectorAll('.sidebar-nav .nav-item').forEach(item => {
    item.addEventListener('click', () => {
      const view = item.getAttribute('data-view');
      if (view) switchView(view);
      const sidebar = document.querySelector('.sidebar');
      const backdrop = document.getElementById('sidebar-backdrop');
      if (sidebar && backdrop) {
        sidebar.classList.remove('open');
        backdrop.classList.remove('active');
      }
    });
  });

  // Toggle de menú móvil
  const mobileBtn = document.getElementById('mobile-menu-btn');
  const sidebar = document.querySelector('.sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  if (mobileBtn && sidebar && backdrop) {
    mobileBtn.addEventListener('click', () => {
      sidebar.classList.toggle('open');
      backdrop.classList.toggle('active');
    });
    backdrop.addEventListener('click', () => {
      sidebar.classList.remove('open');
      backdrop.classList.remove('active');
    });
  }

  // Listener de cambios en el hash de la URL (evitar re-ejecución si la vista ya está activa)
  window.addEventListener('hashchange', () => {
    const hash = window.location.hash.slice(1);
    if (hash && VALID_VIEWS.includes(hash)) {
      if (AppState.activeView !== hash) {
        switchView(hash, false);
      }
    }
  });

  // Cerrar modales al hacer clic fuera del diálogo (excepto modales con formularios o .modal-static)
  document.querySelectorAll('.modal-backdrop, .modal-overlay').forEach(modal => {
    modal.addEventListener('click', (e) => {
      if (e.target === modal) {
        const hasForm = modal.querySelector('form') !== null;
        const isStatic = modal.classList.contains('modal-static') || hasForm;
        if (isStatic) {
          const dialog = modal.querySelector('.modal-dialog, .modal-dialog-preview');
          if (dialog) {
            dialog.classList.remove('modal-pulse');
            void dialog.offsetWidth;
            dialog.classList.add('modal-pulse');
            setTimeout(() => {
              dialog.classList.remove('modal-pulse');
            }, 250);
          }
          return;
        }
        closeModal(modal.id);
      }
    });
  });

  function isFilesDragEvent(e) {
    if (!e.dataTransfer || !e.dataTransfer.types) return false;
    const types = e.dataTransfer.types;
    return types.includes ? types.includes('Files') : Array.from(types).includes('Files');
  }

  // Cerrar modal y previsualizador de forma inmediata al presionar Escape (excepto estáticos/formularios)
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      document.querySelectorAll('.modal-backdrop.active, .modal-backdrop.open, .modal-overlay.active, .modal-overlay.open').forEach(modal => {
        const hasForm = modal.querySelector('form') !== null;
        const isStatic = modal.classList.contains('modal-static') || hasForm;
        if (isStatic) {
          const dialog = modal.querySelector('.modal-dialog, .modal-dialog-preview');
          if (dialog) {
            dialog.classList.remove('modal-pulse');
            void dialog.offsetWidth;
            dialog.classList.add('modal-pulse');
            setTimeout(() => {
              dialog.classList.remove('modal-pulse');
            }, 250);
          }
          return;
        }
        closeModal(modal.id);
      });
      const dz = document.getElementById('file-dropzone-container');
      if (dz) dz.classList.remove('drag-active');
    }
  });

  // Inicializar componentes interactivos
  initTerminal();
  populateShareGroupOptions();

  // Configurar listeners de la dropzone reactiva para Explorador de Archivos
  const dropzone = document.getElementById('file-dropzone-container');
  if (dropzone) {
    let dragCounter = 0;
    dropzone.addEventListener('dragenter', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounter++;
      dropzone.classList.add('drag-active');
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    }, false);

    dropzone.addEventListener('dragover', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
      if (!dropzone.classList.contains('drag-active')) {
        dropzone.classList.add('drag-active');
      }
    }, false);

    dropzone.addEventListener('dragleave', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounter--;
      if (dragCounter <= 0) {
        dragCounter = 0;
        dropzone.classList.remove('drag-active');
      }
    }, false);

    dropzone.addEventListener('drop', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounter = 0;
      dropzone.classList.remove('drag-active');
      const dt = e.dataTransfer;
      const files = dt ? dt.files : null;
      if (files && files.length > 0) {
        uploadFiles(files);
      }
    }, false);
  }

  // Deseleccionar al hacer clic en fondo libre de la cuadricula
  const gridContainer = document.getElementById('files-grid-container');
  if (gridContainer) {
    gridContainer.addEventListener('click', (e) => {
      if (e.target === gridContainer) {
        document.querySelectorAll('.file-card.selected').forEach(c => c.classList.remove('selected'));
        AppState.files.selectedItemPath = null;
      }
    });
  }

  // Prevenir navegación accidental del navegador al arrastrar fuera de la dropzone
  window.addEventListener('dragover', (e) => {
    if (AppState.activeView === 'files') {
      e.preventDefault();
    }
  }, false);
  window.addEventListener('drop', (e) => {
    if (AppState.activeView === 'files') {
      e.preventDefault();
    }
  }, false);

  // Inicializar estado del explorador de archivos y papelera
  setFileViewMode(AppState.files.viewMode, false);
  updateTrashBadge();
  loadStorageUnits();
  initSystemNotifications();

  // Cerrar drawer de notificaciones al hacer clic fuera
  document.addEventListener('click', (e) => {
    const drawer = document.getElementById('notifications-drawer');
    const btn = document.getElementById('btn-notifications');
    if (drawer && drawer.style.display !== 'none' && !drawer.contains(e.target) && !btn?.contains(e.target)) {
      drawer.style.display = 'none';
    }
  });

  // Determinar vista inicial
  const hash = window.location.hash ? window.location.hash.slice(1) : '';
  const initialView = (hash && VALID_VIEWS.includes(hash))
    ? hash
    : (window.SERVER_ACTIVE_VIEW && VALID_VIEWS.includes(window.SERVER_ACTIVE_VIEW)
      ? window.SERVER_ACTIVE_VIEW
      : 'dashboard');

  switchView(initialView, false);

  // Polling ligero de métricas cada 30 segundos
  AppState.refreshInterval = setInterval(() => {
    if (AppState.activeView === 'dashboard') {
      refreshDashboardMetrics();
    }
  }, 30000);
}

// ==============================================================================
// 15. Notificaciones y Alertas Persistentes del Sistema
// ==============================================================================
let _systemNotifications = [];

function initSystemNotifications() {
  try {
    const saved = localStorage.getItem('nas_system_notifications');
    if (saved) {
      _systemNotifications = JSON.parse(saved);
    }
  } catch (e) { }
  renderSystemNotifications();
}

function toggleNotificationsDrawer() {
  const drawer = document.getElementById('notifications-drawer');
  if (!drawer) return;
  const isHidden = (drawer.style.display === 'none' || !drawer.style.display);
  drawer.style.display = isHidden ? 'block' : 'none';
}

function clearNotifications() {
  _systemNotifications = [];
  try {
    localStorage.removeItem('nas_system_notifications');
  } catch (e) { }
  renderSystemNotifications();
}

function addSystemNotification(item) {
  if (!item.id) item.id = 'notif_' + Date.now() + '_' + Math.random().toString(36).substr(2, 5);
  if (!item.time) item.time = new Date().toLocaleTimeString();

  const idx = _systemNotifications.findIndex(x => x.id === item.id);
  if (idx >= 0) {
    _systemNotifications[idx] = item;
  } else {
    _systemNotifications.unshift(item);
  }
  if (_systemNotifications.length > 25) {
    _systemNotifications = _systemNotifications.slice(0, 25);
  }
  try {
    localStorage.setItem('nas_system_notifications', JSON.stringify(_systemNotifications));
  } catch (e) { }
  renderSystemNotifications();
}

function renderSystemNotifications() {
  const badge = document.getElementById('notifications-badge');
  const list = document.getElementById('notifications-list');
  const count = _systemNotifications.length;

  if (badge) {
    badge.textContent = count;
    badge.style.display = count > 0 ? 'inline-block' : 'none';
  }

  if (list) {
    if (count === 0) {
      list.innerHTML = '<div style="text-align:center; padding:16px 0;">No hay notificaciones pendientes</div>';
    } else {
      list.innerHTML = _systemNotifications.map(n => {
        const typeColor = n.type === 'danger' ? 'var(--accent-danger)' : (n.type === 'warning' ? 'var(--accent-warning)' : 'var(--accent-primary)');
        return `
          <div style="border-left:3px solid ${typeColor}; padding:6px 8px; background:rgba(255,255,255,0.03); border-radius:4px;">
            <div style="display:flex; justify-content:space-between; align-items:center;">
              <strong style="color:var(--text-primary); font-size:12px;">${escapeHtml(n.title)}</strong>
              <span style="font-size:10px; color:var(--text-muted);">${escapeHtml(n.time)}</span>
            </div>
            <div style="font-size:11px; margin-top:3px; color:var(--text-secondary);">${escapeHtml(n.message)}</div>
          </div>
        `;
      }).join('');
    }
  }

  renderSystemBanners();
}

function renderSystemBanners() {
  const container = document.getElementById('system-banners');
  if (!container) return;

  const urgent = _systemNotifications.filter(n => n.sticky || n.type === 'danger' || n.type === 'warning');
  if (urgent.length === 0) {
    container.innerHTML = '';
    container.style.display = 'none';
    return;
  }

  container.style.display = 'flex';
  container.style.flexDirection = 'column';
  container.style.gap = '8px';
  container.innerHTML = urgent.map(n => {
    const isDanger = n.type === 'danger';
    const boxClass = isDanger ? 'alert-box-danger' : 'alert-box-warning';
    const iconHref = isDanger ? '#icon-x-circle' : '#icon-alert-triangle';
    return `
      <div class="alert-box ${boxClass}" style="display:flex; justify-content:space-between; align-items:center; margin:0;">
        <div style="display:flex; align-items:center; gap:8px;">
          <svg class="icon icon-sm" style="flex-shrink:0;"><use href="${iconHref}"></use></svg>
          <div>
            <strong>${escapeHtml(n.title)}:</strong> <span>${escapeHtml(n.message)}</span>
          </div>
        </div>
        <button type="button" class="btn btn-secondary btn-xs" onclick="dismissBanner('${escapeHtml(n.id)}')" style="padding:2px 8px; font-size:11px;">Entendido</button>
      </div>
    `;
  }).join('');
}

function dismissBanner(id) {
  _systemNotifications = _systemNotifications.filter(n => n.id !== id);
  try {
    localStorage.setItem('nas_system_notifications', JSON.stringify(_systemNotifications));
  } catch (e) { }
  renderSystemNotifications();
}

function toggleSidebar() {
  const sidebar = document.querySelector('.sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  if (sidebar) sidebar.classList.toggle('open');
  if (backdrop) backdrop.classList.toggle('active');
}

// ==============================================================================
// 16. Aliases Globales para Compatibilidad y Eventos Onclick
// ==============================================================================
// Cabecera y Navegación
window.toggleSidebar = toggleSidebar;
window.toggleNotificationsDrawer = toggleNotificationsDrawer;
window.toggleNotificationDrawer = toggleNotificationsDrawer;
window.clearNotifications = clearNotifications;
window.emptyNotifications = clearNotifications;
window.markAllAsRead = clearNotifications;
window.logout = logoutSession;
window.logoutSession = logoutSession;
window.refreshDashboardMetrics = refreshDashboardMetrics;
window.toggleTheme = toggleTheme;

// Almacenamiento
window.openStorageManage = openStorageManage;
window.openFormatDisk = openFormatDisk;
window.runStorageScrub = runStorageScrub;
window.confirmAuditBtrfs = confirmAuditBtrfs;
window.runStorageTrim = runStorageTrim;
window.loadStorage = loadStorage;
window.loadStorageUnits = loadStorageUnits;
window.submitStorageManage = submitStorageManage;
window.toggleStorageOp = toggleStorageOp;
window.onStorageDeviceChange = onStorageDeviceChange;

// Recursos Compartidos (Samba)
window.openNewShare = openNewShare;
window.openNewShareModal = openNewShare;
window.openEditShare = openEditShare;
window.openSharePermissions = openSharePermissions;
window.deleteShare = deleteShare;
window.copyUncPath = copyUncPath;
window.copyShareUnc = copyShareUnc;
window.repairSambaPermissions = repairSambaPermissions;
window.repairAcls = repairAcls;
window.submitNewShare = submitNewShare;
window.submitEditShare = submitEditShare;
window.toggleSchemeFields = toggleSchemeFields;
window.toggleEditSchemeFields = toggleEditSchemeFields;

// Central de Respaldos (Backups)
window.openNewBackupModal = () => openModal('modal-new-backup');
window.openNewBackup = () => openModal('modal-new-backup');
window.openEditBackup = openEditBackup;
window.editBackup = openEditBackup;
window.runBackupNow = runBackupTask;
window.runBackupTask = runBackupTask;
window.stopBackup = stopBackupTask;
window.stopBackupTask = stopBackupTask;
window.viewBackupLog = viewBackupLogs;
window.viewBackupLogs = viewBackupLogs;
window.deleteBackup = deleteBackupTask;
window.deleteBackupTask = deleteBackupTask;
window.testBackupConnection = testBackupConnection;
window.testEditBackupConnection = testEditBackupConnection;
window.submitNewBackup = submitNewBackup;
window.submitEditBackup = submitEditBackup;
window.toggleBackupFields = toggleBackupFields;
window.toggleEditBackupFields = toggleEditBackupFields;

// Explorador de Archivos y Papelera
window.triggerFileUpload = triggerFileInput;
window.triggerFileInput = triggerFileInput;
window.openNewFolderModal = openNewFolderModal;
window.openNewFolder = openNewFolderModal;
window.downloadFolderZip = downloadFolderZip;
window.downloadCurrentFolderZip = downloadCurrentFolderZip;
window.openEmptyTrashModal = openEmptyTrashModal;
window.emptyTrash = openEmptyTrashModal;
window.confirmEmptyTrash = confirmEmptyTrash;
window.toggleTrashView = toggleTrashView;
window.exitTrashView = exitTrashView;
window.restoreTrashItem = restoreTrashItem;
window.deleteTrashItem = deleteTrashItem;
window.handleTrashAction = handleTrashAction;
window.refreshCurrentFileView = refreshCurrentFileView;
window.setFileViewMode = setFileViewMode;
window.changeFilesRoot = changeFilesRoot;
window.navigateToSubpath = navigateToSubpath;
window.downloadFile = downloadFile;
window.openRenameModal = openRenameModal;
window.submitRenameFile = submitRenameFile;
window.openMoveModal = openMoveModal;
window.submitMoveFile = submitMoveFile;
window.openDeleteModal = openDeleteModal;
window.confirmDeleteFile = confirmDeleteFile;
window.previewFile = previewFile;
window.openFilePreview = previewFile;
window.copyPreviewContent = copyPreviewContent;
window.togglePreviewEditMode = togglePreviewEditMode;
window.saveCurrentPreviewFile = saveCurrentPreviewFile;
window.downloadPreviewFile = downloadPreviewFile;
window.navigatePptxSlide = navigatePptxSlide;

// Usuarios y Grupos
window.openNewUser = openNewUser;
window.openNewUserModal = openNewUser;
window.openNewGroup = () => openModal('modal-new-group');
window.openNewGroupModal = () => openModal('modal-new-group');
window.repairUserAcls = repairAcls;
window.openEditUser = openEditUser;
window.toggleUser = toggleUser;
window.deleteUser = deleteUser;
window.openGroupMembers = openGroupMembers;
window.openGroupMembersModal = openGroupMembers;
window.openRenameGroup = openRenameGroup;
window.deleteGroup = deleteGroup;
window.generateAndSetPassword = generateAndSetPassword;
window.togglePasswordVisibility = togglePasswordVisibility;
window.setUsersFilter = setUsersFilter;
window.applyUsersFilters = applyUsersFilters;
window.submitNewUser = submitNewUser;
window.submitUpdateUser = submitUpdateUser;
window.submitEditUser = submitUpdateUser;
window.submitNewGroup = submitNewGroup;
window.submitRenameGroup = submitRenameGroup;
window.addGroupMember = addGroupMember;
window.submitAddGroupMember = addGroupMember;
window.removeGroupMember = removeGroupMember;
window.setPermTab = setPermTab;
window.loadAccessMatrix = loadAccessMatrix;
window.cyclePerm = cyclePerm;
window.toggleAccessMatrixCell = cyclePerm;

// Servicios y Diagnóstico
window.loadServices = loadServices;
window.loadDiagnostics = loadDiagnostics;
window.startService = startService;
window.stopService = stopService;
window.reloadService = reloadService;
window.restartService = restartService;
window.manageServiceAction = manageServiceAction;
window.runDiagnosticTest = runDiagnosticTest;

// Aplicaciones y Actualizaciones
window.loadUpdates = loadUpdates;
window.checkSystemUpdates = loadUpdates;
window.applySystemUpdate = applySystemUpdate;
window.triggerSystemUpdate = applySystemUpdate;
window.loadApplications = loadApplications;

// Auditoría y Registros
window.loadLogs = loadLogs;
window.setLogSource = setLogSource;
window.debounceLogSearch = debounceLogSearch;
window.exportOrCopyLogs = exportOrCopyLogs;

// Consola Terminal
window.loadTerminal = loadTerminal;
window.clearTerminal = clearTerminal;
window.reconnectTerminal = reconnectTerminal;
window.resizeTerminal = resizeTerminal;
window.sendTerminalKey = sendTerminalKey;
window.runQuickCommand = runQuickCommand;

// Dominio Active Directory
window.loadDomainStatus = loadDomainStatus;
window.handleDomainDiscover = handleDomainDiscover;
window.testDomainConnection = handleDomainDiscover;
window.handleDomainJoin = handleDomainJoin;
window.joinDomain = handleDomainJoin;
window.handleDomainLeave = handleDomainLeave;
window.leaveDomain = handleDomainLeave;

// Modales Generales
window.openModal = openModal;
window.closeModal = closeModal;
window.confirmRebootServer = confirmRebootServer;

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', initApp);
} else {
  initApp();
}
