// ==============================================================================
// 16. Módulo: Active Directory (Domain)
// ==============================================================================
async function loadDomainStatus() {
  const card = document.getElementById('domain-status-card');
  const joinPanel = document.getElementById('domain-join-panel');
  if (!card) return;

  card.innerHTML = '<div style="text-align:center; padding:20px; color:var(--text-muted);">Consultando estado de dominio...</div>';

  try {
    const res = await apiFetch('/api/domain/status');
    const d = res.data || res;
    AppState.domain.status = d;

    const isJoined = d.joined;
    const badge = isJoined
      ? '<span class="badge badge-ok">Unido al Dominio</span>'
      : '<span class="badge badge-gray">Servidor Autónomo (Standalone)</span>';

    const sssdBadge = d.sssd_active
      ? '<span class="badge badge-ok">Activo</span>'
      : '<span class="badge badge-gray">Inactivo</span>';

    let actionBtn = '';
    if (isJoined) {
      actionBtn = `
        <div style="margin-top:16px;">
          <button type="button" class="btn btn-danger btn-sm" onclick="handleDomainLeave()">
            <svg class="icon"><use href="#icon-power"></use></svg> Desvincular del Dominio
          </button>
        </div>
      `;
      if (joinPanel) joinPanel.style.display = 'none';
    } else {
      if (joinPanel) joinPanel.style.display = 'block';
    }

    card.innerHTML = `
      <div style="display:flex; flex-direction:column; gap:12px;">
        <div class="info-row">
          <span class="info-label">Estado de Membresía:</span>
          <div class="info-val">${badge}</div>
        </div>
        <div class="info-row">
          <span class="info-label">Dominio / Realm:</span>
          <strong class="info-val">${escapeHtml(d.domain || d.realm || 'Ninguno (Modo Workgroup)')}</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Grupo de Trabajo (Workgroup):</span>
          <strong class="info-val">${escapeHtml(d.workgroup || 'TEAM-JOFRATO')}</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Controlador de Dominio (KDC):</span>
          <strong class="info-val">${escapeHtml(d.kdc || 'N/A')}</strong>
        </div>
        <div class="info-row">
          <span class="info-label">Servicio SSSD (Autenticación):</span>
          <div class="info-val">${sssdBadge}</div>
        </div>
        ${actionBtn}
      </div>
    `;

    const badgeDomain = document.getElementById('badge-domain');
    if (badgeDomain) {
      badgeDomain.textContent = isJoined ? 'AD OK' : 'AD';
      badgeDomain.className = isJoined ? 'nav-badge badge-ok' : 'nav-badge';
    }

  } catch (err) {
    card.innerHTML = `<p style="color:var(--accent-danger);">Error al consultar estado del dominio: ${escapeHtml(err.message)}</p>`;
  }
}

async function handleDomainDiscover(e) {
  e.preventDefault();
  const domainInput = document.getElementById('discover-domain-name');
  const resultDiv = document.getElementById('domain-discover-result');
  const btn = document.getElementById('btn-discover-domain');
  if (!domainInput || !resultDiv) return;

  const domain = domainInput.value.trim();
  if (!domain) return;

  resultDiv.style.display = 'block';
  resultDiv.innerHTML = '<p style="color:var(--text-muted); font-size:13px;">Buscando controladores de dominio Kerberos / DNS...</p>';
  if (btn) btn.disabled = true;

  try {
    const res = await apiFetch('/api/domain/discover', {
      method: 'POST',
      body: JSON.stringify({ domain }),
    });

    const d = res.data || res;
    resultDiv.innerHTML = `
      <div style="background:var(--bg-surface); padding:12px; border-radius:6px; border:1px solid var(--border-color); font-size:13px;">
        <div style="color:var(--accent-success-text); font-weight:600; margin-bottom:6px;">Controlador de Dominio Detectado</div>
        <div><strong>Dominio:</strong> ${escapeHtml(d.domain_name || domain)}</div>
        <div><strong>Realm:</strong> ${escapeHtml(d.realm_name || 'N/A')}</div>
        <div><strong>Servidores KDC:</strong> ${escapeHtml(Array.isArray(d.kdc) ? d.kdc.join(', ') : (d.kdc || 'N/A'))}</div>
        <div><strong>Software:</strong> ${escapeHtml(d.server_software || 'Active Directory')}</div>
      </div>
    `;
    showToast(`Dominio "${domain}" descubierto exitosamente.`, 'success');
  } catch (err) {
    resultDiv.innerHTML = `<div style="color:var(--accent-danger); font-size:13px;">No se pudo descubrir el dominio: ${escapeHtml(err.message)}</div>`;
  } finally {
    if (btn) btn.disabled = false;
  }
}

async function handleDomainJoin(e) {
  e.preventDefault();
  const domain = document.getElementById('join-domain-name')?.value.trim();
  const user = document.getElementById('join-admin-user')?.value.trim();
  const password = document.getElementById('join-admin-pass')?.value;
  const ou = document.getElementById('join-ou')?.value.trim();
  const btn = document.getElementById('btn-join-domain');

  if (!domain || !user || !password) {
    showToast('Por favor completa todos los campos requeridos para la unión.', 'error');
    return;
  }

  if (btn) {
    btn.disabled = true;
    btn.textContent = 'Uniendo al dominio...';
  }

  try {
    await apiFetch('/api/domain/join', {
      method: 'POST',
      body: JSON.stringify({ domain, user, password, ou }),
    });

    showToast(`Servidor unido con éxito al dominio ${domain}.`, 'success');
    const passInput = document.getElementById('join-admin-pass');
    if (passInput) passInput.value = '';
    loadDomainStatus();
  } catch (err) {
    // Ya mostrado por apiFetch
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = '<svg class="icon"><use href="#icon-domain"></use></svg> Unir al Dominio';
    }
  }
}

