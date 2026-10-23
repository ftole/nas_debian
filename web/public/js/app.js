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
    displayItems: [],
    filterQuery: '',
    sortKey: 'name_asc',
    viewMode: localStorage.getItem('nas_file_view') || 'grid',
    inTrash: false,
    trashItems: [],
    trashFilterQuery: '',
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
let _pendingStoragePayload = null;
let _pendingStorageUrl = null;

function renderStorageOverview(overview) {
  if (!overview) return;
  const pathEl = document.getElementById('stg-main-path');
  if (pathEl) pathEl.textContent = overview.path || '/srv/nas';

  const statTotal = document.getElementById('stg-stat-total');
  if (statTotal) statTotal.textContent = (overview.total_gb !== undefined && overview.total_gb !== null) ? `${overview.total_gb} GB` : '-- GB';

  const statUsed = document.getElementById('stg-stat-used');
  if (statUsed) {
    const usedGb = overview.used_gb ?? '--';
    const pct = overview.usage_percent ?? 0;
    statUsed.textContent = `${usedGb} GB (${pct}%)`;
  }

  const statFree = document.getElementById('stg-stat-free');
  if (statFree) statFree.textContent = (overview.free_gb !== undefined && overview.free_gb !== null) ? `${overview.free_gb} GB` : '-- GB';

  const statFs = document.getElementById('stg-stat-fs');
  if (statFs) {
    const fsName = (overview.filesystem || 'N/A').toUpperCase();
    const devType = overview.device_type || 'HDD';
    statFs.textContent = `${fsName} · ${devType}`;
  }

  const statPercent = document.getElementById('stg-stat-percent');
  if (statPercent) statPercent.textContent = `${overview.usage_percent ?? 0}%`;

  const progress = document.getElementById('stg-main-progress');
  const badge = document.getElementById('stg-threshold-badge');
  const alertBox = document.getElementById('stg-threshold-alert');
  const alertText = document.getElementById('stg-threshold-text');

  const pct = Number(overview.usage_percent ?? 0);
  if (progress) {
    progress.style.width = `${Math.min(100, Math.max(0, pct))}%`;
  }

  if (pct >= 90) {
    if (progress) progress.style.backgroundColor = 'var(--accent-danger)';
    if (badge) {
      badge.className = 'badge badge-err';
      badge.textContent = `Crítico (${pct}%)`;
    }
    if (alertBox && alertText) {
      alertBox.className = 'alert-box alert-box-danger';
      alertBox.style.display = 'flex';
      alertText.textContent = `Capacidad crítica superada (${pct}% en uso). A partir del 95% de uso las tareas de respaldo se cancelarán para proteger la integridad del sistema.`;
    }
  } else if (pct >= 80) {
    if (progress) progress.style.backgroundColor = 'var(--accent-warning)';
    if (badge) {
      badge.className = 'badge badge-warn';
      badge.textContent = `Alerta preventiva (${pct}%)`;
    }
    if (alertBox && alertText) {
      alertBox.className = 'alert-box alert-box-warning';
      alertBox.style.display = 'flex';
      alertText.textContent = `Umbral preventivo del 80% superado (${pct}% en uso). Al superar el 85%, el subsistema de respaldos generará alertas operativas.`;
    }
  } else {
    if (progress) progress.style.backgroundColor = 'var(--accent-primary)';
    if (badge) {
      badge.className = 'badge badge-ok';
      badge.textContent = `Normal (${pct}%)`;
    }
    if (alertBox) alertBox.style.display = 'none';
  }
}

async function loadStorageSubvolumes(overview) {
  const panel = document.getElementById('stg-subvols-panel');
  const tbody = document.getElementById('storage-subvols-body');
  if (!panel || !tbody) return;

  const isBtrfs = Boolean(overview?.is_btrfs);
  if (!isBtrfs) {
    panel.style.display = 'none';
    return;
  }

  panel.style.display = 'block';
  const pathEl = document.getElementById('stg-subvols-path');
  if (pathEl) pathEl.textContent = overview.path || '/srv/nas';

  try {
    const res = await apiFetch('/api/storage/subvolumes?path=' + encodeURIComponent(overview.path || '/srv/nas'), { silentToast: true });
    const subvols = res.data?.subvolumes || res.subvolumes || [];

    if (subvols.length === 0) {
      tbody.innerHTML = '<tr><td colspan="4" style="text-align:center; color:var(--text-muted);">No se detectaron subvolúmenes Btrfs secundarios en este volumen.</td></tr>';
      return;
    }

    tbody.innerHTML = subvols.map(sv => `
      <tr>
        <td><code>${escapeHtml(sv.id || '--')}</code></td>
        <td><strong>${escapeHtml(sv.name || '')}</strong></td>
        <td><code>${escapeHtml(sv.path || '')}</code></td>
        <td><span class="badge badge-ok">Subvolumen Btrfs</span></td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="4" style="text-align:center; color:var(--accent-danger);">Error al consultar subvolúmenes: ${escapeHtml(e.message)}</td></tr>`;
  }
}

function openCreateSubvolumeModal() {
  openStorageManage();
  const opSel = document.getElementById('stg-op');
  if (opSel) {
    opSel.value = 'subvolume';
    toggleStorageOp();
  }
}

async function loadStorage() {
  const tbody = document.getElementById('storage-disks-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/storage/disks');
    const disks = res.data?.disks ?? res.disks ?? [];
    const overview = res.data?.overview ?? res.overview ?? {};

    // Actualizar tarjeta principal de capacidad y estado
    renderStorageOverview(overview);

    // Actualizar tarjetas de inteligencia BTRFS y TRIM
    updateStorageIntelligence(overview);

    // Cargar unidades montadas y subvolúmenes
    loadStorageUnits();
    loadStorageSubvolumes(overview);

    if (disks.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" style="text-align:center;">No se detectaron discos de bloque adicionales.</td></tr>';
      return;
    }

    tbody.innerHTML = disks.map(d => {
      const devPath = d.device || ('/dev/' + d.name);
      const isPart = d.type === 'part';
      const nameHtml = isPart
        ? `<span style="font-family:monospace; color:var(--text-muted); margin-right:4px;">└─</span> <strong>${escapeHtml(d.name)}</strong>`
        : `<strong>${escapeHtml(d.name)}</strong>`;

      let stateBadge = '';
      if (d.mount === '/srv/nas') {
        stateBadge = ' <span class="badge badge-ok">Pool NAS</span>';
      } else if (d.protected) {
        stateBadge = ` <span class="badge badge-err">SO · ${escapeHtml(d.mount || 'protegido')}</span>`;
      } else if (d.in_use) {
        stateBadge = ' <span class="badge badge-warn">En uso</span>';
      } else {
        stateBadge = ' <span class="badge badge-ok">Disponible</span>';
      }

      const typeBadge = isPart
        ? '<span class="badge badge-gray">Partición</span>'
        : '<span class="badge badge-gray">Disco</span>';

      return `
      <tr>
        <td>${nameHtml}${stateBadge}</td>
        <td>${escapeHtml(d.model || 'Genérico')}</td>
        <td>${escapeHtml(d.size)}</td>
        <td>${typeBadge}</td>
        <td><code>${escapeHtml(d.mount || 'Sin montar')}</code></td>
        <td>${escapeHtml(d.fstype || 'N/A')}</td>
        <td style="text-align:right;">
          ${!d.protected ? `<button type="button" class="btn btn-secondary btn-sm" onclick="openStorageManage('${escapeHtml(devPath)}')"><svg class="icon icon-sm"><use href="#icon-wrench"></use></svg> Gestionar</button>` : '<span class="text-muted" style="font-size:11px;">Protegido por SO</span>'}
        </td>
      </tr>
    `;}).join('');
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
      scrubBtn.disabled = true;
      scrubBtn.classList.add('disabled');
      scrubBtn.title = 'Requiere sistema de archivos BTRFS (actual: ' + fsType + ')';
    }
  } else {
    if (scrubBtn) {
      scrubBtn.disabled = false;
      scrubBtn.classList.remove('disabled');
      scrubBtn.title = 'Iniciar auditoría criptográfica BTRFS contra Bit Rot';
    }
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
      trimBtn.disabled = true;
      trimBtn.classList.add('disabled');
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
      trimBtn.disabled = false;
      trimBtn.classList.remove('disabled');
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
        const hasErrors = d.error_summary && !d.error_summary.includes('no errors') && !d.error_summary.includes('0');
        const errBadge = hasErrors
          ? `<span class="badge badge-err" style="margin-left:8px;">${escapeHtml(d.error_summary)}</span>`
          : '<span class="badge badge-ok" style="margin-left:8px;">0 errores detectados</span>';

        const metaParts = [];
        if (d.rate) metaParts.push(`Velocidad: <strong>${escapeHtml(d.rate)}</strong>`);
        if (d.duration) metaParts.push(`Duración: <strong>${escapeHtml(d.duration)}</strong>`);
        if (d.total_to_scrub) metaParts.push(`Examinado: <strong>${escapeHtml(d.total_to_scrub)}</strong>`);

        const metaHtml = metaParts.length
          ? `<div style="display:flex; gap:12px; margin-top:6px; font-size:11px; color:var(--text-secondary);">${metaParts.join(' · ')}</div>`
          : '';

        scrubInfo.innerHTML = `
          <div style="display:flex; flex-direction:column; width:100%;">
            <div style="display:flex; justify-content:space-between; align-items:center;">
              <span>${escapeHtml(d.summary || 'Auditoría finalizada.')}</span>
              ${errBadge}
            </div>
            ${metaHtml}
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

    const modalPath = document.getElementById('trim-modal-path');
    if (modalPath) modalPath.textContent = overview.path || '/srv/nas';
    openModal('modal-trim-ssd');
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function confirmTrimSsd() {
  closeModal('modal-trim-ssd');
  const btn = document.getElementById('btn-trim-storage');
  const confirmBtn = document.getElementById('btn-confirm-trim-ssd');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  if (confirmBtn) { confirmBtn.disabled = true; confirmBtn.classList.add('is-loading'); }

  try {
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
    if (confirmBtn) { confirmBtn.disabled = false; confirmBtn.classList.remove('is-loading'); }
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
        _pendingStorageUrl = url;
        _pendingStoragePayload = payload;
        openModal('modal-storage-unmount-confirm');
        return;
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

async function executeStorageUnmountAndProceed() {
  closeModal('modal-storage-unmount-confirm');
  if (!_pendingStorageUrl || !_pendingStoragePayload) return;

  const btn = document.getElementById('btn-submit-storage');
  const confirmBtn = document.getElementById('btn-confirm-unmount-storage');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  if (confirmBtn) { confirmBtn.disabled = true; confirmBtn.classList.add('is-loading'); }

  try {
    await apiFetch(_pendingStorageUrl, {
      method: 'POST',
      body: JSON.stringify({ ..._pendingStoragePayload, unmount: true }),
      silentToast: true,
    });
    showToast('Dispositivo desmontado y operación completada exitosamente.', 'success');
    closeModal('modal-storage-manage');
    const confirmInput = document.getElementById('stg-confirm');
    if (confirmInput) confirmInput.value = '';
    loadStorage();
    loadStorageUnits();
  } catch (e) {
    showToast(e.message || 'No se pudo completar la operación con desmontaje forzado.', 'error');
  } finally {
    _pendingStoragePayload = null;
    _pendingStorageUrl = null;
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
    if (confirmBtn) { confirmBtn.disabled = false; confirmBtn.classList.remove('is-loading'); }
  }
}

let _storageUnitsData = [];

async function loadStorageUnits() {
  try {
    const res = await apiFetch('/api/storage/units', { silentToast: true });
    const units = res.data?.units || res.units || [];
    _storageUnitsData = units;

    // Poblar tabla en la vista de almacenamiento (#storage-units-body)
    const unitsTbody = document.getElementById('storage-units-body');
    if (unitsTbody) {
      if (units.length === 0) {
        unitsTbody.innerHTML = '<tr><td colspan="7" style="text-align:center; color:var(--text-muted);">No hay unidades de almacenamiento montadas.</td></tr>';
      } else {
        unitsTbody.innerHTML = units.map(u => {
          const used = u.used_gb ?? u.used ?? '0';
          const total = u.total_gb ?? u.total ?? '0';
          const free = u.free_gb ?? u.free ?? '0';
          const pct = Math.round(Number(u.usage_percent ?? 0));
          const roleBadge = u.is_main
            ? '<span class="badge badge-primary">Principal</span>'
            : '<span class="badge badge-info">Adicional</span>';

          const progressColor = pct >= 90 ? 'var(--accent-danger)' : (pct >= 80 ? 'var(--accent-warning)' : 'var(--accent-primary)');

          return `
          <tr>
            <td><strong>${escapeHtml(u.name || u.id)}</strong> <small class="text-muted">(${escapeHtml(u.id)})</small></td>
            <td><code>${escapeHtml(u.path)}</code></td>
            <td><span class="badge badge-gray">${escapeHtml((u.filesystem || 'ext4').toUpperCase())}</span></td>
            <td>${escapeHtml(String(used))} GB / ${escapeHtml(String(total))} GB</td>
            <td><strong style="color:var(--accent-success-text);">${escapeHtml(String(free))} GB</strong></td>
            <td>
              <div style="display:flex; align-items:center; gap:8px;">
                <div style="flex:1; height:6px; background:var(--bg-body); border:1px solid var(--border-color); border-radius:3px; overflow:hidden; min-width:60px;">
                  <div style="height:100%; width:${Math.min(100, Math.max(0, pct))}%; background:${progressColor};"></div>
                </div>
                <span style="font-size:11px; font-weight:600;">${pct}%</span>
              </div>
            </td>
            <td style="text-align:right;">${roleBadge}</td>
          </tr>`;
        }).join('');
      }
    }

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
let _lastCreatedUserMemo = null;

function toggleUserGroupsExpand(btnEl) {
  if (typeof btnEl === 'string') {
    // Compatibilidad si se llama con username
    const el = document.querySelector(`.user-groups-cell[data-username="${CSS.escape(btnEl)}"] .tag-pill-more`);
    if (el) toggleUserGroupsExpand(el);
    return;
  }
  const cell = btnEl?.closest('.user-groups-cell');
  if (!cell) return;
  const moreSpan = cell.querySelector('.user-groups-extra');
  if (!moreSpan) return;
  const isHidden = moreSpan.style.display === 'none';
  moreSpan.style.display = isHidden ? 'inline' : 'none';
  btnEl.textContent = isHidden ? '« Menos' : (btnEl.dataset.originalText || '+más');
}

function renderUserGroupPills(username, groups) {
  const allGroups = (groups || []).filter(g => g.startsWith('grp_') || ['sudo', 'adm'].includes(g));
  if (!allGroups.length) {
    return '<span style="color:var(--text-muted); font-size:12px;"><em>Sin grupos</em></span>';
  }
  if (allGroups.length <= 2) {
    return `<div class="user-groups-cell" data-username="${escapeHtml(username)}">${allGroups.map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ')}</div>`;
  }
  const visible = allGroups.slice(0, 2).map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ');
  const extra = allGroups.slice(2).map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ');
  const remaining = allGroups.length - 2;
  return `
    <div class="user-groups-cell" data-username="${escapeHtml(username)}">
      ${visible}
      <span class="user-groups-extra" style="display:none;"> ${extra}</span>
      <span class="tag-pill tag-pill-more" data-original-text="+${remaining} más" onclick="toggleUserGroupsExpand(this)">+${remaining} más</span>
    </div>`;
}

