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
