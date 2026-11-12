// ==============================================================================
// 15. Módulo: Explorador de Archivos (Cuadrícula Windows, Previsualizador & Papelera)
// ==============================================================================

function getContrastTextColor(hexColor) {
  if (!hexColor || typeof hexColor !== 'string') return '#ffffff';
  let hex = hexColor.trim();
  if (hex.startsWith('rgba') || hex.startsWith('rgb')) {
    const parts = hex.match(/\d+/g);
    if (parts && parts.length >= 3) {
      const r = Number(parts[0]) || 0;
      const g = Number(parts[1]) || 0;
      const b = Number(parts[2]) || 0;
      const yiq = ((r * 299) + (g * 587) + (b * 114)) / 1000;
      return yiq >= 150 ? '#0f172a' : '#ffffff';
    }
    return '#ffffff';
  }
  hex = hex.replace('#', '');
  if (hex.length === 3) {
    hex = hex.split('').map(c => c + c).join('');
  }
  const r = parseInt(hex.substring(0, 2), 16) || 0;
  const g = parseInt(hex.substring(2, 4), 16) || 0;
  const b = parseInt(hex.substring(4, 6), 16) || 0;
  const yiq = ((r * 299) + (g * 587) + (b * 114)) / 1000;
  return (yiq >= 150) ? '#0f172a' : '#ffffff';
}

function getFileFormattedSize(item) {
  if (!item) return '0 B';
  const bytes = Number(item.size_bytes ?? item.size ?? 0);
  if (!isNaN(bytes) && bytes > 0) {
    return formatBytesLocal(bytes);
  }
  if (item.size_formatted && item.size_formatted !== '--') {
    return item.size_formatted;
  }
  return '0 B';
}

function isProtectedFilePath(path) {
  if (!path && AppState.files?.root === 'backups') {
    return true;
  }
  if (!path) return false;
  const clean = String(path).replace(/\\/g, '/').replace(/^\/+/, '');
  const first = clean.split('/')[0];
  return (
    AppState.files?.root === 'backups' ||
    first === 'BACKUPS_HISTORICOS' ||
    first === 'BACKUPS_PERMANENTES' ||
    first === 'LOGS_BACKUP' ||
    first === '.trash'
  );
}

function getFileTypeMeta(item) {
  if (!item) {
    return {
      icon: '#icon-file',
      color: '#64748b',
      headerBg: '#334155',
      badgeClass: 'badge-type-unknown',
      badgeBg: 'rgba(51, 65, 85, 0.25)',
      badgeColor: '#94a3b8',
      badgeBorder: 'rgba(148, 163, 184, 0.35)',
      typeLabel: 'Archivo',
      shortLabel: 'Archivo',
      previewable: false,
      mediaType: 'unknown',
    };
  }

  if (item.is_dir) {
    return {
      icon: '#icon-folder',
      color: '#ffe2bb',
      headerBg: '#ffe2bb',
      badgeClass: 'badge-type-folder',
      badgeBg: 'rgba(255, 226, 187, 0.18)',
      badgeColor: '#ffe2bb',
      badgeBorder: 'rgba(255, 226, 187, 0.4)',
      typeLabel: 'Carpeta de archivos',
      shortLabel: 'Carpeta',
      previewable: false,
      mediaType: 'folder',
    };
  }

  const name = item.name || item.filename || '';
  const ext = name.includes('.') ? name.split('.').pop().toLowerCase() : '';

  // 1. Hojas de cálculo (Excel: verde)
  if (['xlsx', 'xls', 'xlsm', 'xlsb', 'ods', 'csv', 'tsv'].includes(ext)) {
    return {
      icon: '#icon-table',
      color: '#16a34a',
      headerBg: '#107c41',
      badgeClass: 'badge-type-excel',
      badgeBg: 'rgba(16, 124, 65, 0.18)',
      badgeColor: '#22c55e',
      badgeBorder: 'rgba(34, 197, 94, 0.35)',
      typeLabel: ext ? `Hoja de cálculo (${ext.toUpperCase()})` : 'Hoja de cálculo',
      shortLabel: 'Excel',
      previewable: true,
      mediaType: 'excel',
    };
  }

  // 2. Documentos de texto / ofimática (Word: azul)
  if (['docx', 'doc', 'docm', 'dotx', 'dot', 'rtf', 'odt'].includes(ext)) {
    return {
      icon: '#icon-file-text',
      color: '#2563eb',
      headerBg: '#185abd',
      badgeClass: 'badge-type-word',
      badgeBg: 'rgba(24, 90, 189, 0.18)',
      badgeColor: '#3b82f6',
      badgeBorder: 'rgba(59, 130, 246, 0.35)',
      typeLabel: ext ? `Documento Word (${ext.toUpperCase()})` : 'Documento Word',
      shortLabel: 'Word',
      previewable: true,
      mediaType: 'word',
    };
  }

  // 3. Presentaciones (PowerPoint: naranja)
  if (['pptx', 'ppt', 'pptm', 'potx', 'odp'].includes(ext)) {
    return {
      icon: '#icon-presentation',
      color: '#ea580c',
      headerBg: '#c43e1c',
      badgeClass: 'badge-type-presentation',
      badgeBg: 'rgba(196, 62, 28, 0.18)',
      badgeColor: '#f97316',
      badgeBorder: 'rgba(249, 115, 22, 0.35)',
      typeLabel: ext ? `Presentación PowerPoint (${ext.toUpperCase()})` : 'Presentación PowerPoint',
      shortLabel: 'PowerPoint',
      previewable: true,
      mediaType: 'presentation',
    };
  }

  // 4. Documentos PDF (rojo)
  if (ext === 'pdf') {
    return {
      icon: '#icon-file-text',
      color: '#dc2626',
      headerBg: '#b91c1c',
      badgeClass: 'badge-type-pdf',
      badgeBg: 'rgba(185, 28, 28, 0.18)',
      badgeColor: '#ef4444',
      badgeBorder: 'rgba(239, 68, 68, 0.35)',
      typeLabel: 'Documento PDF',
      shortLabel: 'PDF',
      previewable: true,
      mediaType: 'pdf',
    };
  }

  // 5. Imágenes (morado)
  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'ico', 'tiff', 'tif'].includes(ext)) {
    return {
      icon: '#icon-image',
      color: '#9333ea',
      headerBg: '#7e22ce',
      badgeClass: 'badge-type-image',
      badgeBg: 'rgba(126, 34, 206, 0.18)',
      badgeColor: '#a855f7',
      badgeBorder: 'rgba(168, 85, 247, 0.35)',
      typeLabel: ext ? `Imagen ${ext.toUpperCase()}` : 'Imagen',
      shortLabel: ext ? ext.toUpperCase() : 'Imagen',
      previewable: true,
      mediaType: 'image',
    };
  }

  // 6. Video (rosa/fucsia)
  if (['mp4', 'webm', 'mov', 'avi', 'mkv', 'wmv', 'flv', 'm4v'].includes(ext)) {
    return {
      icon: '#icon-video',
      color: '#e11d48',
      headerBg: '#be123c',
      badgeClass: 'badge-type-video',
      badgeBg: 'rgba(190, 18, 60, 0.18)',
      badgeColor: '#f43f5e',
      badgeBorder: 'rgba(244, 63, 94, 0.35)',
      typeLabel: ext ? `Video ${ext.toUpperCase()}` : 'Video',
      shortLabel: ext ? ext.toUpperCase() : 'Video',
      previewable: ['mp4', 'webm'].includes(ext),
      mediaType: 'video',
    };
  }

  // 7. Audio (turquesa/teal)
  if (['mp3', 'wav', 'ogg', 'flac', 'm4a', 'aac', 'wma'].includes(ext)) {
    return {
      icon: '#icon-music',
      color: '#0d9488',
      headerBg: '#0f766e',
      badgeClass: 'badge-type-audio',
      badgeBg: 'rgba(15, 118, 110, 0.18)',
      badgeColor: '#14b8a6',
      badgeBorder: 'rgba(20, 184, 166, 0.35)',
      typeLabel: ext ? `Audio ${ext.toUpperCase()}` : 'Audio',
      shortLabel: ext ? ext.toUpperCase() : 'Audio',
      previewable: ['mp3', 'wav', 'ogg'].includes(ext),
      mediaType: 'audio',
    };
  }

  // 8. Archivos comprimidos (dorado/ámbar)
  if (['zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar', 'tgz'].includes(ext)) {
    return {
      icon: '#icon-archive',
      color: '#d97706',
      headerBg: '#b45309',
      badgeClass: 'badge-type-archive',
      badgeBg: 'rgba(180, 83, 9, 0.18)',
      badgeColor: '#f59e0b',
      badgeBorder: 'rgba(245, 158, 11, 0.35)',
      typeLabel: ext ? `Archivo comprimido (${ext.toUpperCase()})` : 'Archivo comprimido',
      shortLabel: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'tgz'].includes(ext) ? ext.toUpperCase() : 'Comprimido',
      previewable: false,
      mediaType: 'archive',
    };
  }

  // 9. Código y scripts (azul cian/índigo)
  if (['sh', 'bash', 'py', 'js', 'ts', 'jsx', 'tsx', 'php', 'sql', 'json', 'yml', 'yaml', 'html', 'htm', 'css', 'xml', 'rs', 'c', 'cpp', 'h', 'bat', 'cmd', 'ps1'].includes(ext)) {
    const isScript = ['sh', 'bash', 'bat', 'cmd', 'ps1', 'py'].includes(ext);
    return {
      icon: isScript ? '#icon-terminal' : '#icon-file-text',
      color: '#0284c7',
      headerBg: '#0369a1',
      badgeClass: 'badge-type-code',
      badgeBg: 'rgba(3, 105, 161, 0.18)',
      badgeColor: '#38bdf8',
      badgeBorder: 'rgba(56, 189, 248, 0.35)',
      typeLabel: ext ? `Código (${ext.toUpperCase()})` : 'Código',
      shortLabel: ext ? ext.toUpperCase() : 'Código',
      previewable: true,
      mediaType: 'text',
    };
  }

  // 10. Texto plano y configuración (pizarra/acero)
  if (['txt', 'log', 'conf', 'ini', 'env', 'md', 'cfg', 'inf'].includes(ext)) {
    return {
      icon: '#icon-file-text',
      color: '#64748b',
      headerBg: '#334155',
      badgeClass: 'badge-type-text',
      badgeBg: 'rgba(51, 65, 85, 0.25)',
      badgeColor: '#94a3b8',
      badgeBorder: 'rgba(148, 163, 184, 0.35)',
      typeLabel: ext ? `Documento de texto (${ext.toUpperCase()})` : 'Documento de texto',
      shortLabel: ext ? ext.toUpperCase() : 'Texto',
      previewable: true,
      mediaType: 'text',
    };
  }

  // 11. Genérico / desconocido
  return {
    icon: '#icon-file',
    color: '#64748b',
    headerBg: '#334155',
    badgeClass: 'badge-type-unknown',
    badgeBg: 'rgba(51, 65, 85, 0.25)',
    badgeColor: '#94a3b8',
    badgeBorder: 'rgba(148, 163, 184, 0.35)',
    typeLabel: ext ? `Archivo .${ext}` : 'Archivo',
    shortLabel: ext ? ext.toUpperCase() : 'Archivo',
    previewable: true,
    mediaType: 'unknown',
  };
}

