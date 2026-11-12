
async function loadFiles(subpath = null) {
  if (subpath === null) {
    subpath = AppState.files.currentPath || '';
  }
  if (AppState.files.isLoading && AppState.files.currentPath === subpath) {
    return;
  }
  AppState.files.isLoading = true;
  AppState.files.currentPath = subpath;

  const gridContainer = document.getElementById('files-grid-container');
  const tbody = document.getElementById('files-table-body');

  if (gridContainer) {
    gridContainer.innerHTML = '<div style="grid-column:1/-1; text-align:center; padding:30px; color:var(--text-muted);"><svg class="icon spin" style="width:24px; height:24px; color:var(--accent-primary); margin-bottom:8px;"><use href="#icon-refresh"></use></svg><br>Cargando archivos y recursos...</div>';
  }
  if (tbody) {
    tbody.innerHTML = '<tr><td colspan="7" style="text-align:center; padding:18px;">Cargando archivos y directorios...</td></tr>';
  }

  try {
    const root = AppState.files.root || 'nas';
    const query = `/api/files/list?root=${encodeURIComponent(root)}&path=${encodeURIComponent(subpath)}`;
    const res = await apiFetch(query);

    const items = res.data?.items ?? res.items ?? [];
    const breadcrumbs = res.data?.breadcrumbs ?? res.breadcrumbs ?? [];
    const currentPath = res.data?.current_path ?? res.current_path ?? subpath ?? '';

    AppState.files.items = items;
    AppState.files.selectedItemPath = null;
    renderFileBreadcrumbs(breadcrumbs, currentPath);
    applyFilesFilterAndSort();
    updateTrashBadge();
  } catch (e) {
    if (gridContainer) {
      gridContainer.innerHTML = `<div style="grid-column:1/-1; text-align:center; color:var(--accent-danger); padding:24px;">Error al cargar archivos: ${escapeHtml(e.message)}</div>`;
    }
    if (tbody) {
      tbody.innerHTML = `<tr><td colspan="7" style="text-align:center; color:var(--accent-danger); padding:18px;">Error al cargar archivos: ${escapeHtml(e.message)}</td></tr>`;
    }
  } finally {
    AppState.files.isLoading = false;
  }
}

function renderCurrentFiles() {
  const isGrid = AppState.files.viewMode === 'grid';
  const gridContainer = document.getElementById('files-grid-container');
  const tableContainer = document.getElementById('files-table-container');

  if (gridContainer) gridContainer.style.display = isGrid ? 'grid' : 'none';
  if (tableContainer) tableContainer.style.display = isGrid ? 'none' : 'block';

  const itemsToRender = AppState.files.displayItems !== undefined ? AppState.files.displayItems : (AppState.files.items || []);

  if (isGrid) {
    renderFileGrid(itemsToRender);
  } else {
    renderFileTable(itemsToRender);
  }
  updateFilesStatusBar();
}

