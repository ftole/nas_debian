
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

    if (info.smart_available) {
      const smartColor = info.smart_healthy ? 'var(--accent-success-text)' : 'var(--accent-danger)';
      html += `<div style="color:${smartColor}; margin-top:4px;">S.M.A.R.T.: ${escapeHtml(info.smart_detail || (info.smart_healthy ? 'PASSED' : 'FAILED'))}</div>`;
    } else if (info.smart_detail) {
      html += `<div style="color:var(--text-muted); margin-top:2px;">S.M.A.R.T.: ${escapeHtml(info.smart_detail)}</div>`;
    }

    if (info.has_data) {
      html += `<div style="color:var(--accent-warning); margin-top:4px; font-weight:600;">⚠️ ATENCIÓN: El dispositivo contiene particiones o firmas preexistentes.</div>`;
      if (info.signatures && info.signatures.length > 0) {
        html += `<div style="margin-top:2px;">Firmas detectadas: <code>${info.signatures.map(s => escapeHtml(typeof s === 'string' ? s : (s?.type || s?.label || s?.uuid || ''))).filter(Boolean).join(', ')}</code></div>`;
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

    const wcBox = document.getElementById('stg-write-cache-box');
    const wcStatus = document.getElementById('stg-write-cache-status');
    const wcOff = document.getElementById('btn-wc-off');
    const wcOn = document.getElementById('btn-wc-on');
    if (wcBox && wcStatus) {
      if (info.write_cache_detail) {
        wcBox.style.display = 'block';
        wcStatus.textContent = info.write_cache_detail;
        const canControl = Boolean(info.write_cache_supported);
        if (wcOff) wcOff.style.display = canControl && info.write_cache_enabled !== false ? '' : 'none';
        if (wcOn) wcOn.style.display = canControl && info.write_cache_enabled === false ? '' : 'none';
      } else {
        wcBox.style.display = 'none';
      }
    }
  } catch (err) {
    if (inspectBox) inspectBox.style.display = 'block';
    const wcBox = document.getElementById('stg-write-cache-box');
    if (wcBox) wcBox.style.display = 'none';
    if (inspectDetails) inspectDetails.innerHTML = `<span style="color:var(--accent-warning);">No se pudo inspeccionar el dispositivo (${escapeHtml(err.message || 'error')}). Se aplicará verificación estricta.</span>`;
  }
}