function setFileViewMode(mode, triggerRender = true) {
  AppState.files.viewMode = mode;
  try {
    localStorage.setItem('nas_file_view', mode);
  } catch (e) { }

  const btnGrid = document.getElementById('btn-view-grid');
  const btnList = document.getElementById('btn-view-list');
  if (btnGrid) btnGrid.classList.toggle('active', mode === 'grid');
  if (btnList) btnList.classList.toggle('active', mode === 'list');

  if (triggerRender) {
    renderCurrentFiles();
  }
}

function formatBytesLocal(bytes) {
  if (bytes === null || bytes === undefined || isNaN(bytes)) return '0 B';
  const num = Number(bytes);
  if (num === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const i = Math.floor(Math.log(num) / Math.log(k));
  const val = parseFloat((num / Math.pow(k, i)).toFixed(i === 0 ? 0 : 2));
  return `${val} ${sizes[i] || 'B'}`;
}

function updateFilesStatusBar() {
  const totalEl = document.getElementById('files-status-total');
  const sizeEl = document.getElementById('files-status-size');
  const selectedEl = document.getElementById('files-status-selected');
  const storageEl = document.getElementById('files-status-storage-info');

  const items = AppState.files.displayItems !== undefined ? AppState.files.displayItems : (AppState.files.items || []);
  const allItems = AppState.files.items || [];
  const foldersCount = items.filter(i => i.is_dir).length;
  const filesCount = items.filter(i => !i.is_dir).length;

  let totalBytes = 0;
  items.forEach(i => {
    totalBytes += Number(i.size_bytes || i.size || 0);
  });

  if (totalEl) {
    if (AppState.files.filterQuery) {
      totalEl.textContent = `${items.length} de ${allItems.length} elementos (${foldersCount} carpetas, ${filesCount} archivos)`;
    } else {
      totalEl.textContent = `${items.length} elemento(s) (${foldersCount} carpetas, ${filesCount} archivos)`;
    }
  }

  if (sizeEl) {
    sizeEl.textContent = `Tamaño visible: ${formatBytesLocal(totalBytes)}`;
  }

  if (selectedEl) {
    if (AppState.files.selectedItemPath) {
      const selItem = allItems.find(i => (i.relative_path || i.path) === AppState.files.selectedItemPath);
      if (selItem) {
        const itemSize = getFileFormattedSize(selItem) || '0 B';
        const itemType = selItem.is_dir ? `Carpeta (${itemSize})` : itemSize;
        selectedEl.textContent = `• Seleccionado: ${selItem.name} (${itemType})`;
      } else {
        selectedEl.textContent = '';
      }
    } else {
      selectedEl.textContent = '';
    }
  }

  if (storageEl) {
    const rootName = AppState.files.root === 'backups' ? 'Backups (/srv/nas/BACKUPS_HISTORICOS)' : 'Almacenamiento (/srv/nas)';
    const freeStr = AppState.storage?.overview?.free_formatted ? ` • Libre: ${AppState.storage.overview.free_formatted}` : '';
    storageEl.textContent = `${rootName}${freeStr}`;
  }
}

function filterCurrentFiles(query = null) {
  if (query === null) {
    query = document.getElementById('files-filter-input')?.value || '';
  }
  AppState.files.filterQuery = String(query).toLowerCase().trim();
  applyFilesFilterAndSort();
}

function setFilesSort(sortKey) {
  AppState.files.sortKey = sortKey || 'name_asc';
  const sel = document.getElementById('files-sort-select');
  if (sel && sel.value !== AppState.files.sortKey) {
    sel.value = AppState.files.sortKey;
  }
  updateSortIndicators();
  applyFilesFilterAndSort();
}

function toggleTableSort(col) {
  const currentKey = AppState.files.sortKey || 'name_asc';
  let newKey = `${col}_asc`;
  if (currentKey === `${col}_asc`) {
    newKey = `${col}_desc`;
  } else if (currentKey === `${col}_desc`) {
    newKey = `${col}_asc`;
  } else {
    if (col === 'size' || col === 'date') {
      newKey = `${col}_desc`;
    } else {
      newKey = `${col}_asc`;
    }
  }
  setFilesSort(newKey);
}

function updateSortIndicators() {
  const cols = ['name', 'size', 'permissions', 'date'];
  const [activeCol, activeDir] = (AppState.files.sortKey || 'name_asc').split('_');
  cols.forEach(c => {
    const el = document.getElementById(`sort-indicator-${c}`);
    if (el) {
      if (c === activeCol) {
        el.textContent = activeDir === 'desc' ? ' ▼' : ' ▲';
        el.style.opacity = '1';
      } else {
        el.textContent = '';
      }
    }
  });
}

function sortFilesItems(items, sortKey = 'name_asc') {
  const [col, dir] = (sortKey || 'name_asc').split('_');
  const mult = dir === 'desc' ? -1 : 1;

  return [...items].sort((a, b) => {
    const aDir = a.is_dir ? 1 : 0;
    const bDir = b.is_dir ? 1 : 0;
    if (aDir !== bDir) {
      return bDir - aDir;
    }

    if (col === 'size') {
      const aSize = Number(a.size_bytes ?? a.size ?? 0);
      const bSize = Number(b.size_bytes ?? b.size ?? 0);
      return (aSize - bSize) * mult;
    }

    if (col === 'date') {
      const aTime = a.mtime_raw ? Number(a.mtime_raw) : new Date(a.modified_at || a.mtime || 0).getTime();
      const bTime = b.mtime_raw ? Number(b.mtime_raw) : new Date(b.modified_at || b.mtime || 0).getTime();
      return (aTime - bTime) * mult;
    }

    if (col === 'permissions') {
      const aPerm = String(a.permissions || '');
      const bPerm = String(b.permissions || '');
      return aPerm.localeCompare(bPerm) * mult;
    }

    const aName = String(a.name || '');
    const bName = String(b.name || '');
    return aName.localeCompare(bName, undefined, { numeric: true, sensitivity: 'base' }) * mult;
  });
}

function applyFilesFilterAndSort() {
  let list = Array.isArray(AppState.files.items) ? [...AppState.files.items] : [];
  const q = (AppState.files.filterQuery || '').toLowerCase().trim();
  if (q) {
    list = list.filter(item => {
      const name = String(item.name || '').toLowerCase();
      const owner = String(item.owner || '').toLowerCase();
      const group = String(item.group || '').toLowerCase();
      return name.includes(q) || owner.includes(q) || group.includes(q);
    });
  }
  list = sortFilesItems(list, AppState.files.sortKey || 'name_asc');
  AppState.files.displayItems = list;
  renderCurrentFiles();
}

function copyItemUncPath(relPath) {
  const host = window.location.hostname || window.NAS_HOSTNAME || 'SRV-NAS';
  let cur = String(relPath || '').replace(/^\/+|\/+$/g, '');
  if (AppState.files.root === 'backups') {
    cur = cur ? `BACKUPS_HISTORICOS\\${cur.replace(/\//g, '\\')}` : 'BACKUPS_HISTORICOS';
  } else {
    cur = cur ? cur.replace(/\//g, '\\') : '';
  }
  const unc = cur ? `\\\\${host}\\${cur}` : `\\\\${host}`;
  copyUncPath(unc);
}

function copyCurrentUncPath() {
  copyItemUncPath(AppState.files.currentPath);
}

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

function triggerFileInput() {
  const input = document.getElementById('files-hidden-input');
  if (input) input.click();
}

function handleFileSelect(e) {
  const files = e.target.files;
  if (files && files.length > 0) {
    uploadFiles(files);
  }
  e.target.value = '';
}

async function uploadFiles(files) {
  const progressBar = document.getElementById('upload-progress-bar');
  const progressFill = document.getElementById('upload-progress-fill');
  const fileLabel = document.getElementById('upload-file-label');
  const percentLabel = document.getElementById('upload-percent-label');
  const uploadBtn = document.getElementById('btn-upload-files');

  if (uploadBtn) {
    uploadBtn.disabled = true;
    uploadBtn.classList.add('is-loading');
  }

  if (progressBar) progressBar.style.display = 'block';

  let successCount = 0;

  try {
    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      if (fileLabel) fileLabel.textContent = `Subiendo (${i + 1}/${files.length}): ${file.name}`;
      if (percentLabel) percentLabel.textContent = '0%';
      if (progressFill) progressFill.style.width = '0%';

      try {
        await new Promise((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          xhr.open('POST', '/api/files/upload', true);
          const csrf = getCsrfToken();
          if (csrf) {
            xhr.setRequestHeader('X-CSRF-Token', csrf);
          }

          xhr.upload.onprogress = (evt) => {
            if (evt.lengthComputable) {
              const percent = Math.round((evt.loaded / evt.total) * 100);
              if (percentLabel) percentLabel.textContent = `${percent}%`;
              if (progressFill) progressFill.style.width = `${percent}%`;
            }
          };

          xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) {
              try {
                const resp = JSON.parse(xhr.responseText);
                if (resp.status === 'success' || resp.success === true) {
                  successCount++;
                  resolve(resp);
                } else {
                  reject(new Error(resp.message || resp.error || 'Error en respuesta del servidor'));
                }
              } catch (err) {
                reject(err);
              }
            } else {
              let msg = `Fallo HTTP ${xhr.status}: ${xhr.statusText}`;
              try {
                const errResp = JSON.parse(xhr.responseText);
                if (errResp.message || errResp.error) {
                  msg = errResp.message || errResp.error;
                }
              } catch (e) {
                // Conservar mensaje HTTP por defecto si no es JSON
              }
              reject(new Error(msg));
            }
          };

          xhr.onerror = () => reject(new Error('Error de red durante la subida'));

          const formData = new FormData();
          formData.append('root', AppState.files.root || 'nas');
          formData.append('path', AppState.files.currentPath || '');
          formData.append('file', file);

          xhr.send(formData);
        });
      } catch (err) {
        showToast(`Error al subir "${file.name}": ${err.message}`, 'error');
      }
    }
  } finally {
    if (uploadBtn) {
      uploadBtn.disabled = false;
      uploadBtn.classList.remove('is-loading');
    }
    if (progressBar) {
      setTimeout(() => {
        progressBar.style.display = 'none';
        if (progressFill) progressFill.style.width = '0%';
      }, 1200);
    }
    if (successCount > 0) {
      showToast(`Se subieron con éxito ${successCount} archivo(s).`, 'success');
    }
    loadFiles();
  }
}