function renderFileGrid(items) {
  const container = document.getElementById('files-grid-container');
  if (!container) return;
  container.textContent = '';

  if (!items || items.length === 0) {
    const isFiltered = Boolean(AppState.files.filterQuery);
    container.innerHTML = `
      <div style="grid-column:1/-1; text-align:center; padding:45px 20px; color:var(--text-muted);">
        <svg class="icon" style="width:48px; height:48px; color:var(--text-muted); opacity:0.6; margin-bottom:12px;"><use href="${isFiltered ? '#icon-search' : '#icon-folder'}"></use></svg>
        <div style="font-size:15px; font-weight:600; color:var(--text-main); margin-bottom:4px;">${isFiltered ? 'No se encontraron archivos' : 'Esta carpeta está vacía'}</div>
        <div style="font-size:13px;">${isFiltered ? 'Ningún archivo o carpeta coincide con el filtro actual.' : 'Arrastra archivos aquí desde tu equipo o pulsa el botón <strong>Subir archivos</strong>.'}</div>
      </div>
    `;
    return;
  }

  const range = document.createRange();
  range.selectNodeContents(container);
  const html = items.map(item => {
    const isDir = Boolean(item.is_dir);
    const meta = getFileTypeMeta(item);
    const relPath = item.relative_path || item.path || '';
    const isSelected = AppState.files.selectedItemPath === relPath;
    const isProtected = isProtectedFilePath(relPath) || isProtectedFilePath(AppState.files.currentPath);
    const fileSize = getFileFormattedSize(item);
    const sizeStr = fileSize || '0 B';

    const previewBtn = !isDir
      ? `<button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'preview')" title="Previsualizar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-eye"></use></svg></button>`
      : '';

    const downloadBtn = isDir
      ? `<button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'download')" title="Descargar como ZIP"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-download"></use></svg></button>`
      : `<button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'download')" title="Descargar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-download"></use></svg></button>`;

    const writeActions = !isProtected
      ? `
          <button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'rename')" title="Renombrar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-edit"></use></svg></button>
          <button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'move')" title="Mover"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-folder"></use></svg></button>
          <button type="button" class="file-card-action-btn btn-danger-hover" onclick="handleCardAction(event, this, 'delete')" title="Eliminar"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-trash"></use></svg></button>
        `
      : '';

    return `
      <div class="file-card ${isDir ? 'is-folder' : ''} ${isSelected ? 'selected' : ''}"
           tabindex="0"
           data-path="${escapeHtml(relPath)}"
           data-name="${escapeHtml(item.name)}"
           data-is-dir="${isDir ? 'true' : 'false'}"
           onclick="handleCardClick(this)"
           ondblclick="handleCardDblClick(this)"
           onkeydown="handleCardKeyDown(event, this)"
           title="${escapeHtml(item.name)} • ${escapeHtml(meta.typeLabel)} (${escapeHtml(fileSize || '0 B')})">
        
        <div class="file-card-actions">
          ${previewBtn}
          ${downloadBtn}
          <button type="button" class="file-card-action-btn" onclick="handleCardAction(event, this, 'copy-unc')" title="Copiar ruta UNC"><svg class="icon" style="width:14px; height:14px;"><use href="#icon-copy"></use></svg></button>
          ${writeActions}
        </div>

        <div class="file-card-icon-wrap">
          <svg class="file-card-icon ${isDir ? 'icon-folder-colored' : ''}" style="${isDir ? '' : 'color:' + meta.color + ';'}"><use href="${meta.icon}"></use></svg>
        </div>

        <span class="file-card-name" title="${escapeHtml(item.name)}">${escapeHtml(item.name)}</span>
        <div class="file-card-info" style="display:flex; flex-direction:column; align-items:center; gap:3px; margin-top:2px; width:100%;">
          <span class="file-card-type-tag ${meta.badgeClass || ''}" title="${escapeHtml(meta.typeLabel)}">${escapeHtml(meta.shortLabel)}</span>
          <span class="file-card-meta">${escapeHtml(fileSize || '0 B')}</span>
        </div>
      </div>
    `;
  }).join('');

  const fragment = range.createContextualFragment(html);
  container.appendChild(fragment);
}

function handleCardClick(cardEl) {
  const relPath = cardEl.getAttribute('data-path') || '';
  selectFileCard(cardEl, relPath);
}

function handleCardDblClick(cardEl) {
  const relPath = cardEl.getAttribute('data-path') || '';
  const name = cardEl.getAttribute('data-name') || '';
  const isDir = cardEl.getAttribute('data-is-dir') === 'true';
  handleItemDblClick(relPath, name, isDir);
}

function handleCardKeyDown(e, cardEl) {
  if (e.key === 'Enter') {
    e.preventDefault();
    handleCardDblClick(cardEl);
  } else if (e.key === ' ') {
    e.preventDefault();
    handleCardClick(cardEl);
  }
}

