// 6. Módulo: Central de Respaldos (Backups)
// ==============================================================================
let _backupRefreshTimer = null;

let _backupsData = [];
let _permanentBackupsData = [];
let _backupSearchQuery = '';

function filterBackups(val) {
  _backupSearchQuery = typeof val === 'string' ? val : (document.getElementById('backups-search-input')?.value || '');
  renderBackupsTable();
  renderPermanentBackupsTable();
}

function renderBackupsTable() {
  const tbody = document.getElementById('backups-table-body');
  if (!tbody) return;

  if (!_backupsData || _backupsData.length === 0) {
    tbody.innerHTML = '<tr><td colspan="8" style="text-align:center;">No hay tareas de backup programadas actualmente.</td></tr>';
    return;
  }

  const q = (_backupSearchQuery || '').trim().toLowerCase();
  const tasks = q
    ? _backupsData.filter(t => {
        const id = String(t.id || '').toLowerCase();
        const src = String(t.source || '').toLowerCase();
        const dst = String(t.destination || '').toLowerCase();
        const ip = String(t.source_ip || '').toLowerCase();
        const share = String(t.share || '').toLowerCase();
        const path = String(t.path || '').toLowerCase();
        const proto = String(t.protocol || '').toLowerCase();
        return id.includes(q) || src.includes(q) || dst.includes(q) || ip.includes(q) || share.includes(q) || path.includes(q) || proto.includes(q);
      })
    : _backupsData;

  if (tasks.length === 0) {
    tbody.innerHTML = `<tr><td colspan="8" style="text-align:center; color:var(--text-muted); padding:24px 12px;">No se encontraron tareas de respaldo que coincidan con "<strong>${escapeHtml(_backupSearchQuery)}</strong>".</td></tr>`;
    return;
  }

  tbody.innerHTML = tasks.map(t => {
    const isErr = t.last_status === 'Error' || t.last_status === 'FAILED';
    const isCancelled = t.last_status === 'Cancelada' || t.last_status === 'Cancelado';
    const isRunning = Boolean(t.running || t.last_status === 'En curso');
    const statusBadge = t.last_status === 'OK' ? 'badge-ok'
      : (isCancelled ? 'badge-warn'
        : (isErr ? 'badge-err'
          : (isRunning ? 'badge-blue' : 'badge-gray')));
    const pct = Math.max(0, Math.min(100, Number(t.percent || 0)));
    const fillColor = isCancelled ? 'var(--accent-warning)'
      : (isErr ? 'var(--accent-danger)'
        : (isRunning ? 'var(--accent-primary)' : 'var(--accent-success)'));
    const progressSubtitle = isRunning
      ? `${pct}% · ${escapeHtml(t.elapsed || '00:00:00')} / ${escapeHtml(t.remaining || '--:--:--')}${t.speed ? ' · ' + escapeHtml(t.speed) : ''}`
      : (t.last_status === 'OK'
        ? '100% · Completado'
        : (isCancelled
          ? 'Cancelada · Lista para ejecutar'
          : (isErr ? 'Error · Reintentar' : 'Pendiente')));

    const taskSize = t.full_size_formatted || t.size_formatted || t.task_size_formatted || '0 B';
    const diskUsage = (t.disk_usage_formatted && t.disk_usage_formatted !== '0 B')
      ? `Ocupación en disco: ${escapeHtml(t.disk_usage_formatted)}`
      : null;
    const diskText = (t.disk_total_formatted && t.disk_total_formatted !== '')
      ? `Volumen: ${escapeHtml(t.disk_used_formatted)} / ${escapeHtml(t.disk_total_formatted)} (${Math.round(t.disk_used_percent || 0)}%)`
      : (t.disk_summary ? escapeHtml(t.disk_summary) : 'Volumen: N/A');

    return `
    <tr>
      <td><strong>${escapeHtml(t.id)}</strong></td>
      <td><span class="tag-pill">${escapeHtml(t.protocol)}</span></td>
      <td><code>${escapeHtml(t.source || t.destination || 'N/A')}</code></td>
      <td>${escapeHtml(t.cron_desc)}</td>
      <td>${t.retention} snapshots</td>
      <td>
        <div><strong>${escapeHtml(taskSize)}</strong> <span style="font-size:11px; color:var(--text-muted);">(Respaldo)</span></div>
        ${diskUsage ? `<div style="font-size:11px; color:var(--text-muted); margin-top:2px;">${diskUsage}</div>` : ''}
        <div style="font-size:11px; color:var(--text-muted); margin-top:2px;">
          ${diskText}
        </div>
      </td>
      <td>
        <span class="badge ${statusBadge}">${escapeHtml(t.last_status)}</span>
        <div class="progress-bar-wrap" style="margin-top:5px; height:6px;">
          <div class="progress-bar-fill" style="width:${pct}%; background:${fillColor};"></div>
        </div>
        <div style="font-size:11px; color:var(--text-muted); margin-top:3px;">
          ${progressSubtitle}
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
        <button class="btn btn-secondary btn-sm" title="Congelar como permanente (Golden Snapshot)" style="color:var(--accent-warning);" onclick="openFreezeModal('${escapeHtml(t.id)}')">
          <svg class="icon icon-sm" style="color:var(--accent-warning);"><use href="#icon-shield"></use></svg>
        </button>
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
}

async function loadBackups() {
  const tbody = document.getElementById('backups-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/backups');
    const tasks = res.data || [];
    _backupsData = tasks;

    renderBackupsTable();

    const anyRunning = tasks.some(t => Boolean(t.running || t.last_status === 'En curso'));
    scheduleBackupRefresh(anyRunning);
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="8" style="text-align:center; color:var(--accent-danger);">Error al cargar tareas de backup: ${escapeHtml(e.message)}</td></tr>`;
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
    }, 2000);
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
  const auto_permanent = document.getElementById('bkp-auto-permanent')?.checked ?? true;

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
        auto_permanent,
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

  const autoPermEl = document.getElementById('edit-bkp-auto-permanent');
  if (autoPermEl) {
    autoPermEl.checked = t.auto_permanent !== false;
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
  const auto_permanent = document.getElementById('edit-bkp-auto-permanent')?.checked ?? true;

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
      auto_permanent,
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

let _pendingRunBackupTaskId = null;
let _pendingStopBackupTaskId = null;

function runBackupTask(taskId) {
  openRunBackupModal(taskId);
}

function openRunBackupModal(taskId) {
  _pendingRunBackupTaskId = taskId;
  const t = (_backupsData || []).find(x => x.id === taskId);

  const idEl = document.getElementById('confirm-run-backup-id');
  const protoEl = document.getElementById('confirm-run-backup-proto');
  const srcEl = document.getElementById('confirm-run-backup-source');
  const destEl = document.getElementById('confirm-run-backup-dest');
  const cronEl = document.getElementById('confirm-run-backup-cron');

  if (idEl) idEl.textContent = taskId;
  if (protoEl) protoEl.textContent = t ? (t.protocol || 'CIFS (Windows)') : 'CIFS (Windows)';
  if (srcEl) srcEl.textContent = t ? (t.source || t.destination || 'N/A') : 'N/A';
  if (destEl) destEl.textContent = t ? (t.dest_path || `/srv/nas/BACKUPS_HISTORICOS/${taskId}`) : `/srv/nas/BACKUPS_HISTORICOS/${taskId}`;
  if (cronEl) cronEl.textContent = t ? (t.cron_desc || t.cron || 'Programado') : 'Programado';

  const btn = document.getElementById('btn-confirm-run-backup');
  if (btn) {
    btn.disabled = false;
    btn.classList.remove('is-loading');
  }

  openModal('modal-run-backup-confirm');
}

async function executeRunBackupConfirm() {
  const taskId = _pendingRunBackupTaskId;
  if (!taskId) return;

  const btn = document.getElementById('btn-confirm-run-backup');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch(`/api/backups/${encodeURIComponent(taskId)}/run`, { method: 'POST' });
    showToast(`Respaldo de [${taskId}] iniciado en segundo plano.`, 'info');
    closeModal('modal-run-backup-confirm');
    setTimeout(loadBackups, 1200);
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function stopBackupTask(taskId) {
  openStopBackupModal(taskId);
}

function openStopBackupModal(taskId) {
  _pendingStopBackupTaskId = taskId;
  const t = (_backupsData || []).find(x => x.id === taskId);

  const idEl = document.getElementById('confirm-stop-backup-id');
  const progEl = document.getElementById('confirm-stop-backup-progress');
  const elapsedEl = document.getElementById('confirm-stop-backup-elapsed');
  const speedEl = document.getElementById('confirm-stop-backup-speed');

  if (idEl) idEl.textContent = taskId;
  if (progEl) progEl.textContent = t ? `${Math.round(t.percent || 0)}% completado` : 'En ejecución';
  if (elapsedEl) elapsedEl.textContent = t ? (t.elapsed || '00:00:00') : '00:00:00';
  if (speedEl) speedEl.textContent = t ? (t.speed || 'Calculando tasa...') : 'Calculando tasa...';

  const btn = document.getElementById('btn-confirm-stop-backup');
  if (btn) {
    btn.disabled = false;
    btn.classList.remove('is-loading');
  }

  openModal('modal-stop-backup-confirm');
}

async function executeStopBackupConfirm() {
  const taskId = _pendingStopBackupTaskId;
  if (!taskId) return;

  const btn = document.getElementById('btn-confirm-stop-backup');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch(`/api/backups/${encodeURIComponent(taskId)}/stop`, { method: 'POST' });
    showToast(`Tarea de respaldo [${taskId}] cancelada.`, 'warning');
    closeModal('modal-stop-backup-confirm');
    const task = (_backupsData || []).find(x => x.id === taskId);
    if (task) {
      task.running = false;
      task.last_status = 'Cancelada';
    }
    loadBackups();
    setTimeout(loadBackups, 1200);
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
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
// Respaldos Permanentes (Golden Snapshots) y Restauración
// ==============================================================================
let _activeBackupTab = 'rotative';

function switchBackupTab(tab) {
  _activeBackupTab = tab;
  const btnRot = document.getElementById('tab-btn-rotative');
  const btnPerm = document.getElementById('tab-btn-permanent');
  const rotContainer = document.getElementById('backups-rotative-container');
  const permContainer = document.getElementById('backups-permanent-container');
  const searchInput = document.getElementById('backups-search-input');

  if (searchInput) {
    searchInput.placeholder = tab === 'permanent'
      ? 'Buscar por tarea, nombre o etiqueta...'
      : 'Buscar por identificador u origen remoto...';
  }

  if (tab === 'permanent') {
    if (btnRot) btnRot.classList.remove('active');
    if (btnPerm) btnPerm.classList.add('active');
    if (rotContainer) rotContainer.style.display = 'none';
    if (permContainer) permContainer.style.display = '';
    loadPermanentBackups();
  } else {
    if (btnRot) btnRot.classList.add('active');
    if (btnPerm) btnPerm.classList.remove('active');
    if (rotContainer) rotContainer.style.display = '';
    if (permContainer) permContainer.style.display = 'none';
    loadBackups();
  }
}

function renderPermanentBackupsTable() {
  const tbody = document.getElementById('backups-permanent-table-body');
  if (!tbody) return;

  if (!_permanentBackupsData || _permanentBackupsData.length === 0) {
    tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;">No hay respaldos permanentes consolidados actualmente.</td></tr>';
    return;
  }

  const q = (_backupSearchQuery || '').trim().toLowerCase();
  const items = q
    ? _permanentBackupsData.filter(p => {
        const taskName = String(p.task || p.task_id || '').toLowerCase();
        const name = String(p.name || '').toLowerCase();
        const tag = String(p.tag || '').toLowerCase();
        const path = String(p.path || '').toLowerCase();
        return taskName.includes(q) || name.includes(q) || tag.includes(q) || path.includes(q);
      })
    : _permanentBackupsData;

  if (items.length === 0) {
    tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--text-muted); padding:24px 12px;">No se encontraron respaldos permanentes que coincidan con "<strong>${escapeHtml(_backupSearchQuery)}</strong>".</td></tr>`;
    return;
  }

  tbody.innerHTML = items.map(p => {
    const tagHtml = p.tag ? `<span class="tag-pill">${escapeHtml(p.tag)}</span>` : '<span class="text-muted">-</span>';
    const sizeStr = formatBytesLocal(p.size_bytes || 0);
    const filesStr = p.files_count !== undefined ? `${p.files_count} archivos` : '-';

    const taskName = p.task || p.task_id || '';
    const dateStr = p.date || p.created_at || '';

    return `
    <tr>
      <td><strong>${escapeHtml(taskName)}</strong></td>
      <td>
        <div style="font-weight:600;">${escapeHtml(p.name)}</div>
        <small style="color:var(--text-muted); font-size:11px;">${escapeHtml(dateStr)}</small>
      </td>
      <td>${tagHtml}</td>
      <td><code style="font-size:11px;">${escapeHtml(p.path)}</code></td>
      <td>${filesStr} <small class="text-muted">(${sizeStr})</small></td>
      <td style="text-align:right; white-space:nowrap;">
        <button class="btn btn-secondary btn-sm" title="Restaurar este respaldo" style="color:var(--accent-primary);" onclick="openRestoreBackup('${escapeHtml(taskName)}', '${escapeHtml(p.path)}')">
          <svg class="icon icon-sm" style="color:var(--accent-primary);"><use href="#icon-restore"></use></svg> Restaurar
        </button>
        <button class="btn btn-secondary btn-sm" title="Eliminar respaldo permanente" style="color:var(--accent-danger);" onclick="deletePermanentBackup('${escapeHtml(taskName)}', '${escapeHtml(p.name)}')">
          <svg class="icon icon-sm" style="color:var(--accent-danger);"><use href="#icon-trash"></use></svg>
        </button>
      </td>
    </tr>
  `;
  }).join('');
}

async function loadPermanentBackups() {
  const tbody = document.getElementById('backups-permanent-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/backups/permanent');
    _permanentBackupsData = res.data || [];
    renderPermanentBackupsTable();
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--accent-danger);">Error al cargar respaldos permanentes: ${escapeHtml(e.message)}</td></tr>`;
  }
}

async function openFreezeModal(taskId) {
  const select = document.getElementById('freeze-bkp-id');
  const tagInput = document.getElementById('freeze-bkp-tag');
  if (tagInput) tagInput.value = '';

  if (select) {
    select.innerHTML = '<option value="">Cargando tareas...</option>';
    try {
      if (!_backupsData || _backupsData.length === 0) {
        const res = await apiFetch('/api/backups');
        _backupsData = res.data || [];
      }
      select.innerHTML = _backupsData.map(t => `<option value="${escapeHtml(t.id)}">${escapeHtml(t.id)} (${escapeHtml(t.protocol || 'local')})</option>`).join('');
      if (taskId) {
        select.value = taskId;
      }
    } catch (e) {
      select.innerHTML = '<option value="">Error al cargar tareas</option>';
    }
  }

  openModal('modal-freeze-backup');
}

async function submitFreezeBackup(event) {
  if (event) event.preventDefault();
  const select = document.getElementById('freeze-bkp-id');
  const tagInput = document.getElementById('freeze-bkp-tag');
  const btn = document.getElementById('btn-submit-freeze-backup');

  const taskId = select ? select.value : '';
  const tag = tagInput ? tagInput.value.trim() : '';

  if (!taskId) {
    showToast('Selecciona una tarea de respaldo.', 'error');
    return;
  }

  if (btn) btn.disabled = true;
  try {
    const res = await apiFetch('/api/backups/freeze', {
      method: 'POST',
      body: JSON.stringify({ id: taskId, tag: tag || null }),
    });

    closeModal('modal-freeze-backup');
    showToast(res.message || 'Respaldo congelado como permanente exitosamente.', 'success');
    switchBackupTab('permanent');
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) btn.disabled = false;
  }
}