function downloadFile(relPath) {
  const root = encodeURIComponent(AppState.files.root || 'nas');
  const path = encodeURIComponent(relPath);
  window.location.href = `/api/files/download?root=${root}&path=${path}`;
}

let _zipDownloadInProgress = false;

async function downloadFolderZip(relPath, force = false) {
  const cleanRel = String(relPath || '').replace(/^\/+|\/+$/g, '');
  if (!cleanRel) {
    showToast('Seleccione o ingrese a una carpeta específica para descargarla en formato ZIP.', 'warning');
    return;
  }
  if (_zipDownloadInProgress) {
    showToast('Ya hay una compresión ZIP en progreso, por favor espera a que finalice.', 'info');
    return;
  }

  // Alerta preventiva si hay respaldos en ejecución
  if (!force) {
    try {
      const runRes = await apiFetch('/api/backups/running');
      const runData = runRes.data || runRes;
      if (runData && runData.running) {
        const listEl = document.getElementById('zip-running-backups-list');
        if (listEl) {
          const taskNames = (runData.tasks && runData.tasks.length > 0)
            ? runData.tasks.map(t => `<strong>${escapeHtml(t)}</strong>`).join(', ')
            : 'Procesos de sincronización rsync activos';
          listEl.innerHTML = `<svg class="icon icon-sm" style="color:var(--accent-warning); margin-right:4px;"><use href="#icon-alert-triangle"></use></svg> Tareas activas: ${taskNames}`;
        }
        const btnConfirm = document.getElementById('btn-confirm-zip-download');
        if (btnConfirm) {
          btnConfirm.onclick = () => {
            closeModal('modal-zip-backup-warning');
            downloadFolderZip(relPath, true);
          };
        }
        openModal('modal-zip-backup-warning');
        return;
      }
    } catch (e) {
      console.warn('No se pudo verificar el estado de respaldos:', e);
    }
  }

  _zipDownloadInProgress = true;
  const btnZip = document.getElementById('btn-download-zip');
  if (btnZip) {
    btnZip.disabled = true;
    btnZip.classList.add('is-loading');
  }

  const folderName = cleanRel.split('/').pop() || 'carpeta';
  showToast(`Iniciando compresión ZIP de "${folderName}", por favor espera...`, 'info');

  try {
    const root = encodeURIComponent(AppState.files.root || 'nas');
    const path = encodeURIComponent(cleanRel);
    const res = await fetch(`/api/files/download?root=${root}&path=${path}&prepare=1`, {
      method: 'GET',
      credentials: 'same-origin'
    });

    if (!res.ok) {
      let errMsg = `Error al comprimir la carpeta (${res.status})`;
      try {
        const text = (await res.text()).trim();
        if (text) {
          try {
            const parsed = JSON.parse(text);
            errMsg = parsed.error || parsed.message || text;
          } catch (_) {
            errMsg = text;
          }
        }
      } catch (_) { }
      showToast(errMsg, 'error');
      return;
    }

    const contentType = (res.headers.get('Content-Type') || '').toLowerCase();
    if (contentType.includes('application/json')) {
      const data = await res.json();
      if (!data || !data.success || !data.token) {
        showToast((data && (data.error || data.message)) || 'Error al preparar el archivo ZIP.', 'error');
        return;
      }
      const downloadFilename = data.filename || `${folderName}.zip`;
      const sizeSuffix = data.size_formatted ? ` (${data.size_formatted})` : '';
      showToast(`Compresión finalizada${sizeSuffix}. Iniciando descarga de "${downloadFilename}"...`, 'success');

      const link = document.createElement('a');
      link.href = `/api/files/download?root=${root}&path=${path}&token=${encodeURIComponent(data.token)}`;
      link.download = downloadFilename;
      link.style.display = 'none';
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      return;
    }

    const blob = await res.blob();
    let downloadFilename = `${folderName}.zip`;
    const disp = res.headers.get('Content-Disposition') || '';
    const match = disp.match(/filename="?([^";]+)"?/i);
    if (match && match[1]) {
      try {
        downloadFilename = decodeURIComponent(match[1]);
      } catch (_) {
        downloadFilename = match[1];
      }
    }

    showToast(`Compresión finalizada. Iniciando descarga de "${downloadFilename}"...`, 'success');

    const blobUrl = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = blobUrl;
    link.download = downloadFilename;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    setTimeout(() => URL.revokeObjectURL(blobUrl), 60000);
  } catch (err) {
    showToast(`Error durante la compresión o descarga ZIP: ${err.message || err}`, 'error');
  } finally {
    _zipDownloadInProgress = false;
    if (btnZip) {
      btnZip.disabled = false;
      btnZip.classList.remove('is-loading');
    }
  }
}