function copyPasswordInput(inputId) {
  const input = document.getElementById(inputId);
  if (!input || !input.value) {
    showToast('No hay contraseña para copiar.', 'warning');
    return;
  }
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(input.value).then(() => {
      showToast('Contraseña copiada al portapapeles.', 'success');
    }).catch(() => {
      input.select();
      document.execCommand('copy');
      showToast('Contraseña copiada.', 'success');
    });
  } else {
    input.select();
    document.execCommand('copy');
    showToast('Contraseña copiada.', 'info');
  }
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

let _currentUsersTab = 'users';
let _pendingUserActionCallback = null;

function switchUsersTab(tab) {
  _currentUsersTab = tab;
  ['users', 'groups', 'perms'].forEach(t => {
    const btn = document.getElementById(`usr-tab-btn-${t}`);
    const panel = document.getElementById(`usr-panel-${t}`);
    if (btn) btn.classList.toggle('active', t === tab);
    if (panel) panel.style.display = (t === tab) ? 'block' : 'none';
  });
  if (tab === 'perms') {
    loadAccessMatrix();
  }
}

function updateUsersKpis(users, groups) {
  const uList = users || [];
  const gList = groups || [];

  const kpiTotal = document.getElementById('usr-kpi-total');
  if (kpiTotal) kpiTotal.textContent = `${uList.length} cuentas`;

  const superadmins = uList.filter(u => u.is_superadmin).length;
  const admins = uList.filter(u => u.is_admin && !u.is_superadmin).length;
  const operators = uList.filter(u => u.can_web && !u.is_admin).length;
  const standards = uList.filter(u => !u.is_admin && !u.can_web).length;

  const descParts = [];
  if (superadmins) descParts.push(`${superadmins} Superadmin`);
  if (admins) descParts.push(`${admins} Admin`);
  if (operators) descParts.push(`${operators} Operador`);
  if (standards) descParts.push(`${standards} Empleado`);

  const kpiTotalDesc = document.getElementById('usr-kpi-total-desc');
  if (kpiTotalDesc) kpiTotalDesc.textContent = descParts.join(' · ') || 'Cuentas Linux / Samba';

  const activeCount = uList.filter(u => (u.samba_enabled ?? u.enabled) !== false).length;
  const blockedCount = uList.length - activeCount;

  const kpiActive = document.getElementById('usr-kpi-active');
  if (kpiActive) {
    kpiActive.textContent = `${activeCount} Activas`;
    kpiActive.style.color = blockedCount > 0 ? 'var(--accent-warning)' : 'var(--accent-success-text)';
  }

  const kpiBlockedDesc = document.getElementById('usr-kpi-blocked-desc');
  if (kpiBlockedDesc) {
    kpiBlockedDesc.textContent = blockedCount > 0 ? `${blockedCount} cuenta(s) suspendida(s)` : 'Todas las cuentas operativas';
  }

  const sambaCount = uList.filter(u => !!u.is_samba).length;
  const noSambaCount = uList.length - sambaCount;

  const kpiSamba = document.getElementById('usr-kpi-samba');
  if (kpiSamba) kpiSamba.textContent = `${sambaCount} con SMB`;

  const kpiNoSambaDesc = document.getElementById('usr-kpi-nosamba-desc');
  if (kpiNoSambaDesc) kpiNoSambaDesc.textContent = noSambaCount > 0 ? `${noSambaCount} sin sincronización Samba` : '100% sincronizados en Samba';

  const groupsWithMembers = gList.filter(g => (g.members || []).length > 0).length;
  const kpiGroups = document.getElementById('usr-kpi-groups');
  if (kpiGroups) kpiGroups.textContent = `${gList.length} grupos`;

  const kpiGroupsDesc = document.getElementById('usr-kpi-groups-desc');
  if (kpiGroupsDesc) kpiGroupsDesc.textContent = `${groupsWithMembers} con miembros asignados`;

  const tabUserCount = document.getElementById('usr-tab-count');
  if (tabUserCount) tabUserCount.textContent = String(uList.length);

  const tabGroupCount = document.getElementById('grp-tab-count');
  if (tabGroupCount) tabGroupCount.textContent = String(gList.length);

  updateUserChipCounters(uList);
}

function updateUserChipCounters(users) {
  const uList = users || [];
  const cAll = document.getElementById('chip-count-all');
  if (cAll) cAll.textContent = String(uList.length);

  const cAdmin = document.getElementById('chip-count-admin');
  if (cAdmin) cAdmin.textContent = String(uList.filter(u => u.is_admin || u.is_superadmin).length);

  const cOperator = document.getElementById('chip-count-operator');
  if (cOperator) cOperator.textContent = String(uList.filter(u => u.can_web && !u.is_admin).length);

  const cStandard = document.getElementById('chip-count-standard');
  if (cStandard) cStandard.textContent = String(uList.filter(u => !u.is_admin && !u.can_web).length);

  const cBlocked = document.getElementById('chip-count-blocked');
  if (cBlocked) cBlocked.textContent = String(uList.filter(u => (u.samba_enabled ?? u.enabled) === false).length);

  const cSamba = document.getElementById('chip-count-samba');
  if (cSamba) cSamba.textContent = String(uList.filter(u => !!u.is_samba).length);
}

function showUserActionConfirm({ title, icon = 'alert-triangle', alertText, alertType = 'warning', desc, prompt = '¿Deseas proceder con esta acción?', btnText = 'Confirmar', btnClass = 'btn-danger', onConfirm }) {
  const titleEl = document.getElementById('user-action-confirm-title-text');
  if (titleEl) titleEl.textContent = title || 'Confirmar acción';

  const iconEl = document.getElementById('user-action-confirm-icon');
  if (iconEl) {
    iconEl.innerHTML = `<use href="#icon-${escapeHtml(icon)}"></use>`;
    iconEl.style.color = alertType === 'danger' ? 'var(--accent-danger)' : 'var(--accent-warning)';
  }

  const alertBox = document.getElementById('user-action-confirm-alert');
  const alertTextEl = document.getElementById('user-action-confirm-alert-text');
  if (alertBox && alertTextEl) {
    alertBox.className = `alert-box alert-box-${alertType}`;
    alertTextEl.textContent = alertText || '';
  }

  const descEl = document.getElementById('user-action-confirm-desc');
  if (descEl) descEl.textContent = desc || '';

  const promptEl = document.getElementById('user-action-confirm-prompt');
  if (promptEl) promptEl.textContent = prompt;

  const submitBtn = document.getElementById('btn-user-action-confirm-submit');
  if (submitBtn) {
    submitBtn.textContent = btnText;
    submitBtn.className = `btn ${btnClass}`;
  }

  _pendingUserActionCallback = onConfirm;
  openModal('modal-user-action-confirm');
}

async function executeUserActionConfirm() {
  if (typeof _pendingUserActionCallback !== 'function') {
    closeModal('modal-user-action-confirm');
    return;
  }
  const cb = _pendingUserActionCallback;
  _pendingUserActionCallback = null;
  closeModal('modal-user-action-confirm');
  await cb();
}

let _usersSortCol = 'username';
let _usersSortAsc = true;

function sortUsersTable(col) {
  if (_usersSortCol === col) {
    _usersSortAsc = !_usersSortAsc;
  } else {
    _usersSortCol = col;
    _usersSortAsc = true;
  }

  ['username', 'uid', 'role', 'status'].forEach(c => {
    const icon = document.getElementById(`sort-icon-${c}`);
    if (icon) {
      icon.textContent = (c === col) ? (_usersSortAsc ? '▲' : '▼') : '↕';
      icon.style.opacity = (c === col) ? '1' : '0.5';
    }
  });

  _usersData.sort((a, b) => {
    let valA, valB;
    if (col === 'uid') {
      valA = a.uid ?? 0;
      valB = b.uid ?? 0;
      return _usersSortAsc ? (valA - valB) : (valB - valA);
    } else if (col === 'role') {
      const score = u => u.is_superadmin ? 4 : (u.is_admin ? 3 : (u.can_web ? 2 : 1));
      valA = score(a);
      valB = score(b);
      return _usersSortAsc ? (valB - valA) : (valA - valB);
    } else if (col === 'status') {
      valA = (a.samba_enabled ?? a.enabled) ? 1 : 0;
      valB = (b.samba_enabled ?? b.enabled) ? 1 : 0;
      return _usersSortAsc ? (valB - valA) : (valA - valB);
    } else {
      valA = (a.username || '').toLowerCase();
      valB = (b.username || '').toLowerCase();
      return _usersSortAsc ? valA.localeCompare(valB) : valB.localeCompare(valA);
    }
  });

  renderUsersTableBody();
}

