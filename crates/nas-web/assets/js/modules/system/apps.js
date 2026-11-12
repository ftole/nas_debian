
/**
 * Carga y actualiza dinámicamente las 6 tarjetas de tecnologías y aplicaciones.
 */
async function loadApplications() {
  try {
    const res = await apiFetch('/api/system/applications');
    const a = res.data || res;
    if (!a || !a.samba) return;

    window.LAST_SAMBA_SESSIONS = a.samba.sessions_list || [];
    renderSambaSessionsTable(window.LAST_SAMBA_SESSIONS);
    const sambaCountSpan = document.getElementById('app-samba-session-count');
    if (sambaCountSpan) {
      sambaCountSpan.textContent = a.samba.sessions || 0;
    }

    // 1. Servicio de Archivos (SMB)
    const badgeSamba = document.getElementById('app-badge-samba');
    const bodySamba = document.getElementById('app-body-samba');
    if (badgeSamba) {
      badgeSamba.className = a.samba.active ? 'badge badge-ok' : 'badge badge-err';
      badgeSamba.textContent = a.samba.active
        ? `● En ejecución (${a.samba.sessions} ${a.samba.sessions === 1 ? 'sesión' : 'sesiones'})`
        : '○ Detenido';
    }
    if (bodySamba) {
      bodySamba.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Estado:</strong> ${a.samba.active ? '<span class="badge badge-ok">En ejecución</span>' : '<span class="badge badge-err">Detenido</span>'} (Protocolo SMB 3.1.1)</div>
          <div><strong>Sesiones activas:</strong> <span class="badge badge-blue">${a.samba.sessions} conectadas</span></div>
          <div><strong>Compatibilidad:</strong> Red corporativa optimizada para Windows, Mac y Linux sin bloqueos</div>
        </div>
      `;
    }

    // 2. File Station & Portal Web
    const badgeWeb = document.getElementById('app-badge-web');
    const bodyWeb = document.getElementById('app-body-web');
    if (badgeWeb) {
      badgeWeb.className = a.web.active ? 'badge badge-ok' : 'badge badge-err';
      badgeWeb.textContent = a.web.active ? '● En ejecución' : '○ Detenido';
    }
    if (bodyWeb) {
      bodyWeb.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Servicio:</strong> ${a.web.active ? '<span class="badge badge-ok">En ejecución</span>' : '<span class="badge badge-err">Detenido</span>'} (Portal HTTPS + Nginx)</div>
          <div><strong>Motor asíncrono:</strong> <code>${escapeHtml(a.web.rust_version || a.web.php_version)}</code> (${escapeHtml(a.web.pool_mode)})</div>
          <div><strong>Consumo de memoria:</strong> <span class="badge badge-teal">${a.web.memory_mb} MB RAM</span> (Ultraligero)</div>
        </div>
      `;
    }

    // 3. Administrador de Almacenamiento
    const badgeBtrfs = document.getElementById('app-badge-btrfs');
    const bodyBtrfs = document.getElementById('app-body-btrfs');
    if (badgeBtrfs) {
      badgeBtrfs.className = a.btrfs.active ? 'badge badge-ok' : 'badge badge-gray';
      badgeBtrfs.textContent = a.btrfs.active ? '● Saludable' : '○ ' + escapeHtml(a.btrfs.filesystem);
    }
    if (bodyBtrfs) {
      bodyBtrfs.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Estado de volumen:</strong> <span class="badge badge-ok">${a.btrfs.active ? 'Saludable (Btrfs)' : escapeHtml(a.btrfs.filesystem)}</span></div>
          <div><strong>Compresión transparente:</strong> <code>${escapeHtml(a.btrfs.compression)}</code> (Ahorro 20-40% en disco)</div>
          <div><strong>Protección Bit Rot:</strong> ${escapeHtml(a.btrfs.scrub_status)} (${escapeHtml(a.btrfs.scrub_errors)})</div>
        </div>
      `;
    }

    // 4. Active Backup & Replicación
    const badgeRsync = document.getElementById('app-badge-rsync');
    const bodyRsync = document.getElementById('app-body-rsync');
    if (badgeRsync) {
      badgeRsync.className = a.rsync.tasks_count > 0 ? 'badge badge-ok' : 'badge badge-blue';
      badgeRsync.textContent = `● Listo (${a.rsync.tasks_count} ${a.rsync.tasks_count === 1 ? 'tarea' : 'tareas'})`;
    }
    if (bodyRsync) {
      const protoList = (a.rsync.protocols || ['CIFS 3.1.1', 'SSH']).join(', ');
      bodyRsync.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Estado:</strong> <span class="badge badge-ok">Listo</span> (${a.rsync.tasks_count} tareas configuradas)</div>
          <div><strong>Deduplicación histórica:</strong> ${escapeHtml(a.rsync.deduplication)}</div>
          <div><strong>Conectores remotos:</strong> ${escapeHtml(protoList)}</div>
        </div>
      `;
    }

    // 5. Descubrimiento de Red (WSD)
    const badgeWsdd2 = document.getElementById('app-badge-wsdd2');
    const bodyWsdd2 = document.getElementById('app-body-wsdd2');
    if (badgeWsdd2) {
      badgeWsdd2.className = a.wsdd2.active ? 'badge badge-ok' : 'badge badge-err';
      badgeWsdd2.textContent = a.wsdd2.active ? '● En ejecución' : '○ Detenido';
    }
    if (bodyWsdd2) {
      bodyWsdd2.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Estado de red:</strong> ${a.wsdd2.active ? '<span class="badge badge-ok">Visible en red</span>' : '<span class="badge badge-err">No visible</span>'} (WSD / LLMNR)</div>
          <div><strong>Demonios:</strong> WSDD2 ${a.wsdd2.wsdd_active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Inactivo</span>'} | nmbd ${a.wsdd2.nmbd_active ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Inactivo</span>'}</div>
          <div><strong>Compatibilidad:</strong> Detección instantánea en explorador de Windows 10/11</div>
        </div>
      `;
    }

    // 6. Consejero de Seguridad
    const badgeSecurity = document.getElementById('app-badge-security');
    const bodySecurity = document.getElementById('app-body-security');
    if (badgeSecurity) {
      badgeSecurity.className = a.security.active ? 'badge badge-ok' : 'badge badge-warn';
      badgeSecurity.textContent = a.security.active ? '● Protegido' : '● Parcial';
    }
    if (bodySecurity) {
      bodySecurity.innerHTML = `
        <div style="display:flex; flex-direction:column; gap:6px;">
          <div><strong>Estado del sistema:</strong> ${a.security.active ? '<span class="badge badge-ok">Protegido</span>' : '<span class="badge badge-warn">Parcial</span>'} (Auditoría en tiempo real)</div>
          <div><strong>Auditoría del Kernel:</strong> ${escapeHtml(a.security.audit_rules)}</div>
          <div><strong>Monitoreo de accesos:</strong> Vigilancia de <code>sudoers</code>, <code>smb.conf</code> y credenciales</div>
        </div>
      `;
    }
  } catch (e) {
    // Silencioso ante errores menores de red
  }
}

/**
 * Alterna entre las pestañas de Aplicaciones y Actualizaciones:
 * 'components' | 'updates' | 'security'
 */
function switchApplicationsTab(tabName) {
  const tabs = ['components', 'updates', 'security'];
  tabs.forEach(t => {
    const btn = document.getElementById(`apps-tab-btn-${t}`);
    const panel = document.getElementById(`apps-panel-${t}`);
    if (btn) {
      btn.classList.toggle('active', t === tabName);
    }
    if (panel) {
      panel.style.display = (t === tabName) ? 'block' : 'none';
    }
  });
}

/**
 * Comprueba de forma interactiva las actualizaciones y el estado de componentes,
 * con feedback visual en el botón y notificación toast.
 */
async function triggerSystemUpdatesCheck(btn) {
  const targetBtn = btn || document.getElementById('btn-refresh-applications');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await Promise.all([loadUpdates(), loadApplications()]);
    showToast('Estado de componentes y repositorio actualizado.', 'info');
  } catch (e) {
    showToast('Error al comprobar versiones: ' + (e?.message || e), 'error');
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Renderiza la tabla de sesiones SMB concurrentes en el modal.
 */
function renderSambaSessionsTable(sessions) {
  const tbody = document.getElementById('tbody-samba-sessions');
  if (!tbody) return;
  if (!sessions || sessions.length === 0) {
    tbody.innerHTML = `
      <tr>
        <td colspan="7" style="text-align:center; padding:24px; color:var(--text-muted);">
          No hay sesiones SMB activas en este momento.
        </td>
      </tr>
    `;
    return;
  }
  tbody.innerHTML = sessions.map(s => `
    <tr>
      <td><code>${escapeHtml(String(s.pid || '-'))}</code></td>
      <td><strong>${escapeHtml(String(s.username || '-'))}</strong></td>
      <td><code>${escapeHtml(String(s.group || '-'))}</code></td>
      <td><span style="font-family:monospace; font-size:12px;">${escapeHtml(String(s.machine || '-'))}</span></td>
      <td><span class="badge badge-blue">${escapeHtml(String(s.protocol || 'SMB3'))}</span></td>
      <td>${s.encryption && s.encryption !== '-' ? `<span class="badge badge-ok">${escapeHtml(String(s.encryption))}</span>` : '<span style="color:var(--text-muted);">-</span>'}</td>
      <td>${s.signing && s.signing !== '-' ? `<span class="badge badge-teal">${escapeHtml(String(s.signing))}</span>` : '<span style="color:var(--text-muted);">-</span>'}</td>
    </tr>
  `).join('');
}

/**
 * Abre el modal de sesiones SMB concurrentes y refresca la información.
 */
function openSambaSessionsModal() {
  if (window.LAST_SAMBA_SESSIONS) {
    renderSambaSessionsTable(window.LAST_SAMBA_SESSIONS);
  } else {
    loadApplications();
  }
  openModal('modal-samba-sessions');
}

/**
 * Recarga de forma rápida y segura el servicio Samba smbd sin interrumpir conexiones.
 */
async function reloadSambaService(btn) {
  const targetBtn = btn || document.getElementById('btn-reload-samba');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await manageServiceAction('smbd', 'reload');
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Recarga el servicio web nativo nas-web para aplicar cambios de configuración.
 */
async function reloadPhpService(btn) {
  const targetBtn = btn || document.getElementById('btn-reload-php');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await manageServiceAction('nas-web', 'restart');
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Reinicia el servicio de descubrimiento de red WSDD2 en clientes Windows 10/11.
 */
async function restartWsddService(btn) {
  const targetBtn = btn || document.getElementById('btn-restart-wsdd');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    await manageServiceAction('wsdd2', 'restart');
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

/**
 * Inicia o audita el proceso de BTRFS Scrub contra Bit Rot directamente desde la tarjeta.
 */
async function triggerBtrfsScrub(btn) {
  const targetBtn = btn || document.getElementById('btn-scrub-btrfs');
  if (targetBtn) {
    targetBtn.disabled = true;
    targetBtn.classList.add('is-loading');
  }
  try {
    if (typeof runStorageScrub === 'function') {
      await runStorageScrub();
    }
    await loadApplications();
  } finally {
    if (targetBtn) {
      targetBtn.disabled = false;
      targetBtn.classList.remove('is-loading');
    }
  }
}

async function confirmRebootServer() {
  const btn = document.getElementById('btn-confirm-reboot');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/system/reboot', { method: 'POST' });
    closeModal('modal-reboot-server');
    showToast('Reinicio del servidor ordenado. La conexión se reanudará en 1-2 minutos.', 'warning');
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

// ==============================================================================
