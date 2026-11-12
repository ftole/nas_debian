// 12. Módulo: Actualizaciones y Reinicio
// ==============================================================================
async function loadUpdates() {
  const container = document.getElementById('updates-container');
  if (!container) return;

  try {
    const res = await apiFetch('/api/system/updates');
    const u = res.data || res;
    if (!u) return;

    window.LAST_UPDATE_INFO = u;

    // Versión SemVer destacada
    const verTag = u.version ? (u.version.startsWith('v') ? u.version : `v${u.version}`) : 'v0.1.3';
    const latestVer = u.latest_version ? (u.latest_version.startsWith('v') ? u.latest_version : `v${u.latest_version}`) : verTag;

    // Badge verde de verificación criptográfica GPG
    const gpgBadge = u.gpg_verified
      ? `<span class="badge badge-ok" title="Tag criptográficamente firmado y verificado con GPG" style="display:inline-flex; align-items:center; gap:5px; font-weight:600;">
           <svg class="icon" style="width:13px;height:13px;"><use href="#icon-check"></use></svg> Firmado por Francisco Toledo
         </span>`
      : `<span class="badge badge-gray" title="Sin firma GPG detectada en el tag local">Sin firma GPG</span>`;

    // Estado dinámico (Al día o actualización disponible)
    const statusBanner = u.has_updates
      ? `<div style="display:inline-flex; align-items:center; gap:8px; background:rgba(245,158,11,0.15); border:1px solid rgba(245,158,11,0.4); color:#fbbf24; padding:6px 14px; border-radius:6px; font-weight:600; font-size:13px;">
           <svg class="icon" style="width:15px;height:15px;"><use href="#icon-alert"></use></svg>
           <span>Nueva versión <strong>${escapeHtml(latestVer)}</strong> disponible</span>
           <button class="btn btn-primary btn-sm" onclick="openSystemUpdateModal()" style="margin-left:8px;">
             <svg class="icon" style="width:13px;height:13px;"><use href="#icon-download"></use></svg> Actualizar ahora
           </button>
         </div>`
      : `<div style="display:inline-flex; align-items:center; gap:6px; background:rgba(34,197,94,0.12); border:1px solid rgba(34,197,94,0.3); color:#22c55e; padding:6px 14px; border-radius:6px; font-weight:600; font-size:13px;">
           <svg class="icon" style="width:15px;height:15px;"><use href="#icon-check"></use></svg>
           <span>Sistema al día</span>
         </div>`;

    // Actualizar badge en la navegación lateral
    const navBadge = document.getElementById('badge-updates');
    if (navBadge) {
      if (u.has_updates) {
        navBadge.textContent = '¡Actualizar!';
        navBadge.className = 'nav-badge badge-warn';
        navBadge.style.color = '#fbbf24';
      } else {
        navBadge.textContent = 'Al día';
        navBadge.className = 'nav-badge';
        navBadge.style.color = '';
      }
    }

    // Cuadro colapsable con las notas de versión (Release Notes / Changelog)
    const notesContent = u.release_notes || 'Versión estable con optimizaciones para Debian 13, BTRFS Zstandard, Samba 4 y panel web Slate UI.';
    const changelogBlock = `
      <details style="margin-top:14px; background:var(--bg-surface); border:1px solid var(--border-color); border-radius:8px; padding:10px 14px;">
        <summary style="cursor:pointer; font-weight:600; color:var(--text-secondary); display:flex; align-items:center; justify-content:space-between; user-select:none;">
          <span style="display:inline-flex; align-items:center; gap:6px;">
            <svg class="icon" style="width:14px;height:14px;"><use href="#icon-logs"></use></svg>
            Notas de la versión (Changelog)
          </span>
          <span style="font-size:12px; color:var(--text-muted);">(clic para ver detalles)</span>
        </summary>
        <div style="margin-top:10px; padding-top:10px; border-top:1px solid var(--border-color); font-size:13px; line-height:1.6; color:var(--text-secondary); white-space:pre-wrap;">${escapeHtml(notesContent)}</div>
      </details>
    `;

    container.innerHTML = `
      <div style="display:flex; flex-direction:column; gap:16px;">
        <div style="display:flex; align-items:center; justify-content:space-between; flex-wrap:wrap; gap:12px;">
          <div style="display:flex; align-items:center; gap:10px; flex-wrap:wrap;">
            <span style="font-size:26px; font-weight:700; color:var(--text-primary); letter-spacing:-0.5px;">${escapeHtml(verTag)}</span>
            ${gpgBadge}
            <span class="badge badge-blue">${escapeHtml(u.channel || 'main')}</span>
          </div>
          <div>
            ${statusBanner}
          </div>
        </div>

        <div class="card-grid-3" style="gap:10px;">
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Versión instalada:</span>
            <strong class="info-val"><code>${escapeHtml(u.version || '0.1.3')}</code> (${escapeHtml(u.installed_commit || '')})</strong>
          </div>
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Versión más reciente:</span>
            <strong class="info-val"><code>${escapeHtml(latestVer)}</code></strong>
          </div>
          <div class="info-row" style="padding:8px 12px; background:var(--bg-surface); border-radius:6px; border:1px solid var(--border-color);">
            <span class="info-label">Fecha del commit:</span>
            <strong class="info-val">${escapeHtml(u.commit_date || 'N/A')}</strong>
          </div>
        </div>

        ${changelogBlock}
      </div>
    `;
  } catch (e) {
    container.innerHTML = '<p style="color:var(--accent-danger);">Error al consultar actualizaciones: ' + escapeHtml(e.message) + '</p>';
  }
}

