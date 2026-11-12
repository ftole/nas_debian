
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

const ALLOWED_SHEET_TAGS = new Set([
  'TABLE', 'THEAD', 'TBODY', 'TFOOT', 'TR', 'TH', 'TD', 'CAPTION', 'COLGROUP', 'COL', 'SPAN', 'BR',
]);
const ALLOWED_SHEET_ATTRS = new Set(['id', 'colspan', 'rowspan', 'scope']);

function sanitizeSheetNode(node, doc) {
  if (node.nodeType === Node.TEXT_NODE) {
    return doc.createTextNode(node.textContent || '');
  }
  if (node.nodeType !== Node.ELEMENT_NODE) {
    return null;
  }
  const tag = node.tagName.toUpperCase();
  if (!ALLOWED_SHEET_TAGS.has(tag)) {
    return null;
  }
  const clean = doc.createElement(tag.toLowerCase());
  for (const attr of node.attributes) {
    const name = attr.name.toLowerCase();
    if (ALLOWED_SHEET_ATTRS.has(name)) {
      if (name === 'id' && !/^[A-Za-z0-9_-]+$/.test(attr.value)) continue;
      clean.setAttribute(name, attr.value);
    }
  }
  for (const child of node.childNodes) {
    const sanitizedChild = sanitizeSheetNode(child, doc);
    if (sanitizedChild) clean.appendChild(sanitizedChild);
  }
  return clean;
}

function sanitizeSheetFragment(rawHtml) {
  const parser = new DOMParser();
  const parsed = parser.parseFromString(String(rawHtml || ''), 'text/html');
  const fragment = document.createDocumentFragment();
  for (const child of parsed.body.childNodes) {
    const sanitized = sanitizeSheetNode(child, document);
    if (sanitized) fragment.appendChild(sanitized);
  }
  return fragment;
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
  tableContainer.replaceChildren(sanitizeSheetFragment(html));

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

