// ==============================================================================
// Inspector de Archivos Comprimidos (RAR vía WASM + ZIP vía JSZip) 100% Offline
// Puerto en memoria del Extractor de node-unrar-js (MIT) para listar y extraer
// entradas sin tocar el disco del servidor ni depender de CDNs.
// ==============================================================================
const ArchiveInspector = {
  enginePromise: null,
  current: null, // { kind: 'rar'|'zip', extractor|zip, entries, fileName, sizeFormatted }
};

const ARCHIVE_MAX_ENTRY_BYTES = 100 * 1024 * 1024; // 100 MiB descomprimido

const RAR_ERROR_MSG = {
  ERAR_NO_MEMORY: 'Memoria insuficiente para procesar el archivo.',
  ERAR_BAD_DATA: 'La cabecera o los datos del archivo están dañados.',
  ERAR_BAD_ARCHIVE: 'El archivo no es un archivo RAR válido.',
  ERAR_UNKNOWN_FORMAT: 'Formato de archivo desconocido.',
  ERAR_EOPEN: 'Error al abrir el archivo.',
  ERAR_EREAD: 'Error al leer el archivo.',
  ERAR_MISSING_PASSWORD: 'El archivo está protegido con contraseña.',
  ERAR_BAD_PASSWORD: 'La contraseña proporcionada es incorrecta.',
  ERAR_UNKNOWN: 'Error desconocido al procesar el archivo.',
};

const RAR_ERROR_CODE = {
  0: 'ERAR_SUCCESS',
  10: 'ERAR_END_ARCHIVE',
  11: 'ERAR_NO_MEMORY',
  12: 'ERAR_BAD_DATA',
  13: 'ERAR_BAD_ARCHIVE',
  14: 'ERAR_UNKNOWN_FORMAT',
  15: 'ERAR_EOPEN',
  18: 'ERAR_EREAD',
  22: 'ERAR_MISSING_PASSWORD',
  24: 'ERAR_BAD_PASSWORD',
};

class ArchivePreviewError extends Error {
  constructor(reason, message) {
    super(message);
    this.reason = reason;
  }
}

// ------------------------------------------------------------------------------
// Motor unrar (WASM) con carga perezosa
// ------------------------------------------------------------------------------
async function loadUnrarEngine() {
  if (!ArchiveInspector.enginePromise) {
    ArchiveInspector.enginePromise = (async () => {
      if (typeof window.Module !== 'function') {
        await loadScriptLazy('/js/vendor/unrar.js');
      }
      const factory = window.Module;
      if (typeof factory !== 'function') {
        throw new Error('No se pudo inicializar el motor RAR.');
      }
      return factory({ locateFile: (p) => `/js/vendor/${p}` });
    })();
  }
  return ArchiveInspector.enginePromise;
}

// ------------------------------------------------------------------------------
// Archivo en memoria (puerto de DataFile de node-unrar-js)
// ------------------------------------------------------------------------------
class RarDataFile {
  constructor(data) {
    this.buffers = [];
    this.pos = 0;
    this.size = 0;
    if (data) {
      this.buffers.push(data);
      this.size = data.byteLength;
    }
  }
  flatten() {
    if (this.buffers.length <= 1) return;
    const merged = new Uint8Array(this.size);
    let offset = 0;
    for (const buf of this.buffers) {
      merged.set(buf, offset);
      offset += buf.byteLength;
    }
    this.buffers = [merged];
  }
  read(size) {
    this.flatten();
    if (size + this.pos > this.size) return null;
    const oldPos = this.pos;
    this.pos += size;
    return this.buffers[0].slice(oldPos, this.pos);
  }
  readAll() {
    this.flatten();
    return this.buffers[0] || new Uint8Array();
  }
  write(data) {
    this.buffers.push(data);
    this.size += data.byteLength;
    this.pos += data.byteLength;
    return true;
  }
  tell() {
    return this.pos;
  }
  seek(pos, method) {
    let newPos = this.pos;
    if (method === 'SET') newPos = pos;
    else if (method === 'CUR') newPos += pos;
    else newPos = this.size - pos;
    if (newPos < 0 || newPos > this.size) return false;
    this.pos = newPos;
    return true;
  }
}