function openRestoreBackup(taskId, snapshotPath) {
  const taskIdEl = document.getElementById('restore-bkp-task-id');
  const snapPathEl = document.getElementById('restore-bkp-snapshot-path');
  const nameDisplay = document.getElementById('restore-bkp-task-name');
  const snapDisplay = document.getElementById('restore-bkp-snap-display');
  const destType = document.getElementById('restore-bkp-dest-type');
  const customField = document.getElementById('field-restore-custom-dest');
  const customInput = document.getElementById('restore-bkp-custom-path');

  if (taskIdEl) taskIdEl.value = taskId || '';
  if (snapPathEl) snapPathEl.value = snapshotPath || '';
  if (nameDisplay) nameDisplay.textContent = taskId || '';
  if (snapDisplay) snapDisplay.textContent = snapshotPath || '';
  if (destType) destType.value = 'origin';
  if (customField) customField.style.display = 'none';
  if (customInput) customInput.value = '';

  openModal('modal-restore-backup');
}

function toggleRestoreTargetField() {
  const destType = document.getElementById('restore-bkp-dest-type');
  const customField = document.getElementById('field-restore-custom-dest');
  if (destType && customField) {
    customField.style.display = destType.value === 'custom' ? '' : 'none';
  }
}

async function submitRestoreBackup(event) {
  if (event) event.preventDefault();
  const taskIdEl = document.getElementById('restore-bkp-task-id');
  const snapPathEl = document.getElementById('restore-bkp-snapshot-path');
  const destTypeEl = document.getElementById('restore-bkp-dest-type');
  const customPathEl = document.getElementById('restore-bkp-custom-path');
  const btn = document.getElementById('btn-submit-restore-backup');

  const taskId = taskIdEl ? taskIdEl.value : '';
  const snapshot = snapPathEl ? snapPathEl.value : '';
  const destType = destTypeEl ? destTypeEl.value : 'origin';
  let target = null;

  if (destType === 'custom') {
    target = customPathEl ? customPathEl.value.trim() : '';
    if (!target) {
      showToast('Ingresa la ruta local de destino para la restauración.', 'error');
      return;
    }
  }

  if (!window.confirm(`¿Confirmas la restauración del snapshot [${snapshot}]?\nDestino: ${target ? target : 'Origen de red original (sobrescribe archivos)'}`)) {
    return;
  }

  if (btn) btn.disabled = true;
  try {
    const res = await apiFetch('/api/backups/restore', {
      method: 'POST',
      body: JSON.stringify({ id: taskId, snapshot: snapshot, target: target }),
    });

    closeModal('modal-restore-backup');
    showToast(res.message || 'Restauración completada exitosamente.', 'success');
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function deletePermanentBackup(taskId, name) {
  if (!window.confirm(`¿Deseas eliminar permanentemente el respaldo [${name}] de la tarea [${taskId}]? Esta acción no se puede deshacer.`)) {
    return;
  }

  try {
    const res = await apiFetch('/api/backups/permanent/delete', {
      method: 'POST',
      body: JSON.stringify({ id: taskId, name: name }),
    });

    showToast(res.message || `Respaldo permanente [${name}] eliminado.`, 'success');
    loadPermanentBackups();
  } catch (e) {
    // Ya mostrado por apiFetch
  }
}

