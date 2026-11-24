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

// ==============================================================================
// Modales genéricos de confirmación y captura (sustituyen a confirm()/prompt())
// ==============================================================================
let _confirmResolver = null;
let _promptResolver = null;

function _setModalText(id, text, hideWhenEmpty) {
  const el = document.getElementById(id);
  if (!el) return;
  el.textContent = text || '';
  if (hideWhenEmpty) el.style.display = text ? '' : 'none';
}

function confirmModal(options = {}) {
  const opts = Object.assign({
    title: 'Confirmar acción',
    message: '',
    note: '',
    warning: '',
    confirmLabel: 'Confirmar',
    cancelLabel: 'Cancelar',
    danger: false,
  }, options);

  const modal = document.getElementById('modal-confirm');
  if (!modal) {
    // Degradación segura si la plantilla no está disponible
    return Promise.resolve(window.confirm(opts.message));
  }

  if (_confirmResolver) resolveConfirmModal(false);

  _setModalText('modal-confirm-title', opts.title);
  _setModalText('modal-confirm-message', opts.message);
  _setModalText('modal-confirm-note', opts.note, true);
  _setModalText('modal-confirm-warning', opts.warning, true);

  const icon = document.getElementById('modal-confirm-icon');
  if (icon) {
    icon.style.color = opts.danger ? 'var(--accent-danger)' : 'var(--accent-primary)';
    const use = icon.querySelector('use');
    if (use) use.setAttribute('href', opts.danger ? '#icon-alert-triangle' : '#icon-info');
  }

  const btnConfirm = document.getElementById('btn-confirm-accept');
  if (btnConfirm) {
    btnConfirm.textContent = opts.confirmLabel;
    btnConfirm.className = opts.danger ? 'btn btn-danger' : 'btn btn-primary';
  }
  const btnCancel = document.getElementById('btn-cancel-confirm');
  if (btnCancel) btnCancel.textContent = opts.cancelLabel;

  openModal('modal-confirm');
  // En acciones destructivas el foco inicial va a Cancelar (evita Enter accidental)
  const focusTarget = opts.danger ? btnCancel : btnConfirm;
  if (focusTarget) focusTarget.focus();

  return new Promise((resolve) => { _confirmResolver = resolve; });
}

function resolveConfirmModal(value) {
  closeModal('modal-confirm');
  if (_confirmResolver) {
    const resolve = _confirmResolver;
    _confirmResolver = null;
    resolve(value === true);
  }
}

function promptModal(options = {}) {
  const opts = Object.assign({
    title: 'Datos requeridos',
    message: '',
    submitLabel: 'Aceptar',
    cancelLabel: 'Cancelar',
    fields: [],
  }, options);

  const modal = document.getElementById('modal-prompt');
  if (!modal) {
    // Degradación segura con prompts nativos si la plantilla no está disponible
    return (async () => {
      const values = {};
      for (const field of opts.fields) {
        const value = window.prompt(field.label, field.value || '');
        if (value === null) return null;
        values[field.id] = value;
      }
      return values;
    })();
  }

  if (_promptResolver) resolvePromptModal(null);

  _setModalText('modal-prompt-title', opts.title);
  _setModalText('modal-prompt-message', opts.message, true);

  const btnSubmit = document.getElementById('btn-submit-prompt');
  if (btnSubmit) btnSubmit.textContent = opts.submitLabel;
  const btnCancel = document.getElementById('btn-cancel-prompt');
  if (btnCancel) btnCancel.textContent = opts.cancelLabel;

  const container = document.getElementById('modal-prompt-fields');
  if (!container) return Promise.resolve(null);
  container.textContent = '';
  opts.fields.forEach((field) => {
    const group = document.createElement('div');
    group.className = 'form-group';

    const label = document.createElement('label');
    label.setAttribute('for', `modal-prompt-field-${field.id}`);
    label.textContent = field.label;
    group.appendChild(label);

    const input = document.createElement('input');
    input.type = field.type || 'text';
    input.id = `modal-prompt-field-${field.id}`;
    input.dataset.fieldId = field.id;
    if (field.placeholder) input.placeholder = field.placeholder;
    if (field.value) input.value = field.value;
    if (field.autocomplete) input.setAttribute('autocomplete', field.autocomplete);
    if (field.required !== false) input.required = true;
    group.appendChild(input);

    if (field.hint) {
      const small = document.createElement('small');
      small.textContent = field.hint;
      group.appendChild(small);
    }

    container.appendChild(group);
  });

  openModal('modal-prompt');
  const firstInput = container.querySelector('input');
  if (firstInput) firstInput.focus();

  return new Promise((resolve) => { _promptResolver = resolve; });
}

function submitPromptModal(event) {
  event.preventDefault();
  const values = {};
  document.querySelectorAll('#modal-prompt-fields input[data-field-id]').forEach((input) => {
    values[input.dataset.fieldId] = input.value;
  });
  resolvePromptModal(values);
}

function resolvePromptModal(value) {
  closeModal('modal-prompt');
  const form = document.getElementById('form-modal-prompt');
  if (form) form.reset();
  if (_promptResolver) {
    const resolve = _promptResolver;
    _promptResolver = null;
    resolve(value);
  }
}

// Cierre con Escape: ambos modales son .modal-static para el handler global de
// app.js, así que aquí se resuelve la promesa pendiente como cancelación.
document.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape') return;
  if (_confirmResolver) {
    event.preventDefault();
    resolveConfirmModal(false);
  } else if (_promptResolver) {
    event.preventDefault();
    resolvePromptModal(null);
  }
});