// ------------------------------------------------------------------------------
// Extractor RAR en memoria (puerto de Extractor + ExtractorData de node-unrar-js)
// El WASM invoca open/create/read/write/tell/seek/close sobre Module.extractor.
// ------------------------------------------------------------------------------
class RarMemoryExtractor {
  constructor(unrar, data, password = '') {
    this.unrar = unrar;
    this._password = password;
    this._archive = null;
    this.dataFiles = {};
    this.dataFileMap = {};
    this.currentFd = 1;
    this._filePath = '_nas_panel_archive_.rar';
    const fd = this.currentFd++;
    this.dataFiles[this._filePath] = { file: new RarDataFile(new Uint8Array(data)), fd };
    this.dataFileMap[fd] = this._filePath;
    this.unrar.extractor = this;
  }

  getExtractedFileName(filename) {
    return `*extraido*/${filename}`;
  }

  // Callbacks invocados desde el WASM
  open(filename) {
    const entry = this.dataFiles[filename];
    return entry ? entry.fd : 0;
  }
  create(filename) {
    const fd = this.currentFd++;
    this.dataFiles[this.getExtractedFileName(filename)] = { file: new RarDataFile(), fd };
    this.dataFileMap[fd] = this.getExtractedFileName(filename);
    return fd;
  }
  closeFile(fd) {
    const entry = this.dataFiles[this.dataFileMap[fd]];
    if (entry) entry.file.seek(0, 'SET');
  }
  close(fd) {
    this.closeFile(fd);
  }
  read(fd, buf, size) {
    const entry = this.dataFiles[this.dataFileMap[fd]];
    if (!entry) return -1;
    const data = entry.file.read(size);
    if (data === null) return -1;
    this.unrar.HEAPU8.set(data, buf);
    return data.byteLength;
  }
  write(fd, buf, size) {
    const entry = this.dataFiles[this.dataFileMap[fd]];
    if (!entry) return false;
    entry.file.write(this.unrar.HEAPU8.slice(buf, buf + size));
    return true;
  }
  tell(fd) {
    const entry = this.dataFiles[this.dataFileMap[fd]];
    return entry ? entry.file.tell() : -1;
  }
  seek(fd, pos, method) {
    const entry = this.dataFiles[this.dataFileMap[fd]];
    if (!entry) return false;
    return entry.file.seek(pos, method);
  }

  // Lógica de archivo (puerto de Extractor)
  openArc(listOnly) {
    this._archive = new this.unrar.RarArchive();
    const header = this._archive.open(this._filePath, this._password, listOnly);
    if (header.state.errCode !== 0) {
      throw this.fail(header.state.errCode);
    }
    return { headerEncrypted: (header.flags & 0x0080) !== 0 };
  }
  closeArc() {
    if (this._archive) {
      this._archive.delete();
      this._archive = null;
    }
  }
  fail(errCode) {
    const reason = RAR_ERROR_CODE[errCode] || 'ERAR_UNKNOWN';
    this.closeArc();
    return new ArchivePreviewError(reason, RAR_ERROR_MSG[reason] || RAR_ERROR_MSG.ERAR_UNKNOWN);
  }
  dosTimeToIso(dosTime) {
    const bitLen = [5, 6, 5, 5, 4, 7];
    const parts = [];
    let value = dosTime;
    for (const len of bitLen) {
      parts.push(value & ((1 << len) - 1));
      value >>= len;
    }
    parts.reverse();
    const pad = (n) => (n < 10 ? '0' + n : '' + n);
    return `${1980 + parts[0]}-${pad(parts[1])}-${pad(parts[2])} ${pad(parts[3])}:${pad(parts[4])}`;
  }
  processNextFile(shouldSkip) {
    const header = this._archive.getFileHeader();
    if (header.state.errCode === 10) return 'ERAR_END_ARCHIVE';
    if (header.state.errCode !== 0) throw this.fail(header.state.errCode);

    const fileHeader = {
      name: header.name,
      directory: (header.flags & 0x20) !== 0,
      encrypted: (header.flags & 0x04) !== 0,
      packSize: header.packSize,
      unpSize: header.unpSize,
      time: this.dosTimeToIso(header.time),
    };

    const skip = shouldSkip(fileHeader);
    const state = this._archive.readFile(skip);
    if (state.errCode !== 0) throw this.fail(state.errCode);

    return { fileHeader, extracted: !skip };
  }
  listEntries() {
    this.openArc(true);
    const files = [];
    try {
      for (;;) {
        const result = this.processNextFile(() => true);
        if (result === 'ERAR_END_ARCHIVE') break;
        files.push(result.fileHeader);
      }
    } finally {
      this.closeArc();
    }
    return files;
  }
  extractEntry(name) {
    this.openArc(false);
    let data = null;
    try {
      for (;;) {
        const result = this.processNextFile((fh) => fh.name !== name);
        if (result === 'ERAR_END_ARCHIVE') break;
        if (result.extracted && result.fileHeader.name === name) {
          data = this.dataFiles[this.getExtractedFileName(name)].file.readAll();
          break;
        }
      }
    } finally {
      this.closeArc();
    }
    if (!data) {
      throw new ArchivePreviewError('ERAR_EREAD', `No se pudo extraer "${name}" del archivo.`);
    }
    return data;
  }
}