function renderUsersTableBody() {
  const usersTbody = document.getElementById('users-table-body');
  if (!usersTbody) return;

  const isAdminCaller = window.NAS_IS_ADMIN !== false;
  usersTbody.innerHTML = _usersData.map(u => {
    const isProtected = PROTECTED_USERS.includes(u.username) || !!u.is_superadmin;
    const isStandard = !u.is_admin && !u.can_web;
    const rol = u.is_superadmin
      ? '<span class="badge badge-err">Superadmin</span>'
      : (u.is_admin
        ? '<span class="badge badge-warn">Admin</span>'
        : (u.can_web ? '<span class="badge badge-blue">Operador</span>' : '<span class="badge badge-gray">Estándar</span>'));
    const netEnabled = !!(u.samba_enabled ?? u.enabled);
    const canManage = !isProtected && (isAdminCaller || !u.is_admin);

    const shellBadge = (u.shell && u.shell.includes('nologin'))
      ? '<span class="badge badge-gray" title="Sin acceso interactivo (cuenta de red Samba)">Solo Red (nologin)</span>'
      : `<span class="badge badge-ok" title="Shell interactiva habilitada">Terminal (${escapeHtml(u.shell || '/bin/bash')})</span>`;

    return `
    <tr data-username="${escapeHtml(u.username)}" data-fullname="${escapeHtml(u.full_name || '')}" data-admin="${(u.is_admin || u.is_superadmin) ? '1' : '0'}" data-operator="${(u.can_web && !u.is_admin) ? '1' : '0'}" data-standard="${isStandard ? '1' : '0'}" data-enabled="${netEnabled ? '1' : '0'}" data-samba="${u.is_samba ? '1' : '0'}" data-uid="${u.uid}" data-groups="${escapeHtml((u.groups || []).join(' '))}" data-shell="${escapeHtml(u.shell || '')}">
      <td><strong>${escapeHtml(u.username)}</strong><div style="font-size:11px; color:var(--text-muted);">${escapeHtml(u.full_name || 'Sin nombre asignado')}</div></td>
      <td><code>${u.uid}</code></td>
      <td>${rol}</td>
      <td>${shellBadge}</td>
      <td>${renderUserGroupPills(u.username, u.groups)}</td>
      <td>${netEnabled ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Suspendido</span>'}</td>
      <td>${u.is_samba ? (u.samba_enabled !== false ? '<span class="badge badge-ok">Sync</span>' : '<span class="badge badge-warn">SMB Deshab</span>') : '<span class="badge badge-gray">Sin SMB</span>'}</td>
      <td style="text-align:right; white-space:nowrap;">
        ${canManage ? `
          <button class="btn btn-secondary btn-sm" title="Editar" onclick="openEditUser('${escapeHtml(u.username)}')"><svg class="icon icon-sm"><use href="#icon-edit"></use></svg></button>
          <button class="btn btn-secondary btn-sm" title="${netEnabled ? 'Suspender acceso a red' : 'Reactivar acceso a red'}" onclick="toggleUser('${escapeHtml(u.username)}', ${netEnabled ? 'false' : 'true'})"><svg class="icon icon-sm"><use href="#icon-${netEnabled ? 'lock' : 'unlock'}"></use></svg></button>
          <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteUser('${escapeHtml(u.username)}')"><svg class="icon icon-sm"><use href="#icon-trash"></use></svg></button>
        ` : `<span class="badge badge-gray">${isProtected ? (u.is_superadmin ? 'Inmutable' : 'Protegida') : 'Solo lectura'}</span>`}
      </td>
    </tr>`;
  }).join('') || '<tr><td colspan="8" style="text-align:center;">Sin usuarios encontrados.</td></tr>';

  applyUsersFilters();
}

async function loadUsersAndGroups() {
  const groupsTbody = document.getElementById('groups-table-body');

  try {
    const [uRes, gRes] = await Promise.all([
      apiFetch('/api/users'),
      apiFetch('/api/groups'),
    ]);

    _usersData = uRes.data || [];
    _groupsData = gRes.data || [];

    // Actualizar tarjetas de KPI de usuarios y grupos
    updateUsersKpis(_usersData, _groupsData);

    renderUsersTableBody();

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

        const membersHtml = (g.members || []).map(m => {
          if (m === 'www-data') {
            return `<span class="tag-pill" style="border:1px solid var(--accent-primary); color:var(--accent-primary); font-weight:600;" title="Cuenta del servicio web necesaria para el panel">www-data (sistema)</span>`;
          }
          return `<span class="tag-pill">${escapeHtml(m)}</span>`;
        }).join(' ') || '<em style="color:var(--text-muted);">Sin miembros</em>';

        return `
        <tr data-group="${escapeHtml(g.name)}" data-members="${escapeHtml((g.members || []).join(' '))}">
          <td><strong>${escapeHtml(g.name)}</strong></td>
          <td><code>${g.gid}</code></td>
          <td>${isSpecial ? '<span class="badge badge-primary">Especial (Sistema)</span>' : '<span class="badge badge-gray">Departamental</span>'}</td>
          <td>${membersHtml}</td>
          <td style="text-align:right; white-space:nowrap;">
            ${canManageMembers ? `
              <button class="btn btn-secondary btn-sm" title="Gestionar Miembros" onclick="openGroupMembers('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-users"></use></svg></button>
            ` : ''}
            ${!isSpecial && isAdminCaller ? `
              <button class="btn btn-secondary btn-sm" title="Renombrar" onclick="openRenameGroup('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-edit"></use></svg></button>
              <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteGroup('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-trash"></use></svg></button>
            ` : ''}
            ${!canManageMembers && (!isAdminCaller || isSpecial) && !isSpecial ? '<span class="badge badge-gray">Solo lectura</span>' : ''}
          </td>
        </tr>
      `;
      }).join('') || '<tr><td colspan="5" style="text-align:center;">Sin grupos creados.</td></tr>';
      applyGroupsFilters();
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
  let visibleCount = 0;
  document.querySelectorAll('#users-table-body tr[data-username]').forEach(tr => {
    let ok = true;
    if (_usersFilter === 'admin') ok = tr.dataset.admin === '1';
    else if (_usersFilter === 'operator') ok = tr.dataset.operator === '1';
    else if (_usersFilter === 'standard') ok = tr.dataset.standard === '1';
    else if (_usersFilter === 'blocked') ok = tr.dataset.enabled !== '1';
    else if (_usersFilter === 'samba') ok = tr.dataset.samba === '1';

    if (ok && q) {
      const uName = (tr.dataset.username || '').toLowerCase();
      const fName = (tr.dataset.fullname || '').toLowerCase();
      const uid = (tr.dataset.uid || '').toLowerCase();
      const groups = (tr.dataset.groups || '').toLowerCase();
      const shell = (tr.dataset.shell || '').toLowerCase();
      ok = uName.includes(q) || fName.includes(q) || uid.includes(q) || groups.includes(q) || shell.includes(q);
    }
    tr.style.display = ok ? '' : 'none';
    if (ok) visibleCount++;
  });

  let noRes = document.getElementById('users-no-results');
  if (!noRes) {
    const tbody = document.getElementById('users-table-body');
    if (tbody) {
      tbody.insertAdjacentHTML('beforeend', '<tr id="users-no-results" style="display:none;"><td colspan="8" style="text-align:center; padding:24px; color:var(--text-muted);"><svg class="icon icon-lg" style="margin-bottom:8px;"><use href="#icon-users"></use></svg><div>No se encontraron cuentas que coincidan con el filtro.</div></td></tr>');
      noRes = document.getElementById('users-no-results');
    }
  }
  if (noRes) {
    noRes.style.display = (visibleCount === 0 && (_usersData.length > 0)) ? '' : 'none';
  }
}

