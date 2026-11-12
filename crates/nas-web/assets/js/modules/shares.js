// 5. Módulo: Recursos Compartidos (Samba)
// ==============================================================================
let _sharesData = [];

async function loadShares() {
  const tbody = document.getElementById('shares-table-body');
  if (!tbody) return;

  try {
    const res = await apiFetch('/api/shares');
    _sharesData = res.data || [];

    if (_sharesData.length === 0) {
      tbody.innerHTML = '<tr><td colspan="6" style="text-align:center;">No hay recursos compartidos creados actualmente.</td></tr>';
      return;
    }

    tbody.innerHTML = _sharesData.map(s => `
      <tr>
        <td><strong>[${escapeHtml(s.name)}]</strong></td>
        <td><code>${escapeHtml(s.path)}</code></td>
        <td><span class="tag-pill">${escapeHtml(s.scheme_name)}</span></td>
        <td>${s.hidden ? '<span class="badge badge-gray">Oculto ($)</span>' : '<span class="badge badge-ok">Visible</span>'}</td>
        <td>${(s.valid_users || []).map(u => `<span class="tag-pill">${escapeHtml(u)}</span>`).join(' ') || (s.guest_ok ? '<em>Invitados</em>' : '<em>Sistemas</em>')}</td>
        <td style="text-align:right; white-space:nowrap;">
          <button class="btn btn-secondary btn-sm" title="Copiar ruta UNC" onclick="copyShareUnc('${escapeHtml(s.name)}')">
            <svg class="icon icon-sm"><use href="#icon-file-text"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Permisos de acceso" onclick="openSharePermissions('${escapeHtml(s.name)}')">
            <svg class="icon icon-sm"><use href="#icon-shield"></use></svg>
          </button>
          <button class="btn btn-secondary btn-sm" title="Editar" onclick="openEditShare('${escapeHtml(s.name)}')">
            <svg class="icon icon-sm"><use href="#icon-edit"></use></svg>
          </button>
          ${s.name === 'SISTEMAS' ? '' : `
            <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteShare('${escapeHtml(s.name)}')">
              <svg class="icon icon-sm"><use href="#icon-trash"></use></svg>
            </button>
          `}
        </td>
      </tr>
    `).join('');
  } catch (e) {
    tbody.innerHTML = `<tr><td colspan="6" style="text-align:center; color:var(--accent-danger);">Error al cargar recursos compartidos: ${escapeHtml(e.message)}</td></tr>`;
  }
}

async function openNewShare() {
  await populateShareGroupOptions();
  if (window.NAS_SERVER_ROLE === 'BACKUP') {
    const hiddenChk = document.getElementById('share-hidden');
    if (hiddenChk) {
      hiddenChk.checked = true;
      hiddenChk.disabled = true;
    }
    const schemeSelect = document.getElementById('share-scheme');
    if (schemeSelect) {
      const opt4 = schemeSelect.querySelector('option[value="4"]');
      if (opt4) opt4.style.display = 'none';
      if (schemeSelect.value === '4') schemeSelect.value = '1';
    }
  }
  toggleSchemeFields();
  openModal('modal-new-share');
}

function toggleSchemeFields() {
  const scheme = document.getElementById('share-scheme').value;
  const grpBox = document.getElementById('group-share-groups');
  const writeBox = document.getElementById('group-share-write-group');

  if (scheme === '4') {
    grpBox.style.display = 'none';
    writeBox.style.display = 'none';
  } else if (scheme === '2') {
    grpBox.style.display = 'block';
    writeBox.style.display = 'block';
  } else {
    grpBox.style.display = 'block';
    writeBox.style.display = 'none';
  }
}

async function populateShareGroupOptions() {
  const container = document.getElementById('share-groups-list');
  const writeSelect = document.getElementById('share-write-group');
  if (!container) return;

  try {
    const res = await apiFetch('/api/groups');
    AppState.groupsCache = res.data || [];
    let selectable = AppState.groupsCache;
    if (window.NAS_SERVER_ROLE === 'BACKUP') {
      selectable = selectable.filter(g => g.name === 'grp_samba' || g.name === 'grp_backups');
    }

    if (!selectable.length) {
      container.innerHTML = '<em style="color:var(--text-muted);">No hay grupos disponibles. Crea uno en “Usuarios y grupos”.</em>';
      if (writeSelect) writeSelect.innerHTML = '';
      return;
    }

    container.innerHTML = selectable.map(g => {
      const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
      return `
      <label style="display:flex; align-items:center; gap:8px;">
        <input type="checkbox" name="share_group" value="${escapeHtml(g.name)}" ${g.name === 'grp_samba' ? 'checked' : ''}>
        <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
      </label>`;
    }).join('');

    if (writeSelect) {
      writeSelect.innerHTML = selectable.map(g => `
        <option value="${escapeHtml(g.name)}">${escapeHtml(g.name)}</option>
      `).join('');
    }
  } catch (e) {
    console.error('Error cargando grupos para selector:', e);
  }
}