function downloadCurrentFolderZip() {
  const current = String(AppState.files.currentPath || '').replace(/^\/+|\/+$/g, '');
  const selected = String(AppState.files.selectedItemPath || '').replace(/^\/+|\/+$/g, '');
  if (!current && selected) {
    const item = (AppState.files.items || []).find(i => (i.relative_path || i.path) === selected);
    if (item && item.is_dir) {
      downloadFolderZip(selected);
      return;
    }
  }
  downloadFolderZip(current);
}

function openNewFileModal() {
  const input = document.getElementById('new-file-name');
  if (input) input.value = '';
  openModal('modal-new-file');
  if (input) setTimeout(() => input.focus(), 100);
}

async function submitNewFile(e) {
  e.preventDefault();
  const input = document.getElementById('new-file-name');
  const name = input ? input.value.trim() : '';
  if (!name) return;

  const btn = document.getElementById('btn-submit-new-file') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/create', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        path: AppState.files.currentPath || '',
        name: name,
        content: '',
      }),
    });

    closeModal('modal-new-file');
    showToast(`Archivo "${name}" creado exitosamente.`, 'success');
    await loadFiles();
    const createdPath = res.data?.path || (AppState.files.currentPath ? `${AppState.files.currentPath}/${name}` : name);
    previewFile(createdPath, name);
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function openNewFolderModal() {
  const input = document.getElementById('new-folder-name');
  if (input) input.value = '';
  openModal('modal-new-folder');
  if (input) setTimeout(() => input.focus(), 100);
}

async function submitNewFolder(e) {
  e.preventDefault();
  const input = document.getElementById('new-folder-name');
  const name = input ? input.value.trim() : '';
  if (!name) return;

  const btn = document.getElementById('btn-submit-new-folder') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/files/mkdir', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        path: AppState.files.currentPath || '',
        name: name,
      }),
    });

    closeModal('modal-new-folder');
    showToast(`Carpeta "${name}" creada exitosamente.`, 'success');
    loadFiles();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function openRenameModal(relPath, currentName) {
  const oldpathInput = document.getElementById('rename-file-oldpath');
  const newnameInput = document.getElementById('rename-file-newname');
  if (oldpathInput) oldpathInput.value = relPath;
  if (newnameInput) newnameInput.value = currentName;
  openModal('modal-rename-file');
  if (newnameInput) setTimeout(() => newnameInput.focus(), 100);
}

