
function openDeleteModal(relPath, name) {
  const pathInput = document.getElementById('delete-file-path');
  const label = document.getElementById('delete-file-name-label');
  const radioTrash = document.getElementById('del-mode-trash');
  const switchPrivBtn = document.getElementById('btn-switch-priv-delete');
  if (pathInput) pathInput.value = relPath;
  if (label) label.textContent = `"${name}"`;
  if (radioTrash) radioTrash.checked = true;
  if (switchPrivBtn) switchPrivBtn.style.display = window.NAS_IS_ADMIN !== false ? '' : 'none';
  openModal('modal-delete-file');
}

function switchToPrivilegedDelete() {
  const path = document.getElementById('delete-file-path')?.value || '';
  const nameLabel = document.getElementById('delete-file-name-label')?.textContent || '';
  const name = nameLabel.replace(/^"|"$/g, '') || path;
  closeModal('modal-delete-file');
  openPrivilegedDeleteModal(path, name);
}

function openPrivilegedDeleteModal(relPath, name) {
  if (!relPath) return;
  if (isProtectedFilePath(relPath) || isProtectedFilePath(AppState.files.currentPath)) {
    showToast('Los respaldos históricos y rutas del sistema están protegidos contra eliminación.', 'warning');
    return;
  }
  const pathInput = document.getElementById('priv-delete-path');
  const nameInput = document.getElementById('priv-delete-name');
  const label = document.getElementById('priv-delete-path-label');
  const userInput = document.getElementById('priv-delete-user');
  const pwInput = document.getElementById('priv-delete-password');
  const confirmInput = document.getElementById('priv-delete-confirm');
  if (pathInput) pathInput.value = relPath;
  if (nameInput) nameInput.value = name || relPath;
  if (label) label.textContent = `${AppState.files.root || 'nas'}:/${relPath}`;
  if (userInput) userInput.value = '';
  if (pwInput) pwInput.value = '';
  if (confirmInput) confirmInput.value = '';
  openModal('modal-privileged-delete');
}

async function confirmPrivilegedDeleteFile() {
  const path = document.getElementById('priv-delete-path')?.value || '';
  const authUser = (document.getElementById('priv-delete-user')?.value || '').trim();
  const pwInput = document.getElementById('priv-delete-password');
  const authPassword = pwInput?.value || '';
  const confirm = (document.getElementById('priv-delete-confirm')?.value || '').trim();
  if (!path) return;
  if (isProtectedFilePath(path) || isProtectedFilePath(AppState.files.currentPath)) {
    showToast('Ruta protegida: los respaldos históricos no pueden eliminarse desde Archivos.', 'warning');
    closeModal('modal-privileged-delete');
    return;
  }
  if (!authUser) {
    showToast('Indica el usuario del sistema (root o con permiso sudo).', 'warning');
    return;
  }
  if (!authPassword) {
    showToast('Indica la contraseña del usuario del sistema.', 'warning');
    return;
  }
  if (confirm !== 'SI-ELIMINAR') {
    showToast('Escribe exactamente SI-ELIMINAR para confirmar.', 'warning');
    return;
  }

  const btn = document.getElementById('btn-confirm-priv-delete');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }
  try {
    await apiFetch('/api/files/delete-privileged', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        path,
        auth_user: authUser,
        auth_password: authPassword,
        confirm,
      }),
    });
    closeModal('modal-privileged-delete');
    showToast('Elemento eliminado con privilegios del sistema.', 'success');
    loadFiles();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (pwInput) pwInput.value = '';
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

async function confirmDeleteFile() {
  const path = document.getElementById('delete-file-path')?.value;
  if (!path) return;

  if (isProtectedFilePath(path) || isProtectedFilePath(AppState.files.currentPath)) {
    showToast('Los respaldos históricos son inmutables (chattr +i) y están protegidos contra eliminación. Gestione las copias de seguridad desde el módulo Respaldos.', 'warning');
    closeModal('modal-delete-file');
    return;
  }

  const btn = document.getElementById('btn-confirm-delete');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  const isPermanent = document.getElementById('del-mode-permanent')?.checked === true;

  try {
    await apiFetch('/api/files/delete', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        path: path,
        permanent: isPermanent,
      }),
    });

    closeModal('modal-delete-file');
    const msg = isPermanent
      ? 'Elemento eliminado definitivamente del disco.'
      : 'Elemento movido a la papelera de reciclaje.';
    showToast(msg, isPermanent ? 'info' : 'success');
    loadFiles();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

// ==============================================================================
// Papelera de Reciclaje (Trash)
// ==============================================================================
function toggleTrashView() {
  AppState.files.inTrash = !AppState.files.inTrash;
  const normalView = document.getElementById('files-normal-view');
  const trashView = document.getElementById('files-trash-view');
  const btnNewFolder = document.getElementById('btn-new-folder');
  const btnUpload = document.getElementById('btn-upload-files');

  if (AppState.files.inTrash) {
    if (normalView) normalView.style.display = 'none';
    if (trashView) trashView.style.display = 'block';
    if (btnNewFolder) btnNewFolder.style.display = 'none';
    if (btnUpload) btnUpload.style.display = 'none';
    loadTrash();
  } else {
    exitTrashView();
  }
}