function applyGroupsFilters() {
  const q = (document.getElementById('groups-search')?.value || '').toLowerCase().trim();
  document.querySelectorAll('#groups-table-body tr[data-group]').forEach(tr => {
    let ok = true;
    if (q) {
      const gname = (tr.dataset.group || '').toLowerCase();
      const members = (tr.dataset.members || '').toLowerCase();
      ok = gname.includes(q) || members.includes(q);
    }
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
  const verb = enabled ? 'Reactivar' : 'Suspender';
  showUserActionConfirm({
    title: `${verb} cuenta [${username}]`,
    icon: enabled ? 'unlock' : 'lock',
    alertType: enabled ? 'info' : 'warning',
    alertText: enabled
      ? `Se reanudará el acceso de red a carpetas compartidas Samba y servicios para [${username}].`
      : `La cuenta [${username}] perderá acceso inmediato a recursos Samba y sesiones autenticadas.`,
    desc: `El estado se actualizará tanto en el sistema operativo Linux como en la base de datos de usuarios Samba (smbpasswd).`,
    prompt: `¿Confirmas que deseas ${verb.toLowerCase()} la cuenta [${username}]?`,
    btnText: `${verb} cuenta`,
    btnClass: enabled ? 'btn-primary' : 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/users/toggle', {
          method: 'POST',
          body: JSON.stringify({ username, enabled }),
        });
        showToast(`Cuenta [${username}] ${enabled ? 'reactivada exitosamente' : 'suspendida'}.`, 'success');
        loadUsersAndGroups();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
}

function openGroupMembers(group) {
  _activeGroup = group;
  const g = _groupsData.find(x => x.name === group);
  const members = g ? (g.members || []) : [];
  document.getElementById('modal-group-members-title').textContent = `Miembros del grupo ${group} (${members.length})`;
  const searchInput = document.getElementById('group-member-search');
  if (searchInput) searchInput.value = '';

  const list = document.getElementById('group-members-list');
  list.innerHTML = members.length
    ? members.map(m => {
        if (m === 'www-data') {
          return `
            <div style="display:flex; align-items:center; justify-content:space-between; gap:8px; padding:6px 8px; background:var(--bg-surface); border-radius:6px;" data-username="${escapeHtml(m)}">
              <span class="member-name"><strong>${escapeHtml(m)}</strong> <span class="badge badge-primary">Sistema (Protegido)</span></span>
              <span style="font-size:11px; color:var(--text-muted);">Requerido por servicio web</span>
            </div>`;
        }
        return `
          <div style="display:flex; align-items:center; justify-content:space-between; gap:8px; padding:4px 6px; background:var(--bg-surface); border-radius:6px;" data-username="${escapeHtml(m)}">
            <span class="member-name">${escapeHtml(m)}</span>
            <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="removeGroupMember('${escapeHtml(m)}')">Quitar</button>
          </div>`;
      }).join('')
    : '<em style="color:var(--text-muted);">Sin miembros</em>';

  const sel = document.getElementById('group-member-user');
  const available = _usersData.filter(u => !members.includes(u.username) && u.username !== 'root');
  sel.innerHTML = available.length
    ? available.map(u => `<option value="${escapeHtml(u.username)}">${escapeHtml(u.full_name ? `${u.full_name} (${u.username})` : u.username)}</option>`).join('')
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
  if (username === 'www-data') {
    showToast('La cuenta www-data está protegida y es requerida por el panel web.', 'warning');
    return;
  }
  showUserActionConfirm({
    title: `Quitar miembro de grupo`,
    icon: 'users',
    alertType: 'warning',
    alertText: `Se revocará la membresía del usuario [${username}] en el grupo departamental [${_activeGroup}].`,
    desc: `El usuario perderá de inmediato los permisos de acceso y herencia ACL asociados a este grupo.`,
    prompt: `¿Confirmas quitar a [${username}] del grupo [${_activeGroup}]?`,
    btnText: 'Quitar del grupo',
    btnClass: 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/users/groups', {
          method: 'POST',
          body: JSON.stringify({ username, group: _activeGroup, action: 'remove' }),
        });
        showToast(`${username} quitado de ${_activeGroup}.`, 'success');
        loadUsersAndGroups();
        openGroupMembers(_activeGroup);
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
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
  if (!shares.length) {
    container.innerHTML = '<div style="padding:24px; text-align:center; color:var(--text-muted);"><p>No hay recursos compartidos registrados en el sistema.</p></div>';
    return;
  }
  if (!rows.length) {
    if (_permTab === 'groups') {
      container.innerHTML = `
        <div style="padding:24px; text-align:center; color:var(--text-secondary);">
          <svg class="icon icon-lg" style="color:var(--text-muted); margin-bottom:8px;"><use href="#icon-shield"></use></svg>
          <div style="font-weight:600; font-size:14px; margin-bottom:4px;">No hay grupos asignados a los recursos compartidos</div>
          <p style="font-size:12.5px; max-width:480px; margin:0 auto; color:var(--text-muted);">
            Los grupos corporativos actuales no tienen permisos específicos asignados en las carpetas compartidas registradas.
            Puedes asignar grupos directamente en la configuración de cada recurso compartido o alternar a la pestaña <strong>Usuarios × Recursos</strong>.
          </p>
        </div>
      `;
    } else {
      container.innerHTML = '<div style="padding:24px; text-align:center; color:var(--text-muted);"><p>No hay usuarios registrados con acceso a estos recursos.</p></div>';
    }
    return;
  }

  const kind = _permTab === 'groups' ? 'group' : 'user';
  let html = '<table class="nas-table"><thead><tr><th style="min-width:180px;">Entidad</th>';
  shares.forEach(s => { html += `<th>${escapeHtml(s)}</th>`; });
  html += '</tr></thead><tbody>';
  rows.forEach(r => {
    html += `<tr data-entity="${escapeHtml(r.name)}"><td><strong>${escapeHtml(r.name)}</strong>${r.is_admin ? ' <span class="badge badge-warn">Admin</span>' : ''}</td>`;
    shares.forEach(s => {
      const level = r[s] || 'none';
      html += `<td><button type="button" class="chip-btn perm-cell-btn" style="width:100%;" onclick="cyclePerm('${kind}','${escapeHtml(r.name)}','${escapeHtml(s)}', this)">${permBadge(level)}</button></td>`;
    });
    html += '</tr>';
  });
  html += '</tbody></table>';
  container.innerHTML = html;
  applyPermsSearch();
}

function applyPermsSearch() {
  const q = (document.getElementById('perms-search')?.value || '').toLowerCase().trim();
  document.querySelectorAll('#perm-matrix-container table tbody tr[data-entity]').forEach(tr => {
    const ent = (tr.dataset.entity || '').toLowerCase();
    tr.style.display = (!q || ent.includes(q)) ? '' : 'none';
  });
}

async function cyclePerm(kind, name, share, btnEl = null) {
  const order = ['none', 'read', 'write'];
  const row = (_permTab === 'groups' ? _permData.groups : _permData.users).find(r => r.name === name);
  const current = row ? (row[share] || 'none') : 'none';
  const next = order[(order.indexOf(current) + 1) % order.length];

  if (btnEl) {
    btnEl.disabled = true;
    btnEl.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/shares/access', {
      method: 'POST',
      body: JSON.stringify({ share, kind, name, level: next }),
    });
    showToast(`Permiso de ${name} en ${share}: ${next}.`, 'success');
    await loadAccessMatrix();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btnEl) {
      btnEl.disabled = false;
      btnEl.classList.remove('is-loading');
    }
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

    const executeRepair = async (force) => {
      try {
        const payload = { share: shareName || '', force };
        const res = await apiFetch('/api/shares/access/repair', { method: 'POST', body: JSON.stringify(payload) });
        showToast(res.message || 'ACL procesadas correctamente.', 'success');
        loadAccessMatrix();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    };

    if (shareName) {
      const sAudit = sharesMap[shareName] || auditData;
      if (sAudit && sAudit.in_sync) {
        showUserActionConfirm({
          title: `Reaplicar ACL: [${shareName}]`,
          icon: 'shield',
          alertType: 'info',
          alertText: `El recurso [${shareName}] ya tiene sus ACL perfectamente sincronizadas con smb.conf (cero discrepancias).`,
          desc: `La reaplicación forzará la ejecución recursiva de setfacl -R en todas las carpetas y archivos dentro del recurso.`,
          prompt: `¿Deseas forzar la reaplicación recursiva de ACL de todos modos?`,
          btnText: 'Forzar reaplicación',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(true),
        });
      } else {
        const discList = ((sAudit && sAudit.discrepancies) || []).map(d => `• ${d}`).join('<br>');
        showUserActionConfirm({
          title: `Reparar ACL: [${shareName}]`,
          icon: 'alert-triangle',
          alertType: 'warning',
          alertText: `Se detectaron desajustes entre smb.conf y las ACL del sistema en [${shareName}]:<br><div style="margin-top:6px; font-family:monospace; font-size:11.5px;">${discList || 'Discrepancias en permisos'}</div>`,
          desc: `Se reajustarán las ACL POSIX y las máscaras efectivas para que coincidan con la definición del recurso.`,
          prompt: `¿Proceder con la reparación automática de ACL?`,
          btnText: 'Reparar ACL',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(false),
        });
      }
    } else {
      if (outOfSync === 0) {
        showUserActionConfirm({
          title: `Reaplicar ACL en todos los recursos`,
          icon: 'shield',
          alertType: 'info',
          alertText: `Todas las ACL de los ${total} recurso(s) están perfectamente sincronizadas con smb.conf.`,
          desc: `Se ejecutará una reaplicación recursiva total de permisos setfacl sobre todos los recursos compartidos.`,
          prompt: `¿Deseas forzar la reaplicación recursiva en todos los recursos?`,
          btnText: 'Forzar reaplicación total',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(true),
        });
      } else {
        const outList = Object.entries(sharesMap)
          .filter(([_, v]) => !v.in_sync)
          .map(([k, v]) => `• <strong>${k}</strong>: ${(v.discrepancies || []).length} discrepancia(s)`)
          .join('<br>');
        showUserActionConfirm({
          title: `Reparación de ACL del sistema`,
          icon: 'alert-triangle',
          alertType: 'warning',
          alertText: `Auditoría: ${outOfSync} de ${total} recurso(s) requieren sincronización:<br><div style="margin-top:6px; font-size:12px;">${outList}</div>`,
          desc: `La reparación inteligente procesará únicamente las carpetas compartidas con desajustes detectados.`,
          prompt: `¿Proceder con la reparación inteligente de ACL?`,
          btnText: 'Reparar desajustes',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(false),
        });
      }
    }
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

    // Guardar datos para ficha de acceso y bienvenida
    const serverHost = window.location.hostname || 'SRV-NAS';
    const roleTitle = isSuperadmin ? 'Superadministrador' : (isAdmin ? 'Administrador' : (canWeb ? 'Operador Web' : 'Empleado de Red'));
    _lastCreatedUserMemo = {
      username,
      full_name: full_name || '',
      password,
      roleTitle,
      serverHost,
      unc: `\\\\${serverHost}`,
    };

    closeModal('modal-new-user');
    document.getElementById('form-new-user').reset();

    // Mostrar modal de bienvenida con ruta UNC
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
        <div style="margin-bottom:4px;"><strong>Nombre:</strong> ${escapeHtml(full_name || 'Sin nombre asignado')}</div>
        <div style="margin-bottom:4px;"><strong>Rol asignado:</strong> ${roleBadge}</div>
        <div><strong>Contraseña temporal:</strong> <code style="user-select:all;">${escapeHtml(password)}</code></div>
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

function copyUserWelcomeMemo() {
  if (!_lastCreatedUserMemo) {
    showToast('No hay ficha de usuario disponible.', 'warning');
    return;
  }
  const memo = [
    `=== FICHA DE ACCESO A RECURSOS (SRV-NAS) ===`,
    `Servidor / Host: ${_lastCreatedUserMemo.serverHost}`,
    `Ruta de Red Windows (UNC): ${_lastCreatedUserMemo.unc}`,
    `Usuario de Acceso: ${_lastCreatedUserMemo.username}`,
    `Nombre / Cargo: ${_lastCreatedUserMemo.full_name || 'N/A'}`,
    `Rol asignado: ${_lastCreatedUserMemo.roleTitle}`,
    `Contraseña temporal: ${_lastCreatedUserMemo.password || '(asignada en el alta)'}`,
    `============================================`,
  ].join('\n');

  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(memo).then(() => {
      showToast('Ficha de acceso copiada al portapapeles.', 'success');
    }).catch(() => {
      copyUncPath(memo);
    });
  } else {
    copyUncPath(memo);
  }
}

async function deleteUser(username) {
  showUserActionConfirm({
    title: `Eliminar usuario [${username}]`,
    icon: 'trash',
    alertType: 'danger',
    alertText: `Esta acción eliminará de forma permanente la cuenta [${username}] de Linux, Samba y su directorio personal (/home/${username}).`,
    desc: `Los datos en carpetas compartidas se preservarán, pero el usuario no podrá volver a autenticarse.`,
    prompt: `¿Confirmas que deseas eliminar irreversiblemente al usuario [${username}]?`,
    btnText: 'Eliminar usuario',
    btnClass: 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/users/delete', {
          method: 'POST',
          body: JSON.stringify({ username }),
        });
        showToast(`Usuario [${username}] eliminado.`, 'success');
        loadUsersAndGroups();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
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
  showUserActionConfirm({
    title: `Eliminar grupo [${name}]`,
    icon: 'trash',
    alertType: 'danger',
    alertText: `El grupo corporativo [${name}] será eliminado del sistema operativo y de la configuración de carpetas compartidas.`,
    desc: `Los usuarios mantendrán sus cuentas, pero perderán los permisos conferidos por este grupo en Samba y el sistema de archivos.`,
    prompt: `¿Confirmas que deseas eliminar el grupo [${name}]?`,
    btnText: 'Eliminar grupo',
    btnClass: 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/groups/delete', {
          method: 'POST',
          body: JSON.stringify({ name }),
        });
        showToast(`Grupo [${name}] eliminado.`, 'success');
        loadUsersAndGroups();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
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
function ansiToHtml(text) {
  if (!text) return '';
  let str = text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  const colorMap = {
    '30': 'color:#1e293b',
    '31': 'color:#ef4444',
    '32': 'color:#10b981',
    '33': 'color:#f59e0b',
    '34': 'color:#3b82f6',
    '35': 'color:#d946ef',
    '36': 'color:#06b6d4',
    '37': 'color:#e2e8f0',
    '90': 'color:#64748b',
    '91': 'color:#f87171',
    '92': 'color:#34d399',
    '93': 'color:#fbbf24',
    '94': 'color:#60a5fa',
    '95': 'color:#e879f9',
    '96': 'color:#22d3ee',
    '97': 'color:#ffffff',
    '1': 'font-weight:700',
    '4': 'text-decoration:underline',
  };

  let openSpans = 0;
  str = str.replace(/\x1b\[([0-9;]*)m/g, (match, codes) => {
    if (!codes || codes === '0') {
      const res = '</span>'.repeat(openSpans);
      openSpans = 0;
      return res;
    }
    const parts = codes.split(';');
    const styles = [];
    parts.forEach(p => {
      if (colorMap[p]) styles.push(colorMap[p]);
    });
    if (styles.length > 0) {
      openSpans++;
      return `<span style="${styles.join(';')}">`;
    }
    return '';
  });
  if (openSpans > 0) {
    str += '</span>'.repeat(openSpans);
  }
  return str;
}

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
      const currentVal = input.value;
      if (currentVal) {
        await apiFetch('/api/terminal/send', {
          method: 'POST',
          body: JSON.stringify({ data: currentVal, raw: true }),
          silentToast: true,
        });
        input.value = '';
      }
      await sendTerminalKey('TAB');
    } else if (e.ctrlKey && (e.key === 'c' || e.key === 'C')) {
      const selection = window.getSelection().toString();
      if (selection && selection.length > 0) {
        return;
      }
      e.preventDefault();
      await sendTerminalKey('CTRL_C');
    }
  });

  output.addEventListener('click', () => input.focus());

  let resizeTimer = null;
  window.addEventListener('resize', () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
      const view = document.getElementById('view-terminal');
      if (view && view.classList.contains('active')) {
        resizeTerminal();
      }
    }, 400);
  });
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
    const payload = res.data ?? res;
    const text = payload.output ?? '';
    const cwd = payload.cwd ?? '';
    const user = payload.user ?? '';

    if (cwd) {
      const barCwd = document.getElementById('term-bar-cwd');
      const promptCwd = document.getElementById('term-prompt-cwd');
      if (barCwd && barCwd.textContent !== cwd) barCwd.textContent = cwd;
      if (promptCwd && promptCwd.textContent !== cwd) promptCwd.textContent = cwd;
    }
    if (user) {
      const promptUser = document.getElementById('term-prompt-user');
      if (promptUser && promptUser.textContent !== user) promptUser.textContent = user;
    }

    const isNearBottom = (output.scrollHeight - output.scrollTop - output.clientHeight) < 60;
    output.innerHTML = ansiToHtml(text.replace(/\s+$/, '\n'));

    if (isNearBottom) {
      output.scrollTop = output.scrollHeight;
    }
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
    if (!u) return;

    window.LAST_UPDATE_INFO = u;

    // Versión SemVer destacada
    const verTag = u.version ? (u.version.startsWith('v') ? u.version : `v${u.version}`) : 'v0.1.3';
    const latestVer = u.latest_version ? (u.latest_version.startsWith('v') ? u.latest_version : `v${u.latest_version}`) : verTag;

    // Badge verde de verificación criptográfica GPG
    const gpgBadge = u.gpg_verified
      ? `<span class="badge badge-ok" title="Tag criptográficamente firmado y verificado con GPG" style="display:inline-flex; align-items:center; gap:5px; font-weight:600;">
           <svg class="icon" style="width:13px;height:13px;"><use href="#icon-check"></use></svg> Firmado por Francisco Toledo
         </span>`
      : `<span class="badge badge-gray" title="Sin firma GPG detectada en el tag local">Sin firma GPG</span>`;

    // Estado dinámico (Al día o actualización disponible)
    const statusBanner = u.has_updates
      ? `<div style="display:inline-flex; align-items:center; gap:8px; background:rgba(245,158,11,0.15); border:1px solid rgba(245,158,11,0.4); color:#fbbf24; padding:6px 14px; border-radius:6px; font-weight:600; font-size:13px;">
           <svg class="icon" style="width:15px;height:15px;"><use href="#icon-alert"></use></svg>
           <span>Nueva versión <strong>${escapeHtml(latestVer)}</strong> disponible</span>
           <button class="btn btn-primary btn-sm" onclick="openSystemUpdateModal()" style="margin-left:8px;">
             <svg class="icon" style="width:13px;height:13px;"><use href="#icon-download"></use></svg> Actualizar ahora
           </button>
         </div>`
      : `<div style="display:inline-flex; align-items:center; gap:6px; background:rgba(34,197,94,0.12); border:1px solid rgba(34,197,94,0.3); color:#22c55e; padding:6px 14px; border-radius:6px; font-weight:600; font-size:13px;">
           <svg class="icon" style="width:15px;height:15px;"><use href="#icon-check"></use></svg>
           <span>Sistema al día</span>
         </div>`;

    // Actualizar badge en la navegación lateral
    const navBadge = document.getElementById('badge-updates');
    if (navBadge) {
      if (u.has_updates) {
        navBadge.textContent = '¡Actualizar!';
        navBadge.className = 'nav-badge badge-warn';
        navBadge.style.color = '#fbbf24';
      } else {
        navBadge.textContent = 'Al día';
        navBadge.className = 'nav-badge';
        navBadge.style.color = '';
      }
    }

    // Cuadro colapsable con las notas de versión (Release Notes / Changelog)
    const notesContent = u.release_notes || 'Versión estable con optimizaciones para Debian 13, BTRFS Zstandard, Samba 4 y panel web Slate UI.';
    const changelogBlock = `
      <details style="margin-top:14px; background:var(--bg-surface); border:1px solid var(--border-color); border-radius:8px; padding:10px 14px;">
        <summary style="cursor:pointer; font-weight:600; color:var(--text-secondary); display:flex; align-items:center; justify-content:space-between; user-select:none;">
          <span style="display:inline-flex; align-items:center; gap:6px;">
            <svg class="icon" style="width:14px;height:14px;"><use href="#icon-logs"></use></svg>
            Notas de la versión (Changelog)
          </span>
          <span style="font-size:12px; color:var(--text-muted);">(clic para ver detalles)</span>
        </summary>
        <div style="margin-top:10px; padding-top:10px; border-top:1px solid var(--border-color); font-size:13px; line-height:1.6; color:var(--text-secondary); white-space:pre-wrap;">${escapeHtml(notesContent)}</div>
      </details>
    `;

    container.innerHTML = `
      <div style="display:flex; flex-direction:column; gap:16px;">
        <div style="display:flex; align-items:center; justify-content:space-between; flex-wrap:wrap; gap:12px;">
          <div style="display:flex; align-items:center; gap:10px; flex-wrap:wrap;">
            <span style="font-size:26px; font-weight:700; color:var(--text-primary); letter-spacing:-0.5px;">${escapeHtml(verTag)}</span>
            ${gpgBadge}
            <span class="badge badge-blue">${escapeHtml(u.channel || 'main')}</span>
          </div>
          <div>
            ${statusBanner}
          </div>
        </div>

        <div class="card-grid-3" style="gap:10px;">
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Versión instalada:</span>
            <strong class="info-val"><code>${escapeHtml(u.version || '0.1.3')}</code> (${escapeHtml(u.installed_commit || '')})</strong>
          </div>
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Versión más reciente:</span>
            <strong class="info-val"><code>${escapeHtml(latestVer)}</code></strong>
          </div>
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Fecha del commit:</span>
            <strong class="info-val">${escapeHtml(u.commit_date || 'N/A')}</strong>
          </div>
        </div>

        ${changelogBlock}
      </div>
    `;
  } catch (e) {
    container.innerHTML = '<p style="color:var(--accent-danger);">Error al consultar actualizaciones: ' + escapeHtml(e.message) + '</p>';
  }
}