async function toggleDiskWriteCache(enabled) {
  const dev = document.getElementById('stg-device')?.value || '';
  if (!dev) return;
  const btn = document.getElementById(enabled ? 'btn-wc-on' : 'btn-wc-off');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }
  try {
    const res = await apiFetch('/api/storage/write-cache', {
      method: 'POST',
      body: JSON.stringify({ device: dev, enabled: Boolean(enabled), confirm: 'SI-APLICAR' }),
    });
    showToast(res.message || 'Política de caché actualizada.', 'success');
    await onStorageDeviceChange();
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
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
  const confirmEl = document.getElementById('stg-confirm');
  if (confirmEl) confirmEl.value = '';
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

function toggleCreatePoolAction() {
  const action = document.querySelector('input[name="cp-action"]:checked')?.value || 'extend';
  const wrap = document.getElementById('cp-extend-target-wrap');
  if (wrap) wrap.style.display = action === 'extend' ? '' : 'none';
}

function toggleCreatePoolTarget() {
  const scope = document.getElementById('cp-target-scope')?.value || 'unit';
  const wrap = document.getElementById('cp-unit-id-wrap');
  if (wrap) wrap.style.display = scope === 'unit' ? '' : 'none';
}

function updateCreatePoolModeHint() {
  const mode = document.getElementById('cp-pool-mode')?.value || 'btrfs_single';
  const hint = document.getElementById('cp-mode-hint');
  const fsSel = document.getElementById('cp-fstype');
  if (fsSel) {
    fsSel.value = (mode === 'ext4' || mode === 'lvm_ext4') ? 'ext4' : 'btrfs';
  }
  if (!hint) return;
  const count = (_createPoolSelected || []).length;
  const msgs = {
    btrfs_single: count >= 2
      ? 'Suma la capacidad de todos los discos (Data: single) con metadatos replicados en RAID1 entre discos.'
      : 'Crea un volumen Btrfs con compresión zstd:3; podrás sumar más discos en línea después.',
    btrfs_raid1: 'Espejo RAID1 completo (Data + Metadata en RAID1). Requiere al menos 2 discos; la capacidad útil será ~50% del par.',
    ext4: 'Sistema de archivos ext4 clásico sobre 1 solo disco físico.',
    lvm_btrfs: 'Agrupa los discos en un Volume Group LVM con volumen lógico Btrfs.',
    lvm_ext4: 'Agrupa los discos en un Volume Group LVM con volumen lógico ext4.',
  };
  hint.textContent = msgs[mode] || '';
}

function createPoolNext(step) {
  const action = document.querySelector('input[name="cp-action"]:checked')?.value || 'extend';
  if (step === 3 && (_createPoolSelected || []).length === 0) {
    showToast('Selecciona al menos un disco disponible.', 'warning');
    return;
  }
  if (step === 4 && action === 'create') {
    const mode = document.getElementById('cp-pool-mode')?.value || 'btrfs_single';
    const count = (_createPoolSelected || []).length;
    if (mode === 'ext4' && count !== 1) {
      showToast('ext4 directo solo admite 1 disco. Elige Btrfs o LVM para combinar varios discos.', 'warning');
      return;
    }
    if (mode === 'btrfs_raid1' && count < 2) {
      showToast('Btrfs RAID1 requiere al menos 2 discos.', 'warning');
      return;
    }
    const scope = document.getElementById('cp-target-scope')?.value || 'unit';
    if (scope === 'unit') {
      const unitId = (document.getElementById('cp-unit-id')?.value || '').trim();
      if (!unitId || !/^[a-zA-Z0-9_-]{1,32}$/.test(unitId)) {
        showToast('Ingresa un identificador válido para el nuevo pool/unidad (1-32 caracteres alfanuméricos, guion o _).', 'warning');
        return;
      }
    }
  }

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
    const ext = document.getElementById('cp-options-extend');
    const cre = document.getElementById('cp-options-create');
    if (ext) ext.style.display = action === 'extend' ? '' : 'none';
    if (cre) cre.style.display = action === 'create' ? '' : 'none';
    if (action === 'extend') {
      const targetId = document.getElementById('cp-extend-target')?.value || 'principal';
      const targetUnit = (typeof _storageUnitsData !== 'undefined' && Array.isArray(_storageUnitsData))
        ? _storageUnitsData.find(u => u.id === targetId)
        : null;
      const raidWrap = document.getElementById('cp-raid1-meta-wrap');
      const isBtrfs = targetUnit ? Boolean(targetUnit.is_btrfs || String(targetUnit.filesystem || '').toLowerCase() === 'btrfs') : true;
      if (raidWrap) raidWrap.style.display = isBtrfs ? '' : 'none';
    } else {
      toggleCreatePoolTarget();
      updateCreatePoolModeHint();
    }
  }
  if (step === 4) {
    renderCreatePoolSummary();
  }
}

async function loadCreatePoolDisks() {
  const container = document.getElementById('cp-disks-list');
  if (!container) return;
  const prevSelected = new Set(_createPoolSelected || []);
  container.innerHTML = 'Cargando discos...';
  try {
    const res = await apiFetch('/api/storage/disks', { silentToast: true });
    const disks = res.data?.disks || res.disks || [];
    _createPoolDisks = disks.filter(d => !d.protected);
    container.innerHTML = _createPoolDisks.map(d => {
      const dev = d.device || ('/dev/' + d.name);
      const size = d.size || '';
      const disabled = d.in_use ? 'disabled' : '';
      const checked = !d.in_use && prevSelected.has(dev) ? 'checked' : '';
      const hasPreexisting = Boolean((d.children_count && d.children_count > 0) || d.fstype);
      let badge = '<span class="badge badge-ok">Disponible</span>';
      if (d.in_use && d.is_pool_member) {
        badge = `<span class="badge badge-info">En pool (${escapeHtml(d.mount || 'activo')})</span>`;
      } else if (d.in_use) {
        badge = `<span class="badge badge-warn">Montado (${escapeHtml(d.mount || 'en uso')})</span>`;
      } else if (hasPreexisting) {
        badge = `<span class="badge badge-warn">Firmas: ${escapeHtml(d.fstype || 'particiones')}</span>`;
      }
      const media = d.rotational === false ? 'SSD' : (d.rotational === true ? 'HDD' : '');
      return `<label style="display:flex; justify-content:space-between; align-items:center; gap:8px; padding:8px; border-bottom:1px solid var(--border-color); cursor:${d.in_use ? 'not-allowed' : 'pointer'}; opacity:${d.in_use ? '0.65' : '1'};">
        <span>
          <input type="checkbox" class="cp-disk-check" value="${escapeHtml(dev)}" data-size="${escapeHtml(size)}" data-has-data="${hasPreexisting ? '1' : '0'}" ${disabled} ${checked} onchange="updateCreatePoolCapacity()">
          <strong>${escapeHtml(dev)}</strong> — ${escapeHtml(size)} ${escapeHtml(d.model || '')} ${media ? `<small class="text-muted">(${media})</small>` : ''}
        </span>
        ${badge}
      </label>`;
    }).join('') || '<div style="color:var(--text-muted);">No hay discos no protegidos detectados.</div>';
    updateCreatePoolCapacity();
  } catch (e) {
    container.innerHTML = `<div style="color:var(--accent-danger);">${escapeHtml(e.message)}</div>`;
  }
}