async function handleDomainLeave() {
  const user = window.prompt('Introduce el usuario administrador del dominio para desvincular (ej. Administrator):');
  if (!user) return;
  const password = window.prompt('Introduce la contraseña del administrador del dominio:');
  if (!password) return;

  try {
    await apiFetch('/api/domain/leave', {
      method: 'POST',
      body: JSON.stringify({ user, password }),
    });

    showToast('El servidor fue desvinculado del dominio Active Directory.', 'info');
    loadDomainStatus();
  } catch (err) {
    // Ya mostrado por apiFetch
  }
}

async function loadDiagnostics() {
  const overallVal = document.getElementById('diag-overall-val');
  const overallSub = document.getElementById('diag-overall-sub');
  const servicesVal = document.getElementById('diag-services-val');
  const sambaVal = document.getElementById('diag-samba-val');
  const container = document.getElementById('diagnostics-summary-container');

  if (container) {
    container.innerHTML = '<p>Ejecutando auditoría y pruebas de salud del servidor...</p>';
  }

  try {
    const res = await apiFetch('/api/diagnostics');
    const d = res.data || res;

    if (overallVal) {
      if (d.overall_status === 'OK') {
        overallVal.innerHTML = '<span class="badge badge-ok">Saludable (OK)</span>';
      } else {
        overallVal.innerHTML = '<span class="badge badge-warning">Atención requerida</span>';
      }
    }

    if (overallSub) {
      overallSub.textContent = `Host: ${d.hostname} | Uptime: ${d.uptime}`;
    }

    if (servicesVal) {
      servicesVal.textContent = `${d.services_active} / ${d.services_total} Activos`;
    }

    if (sambaVal) {
      sambaVal.innerHTML = d.testparm_ok
        ? '<span class="badge badge-ok">Sintaxis Válida</span>'
        : '<span class="badge badge-danger">Error de Configuración</span>';
    }

    if (container) {
      const servicesArray = Array.isArray(d.services) ? d.services : Object.values(d.services || {});
      const servicesList = servicesArray.map(item => {
        const isOk = item.active;
        const svcName = item.service || '';
        return `
          <div style="display:flex; justify-content:space-between; align-items:center; padding:6px 0; border-bottom:1px solid var(--border-color); font-size:13px;">
            <span><code>${escapeHtml(svcName)}</code> &bull; ${escapeHtml(item.name || '')}</span>
            ${isOk ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-danger">Inactivo</span>'}
          </div>
        `;
      }).join('');

      container.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:16px;">
          <div>
            <h4 style="margin:0 0 8px; font-size:14px;">Estado de Demonios Clave</h4>
            <div style="background:var(--bg-body); border-radius:4px; padding:10px;">
              ${servicesList}
            </div>
          </div>
          <div style="display:flex; gap:20px; flex-wrap:wrap; font-size:13px;">
            <div><strong>Almacenamiento (/srv/nas):</strong> ${d.storage.used_gb} GB / ${d.storage.total_gb} GB (${d.storage.usage_percent}%)</div>
            <div><strong>Recursos Samba:</strong> ${d.shares_count} activos</div>
            <div><strong>Tareas de Respaldo:</strong> ${d.backups_count} programadas</div>
          </div>
        </div>
      `;
    }

    showToast('Diagnóstico del sistema completado con éxito.', 'success');
  } catch (err) {
    if (container) {
      container.innerHTML = `<p style="color:var(--accent-danger);">Error al ejecutar diagnóstico: ${escapeHtml(err.message)}</p>`;
    }
  }
}

function initFilesKeyboardShortcuts() {
  document.addEventListener('keydown', (e) => {
    const filesView = document.getElementById('view-files');
    if (!filesView || !filesView.classList.contains('active')) return;

    if (document.querySelector('.modal-backdrop.active, .modal-backdrop.open, .modal-overlay.active, .modal-overlay.open')) return;

    const tag = (e.target?.tagName || '').toLowerCase();
    const isEditing = tag === 'input' || tag === 'textarea' || tag === 'select' || e.target?.isContentEditable;

    if ((e.ctrlKey && (e.key === 'f' || e.key === 'F')) || (!isEditing && e.key === '/')) {
      const searchInput = AppState.files.inTrash ? document.getElementById('trash-filter-input') : document.getElementById('files-filter-input');
      if (searchInput) {
        e.preventDefault();
        searchInput.focus();
        if (typeof searchInput.select === 'function') searchInput.select();
        return;
      }
    }

    if (isEditing) return;

    if (AppState.files.inTrash) return;

    if (e.key === 'Backspace') {
      if (AppState.files.currentPath) {
        e.preventDefault();
        const parts = AppState.files.currentPath.replace(/^\/+|\/+$/g, '').split('/').filter(Boolean);
        parts.pop();
        navigateToSubpath(parts.join('/'));
      }
      return;
    }

    if (e.key === 'F2') {
      if (AppState.files.selectedItemPath) {
        const item = (AppState.files.items || []).find(i => (i.relative_path || i.path) === AppState.files.selectedItemPath);
        if (item) {
          e.preventDefault();
          openRenameModal(item.relative_path || item.path, item.name);
        }
      }
      return;
    }

    if (e.key === 'Delete') {
      if (AppState.files.selectedItemPath) {
        const item = (AppState.files.items || []).find(i => (i.relative_path || i.path) === AppState.files.selectedItemPath);
        if (item) {
          e.preventDefault();
          openDeleteModal(item.relative_path || item.path, item.name);
        }
      }
      return;
    }
  });
}