function exitTrashView() {
  AppState.files.inTrash = false;
  const normalView = document.getElementById('files-normal-view');
  const trashView = document.getElementById('files-trash-view');
  const btnNewFolder = document.getElementById('btn-new-folder');
  const btnUpload = document.getElementById('btn-upload-files');

  if (trashView) trashView.style.display = 'none';
  if (normalView) normalView.style.display = 'block';
  if (btnNewFolder) btnNewFolder.style.display = '';
  if (btnUpload) btnUpload.style.display = '';
  loadFiles();
}

async function loadTrash() {
  const tbody = document.getElementById('trash-table-body');
  if (tbody) {
    tbody.innerHTML = '<tr><td colspan="7" style="text-align:center; padding:18px;">Consultando papelera de reciclaje...</td></tr>';
  }

  try {
    const root = encodeURIComponent(AppState.files.root || 'nas');
    const res = await apiFetch(`/api/files/trash?root=${root}`);
    const items = res.data?.items ?? res.items ?? [];
    const count = res.data?.count ?? res.count ?? items.length;
    const totalSizeFormatted = res.data?.total_size_formatted ?? res.total_size_formatted ?? '0 B';

    AppState.files.trashItems = items;
    updateTrashBadgeCount(count);

    const kpiBadge = document.getElementById('trash-kpi-badge');
    if (kpiBadge) {
      kpiBadge.textContent = `${count} elementos (${totalSizeFormatted})`;
    }

    renderTrashRows(items);
  } catch (err) {
    if (tbody) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger); padding:18px;">Error al cargar papelera: ${escapeHtml(err.message)}</td></tr>`;
    }
  }
}