function handleCardAction(e, btn, action) {
  e.stopPropagation();
  const card = btn.closest('.file-card');
  if (!card) return;
  const relPath = card.getAttribute('data-path') || '';
  const name = card.getAttribute('data-name') || '';
  const isDir = card.getAttribute('data-is-dir') === 'true';

  if (action === 'preview') {
    previewFile(relPath, name);
  } else if (action === 'download') {
    if (isDir) downloadFolderZip(relPath);
    else downloadFile(relPath);
  } else if (action === 'copy-unc') {
    copyItemUncPath(relPath);
  } else if (action === 'rename') {
    openRenameModal(relPath, name);
  } else if (action === 'move') {
    openMoveModal(relPath, name);
  } else if (action === 'delete') {
    openDeleteModal(relPath, name);
  }
}

function renderFileTable(items) {
  const tbody = document.getElementById('files-table-body');
  if (!tbody) return;
  tbody.textContent = '';

  if (!items || items.length === 0) {
    const isFiltered = Boolean(AppState.files.filterQuery);
    tbody.innerHTML = `
      <tr>
        <td colspan="7" style="text-align:center; padding:35px; color:var(--text-muted);">
          ${isFiltered ? 'Ningún archivo o carpeta coincide con el filtro actual.' : 'Esta carpeta está vacía. Arrastra archivos aquí desde tu equipo o utiliza el botón <strong>Subir archivos</strong>.'}
        </td>
      </tr>
    `;
    return;
  }

  const range = document.createRange();
  range.selectNodeContents(tbody);
  const html = items.map(item => {
    const isDir = Boolean(item.is_dir);
    const meta = getFileTypeMeta(item);
    const relPath = item.relative_path || item.path || '';
    const isSelected = AppState.files.selectedItemPath === relPath;
    const nameClass = isDir ? 'file-row-name is-folder' : 'file-row-name';
    const isProtected = isProtectedFilePath(relPath) || isProtectedFilePath(AppState.files.currentPath);
    const fileSize = getFileFormattedSize(item);
    const tableSizeStr = fileSize || '0 B';

    const previewAction = !isDir
      ? `<button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'preview')" title="Previsualizar archivo"><svg class="icon"><use href="#icon-eye"></use></svg></button>`
      : '';

    const downloadAction = isDir
      ? `<button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'download')" title="Descargar carpeta como ZIP"><svg class="icon"><use href="#icon-download"></use></svg></button>`
      : `<button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'download')" title="Descargar archivo"><svg class="icon"><use href="#icon-download"></use></svg></button>`;

    const ownerStr = escapeHtml(item.owner || 'sistemas');
    const groupStr = escapeHtml(item.group || 'grp_samba');
    const modStr = escapeHtml(item.modified_at || item.mtime || 'N/A');

    return `
      <tr class="file-table-row ${isSelected ? 'selected' : ''}"
          tabindex="0"
          data-path="${escapeHtml(relPath)}"
          data-name="${escapeHtml(item.name)}"
          data-is-dir="${isDir ? 'true' : 'false'}"
          onclick="handleTableRowSelect(this)"
          ondblclick="handleTableRowDblClick(this)"
          onkeydown="handleTableRowKeyDown(event, this)">
        <td style="width:36px; text-align:center;"><svg class="icon ${isDir ? 'icon-folder-colored' : ''}" style="${isDir ? '' : 'color:' + meta.color + ';'} width:18px; height:18px;"><use href="${meta.icon}"></use></svg></td>
        <td>
          <div class="${nameClass}" onclick="handleTableRowNameClick(event, this)" style="display:flex; align-items:center; gap:8px;">
            <span style="font-weight:500;">${escapeHtml(item.name)}</span>
            <span class="file-type-badge ${meta.badgeClass || ''}" title="${escapeHtml(meta.typeLabel)}">${escapeHtml(meta.shortLabel)}</span>
          </div>
        </td>
        <td style="font-weight:500; color:var(--text-main); white-space:nowrap;">${escapeHtml(tableSizeStr)}</td>
        <td><code>${escapeHtml(item.permissions || '0660')}</code></td>
        <td>${ownerStr}:${groupStr}</td>
        <td style="font-size:12px; color:var(--text-muted);">${modStr}</td>
        <td style="text-align:right;">
          <div style="display:flex; justify-content:flex-end; gap:6px;">
            ${previewAction}
            ${downloadAction}
            <button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'copy-unc')" title="Copiar ruta UNC"><svg class="icon"><use href="#icon-copy"></use></svg></button>
            ${!isProtected ? `
              <button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'rename')" title="Renombrar"><svg class="icon"><use href="#icon-edit"></use></svg></button>
              <button type="button" class="btn btn-secondary btn-sm" onclick="handleTableRowAction(this, 'move')" title="Mover"><svg class="icon"><use href="#icon-folder"></use></svg></button>
              <button type="button" class="btn btn-danger btn-sm" onclick="handleTableRowAction(this, 'delete')" title="Eliminar"><svg class="icon"><use href="#icon-trash"></use></svg></button>
            ` : ''}
          </div>
        </td>
      </tr>
    `;
  }).join('');

  const fragment = range.createContextualFragment(html);
  tbody.appendChild(fragment);
}