async function populateEditShareGroupOptions(selectedGroups, selectedWriteGroup) {
  const container = document.getElementById('edit-share-groups-list');
  const writeSelect = document.getElementById('edit-share-write-group');
  if (!container) return;

  try {
    const res = await apiFetch('/api/groups');
    let groups = res.data || [];
    if (window.NAS_SERVER_ROLE === 'BACKUP') {
      groups = groups.filter(g => g.name === 'grp_samba' || g.name === 'grp_backups');
    }
    const sel = (selectedGroups || []).map(g => String(g).replace('@', ''));
    container.innerHTML = groups.map(g => {
      const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
      return `
      <label style="display:flex; align-items:center; gap:8px;">
        <input type="checkbox" name="edit_share_group" value="${escapeHtml(g.name)}" ${sel.includes(g.name) ? 'checked' : ''}>
        <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
      </label>`;
    }).join('');

    if (writeSelect) {
      writeSelect.innerHTML = groups.map(g => `
        <option value="${escapeHtml(g.name)}" ${g.name === selectedWriteGroup ? 'selected' : ''}>${escapeHtml(g.name)}</option>
      `).join('');
    }
  } catch (e) {
    console.error('Error cargando grupos para edición:', e);
  }
}

async function openEditShare(name) {
  const s = _sharesData.find(x => x.name === name);
  if (!s) return;
  document.getElementById('edit-share-name').value = name;
  document.getElementById('edit-share-comment').value = s.comment || '';
  document.getElementById('edit-share-scheme').value = String(s.scheme || 1);
  const hiddenInput = document.getElementById('edit-share-hidden');
  if (hiddenInput) {
    if (window.NAS_SERVER_ROLE === 'BACKUP') {
      hiddenInput.checked = true;
      hiddenInput.disabled = true;
    } else {
      hiddenInput.checked = !!s.hidden;
      hiddenInput.disabled = false;
    }
  }
  const schemeSelect = document.getElementById('edit-share-scheme');
  if (schemeSelect && window.NAS_SERVER_ROLE === 'BACKUP') {
    const opt4 = schemeSelect.querySelector('option[value="4"]');
    if (opt4) opt4.style.display = 'none';
    if (schemeSelect.value === '4') schemeSelect.value = '1';
  }

  const groupTokens = (s.valid_users || []).filter(t => t.startsWith('@'));
  const writeToken = (s.write_list || [])[0] || '';
  await populateEditShareGroupOptions(groupTokens, String(writeToken).replace('@', ''));
  toggleEditSchemeFields();
  openModal('modal-edit-share');
}

function toggleEditSchemeFields() {
  const scheme = document.getElementById('edit-share-scheme').value;
  document.getElementById('edit-group-share-groups').style.display = (scheme === '4') ? 'none' : 'block';
  document.getElementById('edit-group-share-write-group').style.display = (scheme === '2') ? 'block' : 'none';
}

async function submitEditShare(event) {
  event.preventDefault();
  const name = document.getElementById('edit-share-name').value;
  const comment = document.getElementById('edit-share-comment').value;
  const scheme = parseInt(document.getElementById('edit-share-scheme').value, 10);
  let hidden = document.getElementById('edit-share-hidden').checked;
  const writeGroup = document.getElementById('edit-share-write-group').value;
  const groups = Array.from(document.querySelectorAll('input[name="edit_share_group"]:checked')).map(cb => cb.value);

  if (window.NAS_SERVER_ROLE === 'BACKUP') {
    hidden = true;
  }

  const btn = document.getElementById('btn-submit-edit-share') || event.target.querySelector('button[type="submit"]');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    await apiFetch('/api/shares/update', {
      method: 'POST',
      body: JSON.stringify({ name, comment, scheme, hidden, groups, write_group: writeGroup }),
    });
    showToast(`Recurso [${name}] actualizado.`, 'success');
    closeModal('modal-edit-share');
    loadShares();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function submitNewShare(event) {
  event.preventDefault();
  let name = document.getElementById('share-name').value;
  const comment = document.getElementById('share-comment').value;
  const scheme = parseInt(document.getElementById('share-scheme').value, 10);
  let hidden = document.getElementById('share-hidden').checked;
  const writeGroup = document.getElementById('share-write-group').value;

  if (window.NAS_SERVER_ROLE === 'BACKUP') {
    hidden = true;
    if (!name.endsWith('$')) {
      name += '$';
    }
  }

  const selectedGroups = Array.from(document.querySelectorAll('input[name="share_group"]:checked'))
    .map(cb => cb.value);
  const unit = document.getElementById('share-unit')?.value || 'principal';

  const btn = document.getElementById('btn-submit-new-share') || event.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/shares', {
      method: 'POST',
      body: JSON.stringify({
        name,
        comment,
        scheme,
        hidden,
        unit,
        groups: selectedGroups,
        write_group: writeGroup,
      }),
    });

    showToast(`Recurso compartido [${name}] creado exitosamente.`, 'success');
    closeModal('modal-new-share');
    document.getElementById('form-new-share').reset();
    loadShares();
  } catch (e) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

async function deleteShare(name) {
  if (!window.confirm(`¿Confirmas la eliminación del recurso compartido [${name}]? (Los archivos en disco se conservarán)`)) {
    return;
  }

  try {
    await apiFetch('/api/shares/delete', {
      method: 'POST',
      body: JSON.stringify({ name }),
    });

    showToast(`Recurso [${name}] eliminado.`, 'success');
    loadShares();
  } catch (e) {
    // Ya mostrado
  }
}

function openSharePermissions(shareName) {
  switchView('users');
  setTimeout(() => {
    const permContainer = document.getElementById('perm-matrix-container') || document.querySelector('.perm-matrix-card');
    if (permContainer) {
      permContainer.scrollIntoView({ behavior: 'smooth' });
    }
  }, 100);
}

function repairSambaPermissions(shareName = null) {
  return repairAcls(shareName);
}

// ==============================================================================