function renderTrashRows(items) {
  const tbody = document.getElementById('trash-table-body');
  if (!tbody) return;

  const q = (AppState.files.trashFilterQuery || '').toLowerCase().trim();
  let filtered = items;
  if (q) {
    filtered = items.filter(item => {
      const name = String(item.filename || item.name || '').toLowerCase();
      const orig = String(item.original_path || '').toLowerCase();
      const by = String(item.deleted_by || '').toLowerCase();
      return name.includes(q) || orig.includes(q) || by.includes(q);
    });
  }

  if (filtered.length === 0) {
    tbody.innerHTML = `
      <tr>
        <td colspan="7" style="text-align:center; padding:35px; color:var(--text-muted);">
          <svg class="icon" style="width:36px; height:36px; color:var(--accent-warning); opacity:0.7; margin-bottom:8px;"><use href="#icon-trash"></use></svg>
          <div>${q ? 'No se encontraron elementos que coincidan con la búsqueda.' : 'La papelera de reciclaje está vacía.'}</div>
        </td>
      </tr>
    `;
    return;
  }

  tbody.textContent = '';
  const range = document.createRange();
  range.selectNodeContents(tbody);
  const html = filtered.map(item => {
    const isDir = item.is_dir;
    const meta = getFileTypeMeta({ name: item.filename, is_dir: isDir });
    const fileSize = isDir ? '—' : (item.size_formatted || formatBytesLocal(item.size_bytes ?? item.size ?? 0));

    return `
      <tr class="trash-table-row" data-id="${escapeHtml(item.id)}" data-filename="${escapeHtml(item.filename)}" onclick="selectTrashRow(this)">
        <td style="width:36px; text-align:center;"><svg class="icon ${isDir ? 'icon-folder-colored' : ''}" style="${isDir ? '' : 'color:' + meta.color + ';'} width:18px; height:18px;"><use href="${meta.icon}"></use></svg></td>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <strong>${escapeHtml(item.filename)}</strong>
            <span class="file-type-badge ${meta.badgeClass || ''}" title="${escapeHtml(meta.typeLabel)}">${escapeHtml(meta.shortLabel)}</span>
          </div>
        </td>
        <td style="font-family:var(--font-mono); font-size:12px; color:var(--text-muted);">${escapeHtml(item.original_path)}</td>
        <td style="font-weight:500;">${isDir ? '<span style="color:var(--text-muted);">&mdash;</span>' : escapeHtml(fileSize)}</td>
        <td>${escapeHtml(item.deleted_by || 'sistemas')}</td>
        <td style="font-size:12px; color:var(--text-muted);">${escapeHtml(item.deleted_at || 'N/A')}</td>
        <td style="text-align:right;">
          <div style="display:flex; justify-content:flex-end; gap:6px;">
            <button type="button" class="btn btn-secondary btn-sm" onclick="handleTrashAction(this, 'restore')" title="Restaurar a su carpeta original">
              <svg class="icon" style="color:var(--accent-success);"><use href="#icon-restore"></use></svg> Restaurar
            </button>
            <button type="button" class="btn btn-danger btn-sm" onclick="handleTrashAction(this, 'delete')" title="Eliminar definitivamente">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');

  const fragment = range.createContextualFragment(html);
  tbody.appendChild(fragment);
}

function filterTrashItems(query = null) {
  if (query === null) {
    query = document.getElementById('trash-filter-input')?.value || '';
  }
  AppState.files.trashFilterQuery = String(query).toLowerCase().trim();
  renderTrashRows(AppState.files.trashItems || []);
}

function openRestoreAllTrashModal() {
  const count = (AppState.files.trashItems || []).length;
  if (count === 0) {
    showToast('La papelera de reciclaje ya se encuentra vacía.', 'info');
    return;
  }
  const countEl = document.getElementById('restore-all-trash-count');
  if (countEl) {
    countEl.textContent = `${count} elemento(s)`;
  }
  openModal('modal-restore-all-trash');
}

async function confirmRestoreAllTrash() {
  const btn = document.getElementById('btn-confirm-restore-all');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/trash/restore-all', {
      method: 'POST',
      body: JSON.stringify({ root: AppState.files.root || 'nas' }),
    });

    closeModal('modal-restore-all-trash');
    const restored = res.data?.restored ?? res.restored ?? 0;
    showToast(`Se restauraron ${restored} elemento(s) a sus rutas originales.`, 'success');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function selectTrashRow(tr) {
  document.querySelectorAll('#trash-table-body tr.selected').forEach(r => r.classList.remove('selected'));
  if (tr) tr.classList.add('selected');
}

async function handleTrashAction(btn, action) {
  const tr = btn.closest('tr');
  if (!tr) return;
  const id = String(tr.getAttribute('data-id') || '').trim();
  if (!id) return;
  const name = tr.getAttribute('data-filename') || '';

  btn.disabled = true;
  btn.classList.add('is-loading');

  try {
    if (action === 'restore') {
      await restoreTrashItem(id, name);
    } else if (action === 'delete') {
      await deleteTrashItem(id, name);
    }
  } finally {
    btn.disabled = false;
    btn.classList.remove('is-loading');
  }
}

async function updateTrashBadge() {
  try {
    const root = encodeURIComponent(AppState.files.root || 'nas');
    const res = await apiFetch(`/api/files/trash?root=${root}`, { silentToast: true });
    const count = res.data?.count ?? res.count ?? (res.data?.items?.length ?? 0);
    updateTrashBadgeCount(count);
  } catch (e) {
    // Silencioso
  }
}

function updateTrashBadgeCount(count) {
  const badge = document.getElementById('badge-trash-count');
  if (badge) {
    badge.textContent = String(count);
    badge.style.display = count > 0 ? 'inline-flex' : 'none';
  }
}

async function restoreTrashItem(id, name) {
  try {
    await apiFetch('/api/files/trash/restore', {
      method: 'POST',
      body: JSON.stringify({ id, root: AppState.files.root || 'nas' }),
    });

    showToast(`Elemento "${name}" restaurado correctamente a su ubicación.`, 'success');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  }
}

async function deleteTrashItem(id, name) {
  const confirmed = await confirmModal({
    title: 'Eliminar elemento de la papelera',
    message: `¿Eliminar definitivamente "${name}"?`,
    warning: 'Esta acción no se puede deshacer.',
    confirmLabel: 'Eliminar definitivamente',
    danger: true,
  });
  if (!confirmed) return;

  try {
    await apiFetch('/api/files/trash/delete', {
      method: 'POST',
      body: JSON.stringify({ id, root: AppState.files.root || 'nas' }),
    });

    showToast(`Elemento "${name}" eliminado definitivamente.`, 'info');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  }
}

function openEmptyTrashModal() {
  openModal('modal-empty-trash');
}

async function confirmEmptyTrash() {
  const btn = document.getElementById('btn-confirm-empty-trash');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/files/trash/empty', {
      method: 'POST',
      body: JSON.stringify({ root: AppState.files.root || 'nas' }),
    });

    closeModal('modal-empty-trash');
    showToast('Papelera vaciada por completo.', 'info');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function loadScriptLazy(src) {
  return new Promise((resolve, reject) => {
    const existing = document.querySelector(`script[src="${src}"]`);
    if (existing) {
      if (existing.dataset.loaded === 'true') {
        return resolve();
      }
      existing.addEventListener('load', () => resolve());
      existing.addEventListener('error', () => reject(new Error(`Error al cargar librería local: ${src}`)));
      return;
    }
    const s = document.createElement('script');
    s.src = src;
    s.async = true;
    s.onload = () => {
      s.dataset.loaded = 'true';
      resolve();
    };
    s.onerror = () => reject(new Error(`Error al cargar librería local: ${src}`));
    document.head.appendChild(s);
  });
}