async function submitRenameFile(e) {
  e.preventDefault();
  const oldPath = document.getElementById('rename-file-oldpath')?.value;
  const newName = document.getElementById('rename-file-newname')?.value.trim();
  if (!oldPath || !newName) return;

  if (isProtectedFilePath(oldPath) || isProtectedFilePath(AppState.files.currentPath)) {
    showToast('Los respaldos históricos son inmutables y no admiten modificaciones.', 'warning');
    closeModal('modal-rename-file');
    return;
  }

  const btn = document.getElementById('btn-submit-rename-file') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/files/rename', {
      method: 'POST',
      body: JSON.stringify({
        root: AppState.files.root || 'nas',
        old_path: oldPath,
        new_name: newName,
      }),
    });

    closeModal('modal-rename-file');
    showToast(`Elemento renombrado a "${newName}".`, 'success');
    loadFiles();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function openMoveModal(relPath, currentName) {
  const sourceInput = document.getElementById('move-file-source');
  const labelInput = document.getElementById('move-file-name-label');
  const targetInput = document.getElementById('move-file-target');
  if (sourceInput) sourceInput.value = relPath;
  if (labelInput) labelInput.value = currentName || relPath;
  if (targetInput) targetInput.value = AppState.files.currentPath || '';
  openModal('modal-move-file');
  if (targetInput) setTimeout(() => targetInput.focus(), 100);
}

async function submitMoveFile(e) {
  e.preventDefault();
  const source = document.getElementById('move-file-source')?.value;
  const target = document.getElementById('move-file-target')?.value.trim() ?? '';
  if (!source) return;

  if (isProtectedFilePath(source) || isProtectedFilePath(target) || isProtectedFilePath(AppState.files.currentPath)) {
    showToast('Los respaldos históricos son inmutables y no admiten modificaciones.', 'warning');
    closeModal('modal-move-file');
    return;
  }

  const btn = document.getElementById('btn-submit-move-file') || e.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/move', {
      method: 'POST',
      body: JSON.stringify({
        path: source,
        target_dir: target,
      }),
    });

    closeModal('modal-move-file');
    showToast(res.message || 'Elemento movido exitosamente.', 'success');
    loadFiles();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function openDeleteModal(relPath, name) {
  const pathInput = document.getElementById('delete-file-path');
  const label = document.getElementById('delete-file-name-label');
  const radioTrash = document.getElementById('del-mode-trash');
  if (pathInput) pathInput.value = relPath;
  if (label) label.textContent = `"${name}"`;
  if (radioTrash) radioTrash.checked = true;
  openModal('modal-delete-file');
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
    const res = await apiFetch('/api/files/trash');
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
      <tr class="trash-table-row" data-id="${item.id}" data-filename="${escapeHtml(item.filename)}" onclick="selectTrashRow(this)">
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
  const id = parseInt(tr.getAttribute('data-id'), 10);
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
    const res = await apiFetch('/api/files/trash', { silentToast: true });
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
      body: JSON.stringify({ id }),
    });

    showToast(`Elemento "${name}" restaurado correctamente a su ubicación.`, 'success');
    loadTrash();
    updateTrashBadge();
  } catch (err) {
    // Ya mostrado por apiFetch
  }
}