function handleTableRowSelect(tr) {
  const relPath = tr.getAttribute('data-path') || '';
  selectTableRow(tr, relPath);
}

function handleTableRowDblClick(tr) {
  const relPath = tr.getAttribute('data-path') || '';
  const name = tr.getAttribute('data-name') || '';
  const isDir = tr.getAttribute('data-is-dir') === 'true';
  handleItemDblClick(relPath, name, isDir);
}

function handleTableRowKeyDown(e, tr) {
  if (e.key === 'Enter') {
    e.preventDefault();
    handleTableRowDblClick(tr);
  } else if (e.key === ' ') {
    e.preventDefault();
    handleTableRowSelect(tr);
  }
}

function handleTableRowNameClick(e, nameEl) {
  e.stopPropagation();
  const tr = nameEl.closest('tr');
  if (!tr) return;
  const relPath = tr.getAttribute('data-path') || '';
  const name = tr.getAttribute('data-name') || '';
  const isDir = tr.getAttribute('data-is-dir') === 'true';
  selectTableRow(tr, relPath);
  if (isDir) {
    navigateToSubpath(relPath);
  } else {
    previewFile(relPath, name);
  }
}

function selectTableRow(tr, relPath) {
  document.querySelectorAll('#files-table-body tr.selected').forEach(r => r.classList.remove('selected'));
  if (tr) tr.classList.add('selected');
  AppState.files.selectedItemPath = relPath;
  updateFilesStatusBar();
}

function handleTableRowAction(btn, action) {
  if (window.event && typeof window.event.stopPropagation === 'function') {
    window.event.stopPropagation();
  }
  const tr = btn.closest('tr');
  if (!tr) return;
  const relPath = tr.getAttribute('data-path') || '';
  const name = tr.getAttribute('data-name') || '';
  const isDir = tr.getAttribute('data-is-dir') === 'true';

  if (action === 'preview') {
    previewFile(relPath, name);
  } else if (action === 'download') {
    if (isDir) downloadFolderZip(relPath);
    else downloadFile(relPath);
  } else if (action === 'copy-unc') {
    copyItemUncPath(relPath);
  } else if (action === 'rename') {
    openRenameModal(relPath, name);
  } else if (action === 'move') {
    openMoveModal(relPath, name);
  } else if (action === 'delete') {
    openDeleteModal(relPath, name);
  }
}

function selectFileCard(cardEl, relPath) {
  document.querySelectorAll('.file-card.selected').forEach(c => c.classList.remove('selected'));
  if (cardEl) cardEl.classList.add('selected');
  AppState.files.selectedItemPath = relPath;
  updateFilesStatusBar();
}

