
let _storageUnitsData = [];
let _pendingPoolAction = null;

function isLvmDevice(devicePath) {
  const raw = String(devicePath || '').split('[')[0].trim();
  if (!raw) return false;
  if (raw.startsWith('/dev/mapper/') || raw.startsWith('/dev/dm-')) return true;
  const rel = raw.replace(/^\/dev\//, '');
  return rel.includes('/');
}

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
    const fsRaw = String(u.filesystem || 'ext4').toLowerCase();
    const fs = fsRaw.toUpperCase();
    const isBtrfs = Boolean(u.is_btrfs || fsRaw === 'btrfs');
    const isRo = Boolean(u.is_readonly);
    const onOs = Boolean(u.is_main && u.data_root_on_os_disk);
    const members = Array.isArray(u.pool_devices) ? u.pool_devices : [];
    const roleBadge = u.is_main
      ? '<span class="badge badge-primary">Principal</span>'
      : '<span class="badge badge-info">Adicional</span>';
    const progressColor = isRo || pct >= 90 ? 'var(--accent-danger)' : (pct >= 80 ? 'var(--accent-warning)' : 'var(--accent-primary)');
    let statusBadge = pct >= 90
      ? '<span class="badge badge-err">Crítico</span>'
      : (pct >= 80 ? '<span class="badge badge-warn">Alerta</span>' : '<span class="badge badge-ok">Normal</span>');
    if (isRo) {
      statusBadge = '<span class="badge badge-err">Solo Lectura (ro)</span>';
    } else if (onOs) {
      statusBadge = '<span class="badge badge-warn">Sobre disco SO (/)</span>';
    }

    const unitIdEsc = escapeHtml(u.id || 'principal');
    const profileParts = [];
    if (isBtrfs && (u.data_profile || u.metadata_profile)) {
      profileParts.push(`Data: ${escapeHtml(u.data_profile || 'single')} · Meta: ${escapeHtml(u.metadata_profile || 'DUP')}`);
    }
    if (members.length > 0) {
      const canRemoveMember = isBtrfs && !isRo && members.length > 1;
      const memberTags = members.map(m => {
        const mEsc = escapeHtml(m);
        if (canRemoveMember) {
          return `<code>${mEsc}</code> <button type="button" class="btn btn-secondary btn-sm" style="padding:0 5px; font-size:10px; line-height:1.4;" onclick="confirmRemovePoolDevice('${unitIdEsc}', '${mEsc}')" title="Retirar ${mEsc} de este pool Btrfs">×</button>`;
        }
        return `<code>${mEsc}</code>`;
      }).join(', ');
      profileParts.push(`${members.length} disco(s): ${memberTags}`);
    }
    const metaHtml = profileParts.length
      ? `<div style="font-size:11px; color:var(--text-secondary); margin-bottom:8px;">${profileParts.join(' · ')}</div>`
      : '';

    const canExtend = !onOs && !isRo && (isBtrfs || isLvmDevice(u.device));
    const extendBtn = canExtend
      ? `<button type="button" class="btn btn-secondary btn-sm" onclick="openExtendPoolWizard('${unitIdEsc}')" title="Sumar otro disco a este pool">+ Ampliar</button>`
      : '';
    const scrubBtn = isBtrfs
      ? `<button type="button" class="btn btn-secondary btn-sm" onclick="runUnitScrub('${unitIdEsc}')" title="Auditar integridad Btrfs">Scrub</button>`
      : '';
    const umountBtn = !u.is_main
      ? `<button type="button" class="btn btn-danger btn-sm" onclick="confirmUnmountUnit('${unitIdEsc}')" title="Desmontar y retirar de fstab">Desmontar</button>`
      : '';

    return `
      <div class="panel-card" style="margin:0;">
        <div class="panel-card-head" style="display:flex; justify-content:space-between; align-items:center;">
          <h4 style="margin:0; font-size:14px;"><svg class="icon icon-sm" style="color:var(--accent-primary);"><use href="#icon-hard-drive"></use></svg> ${escapeHtml(u.name || u.id)}</h4>
          ${roleBadge}
        </div>
        <div class="panel-card-body">
          <div style="font-size:12px; color:var(--text-muted); margin-bottom:4px;"><code>${escapeHtml(u.path)}</code> · <strong>${escapeHtml(fs)}</strong></div>
          ${metaHtml}
          <div style="display:grid; grid-template-columns:repeat(3,1fr); gap:8px; margin-bottom:10px;">
            <div><span class="text-muted" style="font-size:10px; text-transform:uppercase;">Total</span><div style="font-weight:700;">${escapeHtml(String(total))} GB</div></div>
            <div><span class="text-muted" style="font-size:10px; text-transform:uppercase;">En uso</span><div style="font-weight:700;">${escapeHtml(String(used))} GB</div></div>
            <div><span class="text-muted" style="font-size:10px; text-transform:uppercase;">Libre</span><div style="font-weight:700; color:var(--accent-success-text);">${escapeHtml(String(free))} GB</div></div>
          </div>
          <div style="margin-bottom:6px; display:flex; justify-content:space-between; font-size:12px;">
            <span>Ocupación</span><span style="font-weight:600;">${pct}%</span>
          </div>
          <div style="height:8px; background:var(--bg-body); border:1px solid var(--border-color); border-radius:4px; overflow:hidden; margin-bottom:10px;">
            <div style="height:100%; width:${Math.min(100, Math.max(0, pct))}%; background:${progressColor};"></div>
          </div>
          <div style="display:flex; justify-content:space-between; align-items:center; flex-wrap:wrap; gap:6px;">
            ${statusBadge}
            <div style="display:flex; gap:6px; flex-wrap:wrap;">
              ${extendBtn}
              ${scrubBtn}
              ${umountBtn}
            </div>
          </div>
        </div>
      </div>`;
  }).join('');
}

