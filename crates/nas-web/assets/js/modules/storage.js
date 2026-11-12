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

function storageWizardNext(step) {
  const steps = document.querySelectorAll('.storage-wizard-step');
  steps.forEach((el, idx) => {
    el.style.display = (idx + 1 === step) ? '' : 'none';
  });
  const inds = document.querySelectorAll('[id^="stg-step-ind-"]');
  inds.forEach((ind, idx) => {
    const i = idx + 1;
    ind.className = i === step ? 'badge badge-primary' : (i < step ? 'badge badge-ok' : 'badge badge-gray');
  });
}

async function openStorageManage(preselectedDevice = null) {
  openModal('modal-storage-manage');
  storageWizardNext(1);
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

let _createPoolSelected = [];
let _createPoolDisks = [];

function createPoolNext(step) {
  const steps = document.querySelectorAll('.cp-step');
  steps.forEach((el, idx) => { el.style.display = (idx + 1 === step) ? '' : 'none'; });
  const inds = document.querySelectorAll('[id^="cp-step-ind-"]');
  inds.forEach((ind, idx) => {
    const i = idx + 1;
    ind.className = i === step ? 'badge badge-primary' : (i < step ? 'badge badge-ok' : 'badge badge-gray');
  });
  if (step === 2) {
    loadCreatePoolDisks();
  }
  if (step === 3) {
    const action = document.querySelector('input[name="cp-action"]:checked')?.value || 'extend';
    const ext = document.getElementById('cp-options-extend');
    const cre = document.getElementById('cp-options-create');
    if (ext) ext.style.display = action === 'extend' ? '' : 'none';
    if (cre) cre.style.display = action === 'create' ? '' : 'none';
  }
  if (step === 4) {
    renderCreatePoolSummary();
  }
}

async function loadCreatePoolDisks() {
  const container = document.getElementById('cp-disks-list');
  if (!container) return;
  container.innerHTML = 'Cargando discos...';
  try {
    const res = await apiFetch('/api/storage/disks', { silentToast: true });
    const disks = res.data?.disks || res.disks || [];
    _createPoolDisks = disks.filter(d => !d.protected);
    container.innerHTML = _createPoolDisks.map(d => {
      const dev = d.device || ('/dev/' + d.name);
      const size = d.size || '';
      const disabled = d.in_use ? 'disabled' : '';
      const label = d.in_use ? ' (en uso)' : '';
      return `<label style="display:block; padding:6px; border-bottom:1px solid var(--border-color);">
        <input type="checkbox" class="cp-disk-check" value="${escapeHtml(dev)}" data-size="${escapeHtml(size)}" ${disabled} onchange="updateCreatePoolCapacity()">
        <strong>${escapeHtml(dev)}</strong> — ${escapeHtml(size)} ${escapeHtml(d.model || '')}${label}
      </label>`;
    }).join('') || '<div style="color:var(--text-muted);">No hay discos disponibles.</div>';
    updateCreatePoolCapacity();
  } catch (e) {
    container.innerHTML = `<div style="color:var(--accent-danger);">${escapeHtml(e.message)}</div>`;
  }
}

function updateCreatePoolCapacity() {
  const checks = document.querySelectorAll('.cp-disk-check:checked');
  let total = 0;
  checks.forEach(c => {
    const sizeStr = c.getAttribute('data-size') || '';
    const num = parseFloat(sizeStr);
    if (!isNaN(num)) total += num;
  });
  const el = document.getElementById('cp-selected-capacity');
  if (el) el.textContent = total ? total.toFixed(1) + ' GB (aprox.)' : '0 GB';
  _createPoolSelected = Array.from(checks).map(c => c.value);
}

function renderCreatePoolSummary() {
  const summary = document.getElementById('cp-summary');
  if (!summary) return;
  const action = document.querySelector('input[name="cp-action"]:checked')?.value || 'extend';
  const selected = _createPoolSelected || [];
  if (action === 'extend') {
    summary.innerHTML = `<strong>Ampliar espacio principal</strong><br>Se añadirán ${selected.length} disco(s) al pool /srv/nas: ${selected.map(escapeHtml).join(', ')}.`;
  } else {
    const id = document.getElementById('cp-unit-id')?.value || 'disco2';
    const fs = document.getElementById('cp-fstype')?.value || 'ext4';
    summary.innerHTML = `<strong>Crear espacio nuevo</strong><br>Se creará la unidad "${escapeHtml(id)}" (${escapeHtml(fs)}) con ${selected.length} disco(s): ${selected.map(escapeHtml).join(', ')}.`;
  }
}

async function submitCreatePool() {
  const confirmVal = document.getElementById('cp-confirm')?.value || '';
  if (confirmVal !== 'SI-FORMATEAR') {
    showToast('Escribe SI-FORMATEAR para confirmar.', 'error');
    return;
  }
  const action = document.querySelector('input[name="cp-action"]:checked')?.value || 'extend';
  const selected = _createPoolSelected || [];
  if (selected.length === 0) {
    showToast('Selecciona al menos un disco.', 'error');
    return;
  }
  const btn = document.getElementById('btn-cp-apply');
  if (btn) { btn.disabled = true; btn.textContent = 'Aplicando...'; }
  try {
    if (action === 'extend') {
      const res = await apiFetch('/api/storage/extend-pool-multi', {
        method: 'POST',
        body: JSON.stringify({ devices: selected, confirm: confirmVal }),
      });
      const data = res.data || res;
      if (data.failed) {
        showToast('Ampliación parcial. Revisa resultados.', 'warning');
      } else {
        showToast('Pool ampliado correctamente.', 'success');
      }
    } else {
      const id = document.getElementById('cp-unit-id')?.value || 'disco2';
      const fs = document.getElementById('cp-fstype')?.value || 'ext4';
      for (const dev of selected) {
        await apiFetch('/api/storage/add-unit', {
          method: 'POST',
          body: JSON.stringify({ device: dev, unit_id: id, fstype: fs, confirm: confirmVal }),
        });
      }
      showToast('Espacio(s) creado(s) correctamente.', 'success');
    }
    closeModal('modal-create-pool');
    loadStorage();
    loadStorageUnits();
  } catch (e) {
    showToast(e.message || 'Error al aplicar cambios.', 'error');
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = 'Aplicar'; }
  }
}