async function deleteTrashItem(id, name) {
  if (!window.confirm(`¿Eliminar definitivamente "${name}"? Esta acción no se puede deshacer.`)) {
    return;
  }

  try {
    await apiFetch('/api/files/trash/delete', {
      method: 'POST',
      body: JSON.stringify({ id }),
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

// ==============================================================================
// Previsualizador de Archivos (Previewer 100% Offline con Editor Integrado)
// ==============================================================================
async function previewFile(relPath, fileName) {
  fileName = fileName || (relPath ? relPath.split('/').pop() : 'archivo');
  if (AppState.files.currentPreviewFile?.relPath === relPath && document.getElementById('modal-file-preview')?.classList.contains('active')) {
    return;
  }
  AppState.files.currentPreviewFile = { relPath, fileName };
  AppState.files.currentPreviewText = '';
  AppState.files.isEditingPreview = false;

  const modal = document.getElementById('modal-file-preview');
  if (!modal) return;

  const titleEl = document.getElementById('preview-file-name');
  const metaEl = document.getElementById('preview-file-meta');
  const iconEl = document.getElementById('preview-header-icon');
  const copyBtn = document.getElementById('btn-preview-copy');
  const editBtn = document.getElementById('btn-preview-edit');
  const saveBtn = document.getElementById('btn-preview-save');
  const editLabel = document.getElementById('btn-preview-edit-label');
  const loading = document.getElementById('preview-loading');
  const loadingText = document.getElementById('preview-loading-text');
  const codeContainer = document.getElementById('preview-code-container');
  const contentEl = document.getElementById('preview-code-content');
  const editorEl = document.getElementById('preview-code-editor');
  const imgContainer = document.getElementById('preview-image-container');
  const pdfContainer = document.getElementById('preview-pdf-container');
  const mediaContainer = document.getElementById('preview-media-container');
  const binaryContainer = document.getElementById('preview-binary-container');
  const spreadsheetContainer = document.getElementById('preview-spreadsheet-container');
  const docxContainer = document.getElementById('preview-docx-container');
  const pptxContainer = document.getElementById('preview-pptx-container');

  const ext = fileName.includes('.') ? fileName.split('.').pop().toLowerCase() : '';
  const meta = getFileTypeMeta({ name: fileName, is_dir: false });

  // Buscar el elemento actual para obtener su tamaño formateado si ya existe
  const cleanRel = String(relPath || '').replace(/^\/+/, '');
  const currentItem = (AppState.files.items || []).find(i => String(i.relative_path || i.path || '').replace(/^\/+/, '') === cleanRel || i.name === fileName);
  let sizeFormatted = currentItem ? getFileFormattedSize(currentItem) : '';

  // Configurar encabezado de la modal con el color representativo del fichero
  const modalHeader = modal.querySelector('.modal-header');
  const headerBgColor = meta.headerBg || meta.color;
  if (modalHeader) {
    modalHeader.style.backgroundColor = headerBgColor;
    const textColor = getContrastTextColor(headerBgColor);
    const isLightHeader = textColor === '#0f172a';
    modalHeader.style.borderBottomColor = isLightHeader ? 'rgba(0, 0, 0, 0.12)' : 'rgba(0, 0, 0, 0.25)';
    modalHeader.style.setProperty('--preview-header-bg', headerBgColor);
    modalHeader.style.setProperty('--preview-header-border', isLightHeader ? 'rgba(0, 0, 0, 0.12)' : 'rgba(0, 0, 0, 0.25)');
    modalHeader.style.setProperty('--preview-header-text', textColor);
    modalHeader.style.setProperty('--preview-header-meta', isLightHeader ? 'rgba(15, 23, 42, 0.8)' : 'rgba(255, 255, 255, 0.85)');
    modalHeader.style.setProperty('--preview-btn-bg', isLightHeader ? 'rgba(0, 0, 0, 0.08)' : 'rgba(255, 255, 255, 0.18)');
    modalHeader.style.setProperty('--preview-btn-border', isLightHeader ? 'rgba(0, 0, 0, 0.15)' : 'rgba(255, 255, 255, 0.3)');
    modalHeader.style.setProperty('--preview-btn-hover-bg', isLightHeader ? 'rgba(0, 0, 0, 0.15)' : 'rgba(255, 255, 255, 0.28)');

    if (titleEl) titleEl.style.color = textColor;
    if (metaEl) metaEl.style.color = isLightHeader ? 'rgba(15, 23, 42, 0.8)' : 'rgba(255, 255, 255, 0.85)';
    if (iconEl) {
      iconEl.style.color = textColor;
      iconEl.querySelector('use')?.setAttribute('href', meta.icon);
    }
    const closeBtn = modalHeader.querySelector('.modal-close');
    if (closeBtn) closeBtn.style.color = textColor;

    modalHeader.querySelectorAll('.btn-secondary').forEach(b => {
      b.style.backgroundColor = isLightHeader ? 'rgba(0, 0, 0, 0.08)' : 'rgba(255, 255, 255, 0.18)';
      b.style.color = textColor;
      b.style.borderColor = isLightHeader ? 'rgba(0, 0, 0, 0.15)' : 'rgba(255, 255, 255, 0.3)';
    });

    if (saveBtn) {
      saveBtn.style.backgroundColor = isLightHeader ? '#0f172a' : '#ffffff';
      saveBtn.style.color = isLightHeader ? '#ffffff' : '#0f172a';
      saveBtn.style.borderColor = 'transparent';
    }
  }

  if (titleEl) titleEl.textContent = fileName;
  if (metaEl) metaEl.textContent = sizeFormatted ? `${sizeFormatted} • ${meta.typeLabel}` : meta.typeLabel;
  if (loadingText) loadingText.textContent = 'Cargando previsualización...';
  if (copyBtn) copyBtn.style.display = 'none';
  if (editBtn) editBtn.style.display = 'none';
  if (saveBtn) saveBtn.style.display = 'none';
  if (editLabel) editLabel.textContent = 'Editar';
  if (contentEl) contentEl.style.display = 'block';
  if (editorEl) editorEl.style.display = 'none';

  // Ocultar todos los contenedores y mostrar cargando
  [codeContainer, imgContainer, pdfContainer, mediaContainer, binaryContainer, spreadsheetContainer, docxContainer, pptxContainer].forEach(c => {
    if (c) c.style.display = 'none';
  });
  if (loading) loading.style.display = 'flex';

  openModal('modal-file-preview');

  const root = encodeURIComponent(AppState.files.root || 'nas');
  const encodedPath = encodeURIComponent(relPath);

  // 1. Imágenes
  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'bmp', 'ico'].includes(ext)) {
    const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
    const imgEl = document.getElementById('preview-img-element');
    if (imgEl) {
      imgEl.src = rawUrl;
      imgEl.onload = () => {
        if (loading) loading.style.display = 'none';
        if (imgContainer) imgContainer.style.display = 'flex';
        if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Imagen ${ext.toUpperCase()} • ${imgEl.naturalWidth}x${imgEl.naturalHeight} px`;
      };
      imgEl.onerror = () => {
        if (loading) loading.style.display = 'none';
        if (binaryContainer) {
          binaryContainer.style.display = 'block';
          const bName = document.getElementById('preview-binary-name');
          const bDet = document.getElementById('preview-binary-details');
          if (bName) bName.textContent = fileName;
          if (bDet) bDet.textContent = 'Error al cargar imagen para previsualización.';
        }
      };
    }
    return;
  }

  // 2. PDF
  if (ext === 'pdf') {
    const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}#toolbar=1`;
    const frame = document.getElementById('preview-pdf-frame');
    if (frame) frame.src = rawUrl;
    if (loading) loading.style.display = 'none';
    if (pdfContainer) pdfContainer.style.display = 'block';
    if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Documento PDF (Visualizador nativo del navegador)`;
    return;
  }

  // 3. Audio / Video HTML5
  if (['mp4', 'webm', 'mp3', 'wav', 'ogg'].includes(ext)) {
    const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}`;
    const isVid = ['mp4', 'webm'].includes(ext);
    const vidEl = document.getElementById('preview-video-element');
    const audEl = document.getElementById('preview-audio-element');

    if (isVid && vidEl) {
      vidEl.src = rawUrl;
      vidEl.style.display = 'block';
      if (audEl) audEl.style.display = 'none';
    } else if (audEl) {
      audEl.src = rawUrl;
      audEl.style.display = 'block';
      if (vidEl) vidEl.style.display = 'none';
    }

    if (loading) loading.style.display = 'none';
    if (mediaContainer) mediaContainer.style.display = 'flex';
    if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}${isVid ? 'Video' : 'Audio'} multimedia • ${ext.toUpperCase()}`;
    return;
  }

  // 4. Hojas de Cálculo (Excel: .xlsx, .xls, .ods, .csv, .tsv) — Opción 3 SheetJS Local
  if (['xlsx', 'xls', 'ods', 'csv', 'tsv'].includes(ext)) {
    try {
      if (typeof XLSX === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor local de hojas de cálculo...';
        await loadScriptLazy('/js/vendor/xlsx.full.min.js');
      }

      if (loadingText) loadingText.textContent = 'Descargando y procesando hoja de cálculo...';

      const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
      const response = await fetch(rawUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status} al descargar archivo.`);

      const arrayBuffer = await response.arrayBuffer();
      if (!sizeFormatted && arrayBuffer && arrayBuffer.byteLength) {
        sizeFormatted = formatBytesLocal(arrayBuffer.byteLength);
      }
      const workbook = XLSX.read(new Uint8Array(arrayBuffer), { type: 'array', cellDates: true });

      if (!workbook.SheetNames || workbook.SheetNames.length === 0) {
        throw new Error('El libro no contiene hojas de cálculo legibles.');
      }

      AppState.files.currentWorkbook = workbook;
      AppState.files.currentSheetName = workbook.SheetNames[0];

      renderSpreadsheetWorkbook(workbook, fileName);

      if (loading) loading.style.display = 'none';
      if (spreadsheetContainer) spreadsheetContainer.style.display = 'flex';
      if (metaEl) {
        metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Hoja de cálculo (${ext.toUpperCase()}) • ${workbook.SheetNames.length} hoja(s)`;
      }
      return;
    } catch (err) {
      console.error('Error al procesar hoja de cálculo:', err);
      if (loading) loading.style.display = 'none';
      if (binaryContainer) {
        binaryContainer.style.display = 'block';
        const bName = document.getElementById('preview-binary-name');
        const bDet = document.getElementById('preview-binary-details');
        if (bName) bName.textContent = fileName;
        if (bDet) bDet.textContent = `No se pudo previsualizar la hoja de cálculo: ${err.message || 'error de procesamiento'}`;
      }
      if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Hoja de cálculo`;
      return;
    }
  }

  // 5. Documentos Word (.docx) — Opción 3 docx-preview Local
  if (ext === 'docx') {
    try {
      if (typeof JSZip === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor de descompresión...';
        await loadScriptLazy('/js/vendor/jszip.min.js');
      }
      if (typeof docx === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor de documentos Word...';
        await loadScriptLazy('/js/vendor/docx-preview.min.js');
      }

      if (loadingText) loadingText.textContent = 'Renderizando documento Word...';

      const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
      const response = await fetch(rawUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status} al descargar documento.`);

      const arrayBuffer = await response.arrayBuffer();
      if (!sizeFormatted && arrayBuffer && arrayBuffer.byteLength) {
        sizeFormatted = formatBytesLocal(arrayBuffer.byteLength);
      }
      const docxContentEl = document.getElementById('preview-docx-content');
      if (docxContentEl) {
        docxContentEl.innerHTML = '';
        await docx.renderAsync(arrayBuffer, docxContentEl, null, {
          className: 'docx-document-rendered',
          inWrapper: false,
          ignoreWidth: false,
          ignoreHeight: false,
          breakPages: true,
          renderHeaders: true,
          renderFooters: true,
          useBase64URL: true,
        });
      }

      if (loading) loading.style.display = 'none';
      if (docxContainer) docxContainer.style.display = 'flex';
      if (metaEl) {
        metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Documento Word (.DOCX) • Renderizado local seguro`;
      }
      return;
    } catch (err) {
      console.error('Error al renderizar documento Word:', err);
      if (loading) loading.style.display = 'none';
      if (binaryContainer) {
        binaryContainer.style.display = 'block';
        const bName = document.getElementById('preview-binary-name');
        const bDet = document.getElementById('preview-binary-details');
        if (bName) bName.textContent = fileName;
        if (bDet) bDet.textContent = `No se pudo previsualizar el documento Word: ${err.message || 'error de renderizado'}`;
      }
      if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Documento Word`;
      return;
    }
  }

  // 6. Presentaciones PowerPoint (.pptx) — Opción 3 Inspector de Diapositivas Local
  if (ext === 'pptx') {
    try {
      if (typeof JSZip === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor de descompresión...';
        await loadScriptLazy('/js/vendor/jszip.min.js');
      }

      if (loadingText) loadingText.textContent = 'Extrayendo diapositivas...';

      const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
      const response = await fetch(rawUrl);
      if (!response.ok) throw new Error(`HTTP ${response.status} al descargar presentación.`);

      const arrayBuffer = await response.arrayBuffer();
      if (!sizeFormatted && arrayBuffer && arrayBuffer.byteLength) {
        sizeFormatted = formatBytesLocal(arrayBuffer.byteLength);
      }
      const zip = await JSZip.loadAsync(arrayBuffer);

      const slideEntries = [];
      zip.forEach((entryPath, file) => {
        const match = entryPath.match(/^ppt\/slides\/slide(\d+)\.xml$/i);
        if (match) {
          slideEntries.push({ num: parseInt(match[1], 10), file });
        }
      });

      slideEntries.sort((a, b) => a.num - b.num);

      if (slideEntries.length === 0) {
        throw new Error('La presentación no contiene diapositivas legibles.');
      }

      const parser = new DOMParser();
      const parsedSlides = [];

      for (const entry of slideEntries) {
        const xmlText = await entry.file.async('text');
        const xmlDoc = parser.parseFromString(xmlText, 'application/xml');

        const paragraphs = [];
        const pElements = xmlDoc.getElementsByTagNameNS('*', 'p');
        for (let i = 0; i < pElements.length; i++) {
          const tElements = pElements[i].getElementsByTagNameNS('*', 't');
          let text = '';
          for (let j = 0; j < tElements.length; j++) {
            text += tElements[j].textContent || '';
          }
          text = text.trim();
          if (text) paragraphs.push(text);
        }

        const title = paragraphs.length > 0 ? paragraphs[0] : `Diapositiva ${entry.num}`;
        const bodyParagraphs = paragraphs.length > 1 ? paragraphs.slice(1) : [];

        parsedSlides.push({
          num: entry.num,
          title,
          paragraphs: bodyParagraphs,
        });
      }

      AppState.files.pptxSlides = parsedSlides;
      AppState.files.pptxCurrentIndex = 0;

      renderPptxPresentation(parsedSlides);

      if (loading) loading.style.display = 'none';
      if (pptxContainer) pptxContainer.style.display = 'flex';
      if (metaEl) {
        metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Presentación PowerPoint (.PPTX) • ${parsedSlides.length} diapositiva(s)`;
      }
      return;
    } catch (err) {
      console.error('Error al extraer diapositivas PowerPoint:', err);
      if (loading) loading.style.display = 'none';
      if (binaryContainer) {
        binaryContainer.style.display = 'block';
        const bName = document.getElementById('preview-binary-name');
        const bDet = document.getElementById('preview-binary-details');
        if (bName) bName.textContent = fileName;
        if (bDet) bDet.textContent = `No se pudieron extraer diapositivas: ${err.message || 'formato no legible'}`;
      }
      if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Presentación PowerPoint`;
      return;
    }
  }

  // 7. Formatos heredados binarios de Office (.doc, .ppt)
  if (['doc', 'ppt'].includes(ext)) {
    if (loading) loading.style.display = 'none';
    if (binaryContainer) {
      binaryContainer.style.display = 'block';
      const bName = document.getElementById('preview-binary-name');
      const bDet = document.getElementById('preview-binary-details');
      if (bName) bName.textContent = fileName;
      if (bDet) {
        bDet.textContent = `Formato binario heredado de Microsoft Office (${ext.toUpperCase()}). Para previsualización interactiva en el navegador, guarde el archivo en formato moderno (.${ext}x) o descárguelo directamente.`;
      }
    }
    if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Documento ${ext.toUpperCase()} (Heredado)`;
    return;
  }

  // 8. Archivos de Texto / Código / Configuración
  try {
    const res = await apiFetch(`/api/files/content?root=${root}&path=${encodedPath}`);
    const d = res.data || res;

    AppState.files.currentPreviewText = d.content || '';

    const lines = d.lines_count || (d.content ? d.content.split('\n').length : 0);
    const lineNumbersEl = document.getElementById('preview-line-numbers');

    if (contentEl) {
      contentEl.textContent = d.content || '';
    }
    if (editorEl) {
      editorEl.value = d.content || '';
      editorEl.onscroll = () => {
        if (lineNumbersEl) lineNumbersEl.scrollTop = editorEl.scrollTop;
      };
      editorEl.oninput = () => {
        const val = editorEl.value;
        AppState.files.currentPreviewText = val;
        if (contentEl) contentEl.textContent = val;
        updatePreviewLineNumbers(val);
      };
    }

    updatePreviewLineNumbers(d.content || '');

    if (metaEl) {
      metaEl.textContent = `${d.size_formatted || sizeFormatted || '0 B'} • ${lines} línea(s) • ${d.extension ? d.extension.toUpperCase() : 'TEXTO'}`;
    }

    if (copyBtn) copyBtn.style.display = 'inline-flex';
    if (editBtn) editBtn.style.display = 'inline-flex';
    if (loading) loading.style.display = 'none';
    if (codeContainer) codeContainer.style.display = 'flex';

  } catch (err) {
    if (loading) loading.style.display = 'none';
    if (binaryContainer) {
      binaryContainer.style.display = 'block';
      const bName = document.getElementById('preview-binary-name');
      const bDet = document.getElementById('preview-binary-details');
      if (bName) bName.textContent = fileName;
      if (bDet) bDet.textContent = err.message || 'Archivo no legible como texto plano.';
    }
    if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Archivo binario`;
  }
}

function renderSpreadsheetWorkbook(workbook, fileName) {
  const tabsEl = document.getElementById('preview-sheet-tabs');
  const tableContainer = document.getElementById('preview-spreadsheet-table-container');
  if (!tabsEl || !tableContainer) return;

  tabsEl.innerHTML = '';
  workbook.SheetNames.forEach(sheetName => {
    const tabBtn = document.createElement('button');
    tabBtn.type = 'button';
    tabBtn.className = 'preview-sheet-tab' + (sheetName === AppState.files.currentSheetName ? ' active' : '');
    tabBtn.textContent = sheetName;
    tabBtn.onclick = () => {
      AppState.files.currentSheetName = sheetName;
      document.querySelectorAll('.preview-sheet-tab').forEach(b => b.classList.remove('active'));
      tabBtn.classList.add('active');
      renderActiveSheet(workbook, sheetName);
    };
    tabsEl.appendChild(tabBtn);
  });

  renderActiveSheet(workbook, AppState.files.currentSheetName);
}

function renderActiveSheet(workbook, sheetName) {
  const tableContainer = document.getElementById('preview-spreadsheet-table-container');
  const statsEl = document.getElementById('preview-spreadsheet-stats');
  if (!tableContainer) return;

  const sheet = workbook.Sheets[sheetName];
  if (!sheet) {
    tableContainer.innerHTML = '<div style="padding:24px; text-align:center; color:var(--text-muted);">Hoja vacía</div>';
    return;
  }

  const html = XLSX.utils.sheet_to_html(sheet, { id: 'preview-sheet-table', editable: false });
  tableContainer.innerHTML = html;

  const ref = sheet['!ref'] || '';
  if (statsEl) {
    statsEl.textContent = ref ? `Rango: ${ref}` : '';
  }
}

function renderPptxPresentation(slides) {
  const countEl = document.getElementById('preview-pptx-count');
  const listEl = document.getElementById('preview-pptx-slides-list');
  if (countEl) countEl.textContent = slides.length;
  if (!listEl) return;

  listEl.innerHTML = '';
  slides.forEach((slide, idx) => {
    const thumb = document.createElement('div');
    thumb.className = 'preview-pptx-slide-thumb' + (idx === 0 ? ' active' : '');
    thumb.innerHTML = `<div style="font-weight:600; margin-bottom:2px;">#${slide.num}</div><div style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${escapeHtml(slide.title)}</div>`;
    thumb.onclick = () => {
      AppState.files.pptxCurrentIndex = idx;
      displayPptxSlide(idx);
    };
    listEl.appendChild(thumb);
  });

  displayPptxSlide(0);
}