function openSystemUpdateModal() {
  const uptodateNotice = document.getElementById('system-update-uptodate-notice');
  if (uptodateNotice) {
    uptodateNotice.style.display = (window.LAST_UPDATE_INFO && !window.LAST_UPDATE_INFO.has_updates) ? 'flex' : 'none';
  }

  const progressDiv = document.getElementById('system-update-progress');
  const bar = document.getElementById('system-update-progress-bar');
  const log = document.getElementById('system-update-log');
  const stepLabel = document.getElementById('system-update-step-label');
  const spinner = document.getElementById('system-update-spinner');
  const footer = document.getElementById('system-update-footer');

  if (progressDiv) progressDiv.style.display = 'none';
  if (bar) {
    bar.style.width = '0%';
    bar.style.background = 'var(--accent-primary)';
  }
  if (log) {
    log.style.display = 'none';
    log.textContent = '';
  }
  if (stepLabel) {
    stepLabel.textContent = 'Iniciando actualización...';
    stepLabel.style.color = '';
  }
  if (spinner) spinner.style.display = '';

  if (footer) {
    footer.innerHTML = `
      <button type="button" class="btn btn-secondary" id="btn-cancel-system-update" onclick="closeModal('modal-system-update')">Cancelar</button>
      <button type="button" class="btn btn-primary" id="btn-start-system-update" onclick="startSystemUpdate()">
        <svg class="icon"><use href="#icon-download"></use></svg> Iniciar actualización ahora
      </button>
    `;
  }

  openModal('modal-system-update');
}

function applySystemUpdate() {
  openSystemUpdateModal();
}

async function startSystemUpdate() {
  const startBtn = document.getElementById('btn-start-system-update');
  const cancelBtn = document.getElementById('btn-cancel-system-update');
  const progressDiv = document.getElementById('system-update-progress');
  const bar = document.getElementById('system-update-progress-bar');
  const log = document.getElementById('system-update-log');
  const stepLabel = document.getElementById('system-update-step-label');
  const spinner = document.getElementById('system-update-spinner');
  const footer = document.getElementById('system-update-footer');

  if (startBtn) {
    startBtn.disabled = true;
    startBtn.classList.add('is-loading');
  }
  if (cancelBtn) {
    cancelBtn.disabled = true;
  }
  if (progressDiv) {
    progressDiv.style.display = 'block';
  }

  // Fases visuales informativas
  const phases = [
    { pct: '15%', text: 'Fase 1/5: Respaldo preventivo del estado actual en /tmp/backup_nas_*' },
    { pct: '35%', text: 'Fase 2/5: Avance rápido y verificación de firmas criptográficas GPG' },
    { pct: '55%', text: 'Fase 3/5: Validación de sintaxis de scripts Bash y código PHP' },
    { pct: '75%', text: 'Fase 4/5: Sincronización de /var/www/nas-web y sudoers' },
    { pct: '90%', text: 'Fase 5/5: Recarga de servicios (Nginx, PHP-FPM, Samba)' },
  ];

  let phaseIndex = 0;
  if (bar) bar.style.width = phases[0].pct;
  if (stepLabel) stepLabel.textContent = phases[0].text;

  const phaseInterval = setInterval(() => {
    phaseIndex++;
    if (phaseIndex < phases.length) {
      if (bar) bar.style.width = phases[phaseIndex].pct;
      if (stepLabel) stepLabel.textContent = phases[phaseIndex].text;
    }
  }, 1200);

  try {
    const res = await apiFetch('/api/system/update/run', { method: 'POST' });
    clearInterval(phaseInterval);

    if (bar) {
      bar.style.width = '100%';
      bar.style.background = '#22c55e';
    }
    if (spinner) {
      spinner.style.display = 'none';
    }
    if (stepLabel) {
      stepLabel.innerHTML = '<span style="color:#22c55e;">✔ Actualización completada con éxito</span>';
    }

    const outputText = res.data?.output || res.message || 'Sincronización finalizada correctamente.';
    if (log) {
      log.style.display = 'block';
      log.textContent = outputText;
    }

    if (footer) {
      footer.innerHTML = `
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-system-update')">Cerrar</button>
        <button type="button" class="btn btn-primary" onclick="window.location.reload()">
          <svg class="icon"><use href="#icon-refresh"></use></svg> Recargar panel (Ctrl+F5)
        </button>
      `;
    }

    showToast('Actualización del sistema completada con éxito.', 'success');
    loadUpdates();
  } catch (e) {
    clearInterval(phaseInterval);
    if (bar) {
      bar.style.width = '100%';
      bar.style.background = '#ef4444';
    }
    if (spinner) {
      spinner.style.display = 'none';
    }
    if (stepLabel) {
      stepLabel.innerHTML = `<span style="color:#ef4444;">✘ Error: ${escapeHtml(e.message)}</span>`;
    }
    if (log) {
      log.style.display = 'block';
      log.textContent = e.message;
    }
    if (footer) {
      footer.innerHTML = `
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-system-update')">Cerrar</button>
        <button type="button" class="btn btn-primary" onclick="startSystemUpdate()">Reintentar</button>
      `;
    }
    showToast('Error al actualizar el sistema: ' + e.message, 'error');
  }
}

/**
 * Carga y actualiza dinámicamente las 6 tarjetas de tecnologías y aplicaciones.
 */