function parseDiskSizeGb(sizeStr) {
  const raw = String(sizeStr || '').trim().toUpperCase();
  const num = parseFloat(raw);
  if (isNaN(num)) return 0;
  if (raw.endsWith('T')) return num * 1024;
  if (raw.endsWith('M')) return num / 1024;
  return num;
}

function updateCreatePoolCapacity() {
  const checks = document.querySelectorAll('.cp-disk-check:checked');
  let totalGb = 0;
  let hasDataCount = 0;
  checks.forEach(c => {
    totalGb += parseDiskSizeGb(c.getAttribute('data-size'));
    if (c.getAttribute('data-has-data') === '1') hasDataCount++;
  });
  const el = document.getElementById('cp-selected-capacity');
  if (el) {
    if (totalGb >= 1024) {
      el.textContent = `${(totalGb / 1024).toFixed(2)} TB (${totalGb.toFixed(1)} GB aprox.)`;
    } else {
      el.textContent = totalGb ? `${totalGb.toFixed(1)} GB (aprox.)` : '0 GB';
    }
  }
  const warn = document.getElementById('cp-disks-warning');
  if (warn) {
    if (hasDataCount > 0) {
      warn.style.display = 'block';
      warn.textContent = `⚠️ ${hasDataCount} disco(s) seleccionado(s) contienen particiones o firmas previas. Se requerirá escribir SI-DESTRUIR-DATOS en el último paso.`;
    } else {
      warn.style.display = 'none';
    }
  }
  _createPoolSelected = Array.from(checks).map(c => c.value);
}

function createPoolNeedsDestroyConfirm() {
  const checks = document.querySelectorAll('.cp-disk-check:checked');
  return Array.from(checks).some(c => c.getAttribute('data-has-data') === '1');
}

function renderCreatePoolSummary() {
  const summary = document.getElementById('cp-summary');
  if (!summary) return;
  const action = document.querySelector('input[name="cp-action"]:checked')?.value || 'extend';
  const selected = _createPoolSelected || [];
  const needsDestroy = createPoolNeedsDestroyConfirm();
  const requiredToken = needsDestroy ? 'SI-DESTRUIR-DATOS' : 'SI-FORMATEAR';

  const confirmLabel = document.getElementById('cp-confirm-label');
  const confirmInput = document.getElementById('cp-confirm');
  const confirmHint = document.getElementById('cp-confirm-hint');
  if (confirmLabel) confirmLabel.innerHTML = `Confirmación (escribe <code>${requiredToken}</code>):`;
  if (confirmInput) confirmInput.placeholder = requiredToken;
  if (confirmHint) {
    confirmHint.textContent = needsDestroy
      ? 'Uno o más discos tienen particiones o firmas previas. Escribe SI-DESTRUIR-DATOS para confirmar su formateo irreversible.'
      : 'Escribe SI-FORMATEAR para aplicar la operación.';
  }

  if (action === 'extend') {
    const targetId = document.getElementById('cp-extend-target')?.value || 'principal';
    const targetLabel = targetId === 'principal' ? '/srv/nas (Principal)' : `/srv/nas/unidades/${targetId}`;
    const raidMeta = document.getElementById('cp-raid1-meta')?.checked;
    summary.innerHTML = `<strong>Ampliar pool existente</strong><br>Destino: <code>${escapeHtml(targetLabel)}</code><br>Discos a sumar (${selected.length}): ${selected.map(d => `<code>${escapeHtml(d)}</code>`).join(', ')}${raidMeta ? '<br>Metadatos Btrfs: se asegurará perfil RAID1.' : ''}`;
  } else {
    const scope = document.getElementById('cp-target-scope')?.value || 'unit';
    const unitId = (document.getElementById('cp-unit-id')?.value || '').trim();
    const targetLabel = scope === 'principal' ? '/srv/nas (Principal)' : `/srv/nas/unidades/${unitId}`;
    const mode = document.getElementById('cp-pool-mode')?.value || 'btrfs_single';
    summary.innerHTML = `<strong>Crear pool / unidad nueva</strong><br>Destino: <code>${escapeHtml(targetLabel)}</code><br>Modo: <strong>${escapeHtml(mode)}</strong><br>Discos (${selected.length}): ${selected.map(d => `<code>${escapeHtml(d)}</code>`).join(', ')}`;
  }
}

