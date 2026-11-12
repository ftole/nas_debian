
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

