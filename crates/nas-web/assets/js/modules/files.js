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
