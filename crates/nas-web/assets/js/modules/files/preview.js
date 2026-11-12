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