function handleItemDblClick(relPath, name, isDir) {
  if (isDir) {
    navigateToSubpath(relPath);
  } else {
    previewFile(relPath, name);
  }
}

function refreshCurrentFileView() {
  if (AppState.files.inTrash) {
    loadTrash();
  } else {
    loadFiles();
  }
}

function navigateFolderUp() {
  const current = (AppState.files.currentPath || '').replace(/^\/+|\/+$/g, '');
  if (!current) return;
  const parts = current.split('/');
  parts.pop();
  const parentPath = parts.join('/');
  loadFiles(parentPath);
}

function renderFileBreadcrumbs(breadcrumbs, currentPath) {
  const container = document.getElementById('files-breadcrumbs');
  const upBtn = document.getElementById('btn-folder-up');
  const cleanPath = (currentPath || '').replace(/^\/+|\/+$/g, '');

  if (upBtn) {
    if (!cleanPath) {
      upBtn.disabled = true;
      upBtn.classList.add('disabled');
      upBtn.title = 'Ya te encuentras en el nivel principal';
    } else {
      upBtn.disabled = false;
      upBtn.classList.remove('disabled');
      upBtn.title = 'Subir un nivel (carpeta anterior)';
    }
  }

  if (!container) return;

  let humanRoot = 'Inicio';
  if (AppState.files.root === 'backups') {
    humanRoot = 'Respaldos Históricos';
  } else if (AppState.files.root && AppState.files.root !== 'nas' && AppState.files.root !== 'principal') {
    humanRoot = `Unidad: ${AppState.files.root}`;
  }

  const chevronSep = '<span class="file-breadcrumb-separator"><svg class="icon"><use href="#icon-chevron-right"></use></svg></span>';
  const homeIcon = '<svg class="icon" style="width:13px; height:13px; margin-right:4px;"><use href="#icon-home"></use></svg>';

  const isProtected = isProtectedFilePath(currentPath);
  const btnUpload = document.getElementById('btn-upload-files');
  const btnNewFolder = document.getElementById('btn-new-folder');
  if (btnUpload) btnUpload.style.display = isProtected ? 'none' : '';
  if (btnNewFolder) btnNewFolder.style.display = isProtected ? 'none' : '';

  const lockBadge = isProtected
    ? ' <span class="badge badge-warn" style="margin-left:8px; font-size:11px;" title="Respaldo inmutable contra ransomware (chattr +i)"><svg class="icon" style="width:12px; height:12px; margin-right:3px; vertical-align:middle;"><use href="#icon-shield"></use></svg>Solo Lectura (Inmutable)</span>'
    : '';

  if (!breadcrumbs || breadcrumbs.length === 0 || !cleanPath) {
    container.innerHTML = `<span class="file-breadcrumb-current" title="${escapeHtml(humanRoot)}">${homeIcon} ${escapeHtml(humanRoot)}</span>${lockBadge}`;
    return;
  }

  const rendered = breadcrumbs.map((bc, idx) => {
    const isFirst = idx === 0;
    const isLast = idx === breadcrumbs.length - 1;
    const displayName = isFirst ? humanRoot : bc.name;
    const titleAttr = escapeHtml(displayName);
    const prefix = isFirst ? homeIcon : '';

    if (isLast) {
      return `<span class="file-breadcrumb-current" title="${titleAttr}">${prefix}${escapeHtml(displayName)}</span>${lockBadge}`;
    }
    return `<span class="file-breadcrumb-item" data-path="${escapeHtml(bc.path)}" onclick="navigateToSubpath(this.getAttribute('data-path'))" title="${titleAttr}">${prefix}${escapeHtml(displayName)}</span>${chevronSep}`;
  }).join('');

  container.innerHTML = rendered;
  try {
    container.scrollLeft = container.scrollWidth;
  } catch (e) {}
}

function changeFilesRoot(rootKey) {
  AppState.files.root = rootKey;
  AppState.files.currentPath = '';
  loadFiles('');
}

function navigateToSubpath(path) {
  loadFiles(path);
}