async function submitCreatePool() {
  const confirmVal = (document.getElementById('cp-confirm')?.value || '').trim();
  const needsDestroy = createPoolNeedsDestroyConfirm();
  if (needsDestroy && confirmVal !== 'SI-DESTRUIR-DATOS') {
    showToast('Hay discos con datos o particiones previas. Escribe SI-DESTRUIR-DATOS para confirmar.', 'error');
    return;
  }
  if (!needsDestroy && confirmVal !== 'SI-FORMATEAR' && confirmVal !== 'SI-DESTRUIR-DATOS') {
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
      const targetId = document.getElementById('cp-extend-target')?.value || 'principal';
      const raidMeta = Boolean(document.getElementById('cp-raid1-meta')?.checked);
      const res = await apiFetch('/api/storage/extend-pool-multi', {
        method: 'POST',
        body: JSON.stringify({
          target: targetId,
          devices: selected,
          confirm: confirmVal,
          metadata_raid1: raidMeta,
        }),
      });
      const data = res.data || res;
      if (data.failed) {
        const firstErr = Array.isArray(data.results) ? data.results.find(r => !r.success)?.message : '';
        showToast(firstErr || 'Ampliación parcial o rechazada. Revisa resultados.', 'warning');
      } else {
        showToast(res.message || 'Pool ampliado correctamente.', 'success');
      }
    } else {
      const scope = document.getElementById('cp-target-scope')?.value || 'unit';
      const target = scope === 'principal'
        ? 'principal'
        : (document.getElementById('cp-unit-id')?.value || '').trim();
      const mode = document.getElementById('cp-pool-mode')?.value || 'btrfs_single';
      const unmount = Boolean(document.getElementById('cp-unmount')?.checked);
      const res = await apiFetch('/api/storage/create-pool', {
        method: 'POST',
        body: JSON.stringify({
          target,
          mode,
          devices: selected,
          confirm: confirmVal,
          unmount,
        }),
      });
      showToast(res.message || 'Pool creado y montado correctamente.', 'success');
    }
    const confirmInput = document.getElementById('cp-confirm');
    if (confirmInput) confirmInput.value = '';
    closeModal('modal-create-pool');
    loadStorage();
  } catch (e) {
    showToast(e.message || 'Error al aplicar cambios.', 'error');
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = 'Aplicar'; }
  }
}

function openCreatePoolWizard(actionMode = null, targetPool = null) {
  openModal('modal-create-pool');
  _createPoolSelected = [];
  const confirmEl = document.getElementById('cp-confirm');
  if (confirmEl) confirmEl.value = '';
  const unitIdEl = document.getElementById('cp-unit-id');
  if (unitIdEl) unitIdEl.value = '';
  const unmountEl = document.getElementById('cp-unmount');
  if (unmountEl) unmountEl.checked = false;
  const radioVal = actionMode === 'create' ? 'create' : 'extend';
  const radio = document.querySelector(`input[name="cp-action"][value="${radioVal}"]`);
  if (radio) radio.checked = true;
  if (targetPool) {
    const extSel = document.getElementById('cp-extend-target');
    if (extSel) extSel.value = targetPool;
  }
  toggleCreatePoolAction();
  createPoolNext(1);
  const checks = document.querySelectorAll('.cp-disk-check');
  checks.forEach(c => c.checked = false);
  updateCreatePoolCapacity();
}

function openExtendPoolWizard(targetPool = 'principal') {
  openCreatePoolWizard('extend', targetPool);
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
  document.getElementById('stg-fs-wrap').style.display = (op === 'subvolume' || op === 'extend' || op === 'wipe') ? 'none' : 'block';
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
  if (op === 'wipe') {
    if (confirmText !== 'SI-DESTRUIR-DATOS') {
      showToast('Para limpiar firmas y particiones (wipefs), debes escribir estrictamente SI-DESTRUIR-DATOS.', 'warning');
      return;
    }
  } else if (hasData) {
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

  if (op === 'wipe') {
    url = '/api/storage/wipe';
    payload = { device, confirm: confirmText };
  } else if (op === 'extend') {
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
      const supportsUnmount = op === 'format' || op === 'lvm' || op === 'wipe';
      const isProtectedBlock = /overmount|pool btrfs activo|pertenece al pool/i.test(msg);
      if (supportsUnmount && !isProtectedBlock && /montad|desmont/i.test(msg)) {
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
  } catch (e) {
    showToast(e.message || 'No se pudo completar la operación con desmontaje forzado.', 'error');
  } finally {
    _pendingStoragePayload = null;
    _pendingStorageUrl = null;
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
    if (confirmBtn) { confirmBtn.disabled = false; confirmBtn.classList.remove('is-loading'); }
  }
}