async function loadApplications() {
  try {
    const res = await apiFetch('/api/system/applications');
    const a = res.data || res;
    if (!a || !a.samba) return;

    window.LAST_SAMBA_SESSIONS = a.samba.sessions_list || [];
    renderSambaSessionsTable(window.LAST_SAMBA_SESSIONS);
    const sambaCountSpan = document.getElementById('app-samba-session-count');
    if (sambaCountSpan) {
      sambaCountSpan.textContent = a.samba.sessions || 0;
    }

    // 1. Samba 4 & VFS
    const badgeSamba = document.getElementById('app-badge-samba');
    const bodySamba = document.getElementById('app-body-samba');
    if (badgeSamba) {
      badgeSamba.className = a.samba.active ? 'badge badge-ok' : 'badge badge-err';
      badgeSamba.textContent = a.samba.active
        ? `● Activo (${a.samba.sessions} ${a.samba.sessions === 1 ? 'sesión' : 'sesiones'})`
        : '○ Inactivo';
    }
    if (bodySamba) {
      const vfsList = (a.samba.vfs_modules || ['acl_xattr', 'streams_xattr']).map(m => `<code>${escapeHtml(m)}</code>`).join(', ');
      bodySamba.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Versión:</strong> <code>${escapeHtml(a.samba.version)}</code></div>
          <div><strong>Sesiones concurrentes:</strong> <span class="badge badge-blue">${a.samba.sessions} activas</span></div>
          <div><strong>Módulos VFS:</strong> ${vfsList} (concurrencia Excel sin bloqueos de temporales <code>~$</code>)</div>
        </div>
      `;
    }

    // 2. Nginx & PHP-FPM
    const badgeWeb = document.getElementById('app-badge-web');
    const bodyWeb = document.getElementById('app-body-web');
    if (badgeWeb) {
      badgeWeb.className = a.web.active ? 'badge badge-ok' : 'badge badge-err';
      badgeWeb.textContent = a.web.active ? '● Activo (ondemand)' : '○ Inactivo';
    }
    if (bodyWeb) {
      bodyWeb.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Intérprete:</strong> PHP <code>v${escapeHtml(a.web.php_version)}</code> | <strong>Servidor:</strong> <code>Nginx-light</code></div>
          <div><strong>Pool de procesos:</strong> <code>pm = ${escapeHtml(a.web.pool_mode)}</code> (gestión bajo demanda)</div>
          <div><strong>Consumo de memoria:</strong> <span class="badge badge-teal">${a.web.memory_mb} MB RAM</span> (~0 MB en reposo)</div>
        </div>
      `;
    }

    // 3. BTRFS con Zstandard
    const badgeBtrfs = document.getElementById('app-badge-btrfs');
    const bodyBtrfs = document.getElementById('app-body-btrfs');
    if (badgeBtrfs) {
      badgeBtrfs.className = a.btrfs.active ? 'badge badge-ok' : 'badge badge-gray';
      badgeBtrfs.textContent = a.btrfs.active ? '● Activo (Zstd:3)' : '○ ' + escapeHtml(a.btrfs.filesystem);
    }
    if (bodyBtrfs) {
      bodyBtrfs.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Filesystem:</strong> <code>${escapeHtml(a.btrfs.filesystem)}</code> (Compresión <code>${escapeHtml(a.btrfs.compression)}</code>, ahorro ~20-40%)</div>
          <div><strong>Auditoría Scrub Bit Rot:</strong> <code>${escapeHtml(a.btrfs.scrub_status)}</code> (${escapeHtml(a.btrfs.scrub_errors)})</div>
          <div><strong>Mantenimiento preventivo:</strong> ${escapeHtml(a.btrfs.schedule)}</div>
        </div>
      `;
    }

    // 4. Rsync Multiplataforma
    const badgeRsync = document.getElementById('app-badge-rsync');
    const bodyRsync = document.getElementById('app-body-rsync');
    if (badgeRsync) {
      badgeRsync.className = 'badge badge-blue';
      badgeRsync.textContent = `● ${a.rsync.tasks_count} ${a.rsync.tasks_count === 1 ? 'Tarea' : 'Tareas'}`;
    }
    if (bodyRsync) {
      const protoList = (a.rsync.protocols || ['CIFS 3.1.1', 'SSH']).join(', ');
      bodyRsync.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Deduplicación diferencial:</strong> ${escapeHtml(a.rsync.deduplication)}</div>
          <div><strong>Staging de resiliencia:</strong> <code>${escapeHtml(a.rsync.staging)}</code> (blindaje ante cortes)</div>
          <div><strong>Conectores remotos:</strong> ${escapeHtml(protoList)}</div>
        </div>
      `;
    }

    // 5. WSDD2 & NetBIOS
    const badgeWsdd2 = document.getElementById('app-badge-wsdd2');
    const bodyWsdd2 = document.getElementById('app-body-wsdd2');
    if (badgeWsdd2) {
      badgeWsdd2.className = a.wsdd2.active ? 'badge badge-ok' : 'badge badge-err';
      badgeWsdd2.textContent = a.wsdd2.active ? '● Activo (WSD LLMNR)' : '○ Inactivo';
    }
    if (bodyWsdd2) {
      bodyWsdd2.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Protocolo:</strong> ${escapeHtml(a.wsdd2.protocol)}</div>
          <div><strong>Demonios:</strong> WSDD2 ${a.wsdd2.wsdd_active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Inactivo</span>'} | nmbd ${a.wsdd2.nmbd_active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Inactivo</span>'}</div>
          <div><strong>Compatibilidad:</strong> Visibilidad instantánea en Windows 10/11 sin SMBv1 ni broadcasts obsoletos</div>
        </div>
      `;
    }

    // 6. Seguridad Activa (AIDE & auditd)
    const badgeSecurity = document.getElementById('app-badge-security');
    const bodySecurity = document.getElementById('app-body-security');
    if (badgeSecurity) {
      badgeSecurity.className = a.security.active ? 'badge badge-ok' : 'badge badge-warn';
      badgeSecurity.textContent = a.security.active ? '● Activo (auditd + AIDE)' : '● Parcial';
    }
    if (bodySecurity) {
      bodySecurity.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Auditoría del Kernel:</strong> ${escapeHtml(a.security.audit_rules)}</div>
          <div><strong>Monitoreo de Integridad:</strong> ${escapeHtml(a.security.fim_status)}</div>
          <div><strong>Demonios:</strong> auditd ${a.security.auditd_active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Inactivo</span>'} | AIDE timer ${a.security.aide_active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Inactivo</span>'}</div>
        </div>
      `;
    }
  } catch (e) {
    // Silencioso ante errores menores de red
  }
}

/**
 * Alterna entre las pestañas de Aplicaciones y Actualizaciones:
 * 'components' | 'updates' | 'security'
 */
function switchApplicationsTab(tabName) {
  const tabs = ['components', 'updates', 'security'];
  tabs.forEach(t => {
    const btn = document.getElementById(`apps-tab-btn-${t}`);
    const panel = document.getElementById(`apps-panel-${t}`);
    if (btn) {
      btn.classList.toggle('active', t === tabName);
    }
    if (panel) {
      panel.style.display = (t === tabName) ? 'block' : 'none';
    }
  });
}

/**
 * Comprueba de forma interactiva las actualizaciones y el estado de componentes,
 * con feedback visual en el botón y notificación toast.
 */