function displayPptxSlide(index) {
  const slides = AppState.files.pptxSlides || [];
  if (!slides[index]) return;
  const slide = slides[index];

  const thumbs = document.querySelectorAll('.preview-pptx-slide-thumb');
  thumbs.forEach((t, i) => {
    if (i === index) t.classList.add('active');
    else t.classList.remove('active');
  });

  const labelEl = document.getElementById('preview-pptx-current-label');
  const titleEl = document.getElementById('preview-pptx-slide-title');
  const bodyEl = document.getElementById('preview-pptx-slide-body');
  const prevBtn = document.getElementById('btn-pptx-prev');
  const nextBtn = document.getElementById('btn-pptx-next');

  if (labelEl) labelEl.textContent = `Diapositiva ${index + 1} de ${slides.length}`;
  if (titleEl) titleEl.textContent = slide.title || `Diapositiva ${slide.num}`;

  if (bodyEl) {
    if (slide.paragraphs && slide.paragraphs.length > 0) {
      let html = '<ul style="padding-left:20px; margin:0;">';
      slide.paragraphs.forEach(p => {
        html += `<li style="margin-bottom:8px;">${escapeHtml(p)}</li>`;
      });
      html += '</ul>';
      bodyEl.innerHTML = html;
    } else {
      bodyEl.innerHTML = '<p style="color:var(--text-muted); font-style:italic;">(Esta diapositiva no contiene texto en párrafos o solo contiene elementos visuales)</p>';
    }
  }

  if (prevBtn) prevBtn.disabled = index <= 0;
  if (nextBtn) nextBtn.disabled = index >= slides.length - 1;
}

