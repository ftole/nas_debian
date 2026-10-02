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
};

function showToast(message, type = 'info') {
  const container = document.getElementById('toast-container');
  if (!container) return;

  const toast = document.createElement('div');
  toast.className = `alert-box alert-box-${type === 'error' ? 'danger' : (type === 'success' ? 'success' : 'info')}`;
  toast.style.boxShadow = '0 4px 12px rgba(0,0,0,0.3)';
  toast.style.minWidth = '280px';
  toast.style.transition = 'opacity 0.3s ease';

  toast.innerHTML = `<div>${message}</div>`;
  container.appendChild(toast);

  setTimeout(() => {
    toast.style.opacity = '0';
    setTimeout(() => toast.remove(), 300);
  }, 4000);
}

function openModal(modalId) {
  const modal = document.getElementById(modalId);
  if (modal) {
    modal.classList.add('active');
  }
}

function closeModal(modalId) {
  const modal = document.getElementById(modalId);
  if (modal) {
    modal.classList.remove('active');
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

// ==============================================================================
// 2. Navegación entre Vistas
// ==============================================================================
const VALID_VIEWS = [
  'dashboard', 'logs', 'storage', 'networking', 'services', 'terminal',
  'shares', 'backups', 'users', 'diagnostics', 'updates', 'applications', 'domain'
];

function switchView(viewName, updateHash = true) {
  if (!VALID_VIEWS.includes(viewName)) {
    viewName = 'dashboard';
  }

  AppState.activeView = viewName;

  if (updateHash && window.location.hash !== '#' + viewName) {
    window.location.hash = '#' + viewName;
  }

  // Actualizar sidebar nav
  document.querySelectorAll('.sidebar-nav .nav-item').forEach(item => {
    if (item.getAttribute('data-view') === viewName) {
      item.classList.add('active');
    } else {
      item.classList.remove('active');
    }
  });

  // Actualizar secciones de vista
  document.querySelectorAll('.view-section').forEach(sec => {
    if (sec.id === `view-${viewName}`) {
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
  switch (viewName) {
    case 'dashboard':
      refreshDashboardMetrics();
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
      break;
    case 'services':
      loadServices();
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
    case 'diagnostics':
      loadDiagnostics();
      break;
    case 'updates':
      loadUpdates();
      break;
    case 'applications':
      loadApplications();
      break;
    case 'domain':
      loadDomain();
      break;
  }
}

// ==============================================================================
// 3. Clientes API Asíncronos (Fetch)
// ==============================================================================
async function apiFetch(endpoint, options = {}) {
  try {
    const res = await fetch(endpoint, {
      ...options,
      headers: {
        'Accept': 'application/json',
        'Content-Type': 'application/json',
        ...(options.headers || {}),
      },
    });

    if (res.status === 401) {
      showToast('Sesión no autorizada o expirada. Redirigiendo...', 'warning');
      setTimeout(() => {
        window.location.href = '/login';
      }, 800);
      throw new Error('Sesión expirada.');
    }

    const data = await res.json();
    if (!res.ok || data.success === false) {
      throw new Error(data.error || data.message || `Error ${res.status}`);
    }
    return data;
  } catch (err) {
    if (err.message !== 'Sesión expirada.') {
      showToast(err.message, 'error');
    }
    throw err;
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
    const d = res.data;

    if (d.system) {
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

    feed.innerHTML = logs.map(l => `
      <div style="font-size:12px; border-bottom:1px solid var(--border-color); padding:4px 0;">
        <span style="color:var(--accent-primary); font-family:monospace;">${l.unit}:</span>
        <span>${escapeHtml(l.message)}</span>
      </div>
    `).join('');
  } catch (e) {
    feed.innerHTML = '<p style="color:var(--accent-danger);">No se pudo cargar la actividad reciente.</p>';
  }
}

// ==============================================================================
// 5. Módulo: Recursos Compartidos (Samba)
// ==============================================================================
async function loadShares() {
  const tbody = document.getElementById('shares-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/shares');
    const shares = res.data || [];

    if (shares.length === 0) {
      tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;">No hay recursos compartidos creados actualmente.</td></tr>';
      return;
    }

    tbody.innerHTML = shares.map(s => `
      <tr>
        <td><strong>[${escapeHtml(s.name)}]</strong></td>
        <td><code>${escapeHtml(s.path)}</code></td>
        <td><span class="tag-pill">${escapeHtml(s.scheme_name)}</span></td>
        <td>${s.hidden ? '<span class="badge badge-gray">Oculto ($)</span>' : '<span class="badge badge-ok">Visible</span>'}</td>
        <td>${(s.valid_users || []).map(u => `<span class="tag-pill">${escapeHtml(u)}</span>`).join(' ') || (s.guest_ok ? '<em>Invitados</em>' : '<em>Sistemas</em>')}</td>
        <td style="text-align:right;">
          ${s.name === 'SISTEMAS' ? '' : `
            <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="deleteShare('${escapeHtml(s.name)}')">
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

    container.innerHTML = AppState.groupsCache.map(g => `
      <label style="display:flex; align-items:center; gap:8px;">
        <input type="checkbox" name="share_group" value="${escapeHtml(g.name)}" ${g.name === 'grp_sistemas' ? 'checked' : ''}>
        <span>${escapeHtml(g.name)}</span>
      </label>
    `).join('');

    if (writeSelect) {
      writeSelect.innerHTML = AppState.groupsCache.map(g => `
        <option value="${escapeHtml(g.name)}">${escapeHtml(g.name)}</option>
      `).join('');
    }
  } catch (e) {
    console.error('Error cargando grupos para selector:', e);
  }
}

async function submitNewShare(event) {
  event.preventDefault();
  const name = document.getElementById('share-name').value;
  const comment = document.getElementById('share-comment').value;
  const scheme = parseInt(document.getElementById('share-scheme').value, 10);
  const hidden = document.getElementById('share-hidden').checked;
  const writeGroup = document.getElementById('share-write-group').value;

  const selectedGroups = Array.from(document.querySelectorAll('input[name="share_group"]:checked'))
    .map(cb => cb.value);

  try {
    await apiFetch('/api/shares', {
      method: 'POST',
      body: JSON.stringify({
        name,
        comment,
        scheme,
        hidden,
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
  }
}

async function deleteShare(name) {
  if (!confirm(`¿Confirmas la eliminación del recurso compartido [${name}]? (Los archivos en disco se conservarán)`)) {
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

// ==============================================================================
// 6. Módulo: Central de Respaldos (Backups)
// ==============================================================================
async function loadBackups() {
  const tbody = document.getElementById('backups-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/backups');
    const tasks = res.data || [];

    if (tasks.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" style="text-align:center;">No hay tareas de backup programadas actualmente.</td></tr>';
      return;
    }

    tbody.innerHTML = tasks.map(t => `
      <tr>
        <td><strong>${escapeHtml(t.id)}</strong></td>
        <td><span class="tag-pill">${escapeHtml(t.protocol)}</span></td>
        <td><code>${escapeHtml(t.source)}</code></td>
        <td>${escapeHtml(t.cron_desc)}</td>
        <td>${t.retention} snapshots</td>
        <td>
          ${t.last_status === 'OK' ? '<span class="badge badge-ok">OK</span>' : (t.last_status === 'Error' ? '<span class="badge badge-danger">Error</span>' : '<span class="badge badge-gray">Pendiente</span>')}
        </td>
        <td style="text-align:right;">
          <button class="btn btn-secondary btn-sm" title="Ejecutar ahora" onclick="runBackupTask('${escapeHtml(t.id)}')">
            <svg class="icon icon-sm" style="color:var(--accent-success);"><use href="#icon-play"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Ver bitácora" onclick="viewBackupLogs('${escapeHtml(t.id)}')">
            <svg class="icon icon-sm"><use href="#icon-file"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Eliminar tarea" style="color:var(--accent-danger);" onclick="deleteBackupTask('${escapeHtml(t.id)}')">
            <svg class="icon icon-sm"><use href="#icon-trash"></use></svg>
          </button>
        </td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger);">Error al cargar tareas de backup: ${escapeHtml(e.message)}</td></tr>`;
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
      }),
    });

    showToast(`Tarea de respaldo [${id}] programada con éxito.`, 'success');
    closeModal('modal-new-backup');
    document.getElementById('form-new-backup').reset();
    loadBackups();
  } catch (e) {
    // Ya mostrado
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

async function viewBackupLogs(taskId) {
  const title = document.getElementById('modal-backup-logs-title');
  const content = document.getElementById('modal-backup-logs-content');
  if (title) title.textContent = `Bitácora de respaldo: ${taskId}`;
  if (content) content.textContent = 'Cargando registro...';

  openModal('modal-backup-logs');

  try {
    const res = await apiFetch(`/api/backups/${encodeURIComponent(taskId)}/logs`);
    if (content) content.textContent = res.data.logs || 'Sin contenido registrado aún.';
  } catch (e) {
    if (content) content.textContent = 'Error al leer la bitácora: ' + e.message;
  }
}

async function deleteBackupTask(taskId) {
  if (!confirm(`¿Deseas eliminar la tarea de backup [${taskId}]? Los snapshots históricos ya creados se preservarán.`)) {
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
async function loadStorage() {
  const tbody = document.getElementById('storage-disks-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/storage');
    const disks = res.data.disks || [];

    if (disks.length === 0) {
      tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;">No se detectaron discos de bloque adicionales.</td></tr>';
      return;
    }

    tbody.innerHTML = disks.map(d => `
      <tr>
        <td><strong>${escapeHtml(d.name)}</strong></td>
        <td>${escapeHtml(d.model)}</td>
        <td>${escapeHtml(d.size)}</td>
        <td>${escapeHtml(d.type)}</td>
        <td><code>${escapeHtml(d.mount || 'Sin montar')}</code></td>
        <td>${escapeHtml(d.fstype || 'N/A')}</td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--accent-danger);">Error al cargar inventario: ${escapeHtml(e.message)}</td></tr>`;
  }
}

async function runStorageScrub() {
  try {
    await apiFetch('/api/storage/scrub', { method: 'POST' });
    showToast('Auditoría BTRFS scrub iniciada.', 'info');
  } catch (e) {
    // Ya mostrado
  }
}

async function runStorageTrim() {
  try {
    await apiFetch('/api/storage/trim', { method: 'POST' });
    showToast('Optimización fstrim ejecutada.', 'success');
  } catch (e) {
    // Ya mostrado
  }
}

// ==============================================================================
// 8. Módulo: Usuarios & Grupos
// ==============================================================================
async function loadUsersAndGroups() {
  const usersTbody = document.getElementById('users-table-body');
  const groupsTbody = document.getElementById('groups-table-body');

  try {
    const [uRes, gRes] = await Promise.all([
      apiFetch('/api/users'),
      apiFetch('/api/groups'),
    ]);

    const users = uRes.data || [];
    const groups = gRes.data || [];

    if (usersTbody) {
      usersTbody.innerHTML = users.map(u => `
        <tr>
          <td><strong>${escapeHtml(u.username)}</strong></td>
          <td>${u.uid}</td>
          <td>${u.is_admin ? '<span class="badge badge-warning">Admin</span>' : '<span class="badge badge-gray">Estándar</span>'}</td>
          <td>${u.is_samba ? '<span class="badge badge-ok">Sincronizado</span>' : '<span class="badge badge-danger">Inactivo</span>'}</td>
          <td style="text-align:right;">
            ${['root', 'administrador'].includes(u.username) ? '' : `
              <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="deleteUser('${escapeHtml(u.username)}')">
                <svg class="icon icon-sm"><use href="#icon-trash"></use></svg>
              </button>
            `}
          </td>
        </tr>
      `).join('');
    }

    if (groupsTbody) {
      groupsTbody.innerHTML = groups.map(g => `
        <tr>
          <td><strong>${escapeHtml(g.name)}</strong></td>
          <td>${g.gid}</td>
          <td>${(g.members || []).map(m => `<span class="tag-pill">${escapeHtml(m)}</span>`).join(' ') || '<em>Sin miembros</em>'}</td>
          <td style="text-align:right;">
            ${g.name === 'grp_sistemas' ? '<span class="badge badge-gray">Maestro</span>' : `
              <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="deleteGroup('${escapeHtml(g.name)}')">
                <svg class="icon icon-sm"><use href="#icon-trash"></use></svg>
              </button>
            `}
          </td>
        </tr>
      `).join('');
    }

    populateUserGroupOptions(groups);
  } catch (e) {
    console.error('Error cargando usuarios y grupos:', e);
  }
}

function populateUserGroupOptions(groups) {
  const container = document.getElementById('user-groups-list');
  if (!container) return;

  container.innerHTML = groups.map(g => `
    <label style="display:flex; align-items:center; gap:8px;">
      <input type="checkbox" name="user_group" value="${escapeHtml(g.name)}">
      <span>${escapeHtml(g.name)}</span>
    </label>
  `).join('');
}

async function submitNewUser(event) {
  event.preventDefault();
  const username = document.getElementById('user-uname').value;
  const password = document.getElementById('user-pass').value;
  const isAdmin = document.getElementById('user-is-admin').checked;

  const selectedGroups = Array.from(document.querySelectorAll('input[name="user_group"]:checked'))
    .map(cb => cb.value);

  try {
    await apiFetch('/api/users', {
      method: 'POST',
      body: JSON.stringify({
        username,
        password,
        groups: selectedGroups,
        is_admin: isAdmin,
      }),
    });

    showToast(`Usuario [${username}] creado y sincronizado en Samba.`, 'success');
    closeModal('modal-new-user');
    document.getElementById('form-new-user').reset();
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  }
}

async function deleteUser(username) {
  if (!confirm(`¿Confirmas la eliminación del usuario [${username}] y su directorio home?`)) {
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
  }
}

async function deleteGroup(name) {
  if (!confirm(`¿Confirmas la eliminación del grupo corporativo [${name}]?`)) {
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
        <td style="text-align:right;">
          <button class="btn btn-secondary btn-sm" onclick="restartService('${escapeHtml(s.service)}')">
            <svg class="icon icon-sm"><use href="#icon-refresh"></use></svg> Reiniciar
          </button>
        </td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="4" style="text-align:center; color:var(--accent-danger);">Error al cargar servicios: ${escapeHtml(e.message)}</td></tr>`;
  }
}

async function restartService(service) {
  try {
    await apiFetch('/api/services/manage', {
      method: 'POST',
      body: JSON.stringify({ service, action: 'restart' }),
    });

    showToast(`Servicio [${service}] reiniciado con éxito.`, 'success');
    loadServices();
  } catch (e) {
    // Ya mostrado
  }
}

// ==============================================================================
// 10. Módulo: Registros del Sistema (Logs)
// ==============================================================================
async function loadLogs() {
  const tbody = document.getElementById('logs-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/logs?limit=100');
    const logs = res.data || [];

    if (logs.length === 0) {
      tbody.innerHTML = '<tr><td colspan="3" style="text-align:center;">Sin registros en journald.</td></tr>';
      return;
    }

    tbody.innerHTML = logs.map(l => `
      <tr>
        <td><code>${escapeHtml(l.timestamp || '-')}</code></td>
        <td><span class="tag-pill">${escapeHtml(l.unit || 'system')}</span></td>
        <td style="font-family:monospace; font-size:12px;">${escapeHtml(l.message)}</td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="3" style="text-align:center; color:var(--accent-danger);">Error al leer journald: ${escapeHtml(e.message)}</td></tr>`;
  }
}

// ==============================================================================
// 11. Módulo: Terminal Interactiva Simula/Diagnóstico
// ==============================================================================
function initTerminal() {
  const input = document.getElementById('terminal-input');
  const output = document.getElementById('terminal-output');
  if (!input || !output) return;

  input.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      const cmd = input.value.trim();
      input.value = '';
      if (!cmd) return;

      output.innerHTML += `<p><span style="color:var(--accent-primary); font-weight:bold;">nas&gt;</span> ${escapeHtml(cmd)}</p>`;

      const lower = cmd.toLowerCase();
      if (lower === 'clear' || lower === 'cls') {
        output.innerHTML = '';
        return;
      }

      if (lower === 'help') {
        output.innerHTML += `
          <p>Comandos de diagnóstico disponibles:</p>
          <p>  <code>status</code>    - Muestra estado de servicios y métricas</p>
          <p>  <code>shares</code>    - Lista recursos compartidos de Samba</p>
          <p>  <code>backups</code>   - Lista tareas de respaldo y retention</p>
          <p>  <code>disks</code>     - Lista dispositivos de almacenamiento</p>
          <p>  <code>clear</code>     - Limpia la pantalla de la terminal</p>
        `;
      } else if (lower === 'status') {
        try {
          const res = await apiFetch('/api/metrics');
          output.innerHTML += `<pre>${JSON.stringify(res.data, null, 2)}</pre>`;
        } catch (err) {
          output.innerHTML += `<p style="color:var(--accent-danger);">Error: ${escapeHtml(err.message)}</p>`;
        }
      } else if (lower === 'shares') {
        try {
          const res = await apiFetch('/api/shares');
          output.innerHTML += `<pre>${JSON.stringify(res.data, null, 2)}</pre>`;
        } catch (err) {
          output.innerHTML += `<p style="color:var(--accent-danger);">Error: ${escapeHtml(err.message)}</p>`;
        }
      } else if (lower === 'backups') {
        try {
          const res = await apiFetch('/api/backups');
          output.innerHTML += `<pre>${JSON.stringify(res.data, null, 2)}</pre>`;
        } catch (err) {
          output.innerHTML += `<p style="color:var(--accent-danger);">Error: ${escapeHtml(err.message)}</p>`;
        }
      } else if (lower === 'disks') {
        try {
          const res = await apiFetch('/api/storage');
          output.innerHTML += `<pre>${JSON.stringify(res.data, null, 2)}</pre>`;
        } catch (err) {
          output.innerHTML += `<p style="color:var(--accent-danger);">Error: ${escapeHtml(err.message)}</p>`;
        }
      } else {
        output.innerHTML += `<p style="color:var(--accent-warning);">Comando no reconocido: ${escapeHtml(cmd)}. Escribe <code>help</code>.</p>`;
      }

      output.scrollTop = output.scrollHeight;
    }
  });
}

// ==============================================================================
// 12. Módulo: Actualizaciones y Reinicio
// ==============================================================================
async function loadUpdates() {
  const container = document.getElementById('updates-container');
  if (!container) return;

  try {
    const res = await apiFetch('/api/system/updates');
    const u = res.data;

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

async function confirmRebootServer() {
  try {
    await apiFetch('/api/system/reboot', { method: 'POST' });
    closeModal('modal-reboot-server');
    showToast('Reinicio del servidor ordenado. La conexión se reanudará en 1-2 minutos.', 'warning');
  } catch (e) {
    // Ya mostrado
  }
}

// ==============================================================================
// 13. Utilidades Generales
// ==============================================================================
function escapeHtml(str) {
  if (typeof str !== 'string') return String(str ?? '');
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

// ==============================================================================
// 14. Módulos Adicionales: Redes, Terminal, Aplicaciones, Dominio y Diagnósticos
// ==============================================================================
async function loadNetworking() {
  try {
    const res = await apiFetch('/api/metrics');
    const d = res.data;
    if (d && d.system) {
      const hn = document.getElementById('masthead-hostname');
      if (hn && d.system.hostname) hn.textContent = d.system.hostname;
    }
  } catch (e) {
    // Manejado por apiFetch
  }
}

function loadTerminal() {
  const input = document.getElementById('terminal-input');
  if (input) {
    setTimeout(() => input.focus(), 80);
  }
}

function loadApplications() {
  // Vista informativa de tecnologías y componentes integrados
}

function loadDomain() {
  // Vista informativa de configuración de Grupo de Trabajo / Directorio Activo
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
    const d = res.data;

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
      const servicesList = Object.entries(d.services || {}).map(([svc, item]) => {
        const isOk = item.active;
        return `
          <div style="display:flex; justify-content:space-between; align-items:center; padding:6px 0; border-bottom:1px solid var(--border-color); font-size:13px;">
            <span><code>${escapeHtml(svc)}</code> &bull; ${escapeHtml(item.name || '')}</span>
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
  // Configurar listeners de navegación sidebar
  document.querySelectorAll('.sidebar-nav .nav-item').forEach(item => {
    item.addEventListener('click', () => {
      const view = item.getAttribute('data-view');
      if (view) switchView(view);
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

  // Listener de cambios en el hash de la URL (botones Atrás/Adelante y navegación)
  window.addEventListener('hashchange', () => {
    const hash = window.location.hash.slice(1);
    if (hash && VALID_VIEWS.includes(hash)) {
      switchView(hash, false);
    }
  });

  // Inicializar componentes interactivos
  initTerminal();
  populateShareGroupOptions();

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

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', initApp);
} else {
  initApp();
}