async function triggerSystemUpdatesCheck(btn) {
  const targetBtn = btn || document.getElementById('btn-refresh-applications');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await Promise.all([loadUpdates(), loadApplications()]);
    showToast('Estado de componentes y repositorio actualizado.', 'info');
  } catch (e) {
    showToast('Error al comprobar versiones: ' + (e?.message || e), 'error');
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Renderiza la tabla de sesiones SMB concurrentes en el modal.
 */
function renderSambaSessionsTable(sessions) {
  const tbody = document.getElementById('tbody-samba-sessions');
  if (!tbody) return;
  if (!sessions || sessions.length === 0) {
    tbody.innerHTML = `
      <tr>
        <td colspan="7" style="text-align:center; padding:24px; color:var(--text-muted);">
          No hay sesiones SMB activas en este momento.
        </td>
      </tr>
    `;
    return;
  }
  tbody.innerHTML = sessions.map(s => `
    <tr>
      <td><code>${escapeHtml(String(s.pid || '-'))}</code></td>
      <td><strong>${escapeHtml(String(s.username || '-'))}</strong></td>
      <td><code>${escapeHtml(String(s.group || '-'))}</code></td>
      <td><span style="font-family:monospace; font-size:12px;">${escapeHtml(String(s.machine || '-'))}</span></td>
      <td><span class="badge badge-blue">${escapeHtml(String(s.protocol || 'SMB3'))}</span></td>
      <td>${s.encryption && s.encryption !== '-' ? `<span class="badge badge-ok">${escapeHtml(String(s.encryption))}</span>` : '<span style="color:var(--text-muted);">-</span>'}</td>
      <td>${s.signing && s.signing !== '-' ? `<span class="badge badge-teal">${escapeHtml(String(s.signing))}</span>` : '<span style="color:var(--text-muted);">-</span>'}</td>
    </tr>
  `).join('');
}

/**
 * Abre el modal de sesiones SMB concurrentes y refresca la información.
 */
function openSambaSessionsModal() {
  if (window.LAST_SAMBA_SESSIONS) {
    renderSambaSessionsTable(window.LAST_SAMBA_SESSIONS);
  } else {
    loadApplications();
  }
  openModal('modal-samba-sessions');
}

/**
 * Recarga de forma rápida y segura el servicio Samba smbd sin interrumpir conexiones.
 */
async function reloadSambaService(btn) {
  const targetBtn = btn || document.getElementById('btn-reload-samba');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await manageServiceAction('smbd', 'reload');
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Recarga el servicio PHP-FPM bajo demanda para aplicar cambios de configuración.
 */
async function reloadPhpService(btn) {
  const targetBtn = btn || document.getElementById('btn-reload-php');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await manageServiceAction('php8.4-fpm', 'reload');
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Reinicia el servicio de descubrimiento de red WSDD2 en clientes Windows 10/11.
 */
async function restartWsddService(btn) {
  const targetBtn = btn || document.getElementById('btn-restart-wsdd');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await manageServiceAction('wsdd2', 'restart');
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Inicia o audita el proceso de BTRFS Scrub contra Bit Rot directamente desde la tarjeta.
 */
async function triggerBtrfsScrub(btn) {
  const targetBtn = btn || document.getElementById('btn-scrub-btrfs');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    if (typeof runStorageScrub === 'function') {
      await runStorageScrub();
    }
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
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
const NET_PORT_SERVICES = {
  22: 'sshd', 25: 'exim4', 53: 'dns', 80: 'nginx', 443: 'nginx',
  137: 'nmbd', 138: 'nmbd', 139: 'smbd', 445: 'smbd',
  3702: 'wsdd2', 5355: 'wsdd2',
};

function formatSmbProtocol(raw) {
  if (!raw) return 'N/D';
  const key = String(raw).toUpperCase();
  const map = {
    'SMB2_02': 'SMB 2.02',
    'SMB2_10': 'SMB 2.1',
    'SMB3_00': 'SMB 3.0',
    'SMB3_11': 'SMB 3.1.1',
    'SMB3': 'SMB 3.1.1 (máx.)',
  };
  return map[key] || key;
}

function netBadge(isOk, okLabel, failLabel) {
  return isOk
    ? `<span class="badge badge-ok">${escapeHtml(okLabel)}</span>`
    : `<span class="badge badge-err">${escapeHtml(failLabel)}</span>`;
}

function copyNetUnc(netbios) {
  const host = netbios || window.NAS_HOSTNAME || window.location.hostname || 'SRV-NAS';
  copyUncPath('\\\\' + host);
}

/**
 * Renderiza el inventario de red devuelto por GET /api/network.
 * Refleja exactamente la estructura de la vista server-side de layout.php.
 */
function renderNetworking(d) {
  if (!d || typeof d !== 'object') return;

  const samba = d.samba || {};
  const services = d.services || {};
  const firewall = d.firewall || {};
  const interfaces = Array.isArray(d.interfaces) ? d.interfaces : [];
  const dns = Array.isArray(d.dns) ? d.dns : [];
  const listening = Array.isArray(d.listening) ? d.listening : [];
  const netbios = d.netbios || d.hostname || window.location.hostname;
  const primaryIp = d.primary_ip || window.location.hostname;

  const masthead = document.getElementById('masthead-hostname');
  if (masthead && d.hostname) masthead.textContent = d.hostname;

  const setText = (id, value) => {
    const el = document.getElementById(id);
    if (el) el.textContent = value;
  };

  setText('net-netbios', netbios || 'N/D');
  setText('net-workgroup', d.workgroup || 'N/D');

  const ifaceEl = document.getElementById('net-interfaces');
  if (ifaceEl) {
    ifaceEl.innerHTML = interfaces.length === 0
      ? '<span class="text-muted">Sin interfaces detectadas.</span>'
      : interfaces.map(i => `
        <div class="net-iface">
          <strong>${escapeHtml(i.name || '')}</strong>
          <span class="badge ${i.state === 'UP' ? 'badge-ok' : 'badge-warn'}">${escapeHtml(i.state || 'N/D')}</span>
          <code>${escapeHtml(i.ip || 'sin IPv4')}${i.prefix ? '/' + Number(i.prefix) : ''}</code>
          <span class="text-muted">${i.speed_mbps ? Number(i.speed_mbps) + ' Mbps' : 'velocidad N/D'} · MTU ${Number(i.mtu || 0)}${i.mac ? ' · ' + escapeHtml(i.mac) : ''}</span>
        </div>
      `).join('');
  }

  const gatewayEl = document.getElementById('net-gateway');
  if (gatewayEl) {
    const mode = d.dhcp === null || d.dhcp === undefined
      ? ''
      : `<span class="badge ${d.dhcp ? 'badge-blue' : 'badge-gray'}">${d.dhcp ? 'DHCP' : 'Estática'}</span>`;
    gatewayEl.innerHTML = `<code>${escapeHtml(d.gateway || 'N/D')}</code> ${mode}`;
  }

  const dnsEl = document.getElementById('net-dns');
  if (dnsEl) {
    dnsEl.innerHTML = dns.length === 0
      ? '<span class="text-muted">N/D</span>'
      : escapeHtml(dns.join(' · '));
  }

  const smbEl = document.getElementById('net-smb');
  if (smbEl) {
    smbEl.innerHTML = `${escapeHtml(formatSmbProtocol(samba.min_protocol))} — ${escapeHtml(formatSmbProtocol(samba.max_protocol))}
      <span class="badge badge-gray">Firma: ${escapeHtml(samba.signing || 'N/D')}</span>
      <span class="badge badge-gray">Cifrado: ${escapeHtml(samba.encrypt || 'N/D')}</span>`;
  }

  const nmbdEl = document.getElementById('net-nmbd');
  if (nmbdEl) {
    nmbdEl.innerHTML = netBadge(Boolean(services.nmbd), 'nmbd activo', 'nmbd inactivo');
  }

  const wsdPorts = listening
    .filter(p => [3702, 5355].includes(Number(p.port)))
    .map(p => `${Number(p.port)}/${p.proto || 'udp'}`);
  const wsdEl = document.getElementById('net-wsd');
  if (wsdEl) {
    wsdEl.innerHTML = netBadge(Boolean(services.wsdd2), 'wsdd2 activo', 'wsdd2 inactivo') +
      (wsdPorts.length ? ` <span class="text-muted" style="font-size:12px;">puertos ${escapeHtml(wsdPorts.join(', '))}</span>` : '');
  }

  const fwEl = document.getElementById('net-firewall');
  if (fwEl) {
    fwEl.innerHTML = netBadge(Boolean(firewall.active), 'Activo', 'Inactivo') +
      (firewall.rules_visible === false ? ' <span class="text-muted" style="font-size:12px;">reglas no verificables sin privilegios</span>' : '');
  }

  const sessionsEl = document.getElementById('net-sessions');
  if (sessionsEl) {
    sessionsEl.innerHTML = (d.smb_sessions === null || d.smb_sessions === undefined)
      ? '<span class="text-muted">No verificable</span>'
      : `${Number(d.smb_sessions)} activa(s)`;
  }

  const pathEl = document.getElementById('net-info-path');
  if (pathEl) {
    pathEl.innerHTML = `<code>\\\\${escapeHtml(netbios)}</code> o <code>\\\\${escapeHtml(primaryIp)}</code>
      <button type="button" class="btn btn-secondary btn-sm" title="Copiar ruta UNC" style="margin-left:6px;" onclick="copyNetUnc('${escapeHtml(netbios)}')">
        <svg class="icon icon-sm"><use href="#icon-copy"></use></svg>
      </button>`;
  }

  const healthEl = document.getElementById('net-health-badge');
  if (healthEl) {
    const ok = Boolean(services.smbd) && Boolean(services.nmbd) && Boolean(services.wsdd2);
    healthEl.innerHTML = ok
      ? '<span class="badge badge-ok">Servicios de red activos</span>'
      : '<span class="badge badge-warn">Revisar servicios de red</span>';
  }

  const portsBody = document.getElementById('net-ports-body');
  if (portsBody) {
    portsBody.innerHTML = listening.length === 0
      ? '<span class="text-muted">Sin datos de puertos disponibles.</span>'
      : `<div class="net-port-list">${listening.map(p => {
        const svc = p.service || NET_PORT_SERVICES[Number(p.port)] || '';
        return `<span class="net-port-chip"><strong>${Number(p.port)}/${escapeHtml(p.proto || 'tcp')}</strong>${svc ? ' ' + escapeHtml(svc) : ''}</span>`;
      }).join('')}</div>`;
  }

  const portsCount = document.getElementById('net-ports-count');
  if (portsCount) portsCount.textContent = `${listening.length} puerto(s)`;

  const updatedEl = document.getElementById('net-updated');
  if (updatedEl) {
    updatedEl.textContent = d.generated_at ? `Actualizado ${d.generated_at}` : 'Sin verificar';
  }
}

async function loadNetworking(force = false) {
  try {
    const res = await apiFetch('/api/network' + (force ? '?fresh=1' : ''));
    renderNetworking(res.data || res);
  } catch (e) {
    // Manejado por apiFetch
  }
}

async function loadTerminal() {
  const input = document.getElementById('terminal-input');
  startTerminalSession();

  if (!AppState.terminal.history || AppState.terminal.history.length === 0) {
    try {
      const res = await apiFetch('/api/terminal/history?limit=50', { silentToast: true });
      const items = res.data ?? [];
      if (Array.isArray(items) && items.length > 0) {
        AppState.terminal.history = items.map(h => h.command).reverse();
        AppState.terminal.historyIndex = AppState.terminal.history.length;
      }
    } catch (e) {
      // Tolerante
    }
  }

  if (input) {
    setTimeout(() => input.focus(), 80);
  }
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

function formatBytesLocal(bytes) {
  if (bytes === null || bytes === undefined || isNaN(bytes)) return '0 B';
  const num = Number(bytes);
  if (num === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const i = Math.floor(Math.log(num) / Math.log(k));
  const val = parseFloat((num / Math.pow(k, i)).toFixed(i === 0 ? 0 : 2));
  return `${val} ${sizes[i] || 'B'}`;
}

function updateFilesStatusBar() {
  const totalEl = document.getElementById('files-status-total');
  const sizeEl = document.getElementById('files-status-size');
  const selectedEl = document.getElementById('files-status-selected');
  const storageEl = document.getElementById('files-status-storage-info');

  const items = AppState.files.displayItems !== undefined ? AppState.files.displayItems : (AppState.files.items || []);
  const allItems = AppState.files.items || [];
  const foldersCount = items.filter(i => i.is_dir).length;
  const filesCount = items.filter(i => !i.is_dir).length;

  let totalBytes = 0;
  items.forEach(i => {
    if (!i.is_dir) {
      totalBytes += Number(i.size_bytes || i.size || 0);
    }
  });

  if (totalEl) {
    if (AppState.files.filterQuery) {
      totalEl.textContent = `${items.length} de ${allItems.length} elementos (${foldersCount} carpetas, ${filesCount} archivos)`;
    } else {
      totalEl.textContent = `${items.length} elemento(s) (${foldersCount} carpetas, ${filesCount} archivos)`;
    }
  }

  if (sizeEl) {
    sizeEl.textContent = `Tamaño visible: ${formatBytesLocal(totalBytes)}`;
  }

  if (selectedEl) {
    if (AppState.files.selectedItemPath) {
      const selItem = allItems.find(i => (i.relative_path || i.path) === AppState.files.selectedItemPath);
      if (selItem) {
        const itemType = selItem.is_dir ? 'Carpeta' : (selItem.size_formatted || '0 B');
        selectedEl.textContent = `• Seleccionado: ${selItem.name} (${itemType})`;
      } else {
        selectedEl.textContent = '';
      }
    } else {
      selectedEl.textContent = '';
    }
  }

  if (storageEl) {
    const rootName = AppState.files.root === 'backups' ? 'Backups (/srv/nas/BACKUPS_HISTORICOS)' : 'Almacenamiento (/srv/nas)';
    const freeStr = AppState.storage?.overview?.free_formatted ? ` • Libre: ${AppState.storage.overview.free_formatted}` : '';
    storageEl.textContent = `${rootName}${freeStr}`;
  }
}

function filterCurrentFiles(query = null) {
  if (query === null) {
    query = document.getElementById('files-filter-input')?.value || '';
  }
  AppState.files.filterQuery = String(query).toLowerCase().trim();
  applyFilesFilterAndSort();
}

function setFilesSort(sortKey) {
  AppState.files.sortKey = sortKey || 'name_asc';
  const sel = document.getElementById('files-sort-select');
  if (sel && sel.value !== AppState.files.sortKey) {
    sel.value = AppState.files.sortKey;
  }
  updateSortIndicators();
  applyFilesFilterAndSort();
}

function toggleTableSort(col) {
  const currentKey = AppState.files.sortKey || 'name_asc';
  let newKey = `${col}_asc`;
  if (currentKey === `${col}_asc`) {
    newKey = `${col}_desc`;
  } else if (currentKey === `${col}_desc`) {
    newKey = `${col}_asc`;
  } else {
    if (col === 'size' || col === 'date') {
      newKey = `${col}_desc`;
    } else {
      newKey = `${col}_asc`;
    }
  }
  setFilesSort(newKey);
}

function updateSortIndicators() {
  const cols = ['name', 'size', 'permissions', 'date'];
  const [activeCol, activeDir] = (AppState.files.sortKey || 'name_asc').split('_');
  cols.forEach(c => {
    const el = document.getElementById(`sort-indicator-${c}`);
    if (el) {
      if (c === activeCol) {
        el.textContent = activeDir === 'desc' ? ' ▼' : ' ▲';
        el.style.opacity = '1';
      } else {
        el.textContent = '';
      }
    }
  });
}

function sortFilesItems(items, sortKey = 'name_asc') {
  const [col, dir] = (sortKey || 'name_asc').split('_');
  const mult = dir === 'desc' ? -1 : 1;

  return [...items].sort((a, b) => {
    const aDir = a.is_dir ? 1 : 0;
    const bDir = b.is_dir ? 1 : 0;
    if (aDir !== bDir) {
      return bDir - aDir;
    }

    if (col === 'size') {
      const aSize = Number(a.size_bytes ?? a.size ?? 0);
      const bSize = Number(b.size_bytes ?? b.size ?? 0);
      return (aSize - bSize) * mult;
    }

    if (col === 'date') {
      const aTime = a.mtime_raw ? Number(a.mtime_raw) : new Date(a.modified_at || a.mtime || 0).getTime();
      const bTime = b.mtime_raw ? Number(b.mtime_raw) : new Date(b.modified_at || b.mtime || 0).getTime();
      return (aTime - bTime) * mult;
    }

    if (col === 'permissions') {
      const aPerm = String(a.permissions || '');
      const bPerm = String(b.permissions || '');
      return aPerm.localeCompare(bPerm) * mult;
    }

    const aName = String(a.name || '');
    const bName = String(b.name || '');
    return aName.localeCompare(bName, undefined, { numeric: true, sensitivity: 'base' }) * mult;
  });
}

function applyFilesFilterAndSort() {
  let list = Array.isArray(AppState.files.items) ? [...AppState.files.items] : [];
  const q = (AppState.files.filterQuery || '').toLowerCase().trim();
  if (q) {
    list = list.filter(item => {
      const name = String(item.name || '').toLowerCase();
      const owner = String(item.owner || '').toLowerCase();
      const group = String(item.group || '').toLowerCase();
      return name.includes(q) || owner.includes(q) || group.includes(q);
    });
  }
  list = sortFilesItems(list, AppState.files.sortKey || 'name_asc');
  AppState.files.displayItems = list;
  renderCurrentFiles();
}

function copyItemUncPath(relPath) {
  const host = window.location.hostname || window.NAS_HOSTNAME || 'SRV-NAS';
  let cur = String(relPath || '').replace(/^\/+|\/+$/g, '');
  if (AppState.files.root === 'backups') {
    cur = cur ? `BACKUPS_HISTORICOS\\${cur.replace(/\//g, '\\')}` : 'BACKUPS_HISTORICOS';
  } else {
    cur = cur ? cur.replace(/\//g, '\\') : '';
  }
  const unc = cur ? `\\\\${host}\\${cur}` : `\\\\${host}`;
  copyUncPath(unc);
}

function copyCurrentUncPath() {
  copyItemUncPath(AppState.files.currentPath);
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
    AppState.files.selectedItemPath = null;
    renderFileBreadcrumbs(breadcrumbs, currentPath);
    applyFilesFilterAndSort();
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

  const itemsToRender = AppState.files.displayItems !== undefined ? AppState.files.displayItems : (AppState.files.items || []);

  if (isGrid) {
    renderFileGrid(itemsToRender);
  } else {
    renderFileTable(itemsToRender);
  }
  updateFilesStatusBar();
}

function renderFileGrid(items) {
  const container = document.getElementById('files-grid-container');
  if (!container) return;
  container.textContent = '';

  if (!items || items.length === 0) {
    const isFiltered = Boolean(AppState.files.filterQuery);
    container.innerHTML = `
      <div style="grid-column:1/-1; text-align:center; padding:45px 20px; color:var(--text-muted);">
        <svg class="icon" style="width:48px; height:48px; color:var(--text-muted); opacity:0.6; margin-bottom:12px;"><use href="${isFiltered ? '#icon-search' : '#icon-folder'}"></use></svg>
        <div style="font-size:15px; font-weight:600; color:var(--text-main); margin-bottom:4px;">${isFiltered ? 'No se encontraron archivos' : 'Esta carpeta está vacía'}</div>
        <div style="font-size:13px;">${isFiltered ? 'Ningún archivo o carpeta coincide con el filtro actual.' : 'Arrastra archivos aquí desde tu equipo o pulsa el botón <strong>Subir archivos</strong>.'}</div>
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
          <button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'copy-unc')" title="Copiar ruta UNC"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-copy"></use></svg></button>
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
  } else if (action === 'copy-unc') {
    copyItemUncPath(relPath);
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
    const isFiltered = Boolean(AppState.files.filterQuery);
    tbody.innerHTML = `
      <tr>
        <td colspan="7" style="text-align:center; padding:35px; color:var(--text-muted);">
          ${isFiltered ? 'Ningún archivo o carpeta coincide con el filtro actual.' : 'Esta carpeta está vacía. Arrastra archivos aquí desde tu equipo o utiliza el botón <strong>Subir archivos</strong>.'}
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
            <button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'copy-unc')" title="Copiar ruta UNC"><svg class="icon"><use href="#icon-copy"></use></svg></button>
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
  updateFilesStatusBar();
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
  } else if (action === 'copy-unc') {
    copyItemUncPath(relPath);
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
  updateFilesStatusBar();
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

function openNewFileModal() {
  const input = document.getElementById('new-file-name');
  if (input) input.value = '';
  openModal('modal-new-file');
  if (input) setTimeout(() => input.focus(), 100);
}

async function submitNewFile(e) {
  e.preventDefault();
  const input = document.getElementById('new-file-name');
  const name = input ? input.value.trim() : '';
  if (!name) return;

  const btn = document.getElementById('btn-submit-new-file') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/create', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        path: AppState.files.currentPath || '',
        name: name,
        content: '',
      }),
    });

    closeModal('modal-new-file');
    showToast(`Archivo "${name}" creado exitosamente.`, 'success');
    await loadFiles();
    const createdPath = res.data?.path || (AppState.files.currentPath ? `${AppState.files.currentPath}/${name}` : name);
    previewFile(createdPath, name);
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
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
    const totalSizeFormatted = res.data?.total_size_formatted ?? res.total_size_formatted ?? '0 B';

    AppState.files.trashItems = items;
    updateTrashBadgeCount(count);

    const kpiBadge = document.getElementById('trash-kpi-badge');
    if (kpiBadge) {
      kpiBadge.textContent = `${count} elementos (${totalSizeFormatted})`;
    }

    renderTrashRows(items);
  } catch (err) {
    if (tbody) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger); padding:18px;">Error al cargar papelera: ${escapeHtml(err.message)}</td></tr>`;
    }
  }
}

function renderTrashRows(items) {
  const tbody = document.getElementById('trash-table-body');
  if (!tbody) return;

  const q = (AppState.files.trashFilterQuery || '').toLowerCase().trim();
  let filtered = items;
  if (q) {
    filtered = items.filter(item => {
      const name = String(item.filename || item.name || '').toLowerCase();
      const orig = String(item.original_path || '').toLowerCase();
      const by = String(item.deleted_by || '').toLowerCase();
      return name.includes(q) || orig.includes(q) || by.includes(q);
    });
  }

  if (filtered.length === 0) {
    tbody.innerHTML = `
      <tr>
        <td colspan="7" style="text-align:center; padding:35px; color:var(--text-muted);">
          <svg class="icon" style="width:36px; height:36px; color:var(--accent-warning); opacity:0.7; margin-bottom:8px;"><use href="#icon-trash"></use></svg>
          <div>${q ? 'No se encontraron elementos que coincidan con la búsqueda.' : 'La papelera de reciclaje está vacía.'}</div>
        </td>
      </tr>
    `;
    return;
  }

  tbody.textContent = '';
  const range = document.createRange();
  range.selectNodeContents(tbody);
  const html = filtered.map(item => {
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
}

function filterTrashItems(query = null) {
  if (query === null) {
    query = document.getElementById('trash-filter-input')?.value || '';
  }
  AppState.files.trashFilterQuery = String(query).toLowerCase().trim();
  renderTrashRows(AppState.files.trashItems || []);
}

function openRestoreAllTrashModal() {
  const count = (AppState.files.trashItems || []).length;
  if (count === 0) {
    showToast('La papelera de reciclaje ya se encuentra vacía.', 'info');
    return;
  }
  const countEl = document.getElementById('restore-all-trash-count');
  if (countEl) {
    countEl.textContent = `${count} elemento(s)`;
  }
  openModal('modal-restore-all-trash');
}

async function confirmRestoreAllTrash() {
  const btn = document.getElementById('btn-confirm-restore-all');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/trash/restore-all', {
      method: 'POST',
    });

    closeModal('modal-restore-all-trash');
    const restored = res.data?.restored ?? res.restored ?? 0;
    showToast(`Se restauraron ${restored} elemento(s) a sus rutas originales.`, 'success');
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

function initFilesKeyboardShortcuts() {
  document.addEventListener('keydown', (e) => {
    const filesView = document.getElementById('view-files');
    if (!filesView || !filesView.classList.contains('active')) return;

    if (document.querySelector('.modal-backdrop.active, .modal-backdrop.open, .modal-overlay.active, .modal-overlay.open')) return;

    const tag = (e.target?.tagName || '').toLowerCase();
    const isEditing = tag === 'input' || tag === 'textarea' || tag === 'select' || e.target?.isContentEditable;

    if ((e.ctrlKey && (e.key === 'f' || e.key === 'F')) || (!isEditing && e.key === '/')) {
      const searchInput = AppState.files.inTrash ? document.getElementById('trash-filter-input') : document.getElementById('files-filter-input');
      if (searchInput) {
        e.preventDefault();
        searchInput.focus();
        if (typeof searchInput.select === 'function') searchInput.select();
        return;
      }
    }

    if (isEditing) return;

    if (AppState.files.inTrash) return;

    if (e.key === 'Backspace') {
      if (AppState.files.currentPath) {
        e.preventDefault();
        const parts = AppState.files.currentPath.replace(/^\/+|\/+$/g, '').split('/').filter(Boolean);
        parts.pop();
        navigateToSubpath(parts.join('/'));
      }
      return;
    }

    if (e.key === 'F2') {
      if (AppState.files.selectedItemPath) {
        const item = (AppState.files.items || []).find(i => (i.relative_path || i.path) === AppState.files.selectedItemPath);
        if (item) {
          e.preventDefault();
          openRenameModal(item.relative_path || item.path, item.name);
        }
      }
      return;
    }

    if (e.key === 'Delete') {
      if (AppState.files.selectedItemPath) {
        const item = (AppState.files.items || []).find(i => (i.relative_path || i.path) === AppState.files.selectedItemPath);
        if (item) {
          e.preventDefault();
          openDeleteModal(item.relative_path || item.path, item.name);
        }
      }
      return;
    }
  });
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
  initFilesKeyboardShortcuts();

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
window.switchView = switchView;
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
window.confirmTrimSsd = confirmTrimSsd;
window.executeStorageUnmountAndProceed = executeStorageUnmountAndProceed;
window.openCreateSubvolumeModal = openCreateSubvolumeModal;
window.loadStorageSubvolumes = loadStorageSubvolumes;
window.renderStorageOverview = renderStorageOverview;
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
window.filterCurrentFiles = filterCurrentFiles;
window.setFilesSort = setFilesSort;
window.toggleTableSort = toggleTableSort;
window.copyCurrentUncPath = copyCurrentUncPath;
window.copyItemUncPath = copyItemUncPath;
window.openNewFileModal = openNewFileModal;
window.openNewFile = openNewFileModal;
window.submitNewFile = submitNewFile;
window.filterTrashItems = filterTrashItems;
window.openRestoreAllTrashModal = openRestoreAllTrashModal;
window.confirmRestoreAllTrash = confirmRestoreAllTrash;
window.loadFiles = loadFiles;
window.updateFilesStatusBar = updateFilesStatusBar;
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
window.switchUsersTab = switchUsersTab;
window.executeUserActionConfirm = executeUserActionConfirm;
window.updateUsersKpis = updateUsersKpis;
window.updateUserChipCounters = updateUserChipCounters;
window.applyGroupsFilters = applyGroupsFilters;
window.applyPermsSearch = applyPermsSearch;
window.sortUsersTable = sortUsersTable;
window.renderUsersTableBody = renderUsersTableBody;
window.copyPasswordInput = copyPasswordInput;
window.copyUserWelcomeMemo = copyUserWelcomeMemo;
window.filterGroupMembers = filterGroupMembers;
window.repairAcls = repairAcls;
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
window.openSystemUpdateModal = openSystemUpdateModal;
window.startSystemUpdate = startSystemUpdate;
window.loadApplications = loadApplications;
window.switchApplicationsTab = switchApplicationsTab;
window.triggerSystemUpdatesCheck = triggerSystemUpdatesCheck;
window.renderSambaSessionsTable = renderSambaSessionsTable;
window.openSambaSessionsModal = openSambaSessionsModal;
window.reloadSambaService = reloadSambaService;
window.reloadPhpService = reloadPhpService;
window.restartWsddService = restartWsddService;
window.triggerBtrfsScrub = triggerBtrfsScrub;

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