function confirmRemovePoolDevice(targetPool, device) {
  if (!targetPool || !device) return;
  _pendingPoolAction = { type: 'remove-device', targetPool, device };
  const titleEl = document.getElementById('stg-pool-confirm-title');
  const alertEl = document.getElementById('stg-pool-confirm-alert-text');
  const descEl = document.getElementById('stg-pool-confirm-desc');
  const inputEl = document.getElementById('stg-pool-confirm-input');
  const btnEl = document.getElementById('btn-confirm-pool-action');
  if (titleEl) titleEl.textContent = `Retirar miembro ${device}`;
  if (alertEl) alertEl.textContent = `Se retirará ${device} del pool Btrfs "${targetPool}" mediante 'btrfs device remove'.`;
  if (descEl) descEl.textContent = 'Los bloques de datos asignados a este miembro se migrarán al resto del pool siempre que exista espacio libre suficiente.';
  if (inputEl) inputEl.value = '';
  if (btnEl) btnEl.textContent = 'Retirar miembro';
  openModal('modal-storage-pool-confirm');
}

function confirmUnmountUnit(unitId) {
  if (!unitId || unitId === 'principal') return;
  _pendingPoolAction = { type: 'umount-unit', unitId };
  const titleEl = document.getElementById('stg-pool-confirm-title');
  const alertEl = document.getElementById('stg-pool-confirm-alert-text');
  const descEl = document.getElementById('stg-pool-confirm-desc');
  const inputEl = document.getElementById('stg-pool-confirm-input');
  const btnEl = document.getElementById('btn-confirm-pool-action');
  if (titleEl) titleEl.textContent = `Desmontar unidad ${unitId}`;
  if (alertEl) alertEl.textContent = `Se desmontará /srv/nas/unidades/${unitId} y se retirará su entrada de /etc/fstab.`;
  if (descEl) descEl.textContent = 'Esta operación no borra los datos del disco, pero interrumpirá cualquier recurso o respaldo que apunte a esta ruta.';
  if (inputEl) inputEl.value = '';
  if (btnEl) btnEl.textContent = 'Desmontar unidad';
  openModal('modal-storage-pool-confirm');
}

async function executePoolActionConfirm() {
  if (!_pendingPoolAction) return;
  const inputEl = document.getElementById('stg-pool-confirm-input');
  const confirmVal = (inputEl?.value || '').trim();
  if (confirmVal !== 'SI-FORMATEAR' && confirmVal !== 'SI-DESTRUIR-DATOS') {
    showToast('Debes escribir exactamente SI-FORMATEAR (o SI-DESTRUIR-DATOS) para confirmar.', 'warning');
    return;
  }
  const action = _pendingPoolAction;
  const btnEl = document.getElementById('btn-confirm-pool-action');
  if (btnEl) { btnEl.disabled = true; btnEl.classList.add('is-loading'); }
  try {
    if (action.type === 'remove-device') {
      const res = await apiFetch('/api/storage/remove-device', {
        method: 'POST',
        body: JSON.stringify({ target: action.targetPool, device: action.device, confirm: confirmVal }),
      });
      showToast(res.message || `Dispositivo ${action.device} retirado del pool.`, 'success');
    } else if (action.type === 'umount-unit') {
      const res = await apiFetch('/api/storage/umount-unit', {
        method: 'POST',
        body: JSON.stringify({ unit_id: action.unitId, confirm: confirmVal }),
      });
      showToast(res.message || `Unidad ${action.unitId} desmontada.`, 'success');
    }
    _pendingPoolAction = null;
    if (inputEl) inputEl.value = '';
    closeModal('modal-storage-pool-confirm');
    loadStorage();
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btnEl) { btnEl.disabled = false; btnEl.classList.remove('is-loading'); }
  }
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

    // Poblar selector de mantenimiento (Scrub/TRIM) y wizard de ampliación
    const maintSel = document.getElementById('stg-maint-unit');
    if (maintSel) {
      const curr = maintSel.value || 'principal';
      maintSel.innerHTML = units.map(u => {
        return `<option value="${escapeHtml(u.id)}" ${u.id === curr ? 'selected' : ''}>${escapeHtml(u.name)} (${escapeHtml(String(u.filesystem || '').toUpperCase())})</option>`;
      }).join('') || '<option value="principal">Principal (/srv/nas)</option>';
      if (maintSel.value && maintSel.value !== 'principal' && typeof onMaintenanceUnitChange === 'function') {
        onMaintenanceUnitChange();
      }
    }
    const extendSel = document.getElementById('cp-extend-target');
    if (extendSel) {
      const curr = extendSel.value || 'principal';
      extendSel.innerHTML = units.map(u => {
        return `<option value="${escapeHtml(u.id)}" ${u.id === curr ? 'selected' : ''}>${escapeHtml(u.name)} · ${escapeHtml(String(u.filesystem || '').toUpperCase())}</option>`;
      }).join('') || '<option value="principal">Principal (/srv/nas)</option>';
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

