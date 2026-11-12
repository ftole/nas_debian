function openModal(modalId) {
  let modal = document.getElementById(modalId);
  if (!modal && modalId === 'modal-format-disk') {
    modal = document.getElementById('modal-storage-manage');
  } else if (!modal && modalId === 'modal-storage-manage') {
    modal = document.getElementById('modal-format-disk');
  }
  if (modal) {
    modal.classList.add('active');
    modal.classList.add('open');
  }
}

function closeModal(modalId) {
  let modal = document.getElementById(modalId);
  if (!modal && modalId === 'modal-format-disk') {
    modal = document.getElementById('modal-storage-manage');
  } else if (!modal && modalId === 'modal-storage-manage') {
    modal = document.getElementById('modal-format-disk');
  }
  if (modal) {
    modal.classList.remove('active');
    modal.classList.remove('open');
    if (modalId === 'modal-file-preview') {
      const modalHeader = modal.querySelector('.modal-header');
      if (modalHeader) {
        modalHeader.style.backgroundColor = '';
        modalHeader.style.borderBottomColor = '';
        modalHeader.style.removeProperty('--preview-header-bg');
        modalHeader.style.removeProperty('--preview-header-border');
        modalHeader.style.removeProperty('--preview-header-text');
        modalHeader.style.removeProperty('--preview-header-meta');
        modalHeader.style.removeProperty('--preview-btn-bg');
        modalHeader.style.removeProperty('--preview-btn-border');
        modalHeader.style.removeProperty('--preview-btn-hover-bg');
        const titleEl = document.getElementById('preview-file-name');
        const metaEl = document.getElementById('preview-file-meta');
        const iconEl = document.getElementById('preview-header-icon');
        const closeBtn = modalHeader.querySelector('.modal-close');
        if (titleEl) titleEl.style.color = '';
        if (metaEl) metaEl.style.color = '';
        if (iconEl) iconEl.style.color = '';
        if (closeBtn) closeBtn.style.color = '';
        modalHeader.querySelectorAll('.btn-secondary').forEach(b => {
          b.style.backgroundColor = '';
          b.style.color = '';
          b.style.borderColor = '';
        });
        const saveBtn = document.getElementById('btn-preview-save');
        if (saveBtn) {
          saveBtn.style.backgroundColor = '';
          saveBtn.style.color = '';
          saveBtn.style.borderColor = '';
        }
      }
      const vidEl = document.getElementById('preview-video-element');
      if (vidEl) {
        vidEl.pause();
        vidEl.removeAttribute('src');
        vidEl.load();
      }
      const audEl = document.getElementById('preview-audio-element');
      if (audEl) {
        audEl.pause();
        audEl.removeAttribute('src');
        audEl.load();
      }
      const imgEl = document.getElementById('preview-img-element');
      if (imgEl) {
        imgEl.removeAttribute('src');
      }
      const pdfFrame = document.getElementById('preview-pdf-frame');
      if (pdfFrame) {
        pdfFrame.src = '';
      }
      AppState.files.currentPreviewFile = null;
      AppState.files.isEditingPreview = false;
    }
  }
}