function openCreatePoolWizard() {
  openModal('modal-create-pool');
  createPoolNext(1);
  const checks = document.querySelectorAll('.cp-disk-check');
  checks.forEach(c => c.checked = false);
  updateCreatePoolCapacity();
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

function renderStoragePools(units) {
  const grid = document.getElementById('storage-pools-grid');
  if (!grid) return;
  if (!units || units.length === 0) {
    grid.innerHTML = '<div style="text-align:center; color:var(--text-muted); padding:12px;">No hay pools/volúmenes montados.</div>';
    return;
  }
  grid.innerHTML = units.map(u => {
    const used = Number(u.used_gb ?? u.used ?? 0);
    const total = Number(u.total_gb ?? u.total ?? 0);
    const free = Number(u.free_gb ?? u.free ?? 0);
    const pct = Math.round(Number(u.usage_percent ?? 0));
    const fs = (u.filesystem || 'ext4').toUpperCase();
    const role = u.is_main ? 'Principal' : 'Adicional';
    const roleBadge = u.is_main
      ? '<span class="badge badge-primary">Principal</span>'
      : '<span class="badge badge-info">Adicional</span>';
    const progressColor = pct >= 90 ? 'var(--accent-danger)' : (pct >= 80 ? 'var(--accent-warning)' : 'var(--accent-primary)');
    const statusBadge = pct >= 90
      ? '<span class="badge badge-err">Crítico</span>'
      : (pct >= 80 ? '<span class="badge badge-warn">Alerta</span>' : '<span class="badge badge-ok">Normal</span>');
    return `
      <div class="panel-card" style="margin:0;">
        <div class="panel-card-head" style="display:flex; justify-content:space-between; align-items:center;">
          <h4 style="margin:0; font-size:14px;"><svg class="icon icon-sm" style="color:var(--accent-primary);"><use href="#icon-hard-drive"></use></svg> ${escapeHtml(u.name || u.id)}</h4>
          ${roleBadge}
        </div>
        <div class="panel-card-body">
          <div style="font-size:12px; color:var(--text-muted); margin-bottom:8px;"><code>${escapeHtml(u.path)}</code> · ${escapeHtml(fs)}</div>
          <div style="display:grid; grid-template-columns:repeat(3,1fr); gap:8px; margin-bottom:10px;">
            <div><span class="text-muted" style="font-size:10px; text-transform:uppercase;">Total</span><div style="font-weight:700;">${escapeHtml(String(total))} GB</div></div>
            <div><span class="text-muted" style="font-size:10px; text-transform:uppercase;">En uso</span><div style="font-weight:700;">${escapeHtml(String(used))} GB</div></div>
            <div><span class="text-muted" style="font-size:10px; text-transform:uppercase;">Libre</span><div style="font-weight:700; color:var(--accent-success-text);">${escapeHtml(String(free))} GB</div></div>
          </div>
          <div style="margin-bottom:6px; display:flex; justify-content:space-between; font-size:12px;">
            <span>Ocupación</span><span style="font-weight:600;">${pct}%</span>
          </div>
          <div style="height:8px; background:var(--bg-body); border:1px solid var(--border-color); border-radius:4px; overflow:hidden; margin-bottom:8px;">
            <div style="height:100%; width:${Math.min(100, Math.max(0, pct))}%; background:${progressColor};"></div>
          </div>
          <div style="display:flex; justify-content:space-between; align-items:center;">
            ${statusBadge}
            <span class="text-muted" style="font-size:11px;">${escapeHtml(role)} · Pool = volumen (sin RAID)</span>
          </div>
        </div>
      </div>`;
  }).join('');
}

async function loadStorageUnits() {
  try {
    const res = await apiFetch('/api/storage/units', { silentToast: true });
    const units = res.data?.units || res.units || [];
    _storageUnitsData = units;

    // Renderizar cards de pools/volúmenes (estilo Synology)
    renderStoragePools(units);

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