function openSystemUpdateModal() {
  const uptodateNotice = document.getElementById('system-update-uptodate-notice');
  if (uptodateNotice) {
    uptodateNotice.style.display = (window.LAST_UPDATE_INFO && !window.LAST_UPDATE_INFO.has_updates) ? 'flex' : 'none';
  }

  const progressDiv = document.getElementById('system-update-progress');
  const bar = document.getElementById('system-update-progress-bar');
  const log = document.getElementById('system-update-log');
  const stepLabel = document.getElementById('system-update-step-label');
  const spinner = document.getElementById('system-update-spinner');
  const footer = document.getElementById('system-update-footer');

  if (progressDiv) progressDiv.style.display = 'none';
  if (bar) {
    bar.style.width = '0%';
    bar.style.background = 'var(--accent-primary)';
  }
  if (log) {
    log.style.display = 'none';
    log.textContent = '';
  }
  if (stepLabel) {
    stepLabel.textContent = 'Iniciando actualización...';
    stepLabel.style.color = '';
  }
  if (spinner) spinner.style.display = '';

  if (footer) {
    footer.innerHTML = `
      <button type="button" class="btn btn-secondary" id="btn-cancel-system-update" onclick="closeModal('modal-system-update')">Cancelar</button>
      <button type="button" class="btn btn-primary" id="btn-start-system-update" onclick="startSystemUpdate()">
        <svg class="icon"><use href="#icon-download"></use></svg> Iniciar actualización ahora
      </button>
    `;
  }

  openModal('modal-system-update');
}

function applySystemUpdate() {
  openSystemUpdateModal();
}

async function startSystemUpdate() {
  const startBtn = document.getElementById('btn-start-system-update');
  const cancelBtn = document.getElementById('btn-cancel-system-update');
  const progressDiv = document.getElementById('system-update-progress');
  const bar = document.getElementById('system-update-progress-bar');
  const log = document.getElementById('system-update-log');
  const stepLabel = document.getElementById('system-update-step-label');
  const spinner = document.getElementById('system-update-spinner');
  const footer = document.getElementById('system-update-footer');

  if (startBtn) {
    startBtn.disabled = true;
    startBtn.classList.add('is-loading');
  }
  if (cancelBtn) {
    cancelBtn.disabled = true;
  }
  if (progressDiv) {
    progressDiv.style.display = 'block';
  }

  // Fases visuales informativas
  const phases = [
    { pct: '15%', text: 'Fase 1/5: Respaldo preventivo del estado actual en /tmp/backup_nas_*' },
    { pct: '35%', text: 'Fase 2/5: Avance rápido y verificación de firmas criptográficas GPG' },
    { pct: '55%', text: 'Fase 3/5: Validación de sintaxis de scripts Bash y código Rust' },
    { pct: '75%', text: 'Fase 4/5: Sincronización segura de binarios y reglas sudoers' },
    { pct: '90%', text: 'Fase 5/5: Recarga de servicios (Nginx, nas-web, Samba)' },
  ];

  let phaseIndex = 0;
  if (bar) bar.style.width = phases[0].pct;
  if (stepLabel) stepLabel.textContent = phases[0].text;

  const phaseInterval = setInterval(() => {
    phaseIndex++;
    if (phaseIndex < phases.length) {
      if (bar) bar.style.width = phases[phaseIndex].pct;
      if (stepLabel) stepLabel.textContent = phases[phaseIndex].text;
    }
  }, 1200);

  try {
    const res = await apiFetch('/api/system/update/run', { method: 'POST' });
    clearInterval(phaseInterval);

    if (bar) {
      bar.style.width = '100%';
      bar.style.background = '#22c55e';
    }
    if (spinner) {
      spinner.style.display = 'none';
    }
    if (stepLabel) {
      stepLabel.innerHTML = '<span style="color:#22c55e;">✔ Actualización completada con éxito</span>';
    }

    const outputText = res.data?.output || res.message || 'Sincronización finalizada correctamente.';
    if (log) {
      log.style.display = 'block';
      log.textContent = outputText;
    }

    if (footer) {
      footer.innerHTML = `
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-system-update')">Cerrar</button>
        <button type="button" class="btn btn-primary" onclick="window.location.reload()">
          <svg class="icon"><use href="#icon-refresh"></use></svg> Recargar panel (Ctrl+F5)
        </button>
      `;
    }

    showToast('Actualización del sistema completada con éxito.', 'success');
    loadUpdates();
  } catch (e) {
    clearInterval(phaseInterval);
    if (bar) {
      bar.style.width = '100%';
      bar.style.background = '#ef4444';
    }
    if (spinner) {
      spinner.style.display = 'none';
    }
    if (stepLabel) {
      stepLabel.innerHTML = `<span style="color:#ef4444;">✘ Error: ${escapeHtml(e.message)}</span>`;
    }
    if (log) {
      log.style.display = 'block';
      log.textContent = e.message;
    }
    if (footer) {
      footer.innerHTML = `
        <button type="button" class="btn btn-secondary" onclick="closeModal('modal-system-update')">Cerrar</button>
        <button type="button" class="btn btn-primary" onclick="startSystemUpdate()">Reintentar</button>
      `;
    }
    showToast('Error al actualizar el sistema: ' + e.message, 'error');
  }
}

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