// ------------------------------------------------------------------------------
// Entrada del visor: llamada desde preview.js para .rar / .zip / .jar
// ------------------------------------------------------------------------------
async function previewArchiveFile(fileName, relPath, ext, sizeFormatted) {
  const loading = document.getElementById('preview-loading');
  const loadingText = document.getElementById('preview-loading-text');
  const archiveContainer = document.getElementById('preview-archive-container');
  const binaryContainer = document.getElementById('preview-binary-container');
  const metaEl = document.getElementById('preview-file-meta');

  const showBinaryFallback = (detail) => {
    if (loading) loading.style.display = 'none';
    if (binaryContainer) {
      binaryContainer.style.display = 'block';
      const bName = document.getElementById('preview-binary-name');
      const bDet = document.getElementById('preview-binary-details');
      if (bName) bName.textContent = fileName;
      if (bDet) bDet.textContent = detail;
    }
    if (metaEl) metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Archivo ${ext.toUpperCase()}`;
  };

  try {
    if (loadingText) loadingText.textContent = 'Descargando archivo comprimido...';
    const root = encodeURIComponent(AppState.files.root || 'nas');
    const encodedPath = encodeURIComponent(relPath);
    const rawUrl = `/api/files/raw?root=${root}&path=${encodedPath}&_t=${Date.now()}`;
    const response = await fetch(rawUrl, { credentials: 'same-origin' });
    if (!response.ok) throw new Error(`HTTP ${response.status} al descargar el archivo.`);
    const arrayBuffer = await response.arrayBuffer();
    if (!sizeFormatted && arrayBuffer.byteLength) {
      sizeFormatted = formatBytesLocal(arrayBuffer.byteLength);
    }

    let entries = [];
    if (ext === 'rar') {
      if (loadingText) loadingText.textContent = 'Cargando motor RAR local (WASM)...';
      const unrar = await loadUnrarEngine();
      if (loadingText) loadingText.textContent = 'Leyendo estructura del archivo RAR...';
      const extractor = new RarMemoryExtractor(unrar, arrayBuffer);
      entries = extractor.listEntries().map((fh) => ({
        name: fh.name,
        directory: fh.directory,
        encrypted: fh.encrypted,
        unpSize: fh.unpSize,
        packSize: fh.packSize,
        time: fh.time,
      }));
      ArchiveInspector.current = { kind: 'rar', extractor, entries, fileName };
    } else {
      if (typeof JSZip === 'undefined') {
        if (loadingText) loadingText.textContent = 'Cargando motor de descompresión...';
        await loadScriptLazy('/js/vendor/jszip.min.js');
      }
      if (loadingText) loadingText.textContent = 'Leyendo estructura del archivo ZIP...';
      const zip = await JSZip.loadAsync(arrayBuffer);
      zip.forEach((entryPath, file) => {
        entries.push({
          name: entryPath,
          directory: file.dir,
          encrypted: false,
          unpSize: file._data ? file._data.uncompressedSize : 0,
          packSize: file._data ? file._data.compressedSize : 0,
          time: file.date ? formatArchiveDate(file.date) : '',
        });
      });
      entries.sort((a, b) => a.name.localeCompare(b.name));
      ArchiveInspector.current = { kind: 'zip', zip, entries, fileName };
    }

    if (entries.length === 0) {
      throw new Error('El archivo comprimido no contiene elementos legibles.');
    }

    renderArchiveTable(entries);
    if (loading) loading.style.display = 'none';
    if (archiveContainer) archiveContainer.style.display = 'flex';
    if (metaEl) {
      const totalUnp = entries.reduce((acc, e) => acc + (e.unpSize || 0), 0);
      metaEl.textContent = `${sizeFormatted ? sizeFormatted + ' • ' : ''}Archivo ${ext.toUpperCase()} • ${entries.length} elemento(s) • ${formatBytesLocal(totalUnp)} descomprimido`;
    }
  } catch (err) {
    console.error('Error al inspeccionar archivo comprimido:', err);
    const detail = err instanceof ArchivePreviewError
      ? err.message
      : (err.message || 'No se pudo leer el archivo comprimido.');
    showBinaryFallback(detail);
  }
}

function formatArchiveDate(date) {
  const pad = (n) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

// ------------------------------------------------------------------------------
// Tabla del listado (todo con textContent: sin riesgo XSS)
// ------------------------------------------------------------------------------
function renderArchiveTable(entries) {
  const tbody = document.getElementById('preview-archive-tbody');
  const summary = document.getElementById('preview-archive-summary');
  if (!tbody) return;
  tbody.textContent = '';

  entries.forEach((entry, index) => {
    const tr = document.createElement('tr');
    if (!entry.directory) {
      tr.style.cursor = 'pointer';
      tr.title = 'Clic para previsualizar';
      tr.addEventListener('click', () => previewArchiveEntry(index));
    }

    const tdName = document.createElement('td');
    tdName.style.wordBreak = 'break-all';
    const icon = document.createElement('span');
    icon.textContent = entry.directory ? '📁 ' : '📄 ';
    const nameSpan = document.createElement('span');
    nameSpan.textContent = entry.name;
    tdName.appendChild(icon);
    tdName.appendChild(nameSpan);
    if (entry.encrypted) {
      const lock = document.createElement('span');
      lock.textContent = ' 🔒';
      lock.title = 'Protegido con contraseña';
      tdName.appendChild(lock);
    }
    tr.appendChild(tdName);

    const tdSize = document.createElement('td');
    tdSize.textContent = entry.directory ? '—' : formatBytesLocal(entry.unpSize || 0);
    tr.appendChild(tdSize);

    const tdPack = document.createElement('td');
    tdPack.textContent = entry.directory ? '—' : formatBytesLocal(entry.packSize || 0);
    tr.appendChild(tdPack);

    const tdRatio = document.createElement('td');
    if (!entry.directory && entry.unpSize > 0) {
      tdRatio.textContent = `${Math.round((entry.packSize / entry.unpSize) * 100)}%`;
    } else {
      tdRatio.textContent = '—';
    }
    tr.appendChild(tdRatio);

    const tdTime = document.createElement('td');
    tdTime.textContent = entry.time || '—';
    tr.appendChild(tdTime);

    tbody.appendChild(tr);
  });

  if (summary) {
    const files = entries.filter((e) => !e.directory).length;
    const dirs = entries.length - files;
    summary.textContent = `${files} archivo(s)${dirs ? ` • ${dirs} carpeta(s)` : ''} • Clic en un archivo para previsualizarlo`;
  }
}

// ------------------------------------------------------------------------------
// Previsualización de un elemento interno del archivo comprimido
// ------------------------------------------------------------------------------
const ARCHIVE_TEXT_EXTS = ['txt', 'log', 'md', 'csv', 'tsv', 'json', 'xml', 'ini', 'cfg', 'conf', 'yml', 'yaml', 'sh', 'bash', 'py', 'js', 'ts', 'css', 'html', 'htm', 'sql', 'bat', 'ps1', 'php', 'rb', 'go', 'rs', 'c', 'h', 'cpp', 'java', 'env', 'properties', 'list', 'nfo', 'srt'];
const ARCHIVE_IMG_EXTS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico'];
const ARCHIVE_MIME = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif',
  webp: 'image/webp', bmp: 'image/bmp', ico: 'image/x-icon', pdf: 'application/pdf',
};

async function previewArchiveEntry(index) {
  const cur = ArchiveInspector.current;
  if (!cur) return;
  const entry = cur.entries[index];
  if (!entry || entry.directory) return;

  if ((entry.unpSize || 0) > ARCHIVE_MAX_ENTRY_BYTES) {
    showToast(`"${entry.name}" supera los 100 MiB descomprimidos; descarga el archivo comprimido para extraerlo.`, 'error');
    return;
  }
  if (entry.encrypted) {
    showToast(`"${entry.name}" está protegido con contraseña.`, 'error');
    return;
  }

  try {
    let data;
    if (cur.kind === 'rar') {
      data = cur.extractor.extractEntry(entry.name);
    } else {
      const zipEntry = cur.zip.file(entry.name);
      if (!zipEntry) throw new Error('Entrada no encontrada en el ZIP.');
      data = new Uint8Array(await zipEntry.async('arraybuffer'));
    }
    showArchiveEntryPreview(entry, data);
  } catch (err) {
    console.error('Error al extraer elemento del archivo:', err);
    showToast(err.message || `No se pudo extraer "${entry.name}".`, 'error');
  }
}

function showArchiveEntryPreview(entry, data) {
  const ext = entry.name.includes('.') ? entry.name.split('.').pop().toLowerCase() : '';
  const archiveContainer = document.getElementById('preview-archive-container');
  const backbar = document.getElementById('preview-archive-backbar');
  const metaEl = document.getElementById('preview-file-meta');
  const codeContainer = document.getElementById('preview-code-container');
  const imgContainer = document.getElementById('preview-image-container');
  const pdfContainer = document.getElementById('preview-pdf-container');

  if (archiveContainer) archiveContainer.style.display = 'none';
  if (backbar) backbar.style.display = 'flex';

  const entrySize = formatBytesLocal(data.byteLength);
  const innerName = entry.name.split('/').pop();

  if (ARCHIVE_TEXT_EXTS.includes(ext)) {
    const text = new TextDecoder('utf-8', { fatal: false }).decode(data);
    AppState.files.currentPreviewText = text;
    const contentEl = document.getElementById('preview-code-content');
    if (contentEl) contentEl.textContent = text;
    updatePreviewLineNumbers(text);
    if (codeContainer) codeContainer.style.display = 'flex';
    if (metaEl) metaEl.textContent = `${entrySize} • ${innerName} (dentro de ${ArchiveInspector.current.fileName})`;
    return;
  }

  if (ARCHIVE_IMG_EXTS.includes(ext)) {
    const blob = new Blob([data], { type: ARCHIVE_MIME[ext] || 'image/png' });
    const imgEl = document.getElementById('preview-img-element');
    if (imgEl) {
      imgEl.onload = () => {
        if (metaEl) metaEl.textContent = `${entrySize} • ${imgEl.naturalWidth}x${imgEl.naturalHeight} px • dentro de ${ArchiveInspector.current.fileName}`;
      };
      imgEl.src = URL.createObjectURL(blob);
    }
    if (imgContainer) imgContainer.style.display = 'flex';
    return;
  }

  if (ext === 'pdf') {
    const blob = new Blob([data], { type: 'application/pdf' });
    const frame = document.getElementById('preview-pdf-frame');
    if (frame) frame.src = URL.createObjectURL(blob);
    if (pdfContainer) pdfContainer.style.display = 'block';
    if (metaEl) metaEl.textContent = `${entrySize} • ${innerName} (dentro de ${ArchiveInspector.current.fileName})`;
    return;
  }

  // Formato sin visor interno: ofrecer descarga directa del elemento extraído
  downloadArchiveEntryBlob(entry.name, data);
  showToast(`"${innerName}" no tiene visor interno; se inició su descarga.`, 'info');
  backToArchiveList();
}

function downloadArchiveEntryBlob(name, data) {
  const blob = new Blob([data], { type: 'application/octet-stream' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name.split('/').pop() || 'archivo';
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}

function backToArchiveList() {
  const cur = ArchiveInspector.current;
  const backbar = document.getElementById('preview-archive-backbar');
  const archiveContainer = document.getElementById('preview-archive-container');
  const codeContainer = document.getElementById('preview-code-container');
  const imgContainer = document.getElementById('preview-image-container');
  const pdfContainer = document.getElementById('preview-pdf-container');
  const metaEl = document.getElementById('preview-file-meta');
  const imgEl = document.getElementById('preview-img-element');
  const frame = document.getElementById('preview-pdf-frame');

  if (imgEl && imgEl.src.startsWith('blob:')) {
    URL.revokeObjectURL(imgEl.src);
    imgEl.removeAttribute('src');
  }
  if (frame && frame.src.startsWith('blob:')) {
    URL.revokeObjectURL(frame.src);
    frame.src = '';
  }
  if (codeContainer) codeContainer.style.display = 'none';
  if (imgContainer) imgContainer.style.display = 'none';
  if (pdfContainer) pdfContainer.style.display = 'none';
  if (backbar) backbar.style.display = 'none';
  if (archiveContainer) archiveContainer.style.display = 'flex';

  if (metaEl && cur) {
    const totalUnp = cur.entries.reduce((acc, e) => acc + (e.unpSize || 0), 0);
    metaEl.textContent = `${cur.entries.length} elemento(s) • ${formatBytesLocal(totalUnp)} descomprimido • ${cur.fileName}`;
  }
}
