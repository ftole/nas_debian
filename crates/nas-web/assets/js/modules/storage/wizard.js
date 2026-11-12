
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
