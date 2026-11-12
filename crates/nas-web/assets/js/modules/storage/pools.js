
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

