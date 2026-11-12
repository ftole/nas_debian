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
