// ==============================================================================
// 7. Módulo: Almacenamiento & Mantenimiento
// ==============================================================================
let _scrubPollTimer = null;
let _pendingStoragePayload = null;
let _pendingStorageUrl = null;
let _lastFstabAudit = null;

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

  const metaRow = document.getElementById('stg-pool-meta-row');
  if (metaRow) {
    const parts = [];
    if (overview.is_btrfs && (overview.data_profile || overview.metadata_profile)) {
      parts.push(`Perfil Btrfs: <strong>Data: ${escapeHtml((overview.data_profile || 'single').toUpperCase())}</strong> · <strong>Metadata: ${escapeHtml((overview.metadata_profile || 'DUP').toUpperCase())}</strong>`);
    }
    if (Array.isArray(overview.pool_devices) && overview.pool_devices.length > 0) {
      parts.push(`Miembros (${overview.pool_devices.length}): ${overview.pool_devices.map(d => `<code>${escapeHtml(d)}</code>`).join(', ')}`);
    } else if (overview.device) {
      parts.push(`Origen: <code>${escapeHtml(overview.device)}</code>`);
    }
    if (overview.is_readonly) {
      parts.push('<span class="badge badge-err">SOLO LECTURA (ro)</span>');
    }
    metaRow.innerHTML = parts.join(' &nbsp;·&nbsp; ');
  }

  const osAlert = document.getElementById('stg-os-disk-alert');
  const osAlertText = document.getElementById('stg-os-disk-alert-text');
  if (osAlert && osAlertText) {
    if (overview.is_readonly) {
      osAlert.style.display = 'flex';
      osAlertText.textContent = `¡ALERTA CRÍTICA! El volumen ${overview.path || '/srv/nas'} está montado en solo lectura (ro). Verifica salud S.M.A.R.T., cables y dmesg antes de continuar escribiendo respaldos.`;
    } else if (overview.data_root_on_os_disk) {
      osAlert.style.display = 'flex';
      osAlertText.textContent = `Atención: ${overview.path || '/srv/nas'} reside sobre el disco del sistema operativo (/). Crea o monta un pool dedicado antes de almacenar respaldos masivos.`;
    } else {
      osAlert.style.display = 'none';
    }
  }

  const statPercent = document.getElementById('stg-stat-percent');
  if (statPercent) statPercent.textContent = `${overview.usage_percent ?? 0}%`;

  const progress = document.getElementById('stg-main-progress');
  const badge = document.getElementById('stg-threshold-badge');
  const alertBox = document.getElementById('stg-threshold-alert');
  const alertText = document.getElementById('stg-threshold-text');
  const card = document.getElementById('stg-overview-card');

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
    if (card) card.style.borderColor = 'var(--accent-danger)';
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
    if (card) card.style.borderColor = 'var(--accent-warning)';
  } else {
    if (progress) progress.style.backgroundColor = 'var(--accent-primary)';
    if (badge) {
      badge.className = 'badge badge-ok';
      badge.textContent = `Normal (${pct}%)`;
    }
    if (alertBox) alertBox.style.display = 'none';
    if (card) card.style.borderColor = '';
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

function renderFstabAudit(fstabAudit) {
  _lastFstabAudit = fstabAudit || null;
  const box = document.getElementById('stg-fstab-alert');
  const text = document.getElementById('stg-fstab-alert-text');
  const btn = document.getElementById('btn-sanitize-fstab');
  if (!box || !text) return;
  if (!fstabAudit || fstabAudit.healthy || !Array.isArray(fstabAudit.issues) || fstabAudit.issues.length === 0) {
    box.style.display = 'none';
    return;
  }
  box.style.display = 'flex';
  const fixableCount = Number(
    fstabAudit.fixable_count ?? fstabAudit.issues.filter(i => i.code !== 'missing_uuid').length
  );
  if (btn) {
    btn.style.display = fixableCount > 0 ? '' : 'none';
  }
  const summary = fstabAudit.issues.slice(0, 3).map(i => i.detail).join(' · ');
  const extra = fstabAudit.issues.length > 3 ? ` (+${fstabAudit.issues.length - 3} más)` : '';
  const hint = fixableCount === 0 ? ' (Reconecta el disco o retira la línea en /etc/fstab si fue dado de baja)' : '';
  text.textContent = `Anomalías en /etc/fstab: ${summary}${extra}${hint}`;
}

function sanitizeStorageFstab() {
  const issues = Array.isArray(_lastFstabAudit?.issues) ? _lastFstabAudit.issues : [];
  const summaryEl = document.getElementById('sanitize-fstab-alert-summary');
  const listEl = document.getElementById('sanitize-fstab-issues-list');
  if (summaryEl) {
    summaryEl.textContent = issues.length > 0
      ? `Se detectaron ${issues.length} anomalía(s) en /etc/fstab que conviene corregir antes del próximo reinicio:`
      : 'Se revisará y normalizará /etc/fstab aplicando opciones seguras de almacenamiento.';
  }
  if (listEl) {
    if (issues.length > 0) {
      listEl.innerHTML = issues.map(issue => {
        const lineTag = issue.line_number ? `Línea ${escapeHtml(String(issue.line_number))}: ` : '';
        return `<li>${lineTag}${escapeHtml(issue.detail || '')}</li>`;
      }).join('');
    } else {
      listEl.innerHTML = '<li>Normalización de opciones <code>nofail,x-systemd.device-timeout=15s</code> y retiro de entradas huérfanas.</li>';
    }
  }
  openModal('modal-sanitize-fstab');
}

async function confirmSanitizeFstab() {
  closeModal('modal-sanitize-fstab');
  const btn = document.getElementById('btn-sanitize-fstab');
  const modalBtn = document.getElementById('btn-confirm-sanitize-fstab');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  if (modalBtn) { modalBtn.disabled = true; modalBtn.classList.add('is-loading'); }
  try {
    const res = await apiFetch('/api/storage/fstab-sanitize', {
      method: 'POST',
      body: JSON.stringify({ confirm: 'SI-SANEAR' }),
    });
    showToast(res.message || '/etc/fstab saneado correctamente.', 'success');
    loadStorage();
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
    if (modalBtn) { modalBtn.disabled = false; modalBtn.classList.remove('is-loading'); }
  }
}

async function loadStorage() {
  const tbody = document.getElementById('storage-disks-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/storage/disks');
    const payload = res.data || res;
    const disks = payload.disks ?? [];
    const overview = payload.overview ?? {};
    const fstabAudit = payload.fstab_audit ?? null;

    // Actualizar tarjeta principal de capacidad y estado
    renderStorageOverview(overview);
    renderFstabAudit(fstabAudit);

    // Actualizar tarjetas de inteligencia BTRFS y TRIM
    updateStorageIntelligence(overview);

    // Cargar unidades montadas y subvolúmenes
    loadStorageUnits();
    loadStorageSubvolumes(overview);

    // Ocultar discos protegidos por el sistema operativo
    const visibleDisks = disks.filter(d => !d.protected);

    if (visibleDisks.length === 0) {
      tbody.innerHTML = '<tr><td colspan="7" style="text-align:center;">No hay discos de datos disponibles (los discos del sistema operativo están ocultos).</td></tr>';
      return;
    }

    tbody.innerHTML = visibleDisks.map(d => {
      const devPath = d.device || ('/dev/' + d.name);
      const isPart = d.type === 'part';
      const nameHtml = isPart
        ? `<span style="font-family:monospace; color:var(--text-muted); margin-right:4px;">└─</span> <strong>${escapeHtml(d.name)}</strong>`
        : `<strong>${escapeHtml(d.name)}</strong>`;

      let stateBadge = '';
      if (d.mount === '/srv/nas') {
        stateBadge = d.is_pool_member
          ? ' <span class="badge badge-ok">Pool NAS (Multi-disco)</span>'
          : ' <span class="badge badge-ok">Pool NAS</span>';
      } else if (d.protected) {
        stateBadge = ` <span class="badge badge-err">SO · ${escapeHtml(d.mount || 'protegido')}</span>`;
      } else if (d.is_pool_member && d.mount) {
        stateBadge = ` <span class="badge badge-info">Miembro Pool (${escapeHtml(d.mount)})</span>`;
      } else if (d.in_use) {
        stateBadge = ' <span class="badge badge-warn">En uso</span>';
      } else {
        stateBadge = ' <span class="badge badge-ok">Disponible</span>';
      }

      const mediaTag = d.rotational === false
        ? ' <span class="badge badge-info" title="Estado sólido">SSD</span>'
        : (d.rotational === true ? ' <span class="badge badge-gray" title="Mecánico">HDD</span>' : '');
      const typeBadge = (isPart
        ? '<span class="badge badge-gray">Partición</span>'
        : '<span class="badge badge-gray">Disco</span>') + mediaTag;

      const fsInfo = d.pool_uuid
        ? `${escapeHtml(d.fstype || 'btrfs')} <small class="text-muted">(${escapeHtml(String(d.pool_uuid).slice(0, 8))}…)</small>`
        : escapeHtml(d.fstype || 'N/A');

      return `
      <tr>
        <td>${nameHtml}${stateBadge}</td>
        <td>${escapeHtml(d.model || 'Genérico')}</td>
        <td>${escapeHtml(d.size)}</td>
        <td>${typeBadge}</td>
        <td><code>${escapeHtml(d.mount || 'Sin montar')}</code></td>
        <td>${fsInfo}</td>
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

function getMaintenanceUnitId() {
  return document.getElementById('stg-maint-unit')?.value || 'principal';
}

function getSelectedMaintenanceOverview(fallbackOverview = null) {
  const unitId = getMaintenanceUnitId();
  if (typeof _storageUnitsData !== 'undefined' && Array.isArray(_storageUnitsData)) {
    const found = _storageUnitsData.find(u => u.id === unitId);
    if (found) return found;
  }
  return fallbackOverview;
}

function onMaintenanceUnitChange() {
  const unitOverview = getSelectedMaintenanceOverview();
  if (unitOverview) {
    updateStorageIntelligence(unitOverview);
  } else {
    checkBtrfsScrubStatus(false);
  }
}

function runUnitScrub(unitId) {
  const sel = document.getElementById('stg-maint-unit');
  if (sel) {
    sel.value = unitId || 'principal';
    onMaintenanceUnitChange();
  }
  runStorageScrub();
}

async function checkBtrfsScrubStatus(isPolling = false) {
  try {
    const unitId = getMaintenanceUnitId();
    const qs = unitId && unitId !== 'principal' ? `?unit=${encodeURIComponent(unitId)}` : '';
    const res = await apiFetch('/api/storage/scrub' + qs);
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
  if (_scrubPollTimer) {
    clearTimeout(_scrubPollTimer);
    _scrubPollTimer = null;
  }

  let inFlight = false;
  const step = async () => {
    if (document.hidden || AppState.activeView !== 'storage') {
      _scrubPollTimer = setTimeout(step, 5000);
      return;
    }
    if (inFlight) {
      _scrubPollTimer = setTimeout(step, 3000);
      return;
    }
    inFlight = true;
    let isFinished = false;
    try {
      isFinished = await checkBtrfsScrubStatus(true);
    } finally {
      inFlight = false;
    }
    if (isFinished) {
      _scrubPollTimer = null;
      showToast('Auditoría BTRFS scrub finalizada exitosamente.', 'success');
    } else {
      _scrubPollTimer = setTimeout(step, 3000);
    }
  };

  checkBtrfsScrubStatus(true);
  _scrubPollTimer = setTimeout(step, 3000);
}

async function runStorageScrub() {
  try {
    const unitId = getMaintenanceUnitId();
    let targetOverview = getSelectedMaintenanceOverview();
    if (!targetOverview) {
      const stg = await apiFetch('/api/storage');
      targetOverview = stg.data?.overview || stg.overview || {};
    }
    const isBtrfs = Boolean(targetOverview.is_btrfs || String(targetOverview.filesystem || '').toLowerCase() === 'btrfs');
    const fsName = targetOverview.filesystem || 'ext4';

    if (!isBtrfs) {
      showToast(`Operación cancelada: la unidad "${unitId}" está en ${fsName.toUpperCase()}. BTRFS Scrub requiere Btrfs.`, 'warning');
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
    const unitId = getMaintenanceUnitId();
    const qs = unitId && unitId !== 'principal' ? `?unit=${encodeURIComponent(unitId)}` : '';
    const res = await apiFetch('/api/storage/scrub' + qs, { method: 'POST' });
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
    const unitId = getMaintenanceUnitId();
    let targetOverview = getSelectedMaintenanceOverview();
    if (!targetOverview) {
      const stg = await apiFetch('/api/storage');
      targetOverview = stg.data?.overview || stg.overview || {};
    }
    const supportsTrim = Boolean(targetOverview.supports_trim);
    const devType = targetOverview.device_type || 'HDD (Mecánico)';

    if (!supportsTrim) {
      showToast(`TRIM no ejecutable en "${unitId}": reside en ${devType}. Solo aplica a SSD / NVMe.`, 'warning');
      return;
    }

    const modalPath = document.getElementById('trim-modal-path');
    if (modalPath) modalPath.textContent = targetOverview.path || '/srv/nas';
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
    const unitId = getMaintenanceUnitId();
    const qs = unitId && unitId !== 'principal' ? `?unit=${encodeURIComponent(unitId)}` : '';
    const res = await apiFetch('/api/storage/trim' + qs, { method: 'POST' });
    const freed = res.data?.bytes_trimmed || res.data?.output || res.message || 'bloques descartados';
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
