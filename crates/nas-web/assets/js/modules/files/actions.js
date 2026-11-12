
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