function navigatePptxSlide(delta) {
  const slides = AppState.files.pptxSlides || [];
  const newIndex = (AppState.files.pptxCurrentIndex || 0) + delta;
  if (newIndex >= 0 && newIndex < slides.length) {
    AppState.files.pptxCurrentIndex = newIndex;
    displayPptxSlide(newIndex);
  }
}

function updatePreviewLineNumbers(text) {
  const lineNumbersEl = document.getElementById('preview-line-numbers');
  if (!lineNumbersEl) return;
  const count = text === '' ? 1 : Math.max(1, text.split('\n').length);
  let numsHtml = '';
  for (let i = 1; i <= count; i++) {
    numsHtml += `${i}<br>`;
  }
  lineNumbersEl.innerHTML = numsHtml;
}

function togglePreviewEditMode() {
  const contentEl = document.getElementById('preview-code-content');
  const editorEl = document.getElementById('preview-code-editor');
  const saveBtn = document.getElementById('btn-preview-save');
  const editLabel = document.getElementById('btn-preview-edit-label');

  AppState.files.isEditingPreview = !AppState.files.isEditingPreview;

  if (AppState.files.isEditingPreview) {
    if (contentEl) contentEl.style.display = 'none';
    if (editorEl) {
      editorEl.style.display = 'block';
      editorEl.value = AppState.files.currentPreviewText || '';
      editorEl.focus();
    }
    if (saveBtn) saveBtn.style.display = 'inline-flex';
    if (editLabel) editLabel.textContent = 'Cancelar';
  } else {
    if (editorEl) editorEl.style.display = 'none';
    if (contentEl) {
      contentEl.style.display = 'block';
      contentEl.textContent = AppState.files.currentPreviewText || '';
    }
    if (saveBtn) saveBtn.style.display = 'none';
    if (editLabel) editLabel.textContent = 'Editar';
  }
}

async function saveCurrentPreviewFile() {
  if (!AppState.files.currentPreviewFile?.relPath) return;

  const editorEl = document.getElementById('preview-code-editor');
  const content = editorEl ? editorEl.value : (AppState.files.currentPreviewText || '');
  const relPath = AppState.files.currentPreviewFile.relPath;
  const root = AppState.files.root || 'nas';
  const saveBtn = document.getElementById('btn-preview-save');

  if (saveBtn) {
    saveBtn.disabled = true;
    saveBtn.classList.add('is-loading');
  }

  try {
    const res = await apiFetch('/api/files/save', {
      method: 'POST',
      body: JSON.stringify({
        root: root,
        path: relPath,
        content: content,
      }),
    });

    AppState.files.currentPreviewText = content;
    const contentEl = document.getElementById('preview-code-content');
    if (contentEl) contentEl.textContent = content;

    const metaEl = document.getElementById('preview-file-meta');
    if (metaEl) {
      const ext = AppState.files.currentPreviewFile.fileName.includes('.')
        ? AppState.files.currentPreviewFile.fileName.split('.').pop().toUpperCase()
        : 'TEXTO';
      const lines = content === '' ? 1 : content.split('\n').length;
      metaEl.textContent = `${res.size_formatted || '0 B'} • ${lines} línea(s) • ${ext}`;
    }

    showToast('Archivo guardado correctamente.', 'success');
    togglePreviewEditMode();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (saveBtn) {
      saveBtn.disabled = false;
      saveBtn.classList.remove('is-loading');
    }
  }
}

function copyPreviewContent() {
  if (!AppState.files.currentPreviewText) {
    showToast('No hay contenido para copiar.', 'warning');
    return;
  }

  navigator.clipboard.writeText(AppState.files.currentPreviewText)
    .then(() => showToast('Contenido copiado al portapapeles.', 'success'))
    .catch(() => showToast('No se pudo copiar al portapapeles.', 'error'));
}

function downloadPreviewFile() {
  if (AppState.files.currentPreviewFile?.relPath) {
    downloadFile(AppState.files.currentPreviewFile.relPath);
  }
}

